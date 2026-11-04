use crate::{database::Home, rpc::Rpc};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, fs, io::Write, path::Path};

const MAX_BYTES: usize = 64 * 1024;

/// The authored text and effective developer instructions used by one task.
/// Stored separately from disposable workspaces and editable configuration.
#[derive(Debug, Serialize, Deserialize)]
pub struct Instructions {
    sources: Vec<Source>,
    developer_instructions: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Source {
    path: String,
    text: String,
}

impl Instructions {
    pub fn validate(home: &Home, key: &str, project: &Path) -> Result<()> {
        if Self::read(home, key)?.is_none() {
            Self::load(home, project)?;
        }
        Ok(())
    }

    pub fn read(home: &Home, key: &str) -> Result<Option<Self>> {
        let path = home.path.join("contexts").join(format!("{key}.json"));
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
                format!("Invalid task instruction snapshot: {}", path.display())
            })?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).context("Cannot read task instruction snapshot"),
        }
    }

    pub fn load(home: &Home, project: &Path) -> Result<Self> {
        let root = home.path.join("agent-config");
        let mut sources = Vec::new();
        Self::add_source(&mut sources, &root.join("methodology.md"), false)?;

        if let Some(mapping) = Self::read_text(&root.join("projects.json"), false)? {
            let projects: BTreeMap<String, String> = serde_json::from_str(&mapping)
                .context("Invalid agent-config/projects.json; expected project root to directory name mapping")?;
            if let Some(name) = projects.get(&project.display().to_string()) {
                crate::workspace::valid_id(name)
                    .context("Invalid instruction project directory")?;
                Self::add_source(
                    &mut sources,
                    &root.join("projects").join(name).join("instructions.md"),
                    true,
                )?;
            }
        }

        let instructions = Self {
            sources,
            developer_instructions: None,
        };
        instructions.compose(None)?;
        Ok(instructions)
    }

    fn read_text(path: &Path, required: bool) -> Result<Option<String>> {
        // Read a bounded amount even when a mistakenly selected file is very large.
        use std::io::Read;
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if !required && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => {
                return Err(error).with_context(|| format!("Cannot read {}", path.display()));
            }
        };
        let mut text = String::new();
        file.take((MAX_BYTES + 1) as u64)
            .read_to_string(&mut text)
            .with_context(|| format!("Cannot read UTF-8 instructions: {}", path.display()))?;
        if text.len() > MAX_BYTES {
            bail!("Instruction file exceeds 64 KiB: {}", path.display());
        }
        Ok(Some(text))
    }

    fn add_source(sources: &mut Vec<Source>, path: &Path, required: bool) -> Result<()> {
        if let Some(text) = Self::read_text(path, required)? {
            sources.push(Source {
                path: path.display().to_string(),
                text,
            });
        }
        Ok(())
    }

    fn compose(&self, native: Option<&str>) -> Result<Option<String>> {
        let active: Vec<_> = self
            .sources
            .iter()
            .filter(|s| !s.text.trim().is_empty())
            .collect();
        if active.is_empty() {
            return Ok(None);
        }
        let mut text = native.unwrap_or_default().to_owned();
        text.push_str("\n\n# Personal Aide working agreements\nApply these personal preferences alongside repository instructions. Preserve repository requirements; use these preferences where compatible. Project preferences refine the general methodology.\n");
        for source in active {
            text.push_str(&format!("\n## {}\n\n{}\n", source.path, source.text));
        }
        if text.len() > MAX_BYTES {
            bail!("Combined developer instructions exceed 64 KiB");
        }
        Ok(Some(text))
    }

    pub fn needs_native_config(&self) -> Result<bool> {
        Ok(self.compose(None)?.is_some())
    }

    pub async fn resolve(&mut self, workspace: &str, rpc: &mut Rpc) -> Result<()> {
        let config = rpc.call("config/read", json!({"cwd": workspace})).await?;
        if !config["config"].is_object() {
            bail!("Codex returned no effective configuration");
        }
        let native = &config["config"]["developer_instructions"];
        if !native.is_null() && !native.is_string() {
            bail!("Codex returned invalid developer instructions");
        }
        self.developer_instructions = self.compose(native.as_str())?;
        Ok(())
    }

    pub fn save(&self, home: &Home, key: &str) -> Result<()> {
        let directory = home.path.join("contexts");
        fs::create_dir_all(&directory)?;
        let mut file = tempfile::NamedTempFile::new_in(&directory)?;
        file.write_all(&serde_json::to_vec_pretty(self)?)?;
        file.as_file().sync_all()?;
        match file.persist_noclobber(directory.join(format!("{key}.json"))) {
            Ok(_) => Ok(()),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error).context("Cannot save task instructions"),
        }
    }

    pub fn resume_params(&self, id: &str) -> serde_json::Value {
        let mut params = json!({"threadId": id});
        if let Some(text) = &self.developer_instructions {
            params["developerInstructions"] = json!(text);
        }
        params
    }

    pub fn configure(&self, command: &mut std::process::Command) {
        if let Some(text) = &self.developer_instructions {
            command
                .arg("--config")
                .arg(format!("developer_instructions={}", json!(text)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_project_selection_composition_and_failures() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let home = Home::new(Some(temp.path().join("home")))?;
        let root = home.path.join("agent-config");
        fs::create_dir_all(root.join("projects/personal"))?;
        fs::write(root.join("methodology.md"), "Global methodology")?;
        fs::write(root.join("projects.json"), r#"{"/project/one":"personal"}"#)?;
        fs::write(
            root.join("projects/personal/instructions.md"),
            "Private project",
        )?;
        let selected = Instructions::load(&home, Path::new("/project/one"))?;
        let combined = selected
            .compose(Some("Native developer instructions"))?
            .unwrap();
        assert!(combined.starts_with("Native developer instructions"));
        assert!(combined.find("Global methodology") < combined.find("Private project"));
        assert!(
            !Instructions::load(&home, Path::new("/another/one"))?
                .compose(None)?
                .unwrap()
                .contains("Private project")
        );

        fs::remove_file(root.join("projects/personal/instructions.md"))?;
        assert!(Instructions::load(&home, Path::new("/project/one")).is_err());
        fs::write(
            root.join("projects.json"),
            r#"{"/project/one":"../escape"}"#,
        )?;
        assert!(Instructions::load(&home, Path::new("/project/one")).is_err());
        fs::write(root.join("projects.json"), "invalid")?;
        assert!(Instructions::load(&home, Path::new("/project/one")).is_err());
        Ok(())
    }

    #[test]
    fn snapshots_survive_edits_and_preserve_protocol_text() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let home = Home::new(Some(temp.path().to_owned()))?;
        let text = "TDD\nQuotes: \"hello\"; literal $(command) and `backticks`";
        let original = Instructions {
            sources: vec![],
            developer_instructions: Some(text.into()),
        };
        original.save(&home, "task")?;
        Instructions {
            sources: vec![],
            developer_instructions: Some("Changed".into()),
        }
        .save(&home, "task")?;
        let saved = Instructions::read(&home, "task")?.unwrap();
        assert_eq!(saved.resume_params("thread")["developerInstructions"], text);
        let mut command = std::process::Command::new("codex");
        saved.configure(&mut command);
        let args: Vec<_> = command.get_args().map(|s| s.to_str().unwrap()).collect();
        assert_eq!(args[0], "--config");
        assert_eq!(args[1], format!("developer_instructions={}", json!(text)));
        fs::write(home.path.join("contexts/task.json"), "broken")?;
        assert!(Instructions::read(&home, "task").is_err());
        Ok(())
    }

    #[test]
    fn absent_empty_invalid_utf8_and_oversized_files() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let home = Home::new(Some(temp.path().to_owned()))?;
        assert!(
            Instructions::load(&home, Path::new("/project"))?
                .compose(None)?
                .is_none()
        );
        let root = home.path.join("agent-config");
        fs::create_dir(&root)?;
        let file = root.join("methodology.md");
        fs::write(&file, " \n")?;
        assert!(
            Instructions::load(&home, Path::new("/project"))?
                .compose(None)?
                .is_none()
        );
        fs::write(&file, [0xff])?;
        assert!(Instructions::load(&home, Path::new("/project")).is_err());
        fs::write(&file, "x".repeat(MAX_BYTES + 1))?;
        assert!(Instructions::load(&home, Path::new("/project")).is_err());
        Ok(())
    }
}
