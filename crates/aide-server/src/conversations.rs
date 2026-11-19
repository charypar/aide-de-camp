use crate::{
    database::{Lock, Project},
    instructions::Instructions,
    rpc::Rpc,
    runtime::Runtime,
};
use aide_core::{Lifecycle, Task};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path};

#[derive(clap::Args, Default)]
pub struct ResumeOptions {
    /// Fork this saved conversation into the current task and pin the fork.
    #[arg(value_name = "CONVERSATION_ID", conflicts_with = "pick")]
    pub conversation: Option<String>,
    /// Open Codex's native fork picker and pin selected history to this task.
    #[arg(long)]
    pub pick: bool,
}

impl Runtime {
    pub(crate) async fn fork_conversation(
        &self,
        project: &Project,
        task: &Task,
        rpc: &mut Rpc,
        source: &str,
    ) -> Result<()> {
        if project.task(&task.id)?.thread_id.as_deref() == Some(source) {
            return Ok(());
        }
        let selection = self.prepare_fork(project, task, rpc, source, None).await?;
        let result = rpc.call("thread/fork", selection.params.clone()).await?;
        self.finish_fork(project, &selection, &result)?;
        Ok(())
    }

    pub(crate) async fn prepare_fork(
        &self,
        project: &Project,
        task: &Task,
        rpc: &mut Rpc,
        source: &str,
        picker_pid: Option<u32>,
    ) -> Result<ForkSelection> {
        let transition = project.task_lock(&task.id)?;
        let current = project.task(&task.id)?;
        if current.lifecycle != Lifecycle::Open || current.key != task.key {
            bail!("Task is no longer open");
        }
        crate::activity::CodexProcess::ensure_no_other_ui(&current, picker_pid)?;
        crate::workspace::verify_workspace(&current)?;
        // Also check unbound loaded conversations: an untouched UI may have
        // started a turn before the observer has persisted its association.
        let loaded = rpc.call("thread/loaded/list", json!({})).await?;
        let mut ids: HashSet<String> = loaded["data"]
            .as_array()
            .context("Invalid loaded conversation list")?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        ids.extend(current.thread_id.clone());
        ids.insert(source.to_owned());
        for id in ids {
            let read = rpc.call("thread/read", json!({"threadId":id})).await?;
            let thread = &read["thread"];
            if thread["id"].as_str() != Some(&id) {
                bail!("Codex returned a different conversation");
            }
            if !matches!(
                thread["status"]["type"].as_str(),
                Some("idle" | "notLoaded")
            ) {
                bail!(
                    "Conversation {id} is not idle; finish or interrupt its turn before selecting history"
                );
            }
            if id == source && !crate::worker::is_user_conversation(thread) {
                bail!("Select a user conversation, not an internal helper thread");
            }
        }
        self.record(&current)?;
        let params = Instructions::read(&self.home, &current.key)?
            .context("Missing task instruction snapshot; recover the task backend first")?
            .fork_params(source, &current.workspace, rpc)
            .await?;
        Ok(ForkSelection {
            current,
            source: source.to_owned(),
            params,
            _transition: transition,
        })
    }

    pub(crate) fn finish_fork(
        &self,
        project: &Project,
        selection: &ForkSelection,
        result: &Value,
    ) -> Result<()> {
        let current = &selection.current;
        let source = &selection.source;
        let thread = &result["thread"];
        let id = thread["id"]
            .as_str()
            .context("Fork returned no conversation ID")?;
        if id == source.as_str() || thread["cwd"].as_str() != Some(&current.workspace) {
            bail!(
                "Fork did not create a new conversation in this task's workspace; binding unchanged"
            );
        }
        let path = thread["path"]
            .as_str()
            .context("Fork returned no recording path")?;
        if !Path::new(path).is_file() {
            bail!("Fork history was not saved; binding unchanged");
        }
        let mut selected = current.clone();
        selected.thread_id = Some(id.into());
        selected.native_path = Some(path.into());
        selected.recording = Some(
            self.home
                .path
                .join("recordings")
                .join(&current.key)
                .display()
                .to_string(),
        );
        // Retain the new history before publishing its identity. On any error,
        // the original binding remains authoritative and the source is intact.
        self.record(&selected)?;
        project.repin_conversation(current, &selected)?;
        project.set_parked(&current.id, false)?;
        Ok(())
    }
}

/// Held from fork preparation through durable binding publication.
pub(crate) struct ForkSelection {
    pub params: Value,
    current: Task,
    source: String,
    _transition: Lock,
}
