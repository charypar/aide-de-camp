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

impl Github {
    fn links(home: &Home) -> Result<Vec<Link>> {
        let mut links = Vec::new();
        for root in home.roots()? {
            let Ok(project) = Project::open(Path::new(&root)) else {
                continue;
            };
            for (task, view) in project.all_pull_requests()? {
                if project.task(&task)?.lifecycle == Lifecycle::Open {
                    links.push(Link {
                        root: root.clone(),
                        task,
                        view,
                    });
                }
            }
        }
        Ok(links)
    }
    pub fn decorate(&self, project: &Project, view: &mut TaskView) {
        match project.pull_requests(&view.task.id) {
            Ok(mut links) => {
                let verified = self.verified.lock().unwrap();
                for link in &mut links {
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
        loop {
            tick.tick().await;
            let _ = self.poll(&home).await;
        }
    }
    async fn poll(&self, home: &Home) -> Result<()> {
        let Ok(mut poller) = self.network.try_lock() else {
            return Ok(());
        };
        let links = Self::links(home)?;
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
        let links = Self::links(home)?;
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
                Project::open(Path::new(&link.root))?.observe_pr(&link.task, &view)?;
                self.verified.lock().unwrap().remove(&view.id);
                poller.due.remove(&view.id);
            }
        }
        for (id, result) in results {
            let interval = result.as_ref().map(|o| o.interval()).unwrap_or(300);
            poller.due.insert(id.clone(), time + interval);
            for link in links.iter().filter(|l| l.view.id == id) {
                let project = Project::open(Path::new(&link.root))?;
                // Reload to respect account invalidation above.
                let Some(mut view) = project
                    .pull_requests(&link.task)?
                    .into_iter()
                    .find(|v| v.id == id)
                else {
                    continue;
                };
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
                project.observe_pr(&link.task, &view)?;
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
        // Share the successful result across duplicate associations on the next poll;
        // also invalidate account-dependent caches for this host immediately.
        for link in Self::links(home)?
            .iter()
            .filter(|l| l.view.id.host == id.host)
        {
            if link.view.id == id {
                Project::open(Path::new(&link.root))?.observe_pr(&link.task, &view)?;
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
                Project::open(Path::new(&link.root))?.observe_pr(&link.task, &old)?;
                poller.due.remove(&old.id);
            }
        }
        poller
            .due
            .insert(id, now() + view.observation.as_ref().unwrap().interval());
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
        assert_eq!(Github::links(&home)?.len(), 1);
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
        write(&fixture, json!({"prs":{"1":{"status":"merged"}}}));
        github.network.lock().await.client.cooldown.clear();
        github.refresh(&home, root.to_str().unwrap(), "one").await?;
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
}
