use crate::{app::App, http, session::begin, store::Store, vcs};
use anyhow::Result;
use axum::{Router, routing::get};
use clap::{Parser, Subcommand};
use review_core::*;
use std::{future::IntoFuture, path::PathBuf, sync::Arc};
use tokio::sync::{Mutex, Notify, broadcast};

#[derive(Parser)]
#[command(version, about = "A resumable guided review of your workspace changes")]
struct Cli {
    #[arg(long, default_value = ".", global = true)]
    path: PathBuf,
    /// Reuse this review's port by default; pass 0 to choose a new available port.
    #[arg(long)]
    port: Option<u16>,
    #[arg(long)]
    no_open: bool,
    /// Serve assets from a directory during development.
    #[arg(long)]
    assets: Option<PathBuf>,
    /// Use a deterministic local harness for UI development and acceptance tests.
    #[arg(long)]
    fixture: bool,
    #[command(subcommand)]
    command: Option<Action>,
}
#[derive(Subcommand)]
enum Action {
    Conclude,
    History,
    Doctor,
}
pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    let dir = cli.path.canonicalize()?;
    if matches!(cli.command, Some(Action::Doctor)) {
        for name in ["codex", "git", "jj"] {
            match std::process::Command::new(name).arg("--version").output() {
                Ok(o) => println!("{}", String::from_utf8_lossy(&o.stdout).trim()),
                Err(e) => println!("{name}: {e}"),
            }
        }
        return Ok(());
    }
    if cli.command.is_some() && !crate::state_dir::StateDirectory::exists(&dir) {
        anyhow::bail!("No review exists in this directory. Run review first.");
    }
    let lock = crate::lock::ReviewLock::acquire(&dir)?;
    let mut store = if cli.command.is_some() {
        Store::existing(&dir)?
    } else {
        Store::open(&dir, vcs::capture(&dir)?)?
    };
    if let Some(command) = cli.command {
        match command {
            Action::Conclude => {
                store.conclude()?;
                store.save("conclude_cli", None)?;
                println!(
                    "{}",
                    store
                        .review
                        .export_path
                        .as_deref()
                        .unwrap_or(NO_FEEDBACK_MESSAGE)
                );
                store.cleanup()?;
                drop(lock);
            }
            Action::History => {
                let mut q = store
                    .db
                    .prepare("SELECT state FROM reviews ORDER BY rowid DESC")?;
                for state in q.query_map([], |r| r.get::<_, String>(0))? {
                    let r: Review = serde_json::from_str(&state?)?;
                    println!("{}  {}  {} feedback", r.id, r.status, r.feedback.len());
                }
            }
            Action::Doctor => unreachable!(),
        };
        return Ok(());
    }
    let (events, _) = broadcast::channel(128);
    let app = Arc::new(App {
        store: Mutex::new(store),
        agent: Mutex::new(None),
        events,
        fixture: cli.fixture,
        assets: cli.assets,
        shutdown: Notify::new(),
    });
    let port = match cli.port {
        Some(port) => port,
        None => app.store.lock().await.port()?.unwrap_or(0),
    };
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Could not bind review port {port}: {e}. Use --port 0 to choose another port."
            )
        })?;
    let address = listener.local_addr()?;
    app.store.lock().await.set_port(address.port())?;
    let url = format!("http://{address}");
    println!("Review: {url}");
    if !cli.no_open {
        crate::browser::BrowserLauncher::spawn(app.clone(), url.clone());
    }
    crate::progress::TerminalProgress::spawn(app.clone(), url.clone());
    let a = app.clone();
    tokio::spawn(async move {
        let state = a.store.lock().await.review.clone();
        if state.guide.is_empty() && state.thread_id.is_none() && state.error.is_none() {
            let _ = begin(&a, None).await;
        }
    });
    let router = Router::new()
        .route("/ws", get(http::upgrade))
        .fallback(get(http::asset))
        .with_state(app.clone());
    // Dropping the serve future closes WebSockets promptly; native partial state is durable.
    tokio::select! {
        result = axum::serve(listener, router).into_future() => { result?; },
        _ = tokio::signal::ctrl_c() => {},
        _ = app.shutdown.notified() => {},
    }
    if let Some(agent) = app.agent.lock().await.take() {
        agent.stop().await;
    }
    let mut store = app.store.lock().await;
    if store.review.status == ReviewStatus::Completed {
        store.cleanup()?;
        drop(lock);
    }
    Ok(())
}
