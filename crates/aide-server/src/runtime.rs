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
        let _transition = project.task_lock(&task.id)?;
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
        } else if std::env::var_os("TMUX").is_some() {
            self.tmux(&["switch-client", "-t", &target])?;
        } else {
            // Attaching may last hours. Don't hold a lifecycle lock while the
            // user is attached; a concurrent close can terminate the session.
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
    pub async fn view(&self, task: Task) -> TaskView {
        let terminal_alive = self.alive(&task);
        let mut view = TaskView {
            status: Status::Waiting,
            reason: "Workspace ready".into(),
            terminal_alive,
            backend_alive: false,
            summary: None,
            task,
        };
        if view.task.lifecycle == Lifecycle::Completed {
            view.status = Status::Completed;
            view.reason = "Completed".into();
            return view;
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
        if matches!(
            view.task.lifecycle,
            Lifecycle::Preparing | Lifecycle::Closing
        ) {
            view.status = Status::Attention;
            view.reason = match view.task.lifecycle {
                Lifecycle::Preparing => "Preparing workspace",
                Lifecycle::Closing => "Closing task",
                _ => unreachable!(),
            }
            .into();
        }
        view
    }
    pub async fn recover(&self, project: &mut Project, task: &Task) -> Result<()> {
        self.repair(project, task, true, None).await
    }
    pub async fn cleanup(&self, project: &mut Project, id: &str, confirmed: bool) -> Result<()> {
        if !confirmed {
            bail!("Explicit confirmation required to close a task");
        }
        let _transition = project.task_lock(id)?;
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
