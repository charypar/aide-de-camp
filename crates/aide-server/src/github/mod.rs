mod client;
mod polling;
#[cfg(test)]
mod tests;

use aide_core::{Lifecycle, PrId, PrView, Status, TaskView};
use anyhow::{Result, bail};
pub use polling::Github;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn parse_url(input: &str) -> Result<PrId> {
    let url = url::Url::parse(input.trim())?;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !host.contains('.')
        || host.parse::<std::net::IpAddr>().is_ok()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        bail!("Paste an HTTPS GitHub PR URL, such as https://github.com/owner/repo/pull/123");
    }
    let parts: Vec<_> = url
        .path()
        .trim_end_matches('/')
        .split('/')
        .skip(1)
        .collect();
    if parts.len() < 4
        || parts[2] != "pull"
        || parts.len() > 5
        || (parts.len() == 5 && !["files", "commits", "checks"].contains(&parts[4]))
    {
        bail!("Paste a GitHub pull request URL");
    }
    for name in &parts[..2] {
        if name.is_empty()
            || *name == "."
            || *name == ".."
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            bail!("Invalid GitHub repository in URL");
        }
    }
    let number: u64 = parts[3].parse()?;
    if number == 0 || number > i32::MAX as u64 {
        bail!("Invalid PR number");
    }
    Ok(PrId {
        host,
        owner: parts[0].to_ascii_lowercase(),
        repository: parts[1].to_ascii_lowercase(),
        number,
    })
}

pub fn apply(view: &mut TaskView, prs: Vec<PrView>) {
    view.native_status = Some(view.status.clone());
    view.pull_requests = prs;
    if view.task.lifecycle != Lifecycle::Open || view.status == Status::Starting {
        return;
    }
    let reasons: Vec<_> = view
        .pull_requests
        .iter()
        .filter_map(|pr| {
            let o = pr.observation.as_ref()?;
            if !o.status.actionable() {
                return None;
            }
            let reason = if o.reasons.is_empty() {
                o.status.label().to_owned()
            } else {
                o.reasons.join(", ")
            };
            Some(format!(
                "{}: {}{}",
                pr.id.label(),
                reason,
                if pr.stale { " (stale)" } else { "" }
            ))
        })
        .collect();
    if !reasons.is_empty() {
        view.status = Status::Attention;
        view.reason = format!("{} · {}", view.reason, reasons.join(" · "));
    }
}
