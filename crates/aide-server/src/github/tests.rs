use super::*;

#[test]
fn canonical_urls_and_invalid_input() {
    assert_eq!(
        parse_url(" https://github.com/Owner/Repo/pull/42/files#diff ")
            .unwrap()
            .url(),
        "https://github.com/owner/repo/pull/42"
    );
    for url in [
        "http://github.com/o/r/pull/1",
        "https://evil@github.com/o/r/pull/1",
        "https://127.0.0.1/o/r/pull/1",
        "https://github.com/o/r/issues/1",
        "https://github.com/o/r/pull/0",
        "https://github.com/o/r/pull/1/unknown",
    ] {
        assert!(parse_url(url).is_err(), "{url}");
    }
}

use crate::database::Project;
use aide_core::{PrObservation, PrRole, PrStatus, Task};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub(super) fn task(id: &str) -> Task {
    Task {
        id: id.into(),
        key: format!("{id:0>32}"),
        repository: "repo".into(),
        workspace: "/fixture".into(),
        lifecycle: Lifecycle::Open,
        desired_running: false,
        thread_id: None,
        native_path: None,
        recording: None,
        recovered: false,
        error: None,
        created: 0,
    }
}
pub(super) fn observation(status: PrStatus) -> PrObservation {
    PrObservation {
        title: "Example".into(),
        author: "me".into(),
        viewer: "me".into(),
        viewer_id: "ME".into(),
        role: PrRole::Implementing,
        status,
        reasons: vec![status.label().into()],
        checks: vec![],
        review: None,
        head: None,
        head_sha: None,
        base: None,
        ci_running: status == PrStatus::Running,
        observed_at: now(),
    }
}
pub(super) fn link(number: u64, status: PrStatus) -> PrView {
    PrView {
        id: parse_url(&format!("https://github.com/o/r/pull/{number}")).unwrap(),
        observation: Some(observation(status)),
        last_attempt: now(),
        error: None,
        stale: false,
        historical: false,
    }
}
pub(super) fn write(path: &Path, value: Value) {
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}
fn fixture(value: Value) -> (tempfile::TempDir, client::Client) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("github.json");
    write(&path, value);
    let client = client::Client {
        fixture: Some(path),
        ..Default::default()
    };
    (dir, client)
}
#[tokio::test]
async fn roles_statuses_and_policy_are_read_from_the_api() {
    let (_dir, mut client) = fixture(json!({"prs":{
        "1":{"status":"ready"},"2":{"status":"waiting"},"3":{"status":"running"},"4":{"status":"failed"},
        "5":{"status":"feedback"},"6":{"status":"optional_failed"},"7":{"status":"conflict"},"8":{"status":"draft"},
        "9":{"status":"queued"},"10":{"status":"unknown"},"11":{"status":"merged"},"12":{"status":"ready","no_checks":true},
        "100":{"status":"requested"},"101":{"status":"rerequest"},"102":{"status":"waiting"},"103":{"status":"closed"}
    }}));
    let cases = [
        (1, PrStatus::Ready),
        (2, PrStatus::WaitingReview),
        (3, PrStatus::Running),
        (4, PrStatus::NeedsAttention),
        (5, PrStatus::NeedsAttention),
        (6, PrStatus::Ready),
        (7, PrStatus::NeedsAttention),
        (8, PrStatus::Draft),
        (9, PrStatus::Queued),
        (10, PrStatus::Unknown),
        (11, PrStatus::Merged),
        (12, PrStatus::Ready),
        (100, PrStatus::ReviewRequested),
        (101, PrStatus::ReviewRequestedAgain),
        (102, PrStatus::Waiting),
        (103, PrStatus::Closed),
    ];
    let ids: Vec<_> = cases
        .iter()
        .map(|(n, _)| link(*n, PrStatus::Unknown).id)
        .collect();
    let results = client.fetch(&ids).await.unwrap();
    for (number, status) in cases {
        let o = results
            .iter()
            .find(|(id, _)| id.number == number)
            .unwrap()
            .1
            .as_ref()
            .unwrap();
        assert_eq!(o.status, status, "PR {number}");
        if number >= 100 {
            assert_eq!(o.role, PrRole::Reviewing);
            assert!(o.checks.is_empty());
            assert!(o.head.is_none());
        }
        assert_eq!(o.interval(), if number == 3 { 60 } else { 300 });
    }
    let log =
        fs::read_to_string(client.fixture.as_ref().unwrap().with_extension("json.log")).unwrap();
    assert_eq!(log.lines().count(), 2);
    let own_query: Value = serde_json::from_str(log.lines().nth(1).unwrap()).unwrap();
    assert!(!own_query["query"].as_str().unwrap().contains("number:100"));
}
#[tokio::test]
async fn partial_responses_are_isolated_and_paginated_checks_can_block_readiness() {
    let (_dir, mut client) = fixture(
        json!({"prs":{"1":{"status":"inaccessible"},"2":{"partial":true},"3":{"paginated":true},"4":{}}}),
    );
    let ids: Vec<_> = (1..=4).map(|n| link(n, PrStatus::Ready).id).collect();
    let results = client.fetch(&ids).await.unwrap();
    for n in 1..=2 {
        assert!(
            results
                .iter()
                .find(|(id, _)| id.number == n)
                .unwrap()
                .1
                .is_err()
        );
    }
    let paginated = results
        .iter()
        .find(|(id, _)| id.number == 3)
        .unwrap()
        .1
        .as_ref()
        .unwrap();
    assert_eq!(paginated.status, PrStatus::NeedsAttention);
    assert_eq!(paginated.checks.len(), 2);
    assert_eq!(
        results
            .iter()
            .find(|(id, _)| id.number == 4)
            .unwrap()
            .1
            .as_ref()
            .unwrap()
            .status,
        PrStatus::Ready
    );
}
#[tokio::test]
async fn authentication_rate_limits_and_malformed_responses_are_not_ci_failures() {
    for (state, message) in [
        (json!({"error":401}), "authentication"),
        (json!({"error":429}), "rate limit"),
        (json!({"malformed":true}), "incomplete"),
    ] {
        let (_dir, mut client) = fixture(state);
        let id = link(1, PrStatus::Ready).id;
        let error = client.fetch(std::slice::from_ref(&id)).await.unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
        assert!(client.cooldown["github.com"] > now());
        assert!(
            client
                .fetch(&[id])
                .await
                .unwrap_err()
                .to_string()
                .contains("paused")
        );
    }
}
#[test]
fn attention_aggregation_preserves_native_state_and_lifecycle() {
    let base = TaskView {
        task: task("one"),
        status: Status::Working,
        reason: "Codex working".into(),
        terminal_alive: true,
        backend_alive: true,
        summary: None,
        terminal_activity: None,
        pull_requests: vec![],
        native_status: None,
    };
    let mut view = base.clone();
    let mut stale = link(2, PrStatus::NeedsAttention);
    stale.stale = true;
    apply(&mut view, vec![link(1, PrStatus::Ready), stale]);
    assert_eq!(view.status, Status::Attention);
    assert_eq!(view.native_status, Some(Status::Working));
    assert!(view.reason.contains("Codex working"));
    assert!(view.reason.contains("(stale)"));
    let mut view = base.clone();
    apply(&mut view, vec![link(1, PrStatus::Merged)]);
    assert_eq!(view.status, Status::Working);
    let mut view = base.clone();
    view.status = Status::Attention;
    apply(&mut view, vec![]);
    assert_eq!(view.status, Status::Attention);
    let mut starting = base.clone();
    starting.status = Status::Starting;
    apply(&mut starting, vec![link(1, PrStatus::Ready)]);
    assert_eq!(starting.status, Status::Starting);
    for lifecycle in [Lifecycle::Closing, Lifecycle::Completed] {
        let mut view = base.clone();
        view.task.lifecycle = lifecycle;
        view.status = Status::Closing;
        apply(&mut view, vec![link(1, PrStatus::Ready)]);
        assert_eq!(view.status, Status::Closing);
    }
}
#[test]
fn links_survive_old_worker_writes_and_do_not_resurrect_after_unlink_or_cleanup() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let project = Project::init(dir.path())?;
    let task = task("one");
    project.save(&task)?;
    let mut pr = link(1, PrStatus::Ready);
    assert!(project.link_pr("one", &pr)?);
    assert!(!project.link_pr("one", &pr)?);
    project.save(&task)?;
    assert_eq!(Project::open(dir.path())?.pull_requests("one")?.len(), 1);
    project.unlink_pr("one", &pr.id)?;
    project.observe_pr("one", &pr)?;
    assert!(project.pull_requests("one")?.is_empty());
    project.link_pr("one", &pr)?;
    project.update("one", |t| t.lifecycle = Lifecycle::Completed)?;
    pr.observation = Some(observation(PrStatus::Merged));
    project.observe_pr("one", &pr)?;
    assert_eq!(
        project.pull_requests("one")?[0]
            .observation
            .as_ref()
            .unwrap()
            .status,
        PrStatus::Ready
    );
    assert!(project.unlink_pr("one", &pr.id).is_err());
    assert!(project.link_pr("one", &pr).is_err());
    Ok(())
}

#[tokio::test]
#[ignore = "requires a valid gh login and AIDE_TEST_PR_URL pointing to an existing PR"]
async fn live_github_read_only() -> Result<()> {
    let id = parse_url(&std::env::var("AIDE_TEST_PR_URL")?)?;
    let mut client = client::Client::default();
    let results = client.fetch(&[id]).await?;
    for (id, result) in results {
        let o = result?;
        println!(
            "{}: {} · {} · {} checks (viewer @{})",
            id.label(),
            o.role.label(),
            o.status.label(),
            o.checks.len(),
            o.viewer
        );
    }
    Ok(())
}

#[tokio::test]
async fn only_direct_current_requests_create_review_work_even_across_pages() {
    let (_dir, mut client) = fixture(json!({"prs":{
        "100":{"status":"requested","team_only":true},
        "101":{"status":"rerequest","review_paginated":true}
    }}));
    let results = client
        .fetch(&[
            link(100, PrStatus::Unknown).id,
            link(101, PrStatus::Unknown).id,
        ])
        .await
        .unwrap();
    assert_eq!(results[0].1.as_ref().unwrap().status, PrStatus::Waiting);
    assert_eq!(
        results[1].1.as_ref().unwrap().status,
        PrStatus::ReviewRequestedAgain
    );
    write(
        client.fixture.as_ref().unwrap(),
        json!({"prs":{"101":{"status":"waiting"}}}),
    );
    let results = client
        .fetch(&[link(101, PrStatus::Unknown).id])
        .await
        .unwrap();
    assert_eq!(results[0].1.as_ref().unwrap().status, PrStatus::Waiting);
}

#[tokio::test]
async fn test_merge_checks_take_precedence_over_head_checks() {
    let (_dir, mut client) = fixture(json!({"prs":{
        "1":{"status":"failed","merge_check":"SUCCESS"},
        "2":{"status":"ready","merge_check":"FAILURE"},
        "3":{"status":"failed","merge_check":"EMPTY","merge_state":"BLOCKED"},
        "4":{"status":"failed","merge_check":"NONE","merge_state":"BLOCKED"},
        "5":{"status":"running","merge_check":"SUCCESS"},
        "6":{"status":"failed","merge_check":"IN_PROGRESS"},
        "7":{"status":"ready","merge_check":"SUCCESS","merge_paginated":true},
        "8":{"status":"ready","merge_check":"MALFORMED"},
        "9":{"status":"failed","paginated":true,"merge_check":"SUCCESS"}
    }}));
    let ids: Vec<_> = (1..=9).map(|n| link(n, PrStatus::Unknown).id).collect();
    let results = client.fetch(&ids).await.unwrap();
    for (number, status, source, running) in [
        (1, PrStatus::Ready, "merge-build", false),
        (2, PrStatus::NeedsAttention, "merge-build", false),
        (3, PrStatus::NeedsAttention, "build", false),
        (4, PrStatus::NeedsAttention, "build", false),
        (5, PrStatus::Ready, "merge-build", false),
        (6, PrStatus::Running, "merge-build", true),
        (7, PrStatus::NeedsAttention, "merge-build", false),
        (9, PrStatus::Ready, "merge-build", false),
    ] {
        let observation = results
            .iter()
            .find(|(id, _)| id.number == number)
            .unwrap()
            .1
            .as_ref()
            .unwrap();
        assert_eq!(observation.status, status, "PR {number}");
        assert_eq!(observation.checks[0].name, source, "PR {number}");
        assert_eq!(observation.ci_running, running, "PR {number}");
        assert_eq!(observation.interval(), if running { 60 } else { 300 });
        assert_eq!(observation.checks.len(), if number == 7 { 2 } else { 1 });
    }
    assert!(
        results
            .iter()
            .find(|(id, _)| id.number == 8)
            .unwrap()
            .1
            .is_err()
    );
    let log =
        fs::read_to_string(client.fixture.as_ref().unwrap().with_extension("json.log")).unwrap();
    let pagination: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|v: &Value| v["query"].as_str().unwrap().contains("node(id:"))
        .collect();
    assert_eq!(pagination.len(), 1);
    assert!(
        pagination[0]["query"]
            .as_str()
            .unwrap()
            .contains("node(id:\"merge\")")
    );
}
