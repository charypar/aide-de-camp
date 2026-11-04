use super::{apply, client::Client, now, parse_url};
use crate::database::{Home, Project};
use aide_core::{Lifecycle, PrId, PrView, TaskView};
use anyhow::{Result, bail};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Default)]
pub struct Github {
    network: Arc<tokio::sync::Mutex<Poller>>,
    verified: Arc<Mutex<BTreeSet<PrId>>>,
    identities: Arc<Mutex<BTreeMap<String, String>>>,
    diagnostics: Arc<Mutex<Diagnostics>>,
}
#[derive(Default)]
struct Poller {
    client: Client,
    due: BTreeMap<PrId, u64>,
    identities: BTreeMap<String, String>,
}
#[derive(Clone)]
struct Link {
    root: String,
    task: String,
    view: PrView,
}

type LinkKey = (String, String, PrId);

#[derive(Default)]
struct Diagnostics {
    registry: Option<String>,
    scans: BTreeMap<String, String>,
    writes: BTreeMap<LinkKey, String>,
}
impl Link {
    fn key(&self) -> LinkKey {
        (self.root.clone(), self.task.clone(), self.view.id.clone())
    }
}

impl Github {
    fn root_key(root: &str) -> String {
        Path::new(root)
            .canonicalize()
            .unwrap_or_else(|_| root.into())
            .display()
            .to_string()
    }
    fn links(&self, home: &Home) -> Result<Vec<Link>> {
        let roots = match home.roots() {
            Ok(roots) => {
                self.diagnostics.lock().unwrap().registry = None;
                roots
                    .into_iter()
                    .map(|root| Self::root_key(&root))
                    .collect::<Vec<_>>()
            }
            Err(error) => {
                self.diagnostics.lock().unwrap().registry =
                    Some(format!("Cannot read project registry: {error}"));
                return Err(error);
            }
        };
        let mut links = Vec::new();
        let mut errors = BTreeMap::new();
        let mut scanned = BTreeSet::new();
        for root in &roots {
            let result = Project::open(Path::new(root)).and_then(|project| {
                let records = project.all_pull_requests()?;
                Ok((project, records))
            });
            let (project, records) = match result {
                Ok(result) => result,
                Err(error) => {
                    errors.insert(root.clone(), format!("Cannot read PR links: {error}"));
                    continue;
                }
            };
            scanned.insert(root.clone());
            let mut problems = Vec::new();
            for (task, record) in records {
                match record.and_then(|view| project.task(&task).map(|t| (t, view))) {
                    Ok((record, view)) if record.lifecycle == Lifecycle::Open => {
                        links.push(Link {
                            root: root.clone(),
                            task,
                            view,
                        });
                    }
                    Ok(_) => {}
                    Err(error) => problems.push(format!("Task {task}: {error}")),
                }
            }
            if !problems.is_empty() {
                errors.insert(root.clone(), problems.join("; "));
            }
        }
        let keys: BTreeSet<_> = links.iter().map(Link::key).collect();
        let mut diagnostics = self.diagnostics.lock().unwrap();
        diagnostics.scans = errors;
        diagnostics.writes.retain(|key, _| {
            roots.contains(&key.0) && (!scanned.contains(&key.0) || keys.contains(key))
        });
        Ok(links)
    }
    pub fn project_error(&self, root: &str) -> Option<String> {
        let root = Self::root_key(root);
        let diagnostics = self.diagnostics.lock().unwrap();
        let mut messages = Vec::new();
        messages.extend(diagnostics.registry.iter().cloned());
        messages.extend(diagnostics.scans.get(&root).cloned());
        for ((project, task, id), error) in &diagnostics.writes {
            if project == &root {
                messages.push(format!("Task {task}, {}: {error}", id.label()));
            }
        }
        (!messages.is_empty()).then(|| format!("GitHub monitoring: {}", messages.join("; ")))
    }
    fn store_view(&self, link: &Link, view: &PrView) -> Result<()> {
        let result = Project::open(Path::new(&link.root))
            .and_then(|project| project.observe_pr(&link.task, view));
        let mut diagnostics = self.diagnostics.lock().unwrap();
        match &result {
            Ok(()) => {
                diagnostics.writes.remove(&link.key());
            }
            Err(error) => {
                diagnostics
                    .writes
                    .insert(link.key(), format!("Could not save PR status: {error}"));
            }
        }
        result
    }
    pub fn decorate(&self, project: &Project, view: &mut TaskView) {
        match project.pull_requests(&view.task.id) {
            Ok(mut links) => {
                let verified = self.verified.lock().unwrap();
                for link in &mut links {
                    let key = (
                        project.root.display().to_string(),
                        view.task.id.clone(),
                        link.id.clone(),
                    );
                    if let Some(error) = self.diagnostics.lock().unwrap().writes.get(&key) {
                        link.error = Some(error.clone());
                    }
                    if let Some(viewer) = self.identities.lock().unwrap().get(&link.id.host)
                        && link
                            .observation
                            .as_ref()
                            .is_some_and(|o| &o.viewer_id != viewer)
                        && view.task.lifecycle == Lifecycle::Open
                    {
                        link.observation = None;
                        link.error = Some(
                            "GitHub account changed; refreshing for the active account".into(),
                        );
                    }
                    link.historical = view.task.lifecycle == Lifecycle::Completed;
                    link.stale = !link.historical
                        && (link.error.is_some()
                            || !verified.contains(&link.id)
                            || link.observation.as_ref().is_none_or(|o| {
                                now() > o.observed_at.saturating_add(o.interval() + 30)
                            }));
                }
                apply(view, links);
            }
            Err(_) => view.reason.push_str(" · PR links could not be loaded"),
        }
    }
    pub async fn run(&self, home: Home) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_error = None;
        loop {
            tick.tick().await;
            let error = self.poll(&home).await.err().map(|error| error.to_string());
            if error != last_error
                && let Some(message) = &error
            {
                eprintln!("GitHub polling: {message}");
            }
            last_error = error;
        }
    }
    async fn poll(&self, home: &Home) -> Result<()> {
        let Ok(mut poller) = self.network.try_lock() else {
            return Ok(());
        };
        let links = self.links(home)?;
        let ids: BTreeSet<_> = links.iter().map(|l| l.view.id.clone()).collect();
        poller.due.retain(|id, _| ids.contains(id));
        self.verified.lock().unwrap().retain(|id| ids.contains(id));
        let time = now();
        let due: Vec<_> = ids
            .into_iter()
            .filter(|id| {
                poller.due.get(id).is_none_or(|t| *t <= time)
                    && poller
                        .client
                        .cooldown
                        .get(&id.host)
                        .is_none_or(|t| *t <= time)
            })
            .collect();
        // One host batch per tick bounds API/subprocess work independently of browser count.
        if let Some(first) = due.first() {
            let ids: Vec<_> = due
                .iter()
                .filter(|id| id.host == first.host)
                .take(20)
                .cloned()
                .collect();
            self.refresh_ids(home, &mut poller, &ids).await?;
        }
        Ok(())
    }
    async fn refresh_ids(&self, home: &Home, poller: &mut Poller, ids: &[PrId]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let results = match poller.client.fetch(ids).await {
            Ok(results) => results,
            Err(error) => ids
                .iter()
                .cloned()
                .map(|id| (id, Err(anyhow::anyhow!("{error}"))))
                .collect(),
        };
        *self.identities.lock().unwrap() = poller.client.identities.clone();
        let time = now();
        let mut error = None;
        // Read associations again after network I/O. An unlink must never be resurrected.
        let links = self.links(home)?;
        for (host, viewer) in poller.client.identities.clone() {
            let changed = poller
                .identities
                .insert(host.clone(), viewer.clone())
                .is_some_and(|old| old != viewer);
            if changed {
                poller.due.retain(|id, _| id.host != host);
            }
            for link in links.iter().filter(|l| {
                l.view.id.host == host
                    && l.view
                        .observation
                        .as_ref()
                        .is_some_and(|o| o.viewer_id != viewer)
            }) {
                let mut view = link.view.clone();
                view.observation = None;
                view.error =
                    Some("GitHub account changed; refreshing for the active account".into());
                if let Err(e) = self.store_view(link, &view) {
                    error = Some(format!("Could not save PR status: {e}"));
                }
                self.verified.lock().unwrap().remove(&view.id);
                poller.due.remove(&view.id);
            }
        }
        for (id, result) in results {
            // Failed startup requests need a prompt retry, independent of the
            // normal five-minute observation cadence. Host cooldowns still win.
            let interval = result.as_ref().map(|o| o.interval()).unwrap_or(60);
            poller.due.insert(id.clone(), time + interval);
            for link in links.iter().filter(|l| l.view.id == id) {
                // UPDATE only touches an existing, open-task association; it cannot
                // resurrect an unlink. Avoid re-reading unrelated records here.
                let mut view = link.view.clone();
                if view.observation.as_ref().is_some_and(|o| {
                    poller
                        .client
                        .identities
                        .get(&id.host)
                        .is_some_and(|viewer| viewer != &o.viewer_id)
                }) {
                    view.observation = None;
                }
                view.last_attempt = time;
                match &result {
                    Ok(observation) => {
                        view.observation = Some(observation.clone());
                        view.error = None;
                    }
                    Err(e) => {
                        view.error = Some(e.to_string());
                        error = Some(e.to_string());
                    }
                }
                if let Err(e) = self.store_view(link, &view) {
                    error = Some(format!("Could not save PR status: {e}"));
                    poller.due.insert(id.clone(), time + 60);
                }
            }
            if let Err(e) = &result {
                error = Some(e.to_string());
            }
            self.verified.lock().unwrap().insert(id);
        }
        if let Some(error) = error {
            bail!("{error}");
        }
        Ok(())
    }
    pub async fn link(&self, home: &Home, root: &str, task: &str, url: &str) -> Result<String> {
        let id = parse_url(url)?;
        {
            let project = Project::open(Path::new(root))?;
            if project.task(task)?.lifecycle != Lifecycle::Open {
                bail!("Task is not open");
            }
            if project.pull_requests(task)?.iter().any(|p| p.id == id) {
                return Ok("PR already linked".into());
            }
        }
        let mut poller = self.network.lock().await;
        let results = poller.client.fetch(std::slice::from_ref(&id)).await;
        *self.identities.lock().unwrap() = poller.client.identities.clone();
        let mut results = results?;
        let observation = results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("PR unavailable"))?
            .1?;
        let view = PrView {
            id: id.clone(),
            observation: Some(observation),
            last_attempt: now(),
            error: None,
            stale: false,
            historical: false,
        };
        Project::open(Path::new(root))?.link_pr(task, &view)?;
        self.verified.lock().unwrap().insert(id.clone());
        poller.due.insert(
            id.clone(),
            now() + view.observation.as_ref().unwrap().interval(),
        );
        // The requested link is already durable. Failures updating other tasks
        // are reported on their projects and retried without claiming linking failed.
        for link in self
            .links(home)
            .unwrap_or_default()
            .iter()
            .filter(|l| l.view.id.host == id.host)
        {
            if link.view.id == id {
                if self.store_view(link, &view).is_err() {
                    poller.due.insert(id.clone(), now() + 60);
                }
            } else if link
                .view
                .observation
                .as_ref()
                .is_some_and(|o| Some(&o.viewer_id) != poller.client.identities.get(&id.host))
            {
                let mut old = link.view.clone();
                old.observation = None;
                old.error =
                    Some("GitHub account changed; refreshing for the active account".into());
                let _ = self.store_view(link, &old); // Diagnostics survive until a successful write.
                poller.due.remove(&old.id);
            }
        }
        Ok("PR linked".into())
    }
    pub fn unlink(&self, root: &str, task: &str, url: &str) -> Result<String> {
        Project::open(Path::new(root))?.unlink_pr(task, &parse_url(url)?)?;
        Ok("PR unlinked".into())
    }
    pub async fn refresh(&self, home: &Home, root: &str, task: &str) -> Result<String> {
        let ids = {
            let project = Project::open(Path::new(root))?;
            if project.task(task)?.lifecycle != Lifecycle::Open {
                bail!("Task is not open");
            }
            project
                .pull_requests(task)?
                .into_iter()
                .map(|v| v.id)
                .collect::<BTreeSet<_>>()
        };
        let mut poller = self.network.lock().await;
        let mut by_host: BTreeMap<String, Vec<PrId>> = BTreeMap::new();
        for id in ids {
            by_host.entry(id.host.clone()).or_default().push(id);
        }
        let mut errors = Vec::new();
        for ids in by_host.values() {
            for batch in ids.chunks(20) {
                if let Err(e) = self.refresh_ids(home, &mut poller, batch).await {
                    errors.push(e.to_string());
                }
            }
        }
        if !errors.is_empty() {
            bail!("{}", errors.join("; "));
        }
        Ok("PR statuses refreshed".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::tests::{link, task, write};
    use aide_core::PrStatus;
    use serde_json::json;

    #[tokio::test]
    async fn shared_polling_restart_cadence_identity_and_history() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = Home::new(Some(dir.path().join("home")))?;
        let root = dir.path().join("project");
        std::fs::create_dir(&root)?;
        let project = Project::init(&root)?;
        home.register(&root)?;
        for id in ["one", "two", "history"] {
            project.save(&task(id))?;
            project.link_pr(id, &link(1, PrStatus::Ready))?;
        }
        project.link_pr("history", &link(2, PrStatus::Ready))?;
        project.update("history", |t| t.lifecycle = Lifecycle::Completed)?;
        let fixture = dir.path().join("github.json");
        write(&fixture, json!({"prs":{"1":{"status":"running"}}}));
        let github = Github::default();
        github.network.lock().await.client.fixture = Some(fixture.clone());
        github.poll(&home).await?;
        assert_eq!(
            std::fs::read_to_string(fixture.with_extension("json.log"))?
                .lines()
                .count(),
            2
        );
        assert_eq!(
            project.pull_requests("one")?[0]
                .observation
                .as_ref()
                .unwrap()
                .status,
            PrStatus::Running
        );
        assert_eq!(
            project.pull_requests("two")?[0],
            project.pull_requests("one")?[0]
        );
        assert_eq!(
            project.pull_requests("history")?[0]
                .observation
                .as_ref()
                .unwrap()
                .status,
            PrStatus::Ready
        );
        let id = link(1, PrStatus::Ready).id;
        let due = github.network.lock().await.due[&id];
        assert!((now() + 59..=now() + 60).contains(&due));
        github.poll(&home).await?;
        assert_eq!(
            std::fs::read_to_string(fixture.with_extension("json.log"))?
                .lines()
                .count(),
            2
        );
        // Startup ignores a recent successful persisted observation.
        let restarted = Github::default();
        restarted.network.lock().await.client.fixture = Some(fixture.clone());
        write(&fixture, json!({"prs":{"1":{"status":"ready"}}}));
        restarted.poll(&home).await?;
        assert_eq!(
            std::fs::read_to_string(fixture.with_extension("json.log"))?
                .lines()
                .count(),
            4
        );
        assert!((now() + 299..=now() + 300).contains(&restarted.network.lock().await.due[&id]));
        // A new active identity changes a formerly owned PR to review-only; no CI query.
        write(
            &fixture,
            json!({"viewer":{"id":"NEW","login":"new"},"prs":{"1":{"status":"requested"}}}),
        );
        restarted
            .refresh(&home, root.to_str().unwrap(), "one")
            .await?;
        let o = project.pull_requests("one")?[0]
            .observation
            .clone()
            .unwrap();
        assert_eq!(o.role, aide_core::PrRole::Reviewing);
        assert!(o.checks.is_empty());
        assert_eq!(o.status, PrStatus::ReviewRequested);
        assert_eq!(
            std::fs::read_to_string(fixture.with_extension("json.log"))?
                .lines()
                .count(),
            5
        );
        project.unlink_pr("one", &id)?;
        assert_eq!(restarted.links(&home)?.len(), 1);
        project.unlink_pr("two", &id)?;
        restarted.poll(&home).await?;
        assert!(restarted.network.lock().await.due.is_empty());
        Ok(())
    }
    #[tokio::test]
    async fn failed_refresh_retains_stale_action_and_recovers() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = Home::new(Some(dir.path().join("home")))?;
        let root = dir.path().join("project");
        std::fs::create_dir(&root)?;
        let project = Project::init(&root)?;
        home.register(&root)?;
        project.save(&task("one"))?;
        project.link_pr("one", &link(1, PrStatus::Ready))?;
        let fixture = dir.path().join("github.json");
        write(&fixture, json!({"error":401}));
        let github = Github::default();
        github.network.lock().await.client.fixture = Some(fixture.clone());
        assert!(github.poll(&home).await.is_err());
        let view = &project.pull_requests("one")?[0];
        assert!(view.error.is_some());
        assert_eq!(view.observation.as_ref().unwrap().status, PrStatus::Ready);
        let id = view.id.clone();
        assert!((now() + 59..=now() + 60).contains(&github.network.lock().await.due[&id]));
        write(&fixture, json!({"prs":{"1":{"status":"merged"}}}));
        // Even a due retry must respect host backoff. Then simulate its expiry
        // and recover through background polling, without a manual refresh.
        github.network.lock().await.due.insert(id, now());
        github.poll(&home).await?;
        assert!(project.pull_requests("one")?[0].error.is_some());
        github.network.lock().await.client.cooldown.clear();
        github.poll(&home).await?;
        let view = &project.pull_requests("one")?[0];
        assert!(view.error.is_none());
        assert_eq!(view.observation.as_ref().unwrap().status, PrStatus::Merged);
        Ok(())
    }
    #[tokio::test]
    async fn three_hundred_tasks_use_bounded_deduplicated_batches() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = Home::new(Some(dir.path().join("home")))?;
        let root = dir.path().join("project");
        std::fs::create_dir(&root)?;
        let project = Project::init(&root)?;
        home.register(&root)?;
        for n in 1..=300 {
            let id = n.to_string();
            project.save(&task(&id))?;
            project.link_pr(&id, &link(100 + n, PrStatus::Waiting))?;
            project.link_pr(&id, &link(100, PrStatus::Waiting))?;
        }
        let fixture = dir.path().join("github.json");
        write(&fixture, json!({}));
        let github = Github::default();
        github.network.lock().await.client.fixture = Some(fixture.clone());
        for _ in 0..16 {
            github.poll(&home).await?;
        }
        let log = std::fs::read_to_string(fixture.with_extension("json.log"))?;
        assert_eq!(log.lines().count(), 16);
        for line in log.lines() {
            let v: serde_json::Value = serde_json::from_str(line)?;
            assert!(v["query"].as_str().unwrap().matches(":repository").count() <= 20);
        }
        assert_eq!(github.network.lock().await.due.len(), 301);
        Ok(())
    }
    #[tokio::test]
    async fn unlink_during_network_request_does_not_resurrect_association() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = Home::new(Some(dir.path().join("home")))?;
        let root = dir.path().join("project");
        std::fs::create_dir(&root)?;
        let project = Project::init(&root)?;
        home.register(&root)?;
        project.save(&task("one"))?;
        project.link_pr("one", &link(1, PrStatus::Ready))?;
        let fixture = dir.path().join("github.json");
        write(&fixture, json!({"delay":0.2}));
        let github = Github::default();
        github.network.lock().await.client.fixture = Some(fixture.clone());
        let unlink = async {
            // Wait until gh is in flight, rather than depending on executor timing.
            for _ in 0..100 {
                if fixture.with_extension("json.log").exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(fixture.with_extension("json.log").exists());
            github
                .unlink(
                    root.to_str().unwrap(),
                    "one",
                    &link(1, PrStatus::Ready).id.url(),
                )
                .unwrap();
        };
        let (result, ()) = tokio::join!(github.poll(&home), unlink);
        result?;
        assert!(project.pull_requests("one")?.is_empty());
        Ok(())
    }
    #[tokio::test]
    async fn bad_records_and_projects_do_not_stop_other_links_and_recover() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = Home::new(Some(dir.path().join("home")))?;
        let root = dir.path().join("project");
        std::fs::create_dir(&root)?;
        let project = Project::init(&root)?;
        home.register(&root)?;
        for id in ["one", "two", "bad-task"] {
            project.save(&task(id))?;
        }
        for (id, number) in [("one", 1), ("one", 2), ("two", 3), ("bad-task", 4)] {
            project.link_pr(id, &link(number, PrStatus::Waiting))?;
        }
        let db = rusqlite::Connection::open(root.join(".aide/state.sqlite"))?;
        db.execute(
            "UPDATE github_links SET body='broken' WHERE url=?",
            [link(1, PrStatus::Waiting).id.url()],
        )?;
        db.execute("UPDATE tasks SET body='broken' WHERE id='bad-task'", [])?;
        db.execute(
            "INSERT INTO github_links VALUES('two','invalid-url','{}')",
            [],
        )?;
        let unavailable = dir.path().join("unavailable");
        std::fs::create_dir(&unavailable)?;
        Project::init(&unavailable)?;
        home.register(&unavailable)?;
        let unavailable_db = rusqlite::Connection::open(unavailable.join(".aide/state.sqlite"))?;
        let application: i64 =
            unavailable_db.query_row("PRAGMA application_id", [], |r| r.get(0))?;
        unavailable_db.pragma_update(None, "application_id", 0)?;
        let broken = &project.pull_requests("one")?[0];
        assert!(broken.observation.is_none());
        assert!(broken.error.as_ref().unwrap().contains("could not be read"));
        let fixture = dir.path().join("github.json");
        write(&fixture, json!({}));
        let github = Github::default();
        github.network.lock().await.client.fixture = Some(fixture.clone());
        github.poll(&home).await?;
        // Both the damaged cache and another PR on the same task update normally.
        for pr in project.pull_requests("one")? {
            assert_eq!(pr.observation.unwrap().status, PrStatus::Ready);
            assert!(pr.error.is_none());
        }
        let records = project.all_pull_requests()?;
        let third = records
            .iter()
            .filter_map(|(_, r)| r.as_ref().ok())
            .find(|v| v.id.number == 3)
            .unwrap();
        assert_eq!(third.observation.as_ref().unwrap().status, PrStatus::Ready);
        let error = github.project_error(root.to_str().unwrap()).unwrap();
        assert!(error.contains("bad-task"));
        assert!(error.contains("Invalid stored PR URL"));
        assert!(
            github
                .project_error(unavailable.to_str().unwrap())
                .unwrap()
                .contains("Cannot read PR links")
        );
        project.save(&task("bad-task"))?;
        db.execute("DELETE FROM github_links WHERE url='invalid-url'", [])?;
        unavailable_db.pragma_update(None, "application_id", application)?;
        github.poll(&home).await?;
        assert!(github.project_error(root.to_str().unwrap()).is_none());
        assert!(
            github
                .project_error(unavailable.to_str().unwrap())
                .is_none()
        );
        assert_eq!(
            project.pull_requests("bad-task")?[0]
                .observation
                .as_ref()
                .unwrap()
                .status,
            PrStatus::Ready
        );
        Ok(())
    }

    #[tokio::test]
    async fn failed_writes_are_isolated_visible_stale_and_retryable() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = Home::new(Some(dir.path().join("home")))?;
        let root = dir.path().join("project");
        std::fs::create_dir(&root)?;
        let project = Project::init(&root)?;
        home.register(&root)?;
        for id in ["bad", "good"] {
            project.save(&task(id))?;
            project.link_pr(id, &link(1, PrStatus::Waiting))?;
        }
        project.link_pr("good", &link(2, PrStatus::Waiting))?;
        let db = rusqlite::Connection::open(root.join(".aide/state.sqlite"))?;
        db.execute_batch("CREATE TRIGGER fail_status BEFORE UPDATE ON github_links WHEN OLD.task_id='bad' BEGIN SELECT RAISE(FAIL,'fixture write failure'); END;")?;
        let fixture = dir.path().join("github.json");
        write(&fixture, json!({}));
        let github = Github::default();
        github.network.lock().await.client.fixture = Some(fixture);
        assert!(github.poll(&home).await.is_err());
        for pr in project.pull_requests("good")? {
            assert_eq!(pr.observation.unwrap().status, PrStatus::Ready);
        }
        assert!(
            github
                .project_error(root.to_str().unwrap())
                .unwrap()
                .contains("fixture write failure")
        );
        let runtime = crate::runtime::Runtime::new(&home);
        let mut view = runtime.view(project.task("bad")?).await;
        github.decorate(&project, &mut view);
        assert!(view.pull_requests[0].stale);
        assert_eq!(view.pull_requests[0].color(), "grey");
        assert!(
            view.pull_requests[0]
                .error
                .as_ref()
                .unwrap()
                .contains("Could not save PR status")
        );
        db.execute_batch("DROP TRIGGER fail_status")?;
        github
            .refresh(&home, root.to_str().unwrap(), "good")
            .await?;
        assert!(github.project_error(root.to_str().unwrap()).is_none());
        assert_eq!(
            project.pull_requests("bad")?[0]
                .observation
                .as_ref()
                .unwrap()
                .status,
            PrStatus::Ready
        );
        Ok(())
    }
}
