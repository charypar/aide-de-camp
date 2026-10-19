use crate::{agent, commands::handle, store::Store};
use review_core::*;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::{Mutex, Notify, broadcast};

pub(crate) struct App {
    pub(crate) store: Mutex<Store>,
    pub(crate) agent: Mutex<Option<Arc<agent::Agent>>>,
    pub(crate) events: broadcast::Sender<ServerEvent>,
    pub(crate) fixture: bool,
    pub(crate) assets: Option<PathBuf>,
    pub(crate) shutdown: Notify,
}
pub(crate) type Shared = Arc<App>;

pub(crate) fn publish(app: &App, s: &Store) {
    let _ = app.events.send(ServerEvent::Snapshot {
        review: Box::new(s.review.clone()),
    });
}

impl App {
    pub(crate) async fn command_response(self: &Arc<Self>, text: &str) -> (ServerEvent, bool) {
        let command = match serde_json::from_str::<Envelope>(text) {
            Ok(command) => command,
            Err(error) => {
                return (
                    ServerEvent::Error {
                        id: String::new(),
                        message: error.to_string(),
                    },
                    false,
                );
            }
        };
        let id = command.id.clone();
        let conclude = matches!(command.command, Command::Conclude);

        match handle(self, command).await {
            Ok(()) => {
                let completed =
                    conclude && self.store.lock().await.review.status == ReviewStatus::Completed;
                (ServerEvent::Ack { id }, completed)
            }
            Err(error) => (
                ServerEvent::Error {
                    id,
                    message: error.to_string(),
                },
                false,
            ),
        }
    }
}
