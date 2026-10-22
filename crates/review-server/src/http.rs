use crate::app::Shared;
use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message as WsMessage, WebSocket},
    },
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use review_core::*;
use tokio::sync::broadcast;

#[derive(rust_embed::RustEmbed)]
#[folder = "$OUT_DIR/ui/"]
struct Assets;

pub(crate) async fn asset(State(app): State<Shared>, uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    if path.contains("..") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let data = if let Some(dir) = &app.assets {
        std::fs::read(dir.join(path)).ok()
    } else {
        Assets::get(path).map(|a| a.data.into_owned())
    };
    match data {
        Some(bytes) => (
            [
                (
                    "content-type",
                    mime_guess::from_path(path)
                        .first_or_octet_stream()
                        .to_string(),
                ),
                ("cache-control", "no-store".into()),
                ("x-content-type-options", "nosniff".into()),
            ],
            bytes,
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            "UI assets missing. Run just build-ui.",
        )
            .into_response(),
    }
}

pub(crate) async fn upgrade(
    State(app): State<Shared>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // Only same-origin browser clients; prevents other websites driving the local harness.
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if origin != format!("http://{host}")
        || !(host.starts_with("127.0.0.1:") || host.starts_with("localhost:"))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.max_message_size(2 * 1024 * 1024)
        .on_upgrade(move |socket| connection(app, socket))
}

async fn connection(app: Shared, socket: WebSocket) {
    let (mut send, mut receive) = socket.split();
    let mut events = app.events.subscribe();
    let first = ServerEvent::Snapshot {
        review: Box::new(app.store.lock().await.review.clone()),
    };
    if send
        .send(WsMessage::Text(
            serde_json::to_string(&first).unwrap().into(),
        ))
        .await
        .is_err()
    {
        return;
    }
    loop {
        tokio::select! {
            event = events.recv() => {
                let event = match event {
                    Ok(event) => event,
                    Err(broadcast::error::RecvError::Lagged(_)) => ServerEvent::Snapshot {
                        review: Box::new(app.store.lock().await.review.clone()),
                    },
                    Err(_) => break,
                };
                if send.send(WsMessage::Text(serde_json::to_string(&event).unwrap().into())).await.is_err() {
                    break;
                }
            }
            message = receive.next() => {
                match message {
                    Some(Ok(WsMessage::Text(text))) => {
                        let (response, concluded) = app.command_response(&text).await;

                        // Deliver the terminal state before acknowledging submission and exiting.
                        if concluded {
                            let state = app.store.lock().await.review.clone();
                            println!("{}", state.export_path.as_deref().unwrap_or(NO_FEEDBACK_MESSAGE));
                            let snapshot = ServerEvent::Snapshot { review: Box::new(state) };
                            let _ = send.send(WsMessage::Text(serde_json::to_string(&snapshot).unwrap().into())).await;
                        }
                        let sent = send.send(WsMessage::Text(serde_json::to_string(&response).unwrap().into())).await;
                        if concluded {
                            let _ = send.close().await;
                            app.shutdown.notify_one();
                            break;
                        }
                        if sent.is_err() {
                            break;
                        }
                    }
                    Some(Ok(WsMessage::Ping(payload))) => {
                        let _ = send.send(WsMessage::Pong(payload)).await;
                    }
                    Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}
