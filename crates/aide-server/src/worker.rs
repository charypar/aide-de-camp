use crate::{
    database::{Lock, Project},
    http,
    rpc::Rpc,
    runtime::Runtime,
    terminal::{self, Terminal},
};
use aide_core::{Command, Environment, Lifecycle, Task};
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::{fs, path::Path, process::Stdio, time::Duration};

impl Runtime {
    /// Long-lived tmux worker. No terminal UI child belongs to this process.
    pub async fn worker(&self, root: &Path, id: &str, environment_path: &Path) -> Result<()> {
        let environment: Environment = serde_json::from_slice(&fs::read(environment_path)?)?;
        fs::remove_file(environment_path)?;
        let project = Project::open(root)?;
        let task = project.task(id)?;
        let _lock = Lock::acquire(&self.lock_path(&task))?;
        if task.lifecycle != Lifecycle::Open {
            bail!("Task is not open");
        }
        let socket = self.socket(&task);
        if Rpc::connect(&socket).await.is_ok() {
            bail!("Backend already running");
        }
        let _ = fs::remove_file(&socket);
        let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let mut interrupt =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        let log_path = self.home.path.join("run").join(format!("{}.log", task.key));
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        println!(
            "Aide backend for {}. Ctrl-C stops this backend; the task remains open.\nBackend log: {}",
            task.id,
            log_path.display()
        );
        let mut server = tokio::process::Command::new(
            environment
                .get("AIDE_CODEX")
                .map(String::as_str)
                .unwrap_or("codex"),
        )
        .args([
            "app-server",
            "--listen",
            &format!("unix://{}", socket.display()),
        ])
        .env_clear()
        .envs(&environment)
        .current_dir(&task.workspace)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .kill_on_drop(true)
        .spawn()?;
        let result = tokio::select! {
            result = self.monitor(&project, &task, &mut server) => result,
            _ = hangup.recv() => Ok(()),
            _ = terminate.recv() => Ok(()),
            _ = interrupt.recv() => Ok(()),
        };
        stop_backend(&mut server).await;
        if let Ok(task) = project.task(id) {
            let _ = self.record(&task);
        }
        let _ = fs::remove_file(&socket);
        let message = match &result {
            Ok(()) => "Backend stopped. Run aide codex resume in your work shell to restart it."
                .to_owned(),
            Err(error) => format!(
                "Backend stopped: {error}. Inspect this window, then run aide codex resume."
            ),
        };
        let _ = project.update(id, |t| t.error = Some(message.clone()));
        println!("{message}");
        result
    }
    async fn monitor(
        &self,
        project: &Project,
        task: &Task,
        server: &mut tokio::process::Child,
    ) -> Result<()> {
        let mut rpc = loop {
            if let Some(exit) = server.try_wait()? {
                bail!("Backend exited: {exit}");
            }
            if let Ok(rpc) = Rpc::connect(&self.socket(task)).await {
                break rpc;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        if let Some(id) = &task.thread_id
            && task
                .native_path
                .as_ref()
                .is_some_and(|p| Path::new(p).exists())
        {
            rpc.call("thread/resume", json!({"threadId":id})).await?;
            // Resume restores context only. Reconnect as a metadata observer;
            // native approvals continue to belong to the interactive client.
            rpc = Rpc::connect(&self.socket(task)).await?;
        }
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                status = server.wait() => bail!("Backend exited: {}", status?),
                _ = tick.tick() => {
                    let observed = async {
                        self.observe(project, task, &mut rpc).await?;
                        self.record_periodically(&project.task(&task.id)?)
                    }.await;
                    if let Err(error) = observed {
                        let _ = project.update(&task.id, |t| t.error = Some(format!("Recording/monitor: {error}")));
                        rpc = Rpc::connect(&self.socket(task)).await?;
                    } else if project.task(&task.id)?.error.as_deref().is_some_and(|e| e.starts_with("Recording/monitor:")) {
                        project.update(&task.id, |t| {
                            if t.error.as_deref().is_some_and(|e| e.starts_with("Recording/monitor:")) {
                                t.error = None;
                            }
                        })?;
                    }
                }
            }
        }
    }
    async fn observe(&self, project: &Project, task: &Task, rpc: &mut Rpc) -> Result<()> {
        let current = project.task(&task.id)?;
        let thread = if let Some(id) = &current.thread_id {
            // Once bound, the database is authoritative. Other loaded threads,
            // including /new conversations in the same directory, are unrelated.
            let value = rpc.call("thread/read", json!({"threadId": id})).await?;
            if value["thread"]["id"].as_str() != Some(id) {
                bail!("Codex returned a different conversation than the task's stored ID");
            }
            Some(value["thread"].clone())
        } else {
            let loaded = rpc.call("thread/loaded/list", json!({})).await?;
            let mut candidates = Vec::new();
            if let Some(ids) = loaded["data"].as_array() {
                for id in ids {
                    let value = rpc.call("thread/read", json!({"threadId": id})).await?;
                    let thread = &value["thread"];
                    if thread["cwd"].as_str() == Some(task.workspace.as_str())
                        && is_user_conversation(thread)
                    {
                        candidates.push(thread.clone());
                    }
                }
            }
            if candidates.len() > 1 {
                bail!(
                    "Multiple conversations found before this task was bound; refusing to choose one"
                );
            }
            candidates.pop()
        };
        if let Some(thread) = thread {
            let id = thread["id"].as_str().context("Missing native thread ID")?;
            if let Some(path) = thread["path"].as_str() {
                // Untouched native sessions need not have durable history yet.
                // Bind once saved, so reopening a blank UI cannot pin a stale ID.
                if current.thread_id.is_some() || Path::new(path).exists() {
                    project.bind_conversation(
                        &task.id,
                        id,
                        path,
                        &self
                            .home
                            .path
                            .join("recordings")
                            .join(&task.key)
                            .display()
                            .to_string(),
                    )?;
                }
            }
        }
        Ok(())
    }
    pub async fn resume(&self) -> Result<()> {
        let (project, task) = self.current_task()?;
        if Rpc::connect(&self.socket(&task)).await.is_err() {
            http::request(
                &self.home,
                Command::Recover {
                    project: project.root.display().to_string(),
                    id: task.id.clone(),
                    environment: Some(terminal::environment()?),
                },
            )
            .await?;
        }
        let mut rpc = Rpc::connect(&self.socket(&task))
            .await
            .context("Backend unavailable; inspect the backend window")?;
        self.observe(&project, &task, &mut rpc).await?;
        let task = project.task(&task.id)?;
        drop(rpc);
        // Replace only this foreground command. Exiting the native UI returns
        // to its parent fish shell; the independent tmux worker stays alive.
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new(
            std::env::var_os("AIDE_CODEX").unwrap_or_else(|| "codex".into()),
        );
        if let Some(id) = &task.thread_id {
            if !task
                .native_path
                .as_ref()
                .is_some_and(|p| Path::new(p).exists())
            {
                bail!(
                    "Saved conversation recording is unavailable; refusing to replace it with a new conversation"
                );
            }
            command.arg("resume").arg(id);
        }
        // This process is replaced by the native UI. Record its birth time as
        // well as PID so an old marker cannot hide unrelated work after PID reuse.
        crate::activity::CodexProcess::remember()?;
        let error = command
            .args([
                "--remote",
                &format!("unix://{}", self.socket(&task).display()),
                "--no-alt-screen",
            ])
            .current_dir(&task.workspace)
            .exec();
        Err(error.into())
    }
    fn current_task(&self) -> Result<(Project, Task)> {
        let session = Terminal::current_session()?;
        let option = |name: &str| Terminal::run(&["show-options", "-qv", "-t", &session, name]);
        let root = option("@aide-project")?;
        let id = option("@aide-task")?;
        let home = option("@aide-home")?;
        if root.is_empty() || id.is_empty() || Path::new(&home) != self.home.path {
            bail!("Not this Aide home's task session. Enter it with aide task attach <id>");
        }
        let project = Project::open(Path::new(&root))?;
        let task = project.task(&id)?;
        if task.session() != session || task.lifecycle != Lifecycle::Open {
            bail!("Task session is no longer open");
        }
        let cwd = std::env::current_dir()?.canonicalize()?;
        if let Ok(other) = Project::discover(&cwd) {
            for candidate in other.tasks()? {
                if cwd.starts_with(&candidate.workspace) && candidate.key != task.key {
                    bail!(
                        "Current directory belongs to task {}, but this terminal belongs to {}",
                        candidate.id,
                        task.id
                    );
                }
            }
        }
        Ok((project, task))
    }
}

fn is_user_conversation(thread: &serde_json::Value) -> bool {
    // Modern servers also load internal threads (e.g. `thread_title`) with
    // the user's cwd and source. They must never participate in task binding.
    if !thread["threadSource"].is_null() {
        return thread["threadSource"].as_str() == Some("user");
    }
    // Older servers lack threadSource; exclude their explicit subagent sources
    // while preserving discovery for ordinary legacy conversations.
    let source = &thread["source"];
    source.get("subAgent").is_none() && !source.as_str().is_some_and(|s| s.starts_with("subAgent"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_excludes_internal_threads_and_supports_legacy_user_threads() {
        for source in [
            "thread_title",
            "subagent",
            "guardian_review",
            "future_helper",
        ] {
            assert!(!is_user_conversation(
                &json!({"source":"vscode", "threadSource":source})
            ));
        }
        assert!(is_user_conversation(
            &json!({"source":"vscode", "threadSource":"user"})
        ));
        assert!(is_user_conversation(
            &json!({"source":"cli", "threadSource":null})
        ));
        assert!(is_user_conversation(&json!({})));
        assert!(!is_user_conversation(
            &json!({"source":{"subAgent":{"thread_spawn":{}}}})
        ));
        assert!(!is_user_conversation(&json!({"source":"subAgentReview"})));
    }
}

async fn stop_backend(server: &mut tokio::process::Child) {
    if let Some(pid) = server.id() {
        // A separate process group confines shutdown to this task's backend and
        // its inherited MCP/tool children. The dashboard never calls this on exit.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
        if tokio::time::timeout(Duration::from_secs(5), server.wait())
            .await
            .is_err()
        {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            let _ = server.wait().await;
        }
    }
}
