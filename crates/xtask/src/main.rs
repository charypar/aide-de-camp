use anyhow::{Context, Result, bail};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use tokio::process::{Child, Command};

struct Development {
    root: PathBuf,
    workspace: PathBuf,
    watcher: Option<Child>,
    server: Option<Child>,
}

impl Development {
    fn new(workspace: PathBuf) -> Result<Self> {
        Ok(Self {
            root: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()?,
            workspace: workspace
                .canonicalize()
                .context("Find the repository to review")?,
            watcher: None,
            server: None,
        })
    }

    fn command(&self, executable: &str) -> Command {
        let mut command = Command::new(executable);
        command.current_dir(&self.root).kill_on_drop(true);
        command
    }

    fn stamp(&self) -> Result<Vec<(PathBuf, SystemTime)>> {
        let mut files = Vec::new();
        for path in [
            "crates/review-core",
            "crates/review-server",
            "skills",
            "Cargo.toml",
            "Cargo.lock",
        ] {
            Self::collect(&self.root.join(path), &mut files)?;
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(files)
    }

    fn collect(path: &Path, files: &mut Vec<(PathBuf, SystemTime)>) -> Result<()> {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.is_dir() {
            for entry in std::fs::read_dir(path)? {
                Self::collect(&entry?.path(), files)?;
            }
        } else if metadata.is_file()
            && matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("rs" | "toml" | "lock" | "md")
            )
        {
            files.push((path.to_owned(), metadata.modified()?));
        }
        Ok(())
    }

    async fn stop(child: &mut Option<Child>) {
        if let Some(mut child) = child.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }

    async fn watch(&mut self) -> Result<()> {
        self.watcher = Some(
            self.command("trunk")
                .args(["watch", "--locked"])
                .current_dir(self.root.join("crates/review-ui"))
                .env_remove("NO_COLOR")
                .spawn()?,
        );
        let mut previous = Vec::new();
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            if let Some(status) = self.watcher.as_mut().unwrap().try_wait()? {
                bail!("Trunk watcher exited: {status}");
            }
            let current = self.stamp()?;
            if current != previous {
                previous = current;
                Self::stop(&mut self.server).await;
                if self
                    .command("cargo")
                    .args(["build", "--locked", "-p", "review-server"])
                    .status()
                    .await?
                    .success()
                {
                    self.server = Some(
                        self.command(self.root.join("target/debug/review").to_str().unwrap())
                            .arg("--path")
                            .arg(&self.workspace)
                            .arg("--assets")
                            .arg(self.root.join("dist"))
                            .args(["--port", "4317", "--no-open"])
                            .spawn()?,
                    );
                    println!(
                        "UI changes rebuild automatically; refresh the browser after a rebuild."
                    );
                } else {
                    eprintln!("Build failed. Fix the source and the watcher will try again.");
                }
            }
            if let Some(server) = self.server.as_mut()
                && let Some(status) = server.try_wait()?
            {
                if status.success() {
                    println!("Review submitted. Development server stopped.");
                    return Ok(());
                }
                bail!("Review server exited: {status}");
            }
        }
    }

    async fn run(mut self) -> Result<()> {
        let result = tokio::select! {
            result = self.watch() => result,
            signal = tokio::signal::ctrl_c() => signal.map_err(Into::into),
        };
        Self::stop(&mut self.server).await;
        Self::stop(&mut self.watcher).await;
        result
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("dev")) {
        bail!("Usage: cargo run -p xtask -- dev [repository]");
    }
    Development::new(
        args.next()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".")),
    )?
    .run()
    .await
}
