use crate::{
    attachment,
    cli::{PrAction, TaskAction},
    database::Home,
    peers::Peer,
};
use aide_core::{
    Command, Lifecycle,
    peer::{Operation, Outcome},
};
use anyhow::{Context, Result, bail};
use std::{
    collections::BTreeMap,
    io::{self, IsTerminal, Write},
    time::Duration,
};

fn known(home: &Home) -> Result<Vec<Peer>> {
    match std::fs::read(home.path.join("known-peers.json")) {
        Ok(bytes) => Ok(serde_json::from_slice::<BTreeMap<String, Peer>>(&bytes)?
            .into_values()
            .collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(error) => Err(error.into()),
    }
}

pub async fn list(home: &Home) -> Result<()> {
    for peer in known(home)? {
        let state = match peer.call(Operation::List, Duration::from_secs(5)).await {
            Ok(_) => "Connected".to_owned(),
            Err(error) => format!("Disconnected: {error}"),
        };
        println!(
            "{}\t{}\t{}",
            peer.identity.name, peer.identity.instance, state
        );
    }
    Ok(())
}

pub async fn run(
    home: &Home,
    selector: Option<&str>,
    project_selector: Option<&str>,
    action: TaskAction,
) -> Result<()> {
    if selector.is_none() {
        let operations = crate::operations::Operations::new(home.clone());
        if let TaskAction::List { completed } = &action {
            for project in operations.snapshot().await {
                if project_selector.is_some_and(|s| s != project.name && s != project.root) {
                    continue;
                }
                for task in project.tasks {
                    if !completed && task.task.lifecycle == Lifecycle::Completed {
                        continue;
                    }
                    println!(
                        "local\t{}\t{}\t{:?}\t{}",
                        project.name, task.task.id, task.status, task.reason
                    );
                }
            }
        }
    }
    let mut peers = known(home)?;
    if let Some(selector) = selector {
        peers.retain(|peer| peer.identity.name == selector || peer.identity.instance == selector);
        if peers.len() != 1 {
            bail!("Select exactly one discovered peer by name or instance ID");
        }
    } else if !matches!(action, TaskAction::List { .. }) {
        bail!("--all-peers is only supported for task list");
    }
    for peer in peers {
        let result = peer.call(Operation::List, Duration::from_secs(5)).await;
        let Outcome::Projects { projects } = (match result {
            Ok(result) => result,
            Err(error) if selector.is_none() => {
                println!("{}\tDisconnected: {error}", peer.identity.name);
                continue;
            }
            Err(error) => return Err(error),
        }) else {
            bail!("Unexpected peer response");
        };
        if let TaskAction::List { completed } = &action {
            for project in projects {
                if project_selector.is_some_and(|s| s != project.name && s != project.root) {
                    continue;
                }
                for task in project.tasks {
                    if !completed && task.task.lifecycle == Lifecycle::Completed {
                        continue;
                    }
                    println!(
                        "{}\t{}\t{}\t{:?}\t{}",
                        peer.identity.name, project.name, task.task.id, task.status, task.reason
                    );
                }
            }
            continue;
        }
        let candidates: Vec<_> = projects
            .iter()
            .filter(|p| project_selector.is_none_or(|s| s == p.name || s == p.root))
            .collect();
        if candidates.len() != 1 {
            bail!("Choose the remote project with --project");
        }
        let project = candidates[0];
        let task = |id: &str| {
            project
                .tasks
                .iter()
                .find(|task| task.task.id == id)
                .context("Task not found; refresh task list")
        };
        let root = project.root.clone();
        let request_id = uuid::Uuid::new_v4().to_string();
        let command = match &action {
            TaskAction::Start {
                id,
                repo,
                workspace_only,
                ..
            } => {
                if *workspace_only {
                    bail!("Remote workspace-only creation is not supported; use managed start");
                }
                Command::Start {
                    project: root,
                    id: id.clone(),
                    repository: repo.clone(),
                    open_terminal: false,
                }
            }
            TaskAction::Attach { id, new_window } => {
                let task = &task(id)?.task;
                if *new_window {
                    return attachment::open_remote(home, &peer, &root, task);
                }
                let args = attachment::remote_command(home, &peer, &root, task)?;
                let status = std::process::Command::new(&args[0])
                    .args(&args[1..])
                    .status()?;
                if !status.success() {
                    bail!("SSH attachment failed");
                }
                return Ok(());
            }
            TaskAction::Park { id } | TaskAction::Unpark { id } => Command::SetParked {
                project: root,
                id: id.clone(),
                parked: matches!(action, TaskAction::Park { .. }),
            },
            TaskAction::Recover { id } => Command::Recover {
                project: root,
                id: id.clone(),
                environment: None,
            },
            TaskAction::Cleanup { id } => {
                let task = task(id)?;
                println!(
                    "Close {} / {} / {} and remove {}? {}. History is retained.",
                    peer.identity.name, project.name, id, task.task.workspace, task.reason
                );
                if !io::stdin().is_terminal() {
                    bail!("Cleanup requires interactive confirmation");
                }
                print!("Type {id} to confirm: ");
                io::stdout().flush()?;
                let mut answer = String::new();
                io::stdin().read_line(&mut answer)?;
                if answer.trim() != id {
                    println!("Cancelled");
                    return Ok(());
                }
                Command::Cleanup {
                    project: root,
                    id: id.clone(),
                    confirmed: true,
                }
            }
            TaskAction::Pr { id, command } => match command {
                PrAction::Link { url } => Command::LinkPr {
                    project: root,
                    id: id.clone(),
                    url: url.clone(),
                    request_id,
                },
                PrAction::Unlink { url } => Command::UnlinkPr {
                    project: root,
                    id: id.clone(),
                    url: url.clone(),
                    request_id,
                },
                PrAction::Refresh => Command::RefreshPrs {
                    project: root,
                    id: id.clone(),
                    request_id,
                },
                PrAction::List => {
                    for pr in &task(id)?.pull_requests {
                        println!("{}\t{}", pr.id.url(), pr.label());
                    }
                    return Ok(());
                }
            },
            _ => bail!("This action is not available through the peer API"),
        };
        let key = project
            .tasks
            .iter()
            .find(|task| command.target().is_some_and(|(_, id)| task.task.id == id))
            .map(|task| task.task.key.clone());
        match peer
            .call(Operation::Execute { command, key }, Duration::from_secs(60))
            .await?
        {
            Outcome::Success { message } => println!("{message}"),
            _ => bail!("Unexpected operation response"),
        }
        if let TaskAction::Start { id, new_window, .. } = &action
            && let Outcome::Projects { projects } =
                peer.call(Operation::List, Duration::from_secs(5)).await?
        {
            let task = projects
                .iter()
                .find(|p| p.root == project.root)
                .and_then(|p| p.tasks.iter().find(|t| t.task.id == *id))
                .context("Task started; refresh to attach")?;
            if *new_window {
                attachment::open_remote(home, &peer, &project.root, &task.task)?;
            } else {
                let args = attachment::remote_command(home, &peer, &project.root, &task.task)?;
                if !std::process::Command::new(&args[0])
                    .args(&args[1..])
                    .status()?
                    .success()
                {
                    bail!("Task started; SSH attachment failed");
                }
            }
        }
    }
    Ok(())
}
