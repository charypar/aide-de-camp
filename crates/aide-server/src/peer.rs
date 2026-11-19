use crate::{
    database::{Home, Project},
    operations::Operations,
};
use aide_core::{
    Command,
    peer::{Identity, Operation, Outcome, Request, Response, VERSION},
};
use anyhow::{Context, Result, bail};
use std::path::Path;

#[derive(Clone)]
pub struct Host {
    pub identity: Identity,
    pub operations: Operations,
}

impl Host {
    pub fn new(home: Home) -> Result<Self> {
        let identity = Identity {
            executable: std::env::current_exe()?.display().to_string(),
            home: home.path.display().to_string(),
            version: VERSION,
            instance: home.instance_id()?,
            name: std::env::var("AIDE_NAME").unwrap_or_else(|_| {
                std::process::Command::new("hostname")
                    .output()
                    .ok()
                    .filter(|output| output.status.success())
                    .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
                    .unwrap_or_else(|| "Aide".into())
            }),
            desktop: std::env::var("AIDE_DESKTOP")
                .map(|v| v == "1")
                .unwrap_or(cfg!(target_os = "macos")),
        };
        Ok(Self {
            identity,
            operations: Operations::new(home),
        })
    }

    pub async fn respond(&self, request: Request) -> Response {
        let result = self
            .handle(request.version, &request.instance, request.operation)
            .await;
        Response {
            request_id: request.request_id,
            outcome: result.unwrap_or_else(|error| Outcome::Error {
                message: format!("{error:#}"),
            }),
        }
    }

    fn project(&self, root: &str) -> Result<Project> {
        let root = Path::new(root).canonicalize()?;
        if !self
            .operations
            .home
            .roots()?
            .contains(&root.display().to_string())
        {
            bail!("Project is not registered on this host");
        }
        Project::open(&root)
    }

    async fn handle(&self, version: u32, instance: &str, operation: Operation) -> Result<Outcome> {
        if version != VERSION {
            bail!("Unsupported peer protocol version");
        }
        if instance != self.identity.instance {
            bail!("Host identity changed; rediscover this peer");
        }
        match operation {
            Operation::List => Ok(Outcome::Projects {
                projects: self.operations.snapshot().await,
            }),
            Operation::Get { project, id, key } => {
                let project = self.project(&project)?;
                let task = project.task(&id)?;
                if task.key != key {
                    bail!("Task identity changed; refresh task list");
                }
                let mut task = self
                    .operations
                    .runtime
                    .project_view_with_github(&project.root, task, &self.operations.github)
                    .await;
                task.terminal_activity = crate::activity::ActivitySnapshot::capture()
                    .await
                    .for_task(&task.task);
                Ok(Outcome::Task {
                    task: Box::new(task),
                })
            }
            Operation::Execute { command, key } => {
                // Peer task management never launches an application on its owner.
                match &command {
                    Command::Attach { .. }
                    | Command::Editor { .. }
                    | Command::Review { .. }
                    | Command::Start {
                        open_terminal: true,
                        ..
                    } => bail!("Desktop actions belong to the dashboard host"),
                    Command::Recover {
                        environment: Some(_),
                        ..
                    } => bail!("Peer recovery uses the owning host environment"),
                    _ => {}
                }
                let (root, id) = command
                    .target()
                    .context("Peer API accepts task operations only")?;
                let project = self.project(root)?;
                if !matches!(command, Command::Start { .. }) {
                    let key = key.context("Task identity is required")?;
                    if project.task(id)?.key != key {
                        bail!("Task identity changed; refresh task list");
                    }
                }
                Ok(Outcome::Success {
                    message: self.operations.execute(command).await?,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(host: &Host, operation: Operation) -> Request {
        Request {
            version: VERSION,
            request_id: "1".into(),
            instance: host.identity.instance.clone(),
            operation,
        }
    }

    #[test]
    fn rejected_project_is_not_opened_or_migrated() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let home = Home::new(Some(temp.path().join("home")))?;
        let root = temp.path().join("unregistered");
        std::fs::create_dir_all(root.join(".aide"))?;
        let db = rusqlite::Connection::open(root.join(".aide/state.sqlite"))?;
        db.execute_batch("PRAGMA application_id=1095320645; PRAGMA user_version=1;")?;
        assert!(Host::new(home)?.project(root.to_str().unwrap()).is_err());
        let tables: i64 = db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            tables, 0,
            "Rejected requests must not migrate an unregistered database"
        );
        Ok(())
    }

    #[tokio::test]
    async fn reads_do_not_prepare_tasks_and_wrong_host_cannot_execute() -> Result<()> {
        let (_temp, home, project) = crate::workspace::tests::fixture()?;
        let host = Host::new(home.clone())?;
        assert_eq!(Host::new(home)?.identity.instance, host.identity.instance);
        let response = host.respond(request(&host, Operation::List)).await;
        assert!(
            matches!(response.outcome, Outcome::Projects { projects } if projects.len() == 1 && projects[0].tasks.is_empty())
        );
        let mut wrong = request(
            &host,
            Operation::Execute {
                command: Command::Start {
                    project: project.root.display().to_string(),
                    id: "not-created".into(),
                    repository: None,
                    open_terminal: false,
                },
                key: None,
            },
        );
        wrong.instance = "different-host".into();
        assert!(matches!(
            host.respond(wrong).await.outcome,
            Outcome::Error { .. }
        ));
        assert!(project.tasks()?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn peer_rejects_configuration_requests_without_panicking() -> Result<()> {
        let (_temp, home, _) = crate::workspace::tests::fixture()?;
        let host = Host::new(home)?;
        let response = host
            .respond(request(
                &host,
                Operation::Execute {
                    command: Command::Configuration {
                        request_id: "config".into(),
                        operation: aide_core::configuration::ConfigAction::List,
                    },
                    key: None,
                },
            ))
            .await;
        assert!(
            matches!(response.outcome, Outcome::Error { message } if message.contains("task operations only"))
        );
        Ok(())
    }

    #[tokio::test]
    async fn peer_rejects_desktop_launch_and_foreign_environment() -> Result<()> {
        let (_temp, home, project) = crate::workspace::tests::fixture()?;
        let host = Host::new(home)?;
        for command in [
            Command::Attach {
                project: project.root.display().to_string(),
                id: "missing".into(),
            },
            Command::Recover {
                project: project.root.display().to_string(),
                id: "missing".into(),
                environment: Some(Default::default()),
            },
        ] {
            let response = host
                .respond(request(&host, Operation::Execute { command, key: None }))
                .await;
            assert!(matches!(response.outcome, Outcome::Error { .. }));
        }
        assert!(project.tasks()?.is_empty());
        Ok(())
    }
}
