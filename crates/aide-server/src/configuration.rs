use aide_core::{ConfigAction, ConfigData, ConfigDocument, ConfigListing};
use anyhow::{Context, Result, bail};
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::Command,
};

const MAX_BYTES: usize = 256 * 1024;
const MAX_ENTRIES: usize = 2048;

pub struct AgentConfig {
    root: PathBuf,
}

impl AgentConfig {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn execute(&self, action: ConfigAction) -> Result<ConfigData> {
        match action {
            ConfigAction::List => self.list().map(ConfigData::Listing),
            ConfigAction::Read { path } => self.read(&path).map(ConfigData::Document),
            ConfigAction::Open { path, line } => {
                let args = self.editor_args(path.as_deref(), line)?;
                let status = Command::new("zed")
                    .args(args)
                    .status()
                    .context("Cannot open Zed; copy the path or reference instead")?;
                if !status.success() {
                    bail!("Zed failed to open the configuration (exit {status})");
                }
                Ok(ConfigData::Opened)
            }
        }
    }

    fn supported(path: &Path) -> bool {
        let parts: Vec<_> = path.components().collect();
        if parts.is_empty() || parts.iter().any(|p| !matches!(p, Component::Normal(name) if !name.to_string_lossy().starts_with('.'))) {
            return false;
        }
        if path == Path::new("methodology.md") || path == Path::new("projects.json") {
            return true;
        }
        parts.len() > 1
            && matches!(
                parts[0].as_os_str().to_str(),
                Some("projects" | "skills" | "roles")
            )
            && matches!(
                path.extension().and_then(|s| s.to_str()),
                Some(
                    "md" | "txt"
                        | "json"
                        | "toml"
                        | "yaml"
                        | "yml"
                        | "rst"
                        | "sh"
                        | "py"
                        | "js"
                        | "ts"
                )
            )
    }

    fn root_exists(&self) -> Result<bool> {
        match fs::symlink_metadata(&self.root) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(true),
            Ok(_) => bail!("Configuration root must be a directory, not a symlink"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e).context("Cannot access configuration folder"),
        }
    }

    fn resolve(&self, id: &str) -> Result<PathBuf> {
        if !Self::supported(Path::new(id)) {
            bail!("Unsupported configuration path");
        }
        if !self.root_exists()? {
            bail!("Configuration folder does not exist");
        }
        let mut path = self.root.clone();
        for component in Path::new(id).components() {
            path.push(component);
            if fs::symlink_metadata(&path)
                .with_context(|| format!("Cannot access {id}"))?
                .file_type()
                .is_symlink()
            {
                bail!("Symlinked configuration files and directories are not supported");
            }
        }
        if !path.is_file() {
            bail!("Configuration path is not a regular file");
        }
        Ok(path)
    }

    pub fn list(&self) -> Result<ConfigListing> {
        let exists = self.root_exists()?;
        let mut files = Vec::new();
        if exists {
            self.walk(&self.root, 0, &mut 0, &mut files)?;
        }
        files.sort();
        Ok(ConfigListing {
            root: self.root.display().to_string(),
            exists,
            files,
        })
    }

    fn walk(
        &self,
        dir: &Path,
        depth: usize,
        visited: &mut usize,
        files: &mut Vec<String>,
    ) -> Result<()> {
        if depth > 12 {
            bail!("Configuration folder exceeds the browsing depth limit (12)");
        }
        for entry in fs::read_dir(dir).context("Cannot list configuration folder")? {
            let entry = entry?;
            *visited += 1;
            if *visited > MAX_ENTRIES {
                bail!("Configuration folder exceeds the browsing limit (2048 entries)");
            }
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir()
                && (depth > 0 || matches!(name.to_str(), Some("projects" | "skills" | "roles")))
            {
                self.walk(&path, depth + 1, visited, files)?;
            } else if kind.is_file() {
                let relative = path.strip_prefix(&self.root)?;
                if Self::supported(relative) {
                    files.push(relative.to_string_lossy().into_owned());
                }
            }
        }
        Ok(())
    }

    pub fn read(&self, id: &str) -> Result<ConfigDocument> {
        let path = self.resolve(id)?;
        let file = self.open_file(id)?;
        if !file.metadata()?.is_file() {
            bail!("Configuration path is not a regular file");
        }
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_BYTES {
            bail!("File exceeds the viewer limit (256 KiB); open it in Zed");
        }
        let text = String::from_utf8(bytes).context("File is not UTF-8 text; open it in Zed")?;
        if text.bytes().filter(|byte| *byte == b'\n').count() > 10_000 {
            bail!("File exceeds the viewer limit (10,000 lines); open it in Zed");
        }
        if text.contains('\0') {
            bail!("Binary files cannot be displayed; open the folder in Zed");
        }
        Ok(ConfigDocument {
            path: id.into(),
            absolute_path: path.display().to_string(),
            text,
        })
    }

    // Open each component relative to the previous directory descriptor, so a
    // concurrent replacement with a symlink cannot redirect a content read.
    fn open_file(&self, id: &str) -> Result<fs::File> {
        use std::{
            ffi::CString,
            os::fd::{AsRawFd, FromRawFd},
            os::unix::ffi::OsStrExt,
        };
        let mut file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
            .open(&self.root)
            .context("Cannot open configuration folder")?;
        let components: Vec<_> = Path::new(id).components().collect();
        for (index, component) in components.iter().enumerate() {
            let name = CString::new(component.as_os_str().as_bytes())?;
            let mut flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
            if index + 1 < components.len() {
                flags |= libc::O_DIRECTORY;
            }
            // SAFETY: name is NUL-terminated and file owns a valid directory fd.
            let fd = unsafe { libc::openat(file.as_raw_fd(), name.as_ptr(), flags) };
            if fd < 0 {
                return Err(std::io::Error::last_os_error())
                    .context("Cannot read configuration file");
            }
            // SAFETY: openat returned a fresh descriptor owned by this File.
            file = unsafe { fs::File::from_raw_fd(fd) };
        }
        Ok(file)
    }

    pub fn editor_args(&self, id: Option<&str>, line: u32) -> Result<Vec<String>> {
        if line == 0 {
            bail!("Line numbers start at 1");
        }
        let target = match id {
            Some(id) => format!("{}:{line}", self.resolve(id)?.display()),
            None => {
                if !self.root_exists()? {
                    bail!("Configuration folder does not exist; create it in your external editor");
                }
                self.root.display().to_string()
            }
        };
        Ok(vec![target])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn browse_refresh_and_missing_home_never_write() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("config with spaces");
        let service = AgentConfig::new(root.clone());
        assert!(!service.list()?.exists);
        assert!(!root.exists());
        std::fs::create_dir_all(root.join("skills/example/docs"))?;
        std::fs::create_dir_all(root.join(".git"))?;
        std::fs::write(root.join("methodology.md"), "First\nSecond\n")?;
        std::fs::write(
            root.join("projects.json"),
            "invalid mapping is still viewable",
        )?;
        std::fs::write(
            root.join("skills/example/docs/notes.txt"),
            "Supporting text",
        )?;
        std::fs::write(root.join("skills/example/icon.png"), [0, 255])?;
        std::fs::write(root.join(".git/config"), "private")?;
        assert_eq!(
            service.list()?.files,
            vec![
                "methodology.md",
                "projects.json",
                "skills/example/docs/notes.txt"
            ]
        );
        let doc = service.read("methodology.md")?;
        assert_eq!(doc.text, "First\nSecond\n");
        assert_eq!(
            service.editor_args(Some("methodology.md"), 2)?,
            vec![format!("{}:2", root.join("methodology.md").display())]
        );
        assert_eq!(
            service.editor_args(None, 1)?,
            vec![root.display().to_string()]
        );
        assert!(service.editor_args(Some("methodology.md"), 0).is_err());
        std::fs::write(root.join("methodology.md"), "Changed")?;
        assert_eq!(service.read("methodology.md")?.text, "Changed");
        std::fs::remove_file(root.join("methodology.md"))?;
        assert!(service.read("methodology.md").is_err());
        Ok(())
    }

    #[test]
    fn confines_paths_and_bounds_documents() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("config");
        std::fs::create_dir_all(root.join("skills"))?;
        let service = AgentConfig::new(root.clone());
        std::fs::write(temp.path().join("secret.md"), "secret")?;
        symlink(temp.path().join("secret.md"), root.join("methodology.md"))?;
        symlink(temp.path(), root.join("skills/escape"))?;
        for path in [
            "../secret.md",
            "/etc/passwd",
            "methodology.md",
            "skills/escape/secret.md",
            ".git/config",
        ] {
            assert!(service.read(path).is_err(), "{path}");
            assert!(service.editor_args(Some(path), 1).is_err(), "{path}");
        }
        assert!(service.list()?.files.is_empty());
        std::fs::write(root.join("projects.json"), [255])?;
        assert!(service.read("projects.json").is_err());
        std::fs::write(root.join("projects.json"), vec![b'x'; MAX_BYTES + 1])?;
        assert!(service.read("projects.json").is_err());
        assert_eq!(service.list()?.files, vec!["projects.json"]);
        Ok(())
    }
    #[test]
    fn reports_unreadable_files_and_limits_without_losing_listing() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("config");
        fs::create_dir_all(&root)?;
        let service = AgentConfig::new(root.clone());
        let path = root.join("methodology.md");
        fs::write(&path, "private")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000))?;
        assert!(service.read("methodology.md").is_err());
        assert_eq!(service.list()?.files, vec!["methodology.md"]);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        fs::write(&path, "\n".repeat(10_001))?;
        assert!(
            service
                .read("methodology.md")
                .unwrap_err()
                .to_string()
                .contains("10,000 lines")
        );
        fs::write(&path, "binary\0content")?;
        assert!(service.read("methodology.md").is_err());
        let nested = root.join("skills").join("nested/".repeat(13));
        fs::create_dir_all(nested)?;
        assert!(
            service
                .list()
                .unwrap_err()
                .to_string()
                .contains("depth limit")
        );
        Ok(())
    }
}
