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
    #[arg(long, global = true, help = "Personal data directory (also AIDE_HOME)")]
    home: Option<PathBuf>,
    #[arg(long, default_value_t = 0)]
    port: u16,
    #[arg(long)]
    no_open: bool,
    #[arg(long)]
    assets: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Action>,
}
#[derive(Subcommand)]
enum Action {
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
    Resume,
}
#[derive(Subcommand)]
enum TaskAction {
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

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Some(Action::Review { args }) = &cli.command {
        return review(&std::env::current_dir()?, args);
    }
    let home_path = if matches!(&cli.command, Some(Action::Codex { .. })) && cli.home.is_none() {
        Some(crate::terminal::Terminal::current_home()?)
    } else {
        cli.home
    };
    let home = Home::new(home_path)?;
    let runtime = Runtime::new(&home);
    match cli.command {
        None => http::serve(home, cli.port, cli.no_open, cli.assets).await,
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
            command: CodexAction::Resume,
        }) => runtime.resume().await,
        Some(Action::Doctor) => {
            for program in ["codex", "jj", "git", "tmux", "fish", "alacritty", "zed"] {
                let output = std::process::Command::new(program)
                    .arg("--version")
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
            let mut project = Project::discover(&std::env::current_dir()?)?;
            let root = project.root.display().to_string();
            match command {
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
                    if !runtime.healthy(&task).await {
                        http::request(
                            &home,
                            Command::Recover {
                                project: root,
                                id,
                                environment: Some(crate::terminal::environment()?),
                            },
                        )
                        .await?;
                    }
                    runtime.attach(&project, &task, new_window)?;
                }
                TaskAction::Recover { id } => {
                    http::request(
                        &home,
                        Command::Recover {
                            project: root,
                            id,
                            environment: Some(crate::terminal::environment()?),
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
