use crate::{database::Project, runtime::Runtime};
use aide_core::{Environment, Task};
use anyhow::{Context, Result, bail};
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

/// Only the recorded panes belong to Aide; names and window positions are free
/// for the user to change. An unrelated foreground command is never replaced.
pub struct Terminal {
    home: PathBuf,
    root: PathBuf,
    task: Task,
}
impl Terminal {
    pub fn current_session() -> Result<String> {
        let pane =
            std::env::var("TMUX_PANE").context("Enter a task first with aide task attach <id>")?;
        if !pane.starts_with('%')
            || pane.len() < 2
            || !pane[1..].bytes().all(|c| c.is_ascii_digit())
        {
            bail!("Enter a task first with aide task attach <id>");
        }
        Self::run(&["display-message", "-p", "-t", &pane, "#{session_name}"])
    }

    pub fn current_home() -> Result<PathBuf> {
        let session = Self::current_session()?;
        let home = Self::run(&["show-options", "-qv", "-t", &session, "@aide-home"])?;
        if home.is_empty() {
            bail!("Enter a task first with aide task attach <id>");
        }
        Ok(home.into())
    }

    pub fn new(runtime: &Runtime, project: &Project, task: &Task) -> Self {
        Self {
            home: runtime.home.path.clone(),
            root: project.root.clone(),
            task: task.clone(),
        }
    }
    pub fn command() -> Command {
        let mut command = Command::new("tmux");
        if let Some(socket) = std::env::var_os("AIDE_TMUX_SOCKET") {
            command.arg("-L").arg(socket);
        }
        command
    }
    pub fn run(args: &[&str]) -> Result<String> {
        let output = Self::command().args(args).output()?;
        if !output.status.success() {
            bail!("tmux: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(String::from_utf8(output.stdout)?.trim().into())
    }
    pub fn alive(task: &Task) -> bool {
        Self::command()
            .args(["has-session", "-t", &task.session()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    pub fn managed_pane(task: &Task, role: &str) -> Result<Option<String>> {
        if !Self::alive(task) {
            return Ok(None);
        }
        let pane = Self::run(&[
            "show-options",
            "-qv",
            "-t",
            &task.session(),
            &format!("@aide-{role}"),
        ])?;
        let panes = Self::run(&[
            "list-panes",
            "-s",
            "-t",
            &task.session(),
            "-F",
            "#{pane_id}",
        ])?;
        Ok(panes.lines().any(|p| p == pane).then_some(pane))
    }
    pub fn work_alive(task: &Task) -> Result<bool> {
        let Some(pane) = Self::managed_pane(task, "work")? else {
            return Ok(false);
        };
        Ok(Self::run(&["display-message", "-p", "-t", &pane, "#{pane_dead}"])? == "0")
    }
    fn remember(&self, role: &str, pane: &str) -> Result<()> {
        Self::run(&[
            "set-option",
            "-t",
            &self.task.session(),
            &format!("@aide-{role}"),
            pane,
        ])?;
        Ok(())
    }
    pub fn prepare(&self) -> Result<()> {
        if !Self::alive(&self.task) {
            let pane = Self::run(&[
                "new-session",
                "-d",
                "-P",
                "-F",
                "#{pane_id}",
                "-s",
                &self.task.session(),
                "-n",
                "backend",
                "-c",
                &self.task.workspace,
                "fish",
            ])?;
            self.remember("backend", &pane)?;
            // The initial idle shell is ours; replace it below after setting
            // remain-on-exit. Other existing panes never get this marker.
            Self::run(&["set-option", "-p", "-t", &pane, "@aide-placeholder", "1"])?;
            Self::run(&[
                "set-option",
                "-t",
                &self.task.session(),
                "destroy-unattached",
                "off",
            ])?;
        }
        Self::run(&[
            "set-option",
            "-t",
            &self.task.session(),
            "@aide-project",
            self.root.to_str().context("Project path is not UTF-8")?,
        ])?;
        Self::run(&[
            "set-option",
            "-t",
            &self.task.session(),
            "@aide-home",
            self.home.to_str().context("Home path is not UTF-8")?,
        ])?;
        Self::run(&[
            "set-option",
            "-t",
            &self.task.session(),
            "@aide-task",
            &self.task.id,
        ])?;
        Ok(())
    }
    pub fn backend_stopped(&self) -> Result<bool> {
        let Some(pane) = Self::managed_pane(&self.task, "backend")? else {
            return Ok(true);
        };
        Ok(Self::run(&["display-message", "-p", "-t", &pane, "#{pane_dead}"])? == "1")
    }
    pub fn start_backend(&self, environment: &Environment) -> Result<tempfile::NamedTempFile> {
        let pane = match Self::managed_pane(&self.task, "backend")? {
            Some(pane) => {
                let placeholder = Self::run(&[
                    "show-options",
                    "-p",
                    "-qv",
                    "-t",
                    &pane,
                    "@aide-placeholder",
                ])? == "1";
                if !placeholder && !self.backend_stopped()? {
                    bail!(
                        "Backend pane is still running but unresponsive. Inspect/stop it before resuming; no foreground command was replaced"
                    );
                }
                pane
            }
            None => {
                let pane = Self::run(&[
                    "new-window",
                    "-d",
                    "-P",
                    "-F",
                    "#{pane_id}",
                    "-t",
                    &self.task.session(),
                    "-n",
                    "backend",
                    "-c",
                    &self.task.workspace,
                    "fish",
                ])?;
                self.remember("backend", &pane)?;
                pane
            }
        };
        Self::run(&["set-option", "-p", "-t", &pane, "remain-on-exit", "on"])?;
        Self::run(&["set-option", "-p", "-u", "-t", &pane, "@aide-placeholder"])?;
        let mut handoff = tempfile::NamedTempFile::new_in(self.home.join("run"))?;
        handoff.write_all(&serde_json::to_vec(environment)?)?;
        handoff.flush()?;
        let binary = std::env::current_exe()?;
        Self::run(&[
            "respawn-pane",
            "-k",
            "-t",
            &pane,
            "-c",
            &self.task.workspace,
            binary.to_str().context("Executable path is not UTF-8")?,
            "--home",
            self.home.to_str().unwrap(),
            "internal-worker",
            "--project",
            self.root.to_str().unwrap(),
            "--id",
            &self.task.id,
            "--environment",
            handoff.path().to_str().unwrap(),
        ])?;
        Ok(handoff)
    }
    pub fn ensure_work(&self) -> Result<()> {
        if Self::work_alive(&self.task)? {
            return Ok(());
        }
        let existing = Self::managed_pane(&self.task, "work")?;
        // An initial-version session has no managed-window metadata. Preserve
        // its existing shell and register it rather than opening a duplicate UI.
        let panes = Self::run(&[
            "list-panes",
            "-s",
            "-t",
            &self.task.session(),
            "-F",
            "#{pane_id}",
        ])?;
        let backend = Self::managed_pane(&self.task, "backend")?;
        if backend.is_none()
            && let Some(pane) = panes.lines().next()
        {
            self.remember("work", pane)?;
            return Ok(());
        }
        let binary = std::env::current_exe()?;
        let init = format!(
            "command {} --home {} codex resume",
            quote(binary.to_str().context("Executable path is not UTF-8")?),
            quote(self.home.to_str().unwrap())
        );
        let pane = if let Some(pane) = existing {
            Self::run(&[
                "respawn-pane",
                "-k",
                "-t",
                &pane,
                "-c",
                &self.task.workspace,
                "fish",
                "-i",
                "-C",
                &init,
            ])?;
            pane
        } else {
            Self::run(&[
                "new-window",
                "-d",
                "-P",
                "-F",
                "#{pane_id}",
                "-t",
                &self.task.session(),
                "-n",
                "work",
                "-c",
                &self.task.workspace,
                "fish",
                "-i",
                "-C",
                &init,
            ])?
        };
        self.remember("work", &pane)?;
        Self::run(&["select-window", "-t", &pane])?;
        Ok(())
    }
}

pub fn environment() -> Result<Environment> {
    std::env::vars_os()
        .map(|(key, value)| {
            Ok((
                key.into_string()
                    .map_err(|_| anyhow::anyhow!("An exported variable name is not UTF-8"))?,
                value
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("An exported variable value is not UTF-8"))?,
            ))
        })
        .collect()
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}
