use crate::{
    database::{Home, Lock, Project},
    rpc::Rpc,
    workspace,
};
use aide_core::{Lifecycle, Status, Task, TaskView};
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone)]
pub struct Runtime {
    pub home: Home,
    recordings: std::sync::Arc<std::sync::Mutex<crate::recording::Recorder>>,
}
impl Runtime {
    pub fn new(home: &Home) -> Self {
        Self {
            home: home.clone(),
            recordings: Default::default(),
        }
    }
    pub fn socket(&self, task: &Task) -> PathBuf {
        self.home
            .path
            .join("run")
            .join(format!("{}.sock", &task.key[..16]))
    }
    pub(crate) fn lock_path(&self, task: &Task) -> PathBuf {
        self.home
            .path
            .join("run")
            .join(format!("{}.lock", task.key))
    }
    pub fn alive(&self, task: &Task) -> bool {
        crate::terminal::Terminal::alive(task)
    }
    fn tmux(&self, args: &[&str]) -> Result<String> {
        crate::terminal::Terminal::run(args)
    }
    pub async fn healthy(&self, task: &Task) -> bool {
        self.alive(task)
            && crate::terminal::Terminal::work_alive(task).unwrap_or(false)
            && Rpc::connect(&self.socket(task)).await.is_ok()
    }
    pub async fn ensure(&self, project: &mut Project, task: &Task, recovered: bool) -> Result<()> {
        self.repair(project, task, recovered, None).await
    }
    pub async fn repair(
        &self,
        project: &mut Project,
        task: &Task,
        recovered: bool,
        environment: Option<aide_core::Environment>,
    ) -> Result<()> {
        let transition = project.task_lock(&task.id)?;
        self.repair_locked(project, task, recovered, environment, &transition)
            .await
    }
    pub(crate) async fn repair_locked(
        &self,
        project: &mut Project,
        task: &Task,
        recovered: bool,
        environment: Option<aide_core::Environment>,
        _transition: &Lock,
    ) -> Result<()> {
        if task.lifecycle != Lifecycle::Open {
            bail!("Only open tasks can run");
        }
        workspace::verify_workspace(task)?;
        if self.socket(task).as_os_str().len() >= 104 {
            bail!("AIDE_HOME is too long for a Unix socket; choose a shorter path");
        }
        let task = project.update(&task.id, |t| {
            if t.lifecycle == Lifecycle::Open {
                t.desired_running = true;
            }
        })?;
        if task.lifecycle != Lifecycle::Open {
            bail!("Task is closing");
        }
        let terminal = crate::terminal::Terminal::new(self, project, &task);
        terminal.prepare()?;
        if Rpc::connect(&self.socket(&task)).await.is_err() {
            // Held only while the new worker reads it. Never persist exported
            // secrets in task records, tmux command strings, or recordings.
            let environment = match environment {
                Some(environment) => environment,
                None => crate::terminal::environment()?,
            };
            let handoff = terminal.start_backend(&environment)?;
            let mut ready = false;
            for _ in 0..100 {
                if Rpc::connect(&self.socket(&task)).await.is_ok() {
                    ready = true;
                    break;
                }
                if terminal.backend_stopped()? {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            drop(handoff);
            if !ready {
                bail!("Backend failed to start; inspect the backend window and retry recovery");
            }
            project.update(&task.id, |t| {
                t.recovered |= recovered;
                t.error = None;
            })?;
        }
        terminal.ensure_work()?;
        if project.startup_pending(&task.id)? {
            project.update(&task.id, |t| {
                if t.error
                    .as_deref()
                    .is_some_and(|error| error.starts_with("Startup failed:"))
                {
                    t.error = None;
                }
            })?;
            project.finish_startup(&task.id)?;
        }
        Ok(())
    }
    pub fn attach(&self, project: &Project, task: &Task, new_window: bool) -> Result<()> {
        let transition = project.task_lock(&task.id)?;
        let task = project.task(&task.id)?;
        if task.lifecycle != Lifecycle::Open {
            bail!("Task is not open");
        }
        if !self.alive(&task) {
            bail!("Task terminal is missing. Start aide dashboard mode to recover it");
        }
        project.update(&task.id, |t| t.recovered = false)?;
        let target = task.session();
        if new_window {
            crate::terminal::Terminal::new(self, project, &task).open_window()?;
            project.set_parked(&task.id, false)?;
        } else if std::env::var_os("TMUX").is_some() {
            self.tmux(&["switch-client", "-t", &target])?;
            project.set_parked(&task.id, false)?;
        } else {
            // Attaching may last hours. Don't hold a lifecycle lock while the
            // user is attached; a concurrent close can terminate the session.
            project.set_parked(&task.id, false)?;
            drop(transition);
            let status = crate::terminal::Terminal::command()
                .args(["attach-session", "-t", &target])
                .status()?;
            if !status.success() {
                bail!("tmux attach failed");
            }
        }
        Ok(())
    }
    pub fn detach(&self) -> Result<()> {
        if std::env::var_os("TMUX").is_none() {
            bail!("Not inside a tmux client");
        }
        self.tmux(&["detach-client"])?;
        Ok(())
    }
    pub async fn project_view(&self, root: &Path, task: Task) -> TaskView {
        // Each concurrent observer owns its database connection across awaits.
        let Ok(project) = Project::open(root) else {
            return self.view(task).await;
        };
        let starting = project.startup_pending(&task.id).unwrap_or(false);
        let mut view = self.view(task).await;
        // Startup may finish while observing the backend. Refresh observations
        // as well as metadata so an old missing-backend result cannot flash.
        if let Ok(current) = project.task(&view.task.id)
            && (current != view.task
                || starting != project.startup_pending(&current.id).unwrap_or(false))
        {
            view = self.view(current).await;
        }
        Self::refresh_lifecycle(&project, &mut view);
        if let Err(error) = Self::refresh_parking(&project, &mut view) {
            view.status = Status::Attention;
            view.reason = format!("Could not read task parking: {error}");
        }
        view
    }

    pub async fn set_parked(&self, project: &mut Project, id: &str, parked: bool) -> Result<()> {
        let _transition = project.task_lock(id)?;
        let task = project.task(id)?;
        if task.lifecycle != Lifecycle::Open || project.startup_pending(id)? {
            bail!("Task is not open and ready");
        }
        if parked {
            let view = self.view(task).await;
            if view.status != Status::Waiting {
                bail!("Only ready tasks can be parked");
            }
        }
        project.set_parked(id, parked)
    }

    fn refresh_parking(project: &Project, view: &mut TaskView) -> Result<()> {
        if !project.is_parked(&view.task.id)? {
            return Ok(());
        }
        if view.status == Status::Waiting {
            view.status = Status::Parked;
            view.reason = "Parked — waiting for something external".into();
        } else {
            // Work, attention and lifecycle transitions permanently end parking.
            project.set_parked(&view.task.id, false)?;
        }
        Ok(())
    }

    pub async fn view(&self, task: Task) -> TaskView {
        let terminal_alive = self.alive(&task);
        let mut view = TaskView {
            status: Status::Waiting,
            reason: "Workspace ready".into(),
            terminal_alive,
            backend_alive: false,
            summary: None,
            terminal_activity: None,
            task,
        };
        match view.task.lifecycle {
            Lifecycle::Closing => {
                view.status = Status::Closing;
                view.reason = "Closing task".into();
                return view;
            }
            Lifecycle::Completed => {
                view.status = Status::Completed;
                view.reason = "Completed".into();
                return view;
            }
            _ => {}
        }
        if let Ok(mut rpc) = Rpc::connect(&self.socket(&view.task)).await {
            view.backend_alive = true;
            if let Some(id) = &view.task.thread_id {
                if let Ok(value) = rpc.call("thread/read", json!({"threadId":id})).await {
                    let thread = &value["thread"];
                    (view.status, view.reason) = native_status(&thread["status"]);
                    view.summary = thread["name"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned);
                } else {
                    view.status = Status::Attention;
                    view.reason = "Stored conversation unavailable".into();
                }
            } else {
                view.reason = "Conversation ready; history not saved yet".into();
            }
        }
        if !view.backend_alive {
            view.reason = if terminal_alive {
                "Terminal open; Codex stopped or starting. Run aide codex resume in fish."
            } else {
                "Terminal unavailable"
            }
            .into();
            if view.task.desired_running {
                view.status = Status::Attention;
            }
        }
        if view.task.recovered && view.backend_alive && view.status != Status::Working {
            view.status = Status::Attention;
            view.reason = "Recovered conversation — ready for your attention".into();
        }
        if let Some(error) = &view.task.error {
            view.status = Status::Attention;
            view.reason = error.clone();
        }
        if view.task.lifecycle == Lifecycle::Preparing {
            view.status = Status::Attention;
            view.reason = "Preparing workspace".into();
        }
        view
    }

    pub fn refresh_lifecycle(project: &Project, view: &mut TaskView) {
        // Cleanup may begin while thread/read is in flight. Durable lifecycle
        // state takes precedence over a disappearing backend's observations.
        if let Ok(current) = project.task(&view.task.id) {
            view.task = current;
        }
        match view.task.lifecycle {
            Lifecycle::Closing => {
                view.summary = project.task_summary(&view.task.id).ok().flatten();
                let busy = project.task_busy(&view.task.id);
                if busy {
                    view.status = Status::Closing;
                    view.reason = "Closing task".into();
                } else {
                    // Cleanup can finish between reading the record and checking
                    // its lock. Do not report that successful completion as failure.
                    if let Ok(current) = project.task(&view.task.id)
                        && current.lifecycle == Lifecycle::Completed
                    {
                        view.summary = project.task_summary(&current.id).ok().flatten();
                        view.task = current;
                        view.status = Status::Completed;
                        view.reason = "Completed".into();
                        return;
                    }
                    view.status = Status::Attention;
                    view.reason = "Cleanup interrupted; retry Close task".into();
                }
            }
            Lifecycle::Completed => {
                view.summary = project.task_summary(&view.task.id).ok().flatten();
                view.status = Status::Completed;
                view.reason = "Completed".into();
            }
            Lifecycle::Preparing | Lifecycle::Open => {
                let preparing = view.task.lifecycle == Lifecycle::Preparing;
                let pending = project.startup_pending(&view.task.id).unwrap_or(false);
                if preparing || pending {
                    if project.task_busy(&view.task.id) {
                        view.status = Status::Starting;
                        view.reason = if preparing {
                            "Preparing workspace"
                        } else {
                            "Starting Codex"
                        }
                        .into();
                    } else {
                        // Startup can complete between reading metadata and
                        // checking ownership. Do not classify success as interruption.
                        if let Ok(current) = project.task(&view.task.id)
                            && current.lifecycle == Lifecycle::Open
                            && !project.startup_pending(&current.id).unwrap_or(false)
                        {
                            view.task = current;
                            view.status = Status::Starting;
                            view.reason = "Finishing startup".into();
                            return;
                        }
                        view.status = Status::Attention;
                        let retry = if preparing {
                            "Retry Start with the same task ID"
                        } else {
                            "Retry Recover session"
                        };
                        view.reason = match &view.task.error {
                            Some(error) => format!("{error}. {retry}"),
                            None => format!("Startup interrupted; {retry}"),
                        };
                    }
                }
            }
        }
    }
    pub async fn recover(&self, project: &mut Project, task: &Task) -> Result<()> {
        self.repair(project, task, true, None).await
    }
    pub async fn cleanup(&self, project: &mut Project, id: &str, confirmed: bool) -> Result<()> {
        if !confirmed {
            bail!("Explicit confirmation required to close a task");
        }
        let _transition = project.task_lock(id)?;
        let current = project.task(id)?;
        if current.lifecycle == Lifecycle::Open
            && let Some(summary) = self.view(current).await.summary
        {
            project.remember_summary(id, &summary)?;
        }
        let task = project.update(id, |t| {
            if t.lifecycle != Lifecycle::Completed {
                t.lifecycle = Lifecycle::Closing;
                t.desired_running = false;
            }
        })?;
        if task.lifecycle == Lifecycle::Completed {
            return Ok(());
        }
        if self.alive(&task) {
            self.tmux(&["kill-session", "-t", &task.session()])?;
        }
        let mut held = None;
        for _ in 0..50 {
            if let Ok(lock) = Lock::acquire(&self.lock_path(&task)) {
                held = Some(lock);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let _runtime_lock = held.context("Task runtime is still stopping; retry cleanup")?;
        if Rpc::connect(&self.socket(&task)).await.is_ok() {
            bail!(
                "Codex backend is still alive; workspace preserved. Stop it before retrying cleanup"
            );
        }
        let root = project.root.clone();
        let id = id.to_owned();
        let runtime = self.clone();
        tokio::task::spawn_blocking(move || {
            let project = Project::open(&root)?;
            let mut task = project.task(&id)?;
            runtime.record(&task)?;
            workspace::remove(&project, &mut task)
        })
        .await?
    }
    pub fn record(&self, task: &Task) -> Result<()> {
        self.retain(task, true)
    }
    pub(crate) fn record_periodically(&self, task: &Task) -> Result<()> {
        self.retain(task, false)
    }
    fn retain(&self, task: &Task, final_copy: bool) -> Result<()> {
        if let Some(source) = &task.native_path {
            let path = Path::new(source);
            if !path.exists() {
                return Ok(());
            }
            let target = self.home.path.join("recordings").join(&task.key);
            fs::create_dir_all(&target)?;
            self.recordings
                .lock()
                .map_err(|_| anyhow::anyhow!("Recording cache unavailable"))?
                .retain(
                    path,
                    &target.join(path.file_name().context("Invalid native recording path")?),
                    final_copy,
                )?;
        }
        Ok(())
    }
}

fn native_status(value: &serde_json::Value) -> (Status, String) {
    match value["type"].as_str() {
        Some("active") => {
            let flags = value["activeFlags"].as_array();
            if flags.is_some_and(|f| {
                f.iter()
                    .any(|v| v == "waitingOnApproval" || v == "waitingOnUserInput")
            }) {
                (
                    Status::Attention,
                    "Codex needs your input or approval".into(),
                )
            } else {
                (Status::Working, "Codex is working".into())
            }
        }
        Some("idle") => (Status::Waiting, "Conversation idle".into()),
        Some("notLoaded") => (Status::Attention, "Conversation not loaded".into()),
        _ => (Status::Attention, "Codex status unavailable".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn parking_survives_reload_and_older_workers_and_can_be_cleared() -> Result<()> {
        let (_temp, home, mut project) = workspace::tests::fixture()?;
        let task = workspace::start(&project, "parked", None)?;
        let runtime = Runtime::new(&home);
        runtime.set_parked(&mut project, &task.id, true).await?;
        let reopened = Project::open(&project.root)?;
        reopened.save(&task)?;

        let view = runtime.project_view(&reopened.root, task.clone()).await;
        assert_eq!(view.status, Status::Parked);
        assert!(view.reason.contains("external"));
        assert_eq!(view.task, task);

        // Merely inspecting a parked task does not resume it.
        assert_eq!(
            runtime
                .project_view(&project.root, task.clone())
                .await
                .status,
            Status::Parked
        );
        runtime.set_parked(&mut project, &task.id, false).await?;
        assert_eq!(
            runtime.project_view(&project.root, task).await.status,
            Status::Waiting
        );
        Ok(())
    }

    #[tokio::test]
    async fn parking_does_not_hide_work_attention_or_lifecycle_changes() -> Result<()> {
        let (_temp, home, mut project) = workspace::tests::fixture()?;
        let task = workspace::start(&project, "parked", None)?;
        let runtime = Runtime::new(&home);
        for status in [
            Status::Working,
            Status::Attention,
            Status::Starting,
            Status::Closing,
            Status::Completed,
        ] {
            runtime.set_parked(&mut project, &task.id, true).await?;
            let mut view = runtime.view(task.clone()).await;
            view.status = status.clone();
            let reason = view.reason.clone();
            Runtime::refresh_parking(&project, &mut view)?;
            assert_eq!(view.status, status);
            assert_eq!(view.reason, reason);
            assert!(!project.is_parked(&task.id)?);
            assert_eq!(
                runtime
                    .project_view(&project.root, task.clone())
                    .await
                    .status,
                Status::Waiting
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn parking_rejects_unready_missing_and_busy_tasks() -> Result<()> {
        let (_temp, home, mut project) = workspace::tests::fixture()?;
        let task = workspace::start(&project, "parked", None)?;
        let runtime = Runtime::new(&home);
        assert!(
            runtime
                .set_parked(&mut project, "missing", true)
                .await
                .is_err()
        );
        let lock = project.task_lock(&task.id)?;
        assert!(
            runtime
                .set_parked(&mut project, &task.id, true)
                .await
                .is_err()
        );
        drop(lock);
        project.begin_startup(&task.id)?;
        assert!(
            runtime
                .set_parked(&mut project, &task.id, true)
                .await
                .is_err()
        );
        project.finish_startup(&task.id)?;
        project.update(&task.id, |t| t.error = Some("Needs attention".into()))?;
        assert!(
            runtime
                .set_parked(&mut project, &task.id, true)
                .await
                .is_err()
        );
        for lifecycle in [
            Lifecycle::Preparing,
            Lifecycle::Closing,
            Lifecycle::Completed,
        ] {
            project.update(&task.id, |t| t.lifecycle = lifecycle)?;
            assert!(
                runtime
                    .set_parked(&mut project, &task.id, true)
                    .await
                    .is_err()
            );
        }
        assert!(!project.is_parked(&task.id)?);
        Ok(())
    }

    #[tokio::test]
    async fn startup_survives_reopen_and_worker_writes_and_detects_interruption() -> Result<()> {
        let (_temp, home, project) = workspace::tests::fixture()?;
        let (task, transition) = workspace::start_managed(&project, "starting", None)?;
        let runtime = Runtime::new(&home);
        let reopened = Project::open(&project.root)?;
        // Older workers can rewrite task JSON without erasing startup metadata.
        reopened.save(&task)?;
        let view = runtime.project_view(&reopened.root, task.clone()).await;
        assert_eq!(view.status, Status::Starting);
        assert_eq!(view.reason, "Starting Codex");
        assert!(project.task_lock(&task.id).is_err());

        drop(transition);
        let view = runtime.project_view(&reopened.root, task.clone()).await;
        assert_eq!(view.status, Status::Attention);
        assert!(view.reason.contains("Retry Recover session"));

        let transition = project.task_lock(&task.id)?;
        project.finish_startup(&task.id)?;
        drop(transition);
        let view = runtime.project_view(&reopened.root, task).await;
        assert_eq!(view.status, Status::Waiting);
        Ok(())
    }

    #[tokio::test]
    async fn preparation_and_startup_override_stale_observations_but_keep_failures() -> Result<()> {
        let (_temp, home, project) = workspace::tests::fixture()?;
        let task = workspace::start(&project, "preparing", None)?;
        let runtime = Runtime::new(&home);
        let mut stale = runtime.view(task.clone()).await;
        let transition = project.task_lock(&task.id)?;
        project.update(&task.id, |t| t.lifecycle = Lifecycle::Preparing)?;
        Runtime::refresh_lifecycle(&project, &mut stale);
        assert_eq!(stale.status, Status::Starting);
        assert_eq!(stale.reason, "Preparing workspace");

        project.update(&task.id, |t| {
            t.error = Some("Workspace creation failed".into())
        })?;
        drop(transition);
        Runtime::refresh_lifecycle(&project, &mut stale);
        assert_eq!(stale.status, Status::Attention);
        assert_eq!(
            stale.reason,
            "Workspace creation failed. Retry Start with the same task ID"
        );

        let transition = project.task_lock(&task.id)?;
        project.begin_startup(&task.id)?;
        project.update(&task.id, |t| {
            t.lifecycle = Lifecycle::Open;
            t.desired_running = true;
            t.error = None;
        })?;
        Runtime::refresh_lifecycle(&project, &mut stale);
        assert_eq!(stale.status, Status::Starting);
        project.update(&task.id, |t| {
            t.error = Some("Backend failed to start; retry recovery".into())
        })?;
        drop(transition);
        Runtime::refresh_lifecycle(&project, &mut stale);
        assert_eq!(stale.status, Status::Attention);
        assert_eq!(
            stale.reason,
            "Backend failed to start; retry recovery. Retry Recover session"
        );

        let transition = project.task_lock(&task.id)?;
        project.update(&task.id, |t| t.lifecycle = Lifecycle::Closing)?;
        Runtime::refresh_lifecycle(&project, &mut stale);
        assert_eq!(stale.status, Status::Closing);
        drop(transition);
        Ok(())
    }

    #[tokio::test]
    async fn cleanup_overrides_stale_observation_and_preserves_title_after_reopen() -> Result<()> {
        let (_temp, home, project) = workspace::tests::fixture()?;
        let task = workspace::start(&project, "closing", None)?;
        let runtime = Runtime::new(&home);
        let mut stale = runtime.view(task.clone()).await;
        let lock = project.task_lock(&task.id)?;
        project.update(&task.id, |t| {
            t.lifecycle = Lifecycle::Closing;
        })?;
        project.remember_summary(&task.id, "Retained title")?;
        // An older surviving worker can still rewrite the task JSON on shutdown.
        // It must not erase the independently retained title.
        let current = project.task(&task.id)?;
        project.save(&current)?;
        Runtime::refresh_lifecycle(&project, &mut stale);
        assert_eq!(stale.status, Status::Closing);
        assert_eq!(stale.summary.as_deref(), Some("Retained title"));
        drop(lock);
        let reopened = Project::open(&project.root)?;
        Runtime::refresh_lifecycle(&reopened, &mut stale);
        assert_eq!(stale.status, Status::Attention);
        assert!(stale.reason.contains("retry Close task"));
        reopened.update(&task.id, |t| t.lifecycle = Lifecycle::Completed)?;
        Runtime::refresh_lifecycle(&reopened, &mut stale);
        assert_eq!(stale.status, Status::Completed);
        assert_eq!(stale.summary.as_deref(), Some("Retained title"));
        Ok(())
    }

    #[test]
    fn native_status_does_not_confuse_approval_or_unknown_states_with_completion() {
        assert_eq!(
            native_status(&json!({"type":"active","activeFlags":[]})).0,
            Status::Working
        );
        assert_eq!(
            native_status(&json!({"type":"active","activeFlags":["waitingOnApproval"]})).0,
            Status::Attention
        );
        assert_eq!(
            native_status(&json!({"type":"active","activeFlags":["waitingOnUserInput"]})).0,
            Status::Attention
        );
        assert_eq!(native_status(&json!({"type":"idle"})).0, Status::Waiting);
        assert_eq!(
            native_status(&json!({"type":"future-status"})).0,
            Status::Attention
        );
    }
}
