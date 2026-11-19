use crate::{
    database::{Home, Lock},
    operations::Operations,
};
use aide_core::{Command, Event};
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
use std::{fs, future::IntoFuture, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct Manager {
    pub host: crate::peer::Host,
    pub token: String,
    pub assets: Option<PathBuf>,
    pub public_origin: Option<String>,
    pub peers: Option<crate::peers::Peers>,
}
#[derive(Serialize, Deserialize)]
pub struct Address {
    pub url: String,
    pub token: String,
}

impl std::ops::Deref for Manager {
    type Target = Operations;

    fn deref(&self) -> &Operations {
        &self.host.operations
    }
}

impl Manager {
    fn configuration(&self, operation: aide_core::ConfigAction) -> Result<aide_core::ConfigData> {
        if matches!(operation, aide_core::ConfigAction::Open { .. }) && !self.host.identity.desktop
        {
            bail!(
                "This dashboard host cannot launch desktop applications; edit configuration through SSH or a remote editor"
            );
        }
        crate::configuration::AgentConfig::new(self.home.path.join("agent-config"))
            .execute(operation)
    }

    pub fn new(home: Home, assets: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            host: crate::peer::Host::new(home)?,
            assets,
            public_origin: None,
            peers: None,
            token: uuid::Uuid::new_v4().to_string(),
        })
    }
}

pub async fn serve(
    home: Home,
    port: u16,
    no_open: bool,
    assets: Option<PathBuf>,
    tailnet: bool,
) -> Result<()> {
    let _lock = Lock::acquire(&home.path.join("manager.lock"))
        .context("Aide dashboard is already running")?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let mut manager = Manager::new(home.clone(), assets)?;
    if tailnet {
        manager.public_origin =
            Some(crate::tailnet::configured_origin(listener.local_addr()?.port()).await?);
        manager.peers = Some(crate::peers::Peers::new(
            home.clone(),
            manager.host.identity.instance.clone(),
        )?);
    }
    let manager = Arc::new(manager);
    let discovery = manager.peers.clone().map(|peers| tokio::spawn(peers.run()));
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
    let polling_manager = manager.clone();
    let polling = tokio::spawn(async move {
        polling_manager
            .github
            .run(polling_manager.home.clone())
            .await
    });
    let restoring = manager.clone();
    let restoration = tokio::spawn(async move { restoring.restore().await });
    if !no_open {
        webbrowser::open(
            &manager
                .public_origin
                .as_ref()
                .map(|origin| format!("{origin}/aide/"))
                .unwrap_or_else(|| url.clone()),
        )?;
    }
    let router = Router::new()
        .route("/ws", get(upgrade))
        .route("/api/ws", get(peer_upgrade))
        .route("/api/identity", get(identity))
        .route("/aide/api/ws", get(peer_upgrade))
        .route("/aide/api/identity", get(identity))
        .route("/aide/ws", get(upgrade))
        .fallback(get(asset))
        .with_state(manager);
    tokio::select! {
        result = axum::serve(listener, router).into_future() => result?,
        _ = tokio::signal::ctrl_c() => {},
    }
    restoration.abort();
    if let Some(discovery) = discovery {
        discovery.abort();
    }
    polling.abort();
    let _ = fs::remove_file(home.path.join("manager.json"));
    Ok(())
}

#[derive(rust_embed::RustEmbed)]
#[folder = "$OUT_DIR/ui/"]
struct Assets;
async fn asset(State(manager): State<Arc<Manager>>, uri: Uri) -> Response {
    let path = uri
        .path()
        .strip_prefix("/aide/")
        .unwrap_or(uri.path())
        .trim_start_matches('/');
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
                ("content-security-policy", "frame-ancestors 'none'".into()),
                ("x-frame-options", "DENY".into()),
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
    if !authorized(&manager, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.max_message_size(1024 * 1024)
        .on_upgrade(move |socket| connection(manager, socket))
}

fn authorized(manager: &Manager, headers: &HeaderMap) -> bool {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    match origin {
        Some(origin) => {
            (origin == format!("http://{host}") && host.starts_with("127.0.0.1:"))
                || manager.public_origin.as_deref().is_some_and(|expected| {
                    origin == expected && expected == format!("https://{host}")
                })
        }
        None => {
            headers.get("authorization").and_then(|v| v.to_str().ok())
                == Some(format!("Bearer {}", manager.token).as_str())
                || manager
                    .public_origin
                    .as_deref()
                    .is_some_and(|expected| expected == format!("https://{host}"))
        }
    }
}

async fn identity(State(manager): State<Arc<Manager>>, headers: HeaderMap) -> Response {
    if !authorized(&manager, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    axum::Json(manager.host.identity.clone()).into_response()
}

async fn peer_upgrade(
    State(manager): State<Arc<Manager>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !authorized(&manager, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.max_message_size(1024 * 1024)
        .on_upgrade(move |socket| peer_connection(manager, socket))
}

async fn peer_connection(manager: Arc<Manager>, socket: WebSocket) {
    let (mut writer, mut reader) = socket.split();
    let (responses, mut incoming) = mpsc::channel::<aide_core::peer::Response>(32);
    loop {
        tokio::select! {
            message = reader.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        let Ok(request) = serde_json::from_str::<aide_core::peer::Request>(&text) else {
                            break;
                        };
                        let manager = manager.clone();
                        let responses = responses.clone();
                        // Dropping the connection does not cancel accepted execution.
                        tokio::spawn(async move {
                            let response = manager.host.respond(request).await;
                            let _ = responses.send(response).await;
                        });
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                    _ => break,
                }
            }
            response = incoming.recv() => {
                let Some(response) = response else { break; };
                let text = serde_json::to_string(&response).expect("Serializable peer response");
                if writer.send(Message::Text(text.into())).await.is_err() { break; }
            }
        }
    }
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
            let mut projects = observer.snapshot().await;
            let mut hosts = vec![];
            if let Some(peers) = &observer.peers {
                let (peer_hosts, peer_projects) = peers.views().await;
                hosts = peer_hosts;
                projects.extend(peer_projects);
            }
            if snapshots
                .send(Event::Hosts {
                    local: observer.host.identity.clone(),
                    hosts,
                })
                .await
                .is_err()
            {
                break;
            }
            let event = Event::Snapshot { projects };
            if snapshots.send(event).await.is_err() {
                break;
            }
        }
    });
    loop {
        tokio::select! {
            message = reader.next() => {
                let Some(Ok(Message::Text(text))) = message else { break; };
                let decoded = serde_json::from_str::<BrowserRequest>(&text).map(|request| (Some(request.request_id), request.command))
                    .or_else(|_| serde_json::from_str::<Command>(&text).map(|command| (None, command)));
                match decoded {
                    Ok((correlation, command)) => {
                        let manager = manager.clone();
                        let events = events.clone();
                        if let Command::Configuration { request_id, operation } = command {
                            tokio::spawn(async move {
                                let result = tokio::task::spawn_blocking(move || {
                                    manager.configuration(operation)
                                }).await;
                                let result = match result {
                                    Ok(result) => result.map_err(|error| format!("{error:#}")),
                                    Err(error) => Err(error.to_string()),
                                };
                                let _ = events.send(Event::Configuration { request_id, result }).await;
                            });
                            continue;
                        }

                        // Lifecycle work continues if the browser disconnects.
                        // Per-task/repository locks coordinate other clients too.
                        tokio::spawn(async move {
                            let request_id = match &command {
                                Command::LinkPr {request_id,..} | Command::UnlinkPr {request_id,..} | Command::RefreshPrs {request_id,..} => Some(request_id.clone()),
                                _ => None,
                            };
                            let result = manager.dispatch(command).await;
                            let event = match result {
                                Ok(message) => Event::Result { message, error: false },
                                Err(error) => Event::Result { message: format!("{error:#}"), error: true },
                            };
                            let event = match (request_id,event) {
                                (Some(request_id),Event::Result {message,error}) => Event::PrResult {request_id,message,error},
                                (_,event) => event,
                            };
                            let event = match (correlation, event) {
                                (Some(request_id), Event::Result { message, error }) => Event::ActionResult { request_id, message, error },
                                (_, event) => event,
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

#[derive(Deserialize)]
struct BrowserRequest {
    request_id: String,
    command: Command,
}

pub async fn request(home: &Home, command: Command) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(60), request_inner(home, command))
        .await
        .context("No response; inspect task status before retrying")?
}

async fn request_inner(home: &Home, command: Command) -> Result<()> {
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
            && let Event::Result { message, error } | Event::PrResult { message, error, .. } =
                serde_json::from_str(&text)?
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

#[cfg(test)]
mod tests {
    use super::*;
    use aide_core::peer::{Operation, Outcome, Request, Response as PeerResponse, VERSION};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    #[tokio::test]
    async fn dashboard_cannot_be_framed_by_another_website() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let assets = temp.path().join("assets");
        fs::create_dir(&assets)?;
        fs::write(assets.join("index.html"), "<html></html>")?;
        let manager = Arc::new(Manager::new(
            Home::new(Some(temp.path().join("home")))?,
            Some(assets),
        )?);
        for path in ["/", "/aide/"] {
            let response = asset(State(manager.clone()), path.parse()?).await;
            assert_eq!(
                response
                    .headers()
                    .get("content-security-policy")
                    .and_then(|v| v.to_str().ok()),
                Some("frame-ancestors 'none'")
            );
        }
        Ok(())
    }

    #[test]
    fn serve_origin_policy_rejects_foreign_browsers_even_with_credentials() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut manager = Manager::new(Home::new(Some(temp.path().into()))?, None)?;
        manager.public_origin = Some("https://mac.example.ts.net".into());
        let mut headers = HeaderMap::new();
        headers.insert("host", "mac.example.ts.net".parse()?);
        assert!(authorized(&manager, &headers));
        headers.insert("origin", "https://evil.example".parse()?);
        headers.insert(
            "authorization",
            format!("Bearer {}", manager.token).parse()?,
        );
        assert!(!authorized(&manager, &headers));
        headers.insert("origin", "https://mac.example.ts.net".parse()?);
        assert!(authorized(&manager, &headers));
        headers.insert("host", "different.example.ts.net".parse()?);
        assert!(!authorized(&manager, &headers));
        headers.remove("origin");
        headers.remove("authorization");
        assert!(!authorized(&manager, &headers));
        Ok(())
    }

    #[tokio::test]
    async fn gateway_routes_remote_mutations_and_discards_disconnected_data() -> Result<()> {
        let (_remote_temp, remote_home, project) = crate::workspace::tests::fixture()?;
        let task = crate::workspace::start(&project, "remote-task", None)?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let mut remote = Manager::new(remote_home, None)?;
        // Controlled loopback fixture stands in for Serve's private ingress.
        remote.public_origin = Some(format!("https://{address}"));
        let remote = Arc::new(remote);
        let router = Router::new()
            .route("/aide/api/ws", get(peer_upgrade))
            .with_state(remote.clone());
        let server = tokio::spawn(axum::serve(listener, router).into_future());
        let local_temp = tempfile::tempdir()?;
        let local_home = Home::new(Some(local_temp.path().into()))?;
        let peer = crate::peers::Peer {
            identity: remote.host.identity.clone(),
            origin: format!("http://{address}"),
            identity_conflict: false,
            projects: vec![],
            error: None,
        };
        fs::write(
            local_home.path.join("known-peers.json"),
            serde_json::to_vec(&std::collections::BTreeMap::from([(
                peer.identity.instance.clone(),
                peer,
            )]))?,
        )?;
        let mut gateway = Manager::new(local_home.clone(), None)?;
        let peers = crate::peers::Peers::new(local_home, gateway.host.identity.instance.clone())?;
        peers.refresh().await;
        let (hosts, projects) = peers.views().await;
        assert!(hosts[0].error.is_none());
        assert_eq!(projects[0].tasks[0].task.key, task.key);
        gateway.peers = Some(peers.clone());
        gateway.host.identity.desktop = false;
        let error = gateway
            .dispatch(Command::Editor {
                project: projects[0].root.clone(),
                id: task.id.clone(),
            })
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot launch desktop applications")
        );

        gateway
            .dispatch(Command::SetParked {
                project: projects[0].root.clone(),
                id: task.id.clone(),
                parked: true,
            })
            .await?;
        peers.refresh().await;
        assert_eq!(
            peers.views().await.1[0].tasks[0].status,
            aide_core::Status::Parked
        );
        assert!(
            gateway.snapshot().await.is_empty(),
            "The host API must never aggregate peers"
        );
        server.abort();
        let _ = server.await;
        peers.refresh().await;
        let (hosts, projects) = peers.views().await;
        assert!(hosts[0].error.is_some());
        assert!(projects.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn peer_connection_reads_reconciled_state_after_lost_mutation_response() -> Result<()> {
        let (_temp, home, project) = crate::workspace::tests::fixture()?;
        let task = crate::workspace::start(&project, "park", None)?;
        let manager = Arc::new(Manager::new(home, None)?);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("ws://{}/api/ws", listener.local_addr()?);
        let router = Router::new()
            .route("/api/ws", get(peer_upgrade))
            .with_state(manager.clone());
        let server = tokio::spawn(axum::serve(listener, router).into_future());
        let mut handshake = url.into_client_request()?;
        handshake.headers_mut().insert(
            "authorization",
            format!("Bearer {}", manager.token).parse()?,
        );
        let (mut socket, _) = tokio_tungstenite::connect_async(handshake.clone()).await?;
        let request = Request {
            version: VERSION,
            request_id: "lost".into(),
            instance: manager.host.identity.instance.clone(),
            operation: Operation::Execute {
                command: Command::SetParked {
                    project: project.root.display().to_string(),
                    id: task.id.clone(),
                    parked: true,
                },
                key: Some(task.key.clone()),
            },
        };
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                serde_json::to_string(&request)?.into(),
            ))
            .await?;
        // Deliberately abandon the response, then observe through a new connection.
        drop(socket);
        let (mut socket, _) = tokio_tungstenite::connect_async(handshake).await?;
        let read = Request {
            request_id: "read".into(),
            operation: Operation::Get {
                project: project.root.display().to_string(),
                id: task.id.clone(),
                key: task.key.clone(),
            },
            ..request
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                socket.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&read)?.into())).await?;
                let message = socket.next().await.context("Connection closed")??;
                let response: PeerResponse = serde_json::from_str(message.to_text()?)?;
                assert_eq!(response.request_id, "read");
                if matches!(response.outcome, Outcome::Task { task } if task.status == aide_core::Status::Parked) { break; }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            anyhow::Ok(())
        }).await??;
        server.abort();
        Ok(())
    }
}
