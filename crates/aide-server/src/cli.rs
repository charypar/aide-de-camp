use crate::{
    database::{Home, Project},
    http,
    runtime::Runtime,
    workspace,
};
use aide_core::{Command, Lifecycle};
use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use std::{
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::Stdio,
};

#[derive(Parser)]
#[command(version, about = "Aide de camp — task workspaces and conversations")]
struct Cli {
    #[arg(long, global = true)]
    peer: Option<String>,
    #[arg(long, global = true)]
    project: Option<PathBuf>,
    #[arg(long, global = true, conflicts_with = "peer")]
    all_peers: bool,
    #[arg(long, global = true, help = "Personal data directory (also AIDE_HOME)")]
    home: Option<PathBuf>,
    #[arg(long, default_value_t = 49173)]
    port: u16,
    #[arg(long)]
    no_open: bool,
    /// Use a preconfigured private Tailscale Serve route and discover peers.
    #[arg(long)]
    tailnet: bool,
    #[arg(long)]
    assets: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Action>,
}
#[derive(Subcommand)]
enum Action {
    Peer {
        #[command(subcommand)]
        command: PeerAction,
    },
    #[command(hide = true)]
    AttachTarget {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        key: String,
        #[arg(long)]
        instance: String,
    },
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long, required = true)]
        repo: Vec<PathBuf>,
    },
    Task {
        #[command(subcommand)]
        command: TaskAction,
    },
    #[command(disable_help_flag = true)]
    Review {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(hide = true)]
    InternalWorker {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        environment: PathBuf,
    },
    Codex {
        #[command(subcommand)]
        command: CodexAction,
    },
    Doctor,
}
#[derive(Subcommand)]
enum CodexAction {
    /// Reconnect in this task tmux session; restart a stopped backend if needed.
    Resume(crate::conversations::ResumeOptions),
}
#[derive(Subcommand)]
pub(crate) enum TaskAction {
    /// Link and inspect GitHub pull requests (uses the running dashboard).
    Pr {
        id: String,
        #[command(subcommand)]
        command: PrAction,
    },
    Start {
        id: String,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        workspace_only: bool,
        #[arg(long)]
        new_window: bool,
    },
    List {
        #[arg(long)]
        completed: bool,
    },
    Attach {
        id: String,
        #[arg(long)]
        new_window: bool,
    },
    Detach,
    /// Set an open task aside while waiting for something external.
    Park {
        id: String,
    },
    /// Return a parked task to the active list.
    Unpark {
        id: String,
    },
    #[command(hide = true)]
    Recover {
        id: String,
    },
    Cleanup {
        id: String,
    },
    #[command(disable_help_flag = true)]
    Review {
        id: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum PrAction {
    Link { url: String },
    Unlink { url: String },
    List,
    Refresh,
}

#[derive(Subcommand)]
enum PeerAction {
    List,
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Some(Action::Review { args }) = &cli.command {
        if cli.peer.is_some() || cli.all_peers {
            bail!("Remote guided review is not available yet");
        }
        return review(&std::env::current_dir()?, args);
    }
    let home_path = if matches!(&cli.command, Some(Action::Codex { .. })) && cli.home.is_none() {
        Some(crate::terminal::Terminal::current_home()?)
    } else {
        cli.home
    };
    let home = Home::new(home_path)?;
    if cli.peer.is_some() || cli.all_peers {
        let Some(Action::Task { command }) = cli.command else {
            bail!("Use --peer with a task command");
        };
        return crate::remote_cli::run(
            &home,
            cli.peer.as_deref(),
            cli.project.as_deref().and_then(Path::to_str),
            command,
        )
        .await;
    }
    let runtime = Runtime::new(&home);
    match cli.command {
        Some(Action::Peer {
            command: PeerAction::List,
        }) => crate::remote_cli::list(&home).await,
        Some(Action::AttachTarget {
            project,
            id,
            key,
            instance,
        }) => {
            if home.instance_id()? != instance {
                bail!("Aide host identity changed");
            }
            let mut project = Project::open(&project)?;
            let task = project.task(&id)?;
            if task.key != key {
                bail!("Task identity changed; refresh task list");
            }
            runtime.recover(&mut project, &task).await?;
            runtime.attach(&project, &task, false)
        }
        None => http::serve(home, cli.port, cli.no_open, cli.assets, cli.tailnet).await,
        Some(Action::Init { path, repo }) => {
            workspace::initialize(&home, &path, &repo)?;
            println!("Registered {}", path.canonicalize()?.display());
            Ok(())
        }
        Some(Action::InternalWorker {
            project,
            id,
            environment,
        }) => runtime.worker(&project, &id, &environment).await,
        Some(Action::Codex {
            command: CodexAction::Resume(options),
        }) => runtime.resume(options).await,
        Some(Action::Doctor) => {
            for program in [
                "codex",
                "jj",
                "git",
                "tmux",
                "fish",
                "alacritty",
                "zed",
                "gh",
            ] {
                let output = std::process::Command::new(program)
                    .arg(if program == "tmux" { "-V" } else { "--version" })
                    .output();
                println!(
                    "{program}: {}",
                    match output {
                        Ok(o) if o.status.success() =>
                            String::from_utf8_lossy(&o.stdout).trim().to_string(),
                        _ => "unavailable".into(),
                    }
                );
            }
            Ok(())
        }
        Some(Action::Review { .. }) => unreachable!(),
        Some(Action::Task { command }) => {
            if matches!(command, TaskAction::Detach) {
                return runtime.detach();
            }
            let mut project = match &cli.project {
                Some(root) => Project::open(Path::new(root))?,
                None => Project::discover(&std::env::current_dir()?)?,
            };
            let root = project.root.display().to_string();
            match command {
                TaskAction::Pr { id, command } => {
                    project.task(&id)?;
                    let request_id = uuid::Uuid::new_v4().to_string();
                    let command = match command {
                        PrAction::List => {
                            let historical = project.task(&id)?.lifecycle == Lifecycle::Completed;
                            for mut pr in project.pull_requests(&id)? {
                                pr.historical = historical;
                                pr.stale = !historical
                                    && (pr.error.is_some()
                                        || pr.observation.as_ref().is_none_or(|o| {
                                            crate::github::now()
                                                > o.observed_at.saturating_add(o.interval() + 30)
                                        }));
                                println!(
                                    "{}\t{}\t{}\t{}\tattempt={}\t{}",
                                    pr.id.url(),
                                    pr.observation
                                        .as_ref()
                                        .map(|o| o.role.label())
                                        .unwrap_or("Unknown"),
                                    pr.label(),
                                    pr.observation
                                        .as_ref()
                                        .map(|o| o.reasons.join("; "))
                                        .unwrap_or_default(),
                                    pr.last_attempt,
                                    pr.error.as_deref().unwrap_or(if historical {
                                        "Historical observation"
                                    } else {
                                        "Cached; use refresh for latest status"
                                    })
                                );
                            }
                            return Ok(());
                        }
                        PrAction::Link { url } => Command::LinkPr {
                            project: root,
                            id,
                            url,
                            request_id,
                        },
                        PrAction::Unlink { url } => Command::UnlinkPr {
                            project: root,
                            id,
                            url,
                            request_id,
                        },
                        PrAction::Refresh => Command::RefreshPrs {
                            project: root,
                            id,
                            request_id,
                        },
                    };
                    http::request(&home, command).await?;
                }
                TaskAction::Start {
                    id,
                    repo,
                    workspace_only,
                    new_window,
                } => {
                    if workspace_only {
                        let task = workspace::start(&project, &id, repo.as_deref())?;
                        println!("{}", task.workspace);
                    } else {
                        http::request(
                            &home,
                            Command::Start {
                                project: root,
                                id: id.clone(),
                                repository: repo,
                                open_terminal: false,
                            },
                        )
                        .await?;
                        runtime.attach(&project, &project.task(&id)?, new_window)?;
                    }
                }
                TaskAction::List { completed } => {
                    let activity = crate::activity::ActivitySnapshot::capture().await;
                    for task in project.tasks()? {
                        if !completed && task.lifecycle == Lifecycle::Completed {
                            continue;
                        }
                        let terminal = activity.for_task(&task);
                        let view = runtime.project_view(&project.root, task).await;
                        println!(
                            "{}\t{:?}\t{}\t{}\tterminal={}",
                            view.task.id,
                            view.status,
                            view.reason,
                            view.task.workspace,
                            match terminal {
                                Some(aide_core::TerminalActivity::Idle) => "Idle",
                                Some(aide_core::TerminalActivity::Working) => "Working",
                                None => "Unavailable",
                            }
                        );
                    }
                }
                TaskAction::Attach { id, new_window } => {
                    let task = project.task(&id)?;
                    runtime.recover(&mut project, &task).await?;
                    runtime.attach(&project, &task, new_window)?;
                }
                TaskAction::Recover { id } => {
                    http::request(
                        &home,
                        Command::Recover {
                            project: root,
                            id,
                            environment: None,
                        },
                    )
                    .await?
                }
                TaskAction::Cleanup { id } => {
                    let mut view = runtime.view(project.task(&id)?).await;
                    Runtime::refresh_lifecycle(&project, &mut view);
                    println!(
                        "Close {id} and remove {}? History and recordings will be retained.",
                        view.task.workspace
                    );
                    if view.backend_alive || view.terminal_alive {
                        println!(
                            "Warning: a task terminal or Codex session is open. {}",
                            view.reason
                        );
                    }
                    if !io::stdin().is_terminal() {
                        bail!(
                            "Cleanup requires interactive confirmation; use the dashboard when running without a terminal"
                        );
                    }
                    print!("Type {id} to confirm: ");
                    io::stdout().flush()?;
                    let mut answer = String::new();
                    io::stdin().read_line(&mut answer)?;
                    if answer.trim() != id {
                        println!("Cancelled");
                        return Ok(());
                    }
                    runtime.cleanup(&mut project, &id, true).await?;
                    println!("Closed {id}");
                }
                TaskAction::Park { id } => {
                    runtime.set_parked(&mut project, &id, true).await?;
                    println!("Parked {id}");
                }
                TaskAction::Unpark { id } => {
                    runtime.set_parked(&mut project, &id, false).await?;
                    println!("Unparked {id}");
                }
                TaskAction::Review { id, args } => {
                    let _transition = project.task_lock(&id)?;
                    let task = project.task(&id)?;
                    workspace::verify_workspace(&task)?;
                    if args
                        .iter()
                        .any(|s| s == "--path" || s.starts_with("--path="))
                    {
                        bail!("Task review uses the task workspace; --path cannot override it");
                    }
                    project.set_parked(&id, false)?;
                    review(Path::new(&task.workspace), &args)?;
                }
                TaskAction::Detach => unreachable!(),
            }
            Ok(())
        }
    }
}

pub fn review_binary() -> PathBuf {
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("aide-review")));
    sibling
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("aide-review"))
}
fn review(directory: &Path, args: &[String]) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let error = std::process::Command::new(review_binary())
        .args(args)
        .current_dir(directory)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .exec();
    Err(error.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dashboard_defaults_to_stable_port_with_ephemeral_override() {
        assert_eq!(Cli::try_parse_from(["aide"]).unwrap().port, 49173);
        assert_eq!(
            Cli::try_parse_from(["aide", "--port", "0"]).unwrap().port,
            0
        );
    }

    #[test]
    fn project_selector_remains_compatible_with_worker_and_attachment_commands() {
        let worker = Cli::try_parse_from([
            "aide",
            "internal-worker",
            "--project",
            "/tmp/project",
            "--id",
            "task",
            "--environment",
            "/tmp/env",
        ])
        .unwrap();
        assert!(
            matches!(worker.command, Some(Action::InternalWorker { project, .. }) if project == Path::new("/tmp/project"))
        );
        let attach = Cli::try_parse_from([
            "aide",
            "attach-target",
            "--project",
            "/tmp/project",
            "--id",
            "task",
            "--key",
            "key",
            "--instance",
            "host",
        ])
        .unwrap();
        assert!(
            matches!(attach.command, Some(Action::AttachTarget { project, .. }) if project == Path::new("/tmp/project"))
        );
        let remote = Cli::try_parse_from([
            "aide",
            "task",
            "list",
            "--peer",
            "vm",
            "--project",
            "project",
        ])
        .unwrap();
        assert_eq!(remote.peer.as_deref(), Some("vm"));
        assert_eq!(remote.project.as_deref(), Some(Path::new("project")));
    }
}

#[cfg(test)]
mod resume_tests {
    use super::*;

    #[test]
    fn resume_accepts_explicit_history_selection_without_changing_plain_resume() {
        for args in [
            vec!["aide", "codex", "resume"],
            vec!["aide", "codex", "resume", "--pick"],
            vec!["aide", "codex", "resume", "previous-thread"],
        ] {
            assert!(Cli::try_parse_from(&args).is_ok(), "{args:?}");
        }
        assert!(Cli::try_parse_from(["aide", "codex", "resume", "--pick", "id"]).is_err());
    }
}
