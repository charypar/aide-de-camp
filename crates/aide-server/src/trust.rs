use crate::{database::Project, rpc::Rpc};
use aide_core::Task;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::Path;

/// Trust decisions come from Codex's user configuration, never task files.
pub(crate) struct WorkspaceTrust;

impl WorkspaceTrust {
    pub async fn inherit(project: &Project, task: &Task, rpc: &mut Rpc) -> Result<()> {
        let source = project
            .repositories()?
            .into_iter()
            .find(|repository| repository.name == task.repository)
            .context("Task repository is no longer registered")?;
        let response = rpc
            .call(
                "config/read",
                json!({"cwd":task.workspace,"includeLayers":true}),
            )
            .await?;
        if let Some(params) = Self::edit(
            &response,
            &project.root,
            Path::new(&source.checkout),
            Path::new(&task.workspace),
        ) {
            // The user config's version prevents a concurrent explicit trust
            // decision from being overwritten. Leave retries to the next launch.
            rpc.call("config/batchWrite", params).await?;
        }
        Ok(())
    }

    fn edit(response: &Value, project: &Path, source: &Path, workspace: &Path) -> Option<Value> {
        let user = response["layers"]
            .as_array()?
            .iter()
            .find(|layer| layer["name"]["type"] == "user")?;
        let projects = user["config"]["projects"].as_object()?;
        let workspace_key = workspace.to_str()?;
        // Preserve even unknown future decisions rather than replacing them.
        if projects
            .get(workspace_key)
            .is_some_and(|entry| !entry["trust_level"].is_null())
        {
            return None;
        }
        let denied = |entries: &serde_json::Map<String, Value>| {
            entries.iter().any(|(path, entry)| {
                entry["trust_level"] == "untrusted"
                    && (workspace.starts_with(path)
                        || source.starts_with(path)
                        || project.starts_with(path))
            })
        };
        if denied(projects)
            || response["config"]["projects"]
                .as_object()
                .is_some_and(denied)
        {
            return None;
        }
        let trusted = [project, source].iter().any(|path| {
            path.to_str()
                .and_then(|path| projects.get(path))
                .is_some_and(|entry| entry["trust_level"] == "trusted")
        });
        if !trusted {
            return None;
        }
        let key = workspace_key.replace('\\', "\\\\").replace('"', "\\\"");
        Some(json!({
            "filePath":user["name"]["file"].as_str()?,
            "expectedVersion":user["version"].as_str()?,
            "reloadUserConfig":true,
            "edits":[{"keyPath":format!("projects.\"{key}\".trust_level"),"value":"trusted","mergeStrategy":"replace"}]
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(projects: Value) -> Value {
        json!({"layers":[{"name":{"type":"user","file":"/home/user/.codex/config.toml"},"version":"v1","config":{"projects":projects}}]})
    }

    fn edit(response: &Value) -> Option<Value> {
        WorkspaceTrust::edit(
            response,
            Path::new("/project"),
            Path::new("/project/repo"),
            Path::new("/project/tasks/task"),
        )
    }

    #[test]
    fn project_or_source_trust_grants_only_the_task_workspace() {
        for trusted in ["/project", "/project/repo"] {
            let result = edit(&config(json!({trusted:{"trust_level":"trusted"}}))).unwrap();
            assert_eq!(result["expectedVersion"], "v1");
            assert_eq!(result["filePath"], "/home/user/.codex/config.toml");
            assert_eq!(
                result["edits"],
                json!([{"keyPath":"projects.\"/project/tasks/task\".trust_level","value":"trusted","mergeStrategy":"replace"}])
            );
        }
    }

    #[test]
    fn explicit_decisions_and_untrusted_ancestors_are_preserved() {
        for denied in [
            "/",
            "/project",
            "/project/repo",
            "/project/tasks",
            "/project/tasks/task",
        ] {
            let mut projects = json!({"/project":{"trust_level":"trusted"},"/project/repo":{"trust_level":"trusted"}});
            projects[denied] = json!({"trust_level":"untrusted"});
            assert!(edit(&config(projects)).is_none(), "{denied}");
        }
        for level in ["trusted", "untrusted", "future-value"] {
            assert!(edit(&config(json!({"/project":{"trust_level":"trusted"},"/project/tasks/task":{"trust_level":level}}))).is_none());
        }
    }

    #[test]
    fn unknown_trust_or_project_scoped_self_trust_cannot_grant_access() {
        assert!(edit(&config(json!({}))).is_none());
        assert!(edit(&config(json!({"/project-other":{"trust_level":"trusted"}}))).is_none());
        let mut response = config(json!({"/project":{"trust_level":"trusted"}}));
        response["layers"][0]["name"]["type"] = json!("project");
        assert!(edit(&response).is_none());
        assert!(
            edit(&json!({"config":{"projects":{"/project":{"trust_level":"trusted"}}}})).is_none()
        );
    }

    #[test]
    fn version_is_required_and_effective_denials_override_user_trust() {
        let mut response = config(json!({"/project":{"trust_level":"trusted"}}));
        response["layers"][0]["version"] = Value::Null;
        assert!(edit(&response).is_none());
        response["layers"][0]["version"] = json!("v1");
        response["config"] = json!({"projects":{"/project/tasks":{"trust_level":"untrusted"}}});
        assert!(edit(&response).is_none());
    }

    #[tokio::test]
    #[ignore = "Uses installed Codex with isolated config; no authentication or model turns"]
    async fn live_codex_trust_persistence_and_conflict() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let home = directory.path().join("codex");
        let project = directory.path().join("project");
        let source = project.join("repo");
        let workspace = project.join("tasks/task");
        std::fs::create_dir_all(&home)?;
        std::fs::create_dir_all(&source)?;
        std::fs::create_dir_all(&workspace)?;
        let config_path = home.join("config.toml");
        std::fs::write(
            &config_path,
            format!(
                "# Preserve this comment\nmodel = \"test-model\"\n[projects.{}]\ntrust_level = \"trusted\"\n",
                json!(project.to_str().unwrap())
            ),
        )?;
        let socket = directory.path().join("codex.sock");
        let mut child = tokio::process::Command::new("codex")
            .args([
                "app-server",
                "--listen",
                &format!("unix://{}", socket.display()),
            ])
            .env("CODEX_HOME", &home)
            .current_dir(&workspace)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut rpc = tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                if let Ok(rpc) = Rpc::connect(&socket).await {
                    return Ok::<_, anyhow::Error>(rpc);
                }
                anyhow::ensure!(
                    child.try_wait()?.is_none(),
                    "Codex exited before connection"
                );
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await??;
        let response = rpc
            .call("config/read", json!({"cwd":workspace,"includeLayers":true}))
            .await?;
        let write = WorkspaceTrust::edit(&response, &project, &source, &workspace)
            .expect("inherit project trust");
        // Simulate the user's decision arriving between our read and write.
        use std::io::Write;
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(&config_path)?,
            "\n[projects.{}]\ntrust_level = \"untrusted\"",
            json!(workspace.to_str().unwrap())
        )?;
        assert!(rpc.call("config/batchWrite", write).await.is_err());
        assert!(std::fs::read_to_string(&config_path)?.contains("untrusted"));
        let other = project.join("tasks/another");
        std::fs::create_dir_all(&other)?;
        let response = rpc
            .call("config/read", json!({"cwd":other,"includeLayers":true}))
            .await?;
        let write = WorkspaceTrust::edit(&response, &project, &source, &other)
            .expect("retry with current config version");
        rpc.call("config/batchWrite", write).await?;
        let response = rpc
            .call("config/read", json!({"cwd":other,"includeLayers":true}))
            .await?;
        assert_eq!(
            response["config"]["projects"][other.to_str().unwrap()]["trust_level"],
            "trusted"
        );
        assert!(WorkspaceTrust::edit(&response, &project, &source, &other).is_none());
        let saved = std::fs::read_to_string(&config_path)?;
        assert!(saved.contains("# Preserve this comment"));
        assert!(saved.contains("test-model"));
        assert!(saved.contains("untrusted"));
        child.kill().await?;
        child.wait().await?;
        Ok(())
    }
}
