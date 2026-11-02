use super::now;
use aide_core::{PrCheck, PrId, PrObservation, PrReview, PrRole, PrStatus};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{collections::BTreeMap, process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt;

#[derive(Default)]
pub struct Client {
    pub cooldown: BTreeMap<String, u64>,
    pub identities: BTreeMap<String, String>,
    #[cfg(test)]
    pub fixture: Option<std::path::PathBuf>,
}
impl Client {
    pub async fn query(&mut self, host: &str, query: String) -> Result<Value> {
        let time = now();
        if self.cooldown.get(host).is_some_and(|until| *until > time) {
            bail!(
                "GitHub requests paused until {} (rate limit or retry backoff)",
                self.cooldown[host]
            );
        }
        let result = self.request(host, query).await;
        if result.is_err() {
            self.cooldown
                .entry(host.into())
                .and_modify(|t| *t = (*t).max(time + 60))
                .or_insert(time + 60);
        }
        result
    }
    async fn request(&mut self, host: &str, query: String) -> Result<Value> {
        let program = std::env::var_os("AIDE_GH").unwrap_or_else(|| "gh".into());
        #[cfg(test)]
        let program = if self.fixture.is_some() {
            std::ffi::OsString::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tests/aide-fixtures/gh"
            ))
        } else {
            program
        };
        let mut command = tokio::process::Command::new(program);
        #[cfg(test)]
        if let Some(path) = &self.fixture {
            command.env("AIDE_GITHUB_FIXTURE", path);
        }
        let mut child = command
            .args([
                "api",
                "graphql",
                "--hostname",
                host,
                "--include",
                "--input",
                "-",
            ])
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("GitHub CLI unavailable; install gh and run gh auth login")?;
        let input = serde_json::to_vec(&json!({"query":query}))?;
        let output = tokio::time::timeout(Duration::from_secs(25), async {
            let mut stdin = child.stdin.take().context("GitHub CLI stdin unavailable")?;
            stdin.write_all(&input).await?;
            drop(stdin);
            Ok::<_, anyhow::Error>(child.wait_with_output().await?)
        })
        .await
        .context("GitHub request timed out; retry when connected")??;
        let text = String::from_utf8(output.stdout).context("GitHub returned invalid output")?;
        let start = text.find('{');
        let headers = &text[..start.unwrap_or(text.len())];
        self.observe_limits(host, headers);
        let body: Value = start
            .and_then(|i| serde_json::from_str(&text[i..]).ok())
            .unwrap_or(Value::Null);
        if (!output.status.success() || body.get("errors").is_some())
            && body["data"]["viewer"]["id"].as_str().is_none()
        {
            let diagnostic = format!(
                "{} {} {}",
                headers,
                body,
                String::from_utf8_lossy(&output.stderr)
            )
            .to_lowercase();
            // Never forward arbitrary CLI stderr or response bodies to the browser/logs:
            // they can contain credential material from wrappers or remote errors.
            if diagnostic.contains("rate limit")
                || diagnostic.contains("rate_limit")
                || headers.contains("429")
            {
                self.cooldown
                    .entry(host.into())
                    .and_modify(|t| *t = (*t).max(now() + 60))
                    .or_insert(now() + 60);
                bail!("GitHub rate limit reached; refresh will retry after the cooldown");
            }
            if diagnostic.contains("saml") || diagnostic.contains("sso") {
                bail!(
                    "GitHub organization SSO authorization required; authorize gh for this organization"
                );
            }
            if headers.contains("401")
                || diagnostic.contains("auth login")
                || diagnostic.contains("bad credentials")
            {
                bail!(
                    "GitHub authentication unavailable; run gh auth login for this host (check GH_TOKEN overrides)"
                );
            }
            if headers.contains("403")
                || headers.contains("404")
                || diagnostic.contains("not accessible")
                || diagnostic.contains("could not resolve to a")
            {
                bail!(
                    "GitHub PR unavailable or access denied; verify the URL and account permissions"
                );
            }
            bail!(
                "GitHub query failed; check connectivity, gh version, account permissions and host compatibility"
            );
        }
        let mut data = body
            .get("data")
            .filter(|v| v.is_object())
            .cloned()
            .context("GitHub returned an incomplete response")?;
        data["_errors"] = body["errors"].clone();
        Ok(data)
    }
    fn observe_limits(&mut self, host: &str, headers: &str) {
        let mut remaining = None;
        let mut reset = None;
        for line in headers.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            match key.trim().to_ascii_lowercase().as_str() {
                "retry-after" => {
                    if let Ok(seconds) = value.trim().parse::<u64>() {
                        self.cooldown
                            .insert(host.into(), now().saturating_add(seconds));
                    }
                }
                "x-ratelimit-remaining" => remaining = value.trim().parse::<u64>().ok(),
                "x-ratelimit-reset" => reset = value.trim().parse::<u64>().ok(),
                _ => {}
            }
        }
        if remaining == Some(0) {
            self.cooldown
                .insert(host.into(), reset.unwrap_or(now() + 300).max(now() + 1));
        }
    }
    pub async fn fetch(&mut self, ids: &[PrId]) -> Result<Vec<(PrId, Result<PrObservation>)>> {
        let host = &ids.first().context("No PRs requested")?.host;
        let data = self.query(host, base_query(ids)).await?;
        let viewer = identity(&data)?;
        self.identities.insert(host.clone(), viewer.0.clone());
        let mut results = Vec::new();
        let mut own = Vec::new();
        for (index, id) in ids.iter().enumerate() {
            let pr = &data[format!("p{index}")]["pullRequest"];
            match complete_alias(&data, index).and_then(|_| basic(pr, &viewer)) {
                Ok(o)
                    if o.role == PrRole::Implementing
                        && !matches!(o.status, PrStatus::Merged | PrStatus::Closed) =>
                {
                    own.push((id.clone(), o))
                }
                Ok(mut o) if o.role == PrRole::Reviewing && o.status == PrStatus::Waiting => {
                    let result = self
                        .reviewing(host, id, pr, &viewer, &mut o)
                        .await
                        .map(|_| o);
                    results.push((id.clone(), result));
                }
                result => results.push((id.clone(), result)),
            }
        }
        if !own.is_empty() {
            let own_ids: Vec<_> = own.iter().map(|(id, _)| id.clone()).collect();
            match self.query(host, own_query(&own_ids)).await {
                Ok(details) => {
                    if identity(&details)? != viewer {
                        self.identities.insert(host.clone(), identity(&details)?.0);
                        bail!(
                            "GitHub account changed during refresh; retry with the intended account"
                        );
                    }
                    for (index, (id, mut observation)) in own.into_iter().enumerate() {
                        let pr = &details[format!("p{index}")]["pullRequest"];
                        let result = match complete_alias(&details, index) {
                            Ok(()) => self
                                .implementation(host, &id, pr, &viewer, &mut observation)
                                .await
                                .map(|_| observation),
                            Err(error) => Err(error),
                        };
                        results.push((id, result));
                    }
                }
                Err(error) => {
                    for (id, _) in own {
                        results.push((id, Err(anyhow::anyhow!("{error}"))));
                    }
                }
            }
        }
        if self.identities.get(host).is_some_and(|id| id != &viewer.0) {
            bail!("GitHub account changed during refresh; retry with the intended account");
        }
        Ok(results)
    }
    async fn reviewing(
        &mut self,
        host: &str,
        id: &PrId,
        pr: &Value,
        viewer: &(String, String),
        o: &mut PrObservation,
    ) -> Result<()> {
        let mut requests = pr["reviewRequests"].clone();
        let mut pages = 0;
        loop {
            let nodes = requests["nodes"]
                .as_array()
                .context("Missing current review requests")?;
            if nodes.iter().any(|request| {
                request["requestedReviewer"]["__typename"] == "User"
                    && request["requestedReviewer"]["id"].as_str() == Some(&viewer.0)
            }) {
                o.status = if pr["viewerLatestReview"]["submittedAt"].is_string() {
                    PrStatus::ReviewRequestedAgain
                } else {
                    PrStatus::ReviewRequested
                };
                break;
            }
            if !requests["pageInfo"]["hasNextPage"]
                .as_bool()
                .context("Incomplete review requests")?
            {
                break;
            }
            pages += 1;
            if pages > 20 {
                bail!("Too many review request pages; status is incomplete");
            }
            let cursor = required(&requests["pageInfo"], "endCursor")?;
            let query = format!(
                "query {{ viewer {{ id login }} p0:repository(owner:{},name:{}) {{ pullRequest(number:{}) {{ reviewRequests(first:100,after:{}) {{ {REVIEW_REQUESTS} }} }} }} }}",
                json!(id.owner),
                json!(id.repository),
                id.number,
                json!(cursor)
            );
            let data = self.query(host, query).await?;
            if identity(&data)? != *viewer {
                self.identities.insert(host.into(), identity(&data)?.0);
                bail!("GitHub account changed during refresh");
            }
            complete_alias(&data, 0)?;
            requests = data["p0"]["pullRequest"]["reviewRequests"].clone();
        }
        o.reasons.push(o.status.label().into());
        Ok(())
    }
    async fn implementation(
        &mut self,
        host: &str,
        id: &PrId,
        pr: &Value,
        viewer: &(String, String),
        o: &mut PrObservation,
    ) -> Result<()> {
        let current = basic(pr, viewer)?;
        if current.role != PrRole::Implementing {
            bail!("PR author changed during refresh");
        }
        *o = current;
        if matches!(o.status, PrStatus::Merged | PrStatus::Closed) {
            return Ok(());
        }
        o.head = Some(required(pr, "headRefName")?.into());
        o.base = Some(required(pr, "baseRefName")?.into());
        o.head_sha = Some(required(pr, "headRefOid")?.into());
        let head = &pr["commits"]["nodes"][0]["commit"];
        if head["oid"].as_str() != o.head_sha.as_deref() {
            bail!("PR head changed or commit data is incomplete; retrying");
        }
        // GitHub may evaluate a test merge as well as the head. Include both;
        // mergeStateStatus remains the authority on policy satisfaction.
        for commit in [head, &pr["potentialMergeCommit"]] {
            if commit.is_null() {
                continue;
            }
            let rollup = &commit["statusCheckRollup"];
            if rollup.is_null() {
                continue;
            }
            let mut contexts = rollup["contexts"].clone();
            let mut pages = 0;
            loop {
                let nodes = contexts["nodes"]
                    .as_array()
                    .context("Incomplete check results")?;
                for check in nodes {
                    let parsed = parse_check(check)?;
                    if matches!(
                        parsed.result.as_str(),
                        "QUEUED" | "IN_PROGRESS" | "WAITING" | "PENDING" | "REQUESTED"
                    ) {
                        o.ci_running = true;
                    }
                    o.checks.push(parsed);
                }
                if !contexts["pageInfo"]["hasNextPage"]
                    .as_bool()
                    .context("Missing check pagination")?
                {
                    break;
                }
                pages += 1;
                if pages > 20 {
                    bail!("Too many check pages; status is incomplete");
                }
                let cursor = required(&contexts["pageInfo"], "endCursor")?;
                let query = format!(
                    "query {{ viewer {{ id login }} node(id:{}) {{ ... on Commit {{ statusCheckRollup {{ contexts(first:100,after:{}) {{ {} }} }} }} }} }}",
                    json!(required(commit, "id")?),
                    json!(cursor),
                    check_fields(id.number)
                );
                let page = self.query(host, query).await?;
                if identity(&page)? != *viewer {
                    self.identities.insert(host.into(), identity(&page)?.0);
                    bail!("GitHub account changed during check pagination");
                }
                contexts = page["node"]["statusCheckRollup"]["contexts"].clone();
            }
        }
        classify_implementation(pr, o)?;
        Ok(())
    }
}
fn complete_alias(data: &Value, index: usize) -> Result<()> {
    if data["_errors"].as_array().is_some_and(|errors| {
        errors.iter().any(|e| {
            e["path"][0]
                .as_str()
                .is_none_or(|path| path == format!("p{index}"))
        })
    }) {
        bail!("GitHub returned incomplete PR data; check repository and checks permissions");
    }
    Ok(())
}
fn required<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .filter(|v| !v.is_empty())
        .with_context(|| format!("Incomplete GitHub response: {field}"))
}
fn identity(data: &Value) -> Result<(String, String)> {
    Ok((
        required(&data["viewer"], "id")?.into(),
        required(&data["viewer"], "login")?.into(),
    ))
}
fn basic(pr: &Value, viewer: &(String, String)) -> Result<PrObservation> {
    if pr.is_null() {
        bail!("PR unavailable or access denied; check its URL and gh account");
    }
    let author = &pr["author"];
    let role = if required(author, "id")? == viewer.0 {
        PrRole::Implementing
    } else {
        PrRole::Reviewing
    };
    let status = match required(pr, "state")? {
        "MERGED" => PrStatus::Merged,
        "CLOSED" => PrStatus::Closed,
        "OPEN" => PrStatus::Waiting,
        _ => bail!("Unknown PR lifecycle"),
    };
    let observation = PrObservation {
        title: required(pr, "title")?.into(),
        author: required(author, "login")?.into(),
        viewer: viewer.1.clone(),
        viewer_id: viewer.0.clone(),
        role,
        status,
        reasons: vec![],
        checks: vec![],
        review: None,
        head: None,
        head_sha: None,
        base: None,
        ci_running: false,
        observed_at: now(),
    };
    Ok(observation)
}
fn parse_check(check: &Value) -> Result<PrCheck> {
    let (name, result, url) = match required(check, "__typename")? {
        "CheckRun" => (
            required(check, "name")?,
            if required(check, "status")? == "COMPLETED" {
                required(check, "conclusion")?
            } else {
                required(check, "status")?
            },
            check["detailsUrl"].as_str(),
        ),
        "StatusContext" => (
            required(check, "context")?,
            required(check, "state")?,
            check["targetUrl"].as_str(),
        ),
        _ => bail!("Unknown GitHub check type"),
    };
    Ok(PrCheck {
        name: name.into(),
        result: result.into(),
        url: url
            .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
            .map(str::to_owned),
        required: check["isRequired"]
            .as_bool()
            .context("Missing check requirements")?,
    })
}
#[derive(PartialEq)]
enum MergeState {
    Clean,
    Blocked,
    Behind,
    Dirty,
    Unstable,
    Unknown,
    Other,
}
impl From<&str> for MergeState {
    fn from(value: &str) -> Self {
        match value {
            "CLEAN" => Self::Clean,
            "BLOCKED" => Self::Blocked,
            "BEHIND" => Self::Behind,
            "DIRTY" => Self::Dirty,
            "UNSTABLE" => Self::Unstable,
            "UNKNOWN" => Self::Unknown,
            _ => Self::Other,
        }
    }
}
fn classify_implementation(pr: &Value, o: &mut PrObservation) -> Result<()> {
    let merge = MergeState::from(required(pr, "mergeStateStatus")?);
    let mergeable = required(pr, "mergeable")?;
    let review = pr
        .get("reviewDecision")
        .context("Missing review decision")?
        .as_str()
        .unwrap_or("");
    pr.get("mergeQueueEntry")
        .context("Missing merge queue state")?;
    if !["", "APPROVED", "CHANGES_REQUESTED", "REVIEW_REQUIRED"].contains(&review) {
        bail!("Unknown review decision");
    }
    o.review = Some(match review {
        "APPROVED" => PrReview::Approved,
        "CHANGES_REQUESTED" => PrReview::ChangesRequested,
        "REVIEW_REQUIRED" => PrReview::Required,
        _ => PrReview::NotRequired,
    });
    let draft = pr["isDraft"].as_bool().context("Missing draft state")?;
    let required_failure = o.checks.iter().any(|c| {
        c.required
            && [
                "FAILURE",
                "ERROR",
                "CANCELLED",
                "TIMED_OUT",
                "ACTION_REQUIRED",
                "STALE",
                "STARTUP_FAILURE",
            ]
            .contains(&c.result.as_str())
    });
    if review == "CHANGES_REQUESTED" {
        o.reasons.push("Changes requested".into());
    }
    if required_failure {
        o.reasons.push("CI failed".into());
    }
    if merge == MergeState::Dirty || mergeable == "CONFLICTING" {
        o.reasons.push("Merge conflicts".into());
    }
    if merge == MergeState::Behind {
        o.reasons.push("Branch update required".into());
    }
    o.status = if !o.reasons.is_empty() {
        PrStatus::NeedsAttention
    } else if draft {
        PrStatus::Draft
    } else if !pr["mergeQueueEntry"].is_null() {
        PrStatus::Queued
    } else if (merge == MergeState::Clean
        || (merge == MergeState::Unstable
            && o.checks.iter().any(|c| {
                !c.required && !["SUCCESS", "NEUTRAL", "SKIPPED"].contains(&c.result.as_str())
            })
            && o.checks
                .iter()
                .filter(|c| c.required)
                .all(|c| ["SUCCESS", "NEUTRAL", "SKIPPED"].contains(&c.result.as_str()))))
        && mergeable == "MERGEABLE"
        && review != "REVIEW_REQUIRED"
    {
        PrStatus::Ready
    } else if o.ci_running {
        PrStatus::Running
    } else if review == "REVIEW_REQUIRED" {
        PrStatus::WaitingReview
    } else if merge == MergeState::Blocked {
        o.reasons
            .push("Merge blocked — check GitHub requirements".into());
        PrStatus::NeedsAttention
    } else if merge == MergeState::Unstable {
        o.reasons
            .push("Checks are not passing — check GitHub requirements".into());
        PrStatus::Unknown
    } else {
        PrStatus::Unknown
    };
    if o.reasons.is_empty() {
        o.reasons.push(o.status.label().into());
    }
    Ok(())
}
const REVIEW_REQUESTS: &str = "pageInfo { hasNextPage endCursor } nodes { requestedReviewer { __typename ... on User { id } } }";
const COMMON: &str = "title state author { login ... on User { id } ... on Bot { id } } reviewRequests(first:100) { pageInfo { hasNextPage endCursor } nodes { requestedReviewer { __typename ... on User { id } } } } viewerLatestReview { submittedAt }";
pub fn base_query(ids: &[PrId]) -> String {
    let parts: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            format!(
                "p{i}:repository(owner:{},name:{}) {{ pullRequest(number:{}) {{ {COMMON} }} }}",
                json!(id.owner),
                json!(id.repository),
                id.number
            )
        })
        .collect();
    format!("query {{ viewer {{ id login }} {} }}", parts.join(" "))
}
fn check_fields(number: u64) -> String {
    format!(
        "pageInfo {{ hasNextPage endCursor }} nodes {{ __typename ... on CheckRun {{ name status conclusion detailsUrl isRequired(pullRequestNumber:{number}) }} ... on StatusContext {{ context state targetUrl isRequired(pullRequestNumber:{number}) }} }}"
    )
}
fn own_query(ids: &[PrId]) -> String {
    let parts:Vec<_> = ids.iter().enumerate().map(|(i,id)| {
        let commit = format!("id oid statusCheckRollup {{ contexts(first:100) {{ {} }} }}",check_fields(id.number));
        format!("p{i}:repository(owner:{},name:{}) {{ pullRequest(number:{}) {{ {COMMON} isDraft mergeable mergeStateStatus reviewDecision mergeQueueEntry {{ state }} headRefName headRefOid baseRefName commits(last:1) {{ nodes {{ commit {{ {commit} }} }} }} potentialMergeCommit {{ {commit} }} }} }}",json!(id.owner),json!(id.repository),id.number)
    }).collect();
    format!("query {{ viewer {{ id login }} {} }}", parts.join(" "))
}
