use crate::{
    database::{Home, Project},
    runtime::Runtime,
    workspace,
};
use aide_core::{Command, Lifecycle, ProjectView};
use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use std::path::Path;

/// Host-owned operations, independent of browser and peer transports.
#[derive(Clone)]
pub struct Operations {
    pub home: Home,
    pub runtime: Runtime,
    pub github: crate::github::Github,
}

impl Operations {
    pub fn new(home: Home) -> Self {
        Self {
            runtime: Runtime::new(&home),
            home,
            github: crate::github::Github::default(),
        }
    }
    pub async fn snapshot(&self) -> Vec<ProjectView> {
        let activity = crate::activity::ActivitySnapshot::capture().await;
        let mut projects = Vec::new();
        for root in self.home.roots().unwrap_or_default() {
            let name = Path::new(&root)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let mut view = ProjectView {
                source: None,
                root: root.clone(),
                name,
                repositories: vec![],
                tasks: vec![],
                error: None,
            };
            match Project::open(Path::new(&root)) {
                Ok(project) => match (project.repositories(), project.tasks()) {
                    (Ok(repositories), Ok(tasks)) => {
                        view.repositories = repositories;
                        view.tasks = futures_util::stream::iter(tasks)
                            .map(|t| async {
                                let mut view = self
                                    .runtime
                                    .project_view_with_github(Path::new(&root), t, &self.github)
                                    .await;
                                view.terminal_activity = activity.for_task(&view.task);
                                view
                            })
                            .buffered(8)
                            .collect()
                            .await;
                        for task in &mut view.tasks {
                            Runtime::refresh_lifecycle(&project, task);
                        }
                    }
                    (Err(e), _) | (_, Err(e)) => view.error = Some(e.to_string()),
                },
                Err(error) => view.error = Some(error.to_string()),
            }
            if let Some(error) = self.github.project_error(&root) {
                view.error = Some(match view.error {
                    Some(existing) => format!("{existing} · {error}"),
                    None => error,
                });
            }
            projects.push(view);
        }
        projects
    }
    pub async fn restore(&self) {
        for root in self.home.roots().unwrap_or_default() {
            if let Ok(mut project) = Project::open(Path::new(&root)) {
                for task in project.tasks().unwrap_or_default() {
                    if task.lifecycle == Lifecycle::Open
                        && task.desired_running
                        && !self.runtime.alive(&task)
                        && let Err(error) = self.runtime.ensure(&mut project, &task, true).await
                    {
                        let _ = project.update(&task.id, |t| {
                            t.error = Some(format!("Recovery failed: {error}"))
                        });
                    }
                }
            }
        }
    }
    pub async fn execute(&self, command: Command) -> Result<String> {
        match command {
            Command::Configuration { .. } => {
                bail!("Configuration requests require a correlated response")
            }
            Command::LinkPr {
                project, id, url, ..
            } => self.github.link(&self.home, &project, &id, &url).await,
            Command::UnlinkPr {
                project, id, url, ..
            } => self.github.unlink(&project, &id, &url),
            Command::RefreshPrs { project, id, .. } => {
                self.github.refresh(&self.home, &project, &id).await
            }
            Command::Start {
                project,
                id,
                repository,
                open_terminal,
            } => {
                let root = project.clone();
                let key = id.clone();
                let (task, transition) = tokio::task::spawn_blocking(move || {
                    workspace::start_managed(
                        &Project::open(Path::new(&root))?,
                        &key,
                        repository.as_deref(),
                    )
                })
                .await??;
                let mut project = Project::open(Path::new(&project))?;
                if let Err(error) = self
                    .runtime
                    .repair_locked(&mut project, &task, false, None, &transition)
                    .await
                {
                    project.update(&task.id, |t| {
                        t.error = Some(format!("Startup failed: {error}"))
                    })?;
                    return Err(error);
                }
                drop(transition);
                if open_terminal {
                    self.runtime.attach(&project, &task, true)?;
                }
                Ok(format!("Started {}", task.id))
            }
            Command::Attach { project, id } => {
                let mut project = Project::open(Path::new(&project))?;
                let task = project.task(&id)?;
                self.runtime.recover(&mut project, &task).await?;
                self.runtime.attach(&project, &task, true)?;
                Ok("Opened terminal".into())
            }
            Command::SetParked {
                project,
                id,
                parked,
            } => {
                let mut project = Project::open(Path::new(&project))?;
                self.runtime.set_parked(&mut project, &id, parked).await?;
                Ok(if parked {
                    "Task parked"
                } else {
                    "Task unparked"
                }
                .into())
            }
            Command::Recover {
                project,
                id,
                environment,
            } => {
                let mut project = Project::open(Path::new(&project))?;
                let task = project.task(&id)?;
                self.runtime
                    .repair(&mut project, &task, true, environment)
                    .await?;
                project.set_parked(&id, false)?;
                Ok("Session ready".into())
            }
            Command::Cleanup {
                project,
                id,
                confirmed,
            } => {
                let mut project = Project::open(Path::new(&project))?;
                self.runtime.cleanup(&mut project, &id, confirmed).await?;
                Ok(format!("Closed {id}"))
            }
            Command::Editor { project, id } => {
                let project = Project::open(Path::new(&project))?;
                let _transition = project.task_lock(&id)?;
                let task = project.task(&id)?;
                if task.lifecycle != Lifecycle::Open {
                    bail!("Task is not open");
                }
                workspace::verify_workspace(&task)?;
                std::process::Command::new("zed")
                    .arg(&task.workspace)
                    .spawn()
                    .context("Open Zed")?;
                project.set_parked(&id, false)?;
                Ok("Opened Zed".into())
            }
            Command::Review { project, id } => {
                let project = Project::open(Path::new(&project))?;
                let _transition = project.task_lock(&id)?;
                let task = project.task(&id)?;
                if task.lifecycle != Lifecycle::Open {
                    bail!("Task is not open");
                }
                workspace::verify_workspace(&task)?;
                let binary = crate::cli::review_binary();
                std::process::Command::new("alacritty")
                    .env_remove("TMUX")
                    .env_remove("TMUX_PANE")
                    .args([
                        "--working-directory",
                        &task.workspace,
                        "--title",
                        &format!("Review · {id}"),
                        "-e",
                    ])
                    .arg(binary)
                    .current_dir(&task.workspace)
                    .spawn()
                    .context("Open review terminal")?;
                project.set_parked(&id, false)?;
                Ok("Opened review".into())
            }
        }
    }
}
