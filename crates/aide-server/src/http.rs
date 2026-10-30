use crate::{
    database::{Home, Lock, Project},
    runtime::Runtime,
    workspace,
};
use aide_core::{Command, Event, Lifecycle, ProjectView};
use anyhow::{Context, Result, bail};
use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    future::IntoFuture,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct Manager {
    pub home: Home,
    pub runtime: Runtime,
    pub token: String,
    pub assets: Option<PathBuf>,
}
#[derive(Serialize, Deserialize)]
pub struct Address {
    pub url: String,
    pub token: String,
}

impl Manager {
    pub fn new(home: Home, assets: Option<PathBuf>) -> Self {
        Self {
            runtime: Runtime::new(&home),
            home,
            assets,
            token: uuid::Uuid::new_v4().to_string(),
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
                                let mut view = self.runtime.project_view(Path::new(&root), t).await;
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

pub async fn serve(home: Home, port: u16, no_open: bool, assets: Option<PathBuf>) -> Result<()> {
    let _lock = Lock::acquire(&home.path.join("manager.lock"))
        .context("Aide dashboard is already running")?;
    let manager = Arc::new(Manager::new(home.clone(), assets));
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let url = format!("http://{}", listener.local_addr()?);
    let address = Address {
        url: url.clone(),
        token: manager.token.clone(),
    };
    fs::write(
        home.path.join("manager.json"),
        serde_json::to_vec(&address)?,
    )?;
    println!("Aide: {url}");
    let restoring = manager.clone();
    let restoration = tokio::spawn(async move { restoring.restore().await });
    if !no_open {
        webbrowser::open(&url)?;
    }
    let router = Router::new()
        .route("/ws", get(upgrade))
        .fallback(get(asset))
        .with_state(manager);
    tokio::select! {
        result = axum::serve(listener, router).into_future() => result?,
        _ = tokio::signal::ctrl_c() => {},
    }
    restoration.abort();
    let _ = fs::remove_file(home.path.join("manager.json"));
    Ok(())
}

#[derive(rust_embed::RustEmbed)]
#[folder = "$OUT_DIR/ui/"]
struct Assets;
async fn asset(State(manager): State<Arc<Manager>>, uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    if path.contains("..") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let data = match &manager.assets {
        Some(dir) => fs::read(dir.join(path)).ok(),
        None => Assets::get(path).map(|v| v.data.into_owned()),
    };
    match data {
        Some(data) => (
            [
                (
                    "content-type",
                    mime_guess::from_path(path)
                        .first_or_octet_stream()
                        .to_string(),
                ),
                ("cache-control", "no-store".into()),
            ],
            data,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
async fn upgrade(
    State(manager): State<Arc<Manager>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    let authorised = match origin {
        Some(origin) => origin == format!("http://{host}") && host.starts_with("127.0.0.1:"),
        None => {
            headers.get("authorization").and_then(|v| v.to_str().ok())
                == Some(format!("Bearer {}", manager.token).as_str())
        }
    };
    if !authorised {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.max_message_size(1024 * 1024)
        .on_upgrade(move |socket| connection(manager, socket))
}
async fn connection(manager: Arc<Manager>, socket: WebSocket) {
    let (mut writer, mut reader) = socket.split();
    let (events, mut incoming) = mpsc::channel(32);
    let snapshots = events.clone();
    let observer = manager.clone();
    let observation = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        loop {
            tick.tick().await;
            let event = Event::Snapshot {
                projects: observer.snapshot().await,
            };
            if snapshots.send(event).await.is_err() {
                break;
            }
        }
    });
    loop {
        tokio::select! {
            message = reader.next() => {
                let Some(Ok(Message::Text(text))) = message else { break; };
                match serde_json::from_str::<Command>(&text) {
                    Ok(command) => {
                        let manager = manager.clone();
                        let events = events.clone();
                        // Lifecycle work continues if the browser disconnects.
                        // Per-task/repository locks coordinate other clients too.
                        tokio::spawn(async move {
                            let result = manager.execute(command).await;
                            let event = match result {
                                Ok(message) => Event::Result { message, error: false },
                                Err(error) => Event::Result { message: format!("{error:#}"), error: true },
                            };
                            let _ = events.send(event).await;
                        });
                    }
                    Err(error) => {
                        let event = Event::Result { message: error.to_string(), error: true };
                        if writer.send(Message::Text(serde_json::to_string(&event).unwrap().into())).await.is_err() { break; }
                    }
                }
            },
            event = incoming.recv() => {
                let Some(event) = event else { break; };
                if writer.send(Message::Text(serde_json::to_string(&event).unwrap().into())).await.is_err() { break; }
            }
        }
    }
    observation.abort();
}

pub async fn request(home: &Home, command: Command) -> Result<()> {
    use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};
    let address: Address = serde_json::from_slice(
        &fs::read(home.path.join("manager.json")).context("Start aide dashboard mode first")?,
    )?;
    let mut request =
        format!("{}/ws", address.url.replacen("http:", "ws:", 1)).into_client_request()?;
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {}", address.token))?,
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .context("Dashboard unavailable; start aide first")?;
    socket
        .send(Message::Text(serde_json::to_string(&command)?.into()))
        .await?;
    while let Some(message) = socket.next().await {
        if let Message::Text(text) = message?
            && let Event::Result { message, error } = serde_json::from_str(&text)?
        {
            if error {
                bail!("{message}");
            }
            println!("{message}");
            return Ok(());
        }
    }
    bail!("Dashboard disconnected; inspect task status before retrying")
}
