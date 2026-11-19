use crate::{attachment, http::Manager};
use aide_core::{
    Command,
    peer::{Operation, Outcome},
};
use anyhow::{Context, Result, bail};
use std::time::Duration;

impl Manager {
    pub async fn dispatch(&self, mut command: Command) -> Result<String> {
        let (target, id) = command
            .target()
            .context("Configuration requests require a correlated response")?;
        let Some(target) = target.strip_prefix("peer:") else {
            if !self.host.identity.desktop
                && matches!(
                    command,
                    Command::Attach { .. }
                        | Command::Editor { .. }
                        | Command::Review { .. }
                        | Command::Start {
                            open_terminal: true,
                            ..
                        }
                )
            {
                bail!("This dashboard host cannot launch desktop applications");
            }
            return self.host.operations.execute(command).await;
        };
        let (instance, root) = target
            .split_once(':')
            .context("Invalid peer project reference")?;
        let peer = self
            .peers
            .as_ref()
            .context("Peer discovery is disabled")?
            .get(instance)
            .await?;
        let root = root.to_owned();
        let task = peer
            .projects
            .iter()
            .find(|p| p.root == root)
            .and_then(|p| p.tasks.iter().find(|t| t.task.id == id))
            .map(|t| t.task.clone());
        let key = task.as_ref().map(|task| task.key.clone());
        *command.project_mut().context("Task command required")? = root.clone();
        let launch = matches!(
            command,
            Command::Attach { .. }
                | Command::Start {
                    open_terminal: true,
                    ..
                }
        );
        if launch && !self.host.identity.desktop {
            bail!("This dashboard host cannot launch terminal windows");
        }
        if matches!(command, Command::Editor { .. }) {
            if !self.host.identity.desktop {
                bail!("This dashboard host cannot launch desktop applications");
            }
            let editor = crate::editor::RemoteEditor::new(&self.home, &peer)?;
            editor
                .open(
                    &peer,
                    &root,
                    &task.context("Task unavailable; refresh the task list")?,
                )
                .await?;
            return Ok(format!("Requested Zed on {}", self.host.identity.name));
        }
        if matches!(command, Command::Review { .. }) {
            bail!("Remote review integration is not available yet");
        }
        if matches!(command, Command::Attach { .. }) {
            attachment::open_remote(
                &self.home,
                &peer,
                &root,
                &task.context("Task unavailable; refresh the task list")?,
            )?;
            return Ok(format!("Opened terminal on {}", self.host.identity.name));
        }
        if let Command::Start { open_terminal, .. } = &mut command {
            *open_terminal = false;
        }
        if let Command::Recover { environment, .. } = &mut command {
            *environment = None;
        }
        let id = command
            .target()
            .context("Task command required")?
            .1
            .to_owned();
        let result = peer
            .call(Operation::Execute { command, key }, Duration::from_secs(60))
            .await;
        if result.is_err()
            && let Some(peers) = &self.peers
        {
            // Reconcile via reads; never replay the mutation after a lost result.
            peers.refresh().await;
        }
        let outcome = result?;
        let Outcome::Success { message } = outcome else {
            bail!("Unexpected peer action response");
        };
        if launch {
            let outcome = peer.call(Operation::List, Duration::from_secs(5)).await?;
            let Outcome::Projects { projects } = outcome else {
                bail!("Task started; refresh the task list to attach");
            };
            let task = projects
                .iter()
                .find(|p| p.root == root)
                .and_then(|p| p.tasks.iter().find(|t| t.task.id == id))
                .context("Task started; refresh the task list to attach")?;
            attachment::open_remote(&self.home, &peer, &root, &task.task)
                .context("Task started; terminal launch failed")?;
        }
        Ok(message)
    }
}
