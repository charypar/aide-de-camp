use crate::{database::Home, tailnet};
use aide_core::{
    ProjectView,
    peer::{HostView, Identity, Operation, Outcome, Request, Response, Source, VERSION},
};
use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::sync::RwLock;

#[derive(Clone, Serialize, Deserialize)]
pub struct Peer {
    pub identity: Identity,
    pub origin: String,
    #[serde(default)]
    pub identity_conflict: bool,
    #[serde(skip)]
    pub projects: Vec<ProjectView>,
    #[serde(skip)]
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct Peers {
    home: Home,
    own: String,
    client: reqwest::Client,
    entries: Arc<RwLock<BTreeMap<String, Peer>>>,
}

impl Peers {
    pub fn new(home: Home, own: String) -> Result<Self> {
        let mut entries: BTreeMap<String, Peer> =
            match std::fs::read(home.path.join("known-peers.json")) {
                Ok(bytes) => serde_json::from_slice(&bytes)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
                Err(error) => return Err(error.into()),
            };
        for peer in entries.values_mut() {
            peer.error = Some(
                if peer.identity_conflict {
                    "Conflicting host identity"
                } else {
                    "Disconnected"
                }
                .into(),
            );
        }
        Ok(Self {
            home,
            own,
            entries: Arc::new(RwLock::new(entries)),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(4))
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    async fn discover(&self) -> Result<()> {
        let inventory = tailnet::inventory().await?;
        let results = futures_util::stream::iter(inventory.peers.into_values())
            .map(|device| async move {
                let origin = device.origin()?;
                let identity: Identity = self
                    .client
                    .get(format!("{origin}/aide/api/identity"))
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?;
                anyhow::Ok((origin, identity))
            })
            .buffer_unordered(8)
            .collect::<Vec<_>>()
            .await;
        let mut entries = self.entries.write().await;
        for (origin, identity) in results.into_iter().flatten() {
            if identity.instance == self.own {
                bail!(
                    "Another Tailscale device has this Aide instance ID; regenerate the cloned home identity"
                );
            }
            if let Some(old) = entries.get_mut(&identity.instance) {
                if old.origin != origin {
                    old.identity_conflict = true;
                    old.error = Some(
                        "Conflicting host identity; regenerate the cloned Aide identity".into(),
                    );
                    old.projects.clear();
                } else {
                    old.identity = identity;
                }
                continue;
            }
            entries.insert(
                identity.instance.clone(),
                Peer {
                    identity,
                    origin,
                    identity_conflict: false,
                    projects: vec![],
                    error: Some("Connecting".into()),
                },
            );
        }
        let temp = self.home.path.join("known-peers.json.tmp");
        std::fs::write(&temp, serde_json::to_vec(&*entries)?)?;
        std::fs::rename(temp, self.home.path.join("known-peers.json"))?;
        Ok(())
    }

    pub async fn run(self) {
        let mut ticks = 0u32;
        loop {
            if ticks.is_multiple_of(6)
                && let Err(error) = self.discover().await
            {
                eprintln!("Peer discovery: {error:#}");
            }
            self.refresh().await;
            ticks = ticks.wrapping_add(1);
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    pub async fn refresh(&self) {
        let peers: Vec<_> = self.entries.read().await.values().cloned().collect();
        let results = futures_util::stream::iter(peers)
            .map(|peer| async {
                let result = peer.call(Operation::List, Duration::from_secs(5)).await;
                (peer.identity.instance, result)
            })
            .buffer_unordered(8)
            .collect::<Vec<_>>()
            .await;
        let mut entries = self.entries.write().await;
        for (id, result) in results {
            if let Some(peer) = entries.get_mut(&id) {
                if peer.identity_conflict {
                    continue;
                }
                match result {
                    Ok(Outcome::Projects { projects }) => {
                        peer.projects = projects;
                        peer.error = None;
                    }
                    Ok(_) => {
                        peer.projects.clear();
                        peer.error = Some("Unexpected peer response".into());
                    }
                    Err(error) => {
                        peer.projects.clear();
                        peer.error = Some(format!("Disconnected: {error:#}"));
                    }
                }
            }
        }
        drop(entries);
    }

    pub async fn get(&self, id: &str) -> Result<Peer> {
        let entries = self.entries.read().await;
        let peer = entries
            .get(id)
            .context("Unknown peer; wait for discovery")?;
        if let Some(error) = &peer.error {
            bail!("{error}");
        }
        Ok(peer.clone())
    }

    pub async fn views(&self) -> (Vec<HostView>, Vec<ProjectView>) {
        let entries = self.entries.read().await;
        let mut hosts = vec![];
        let mut projects = vec![];
        for peer in entries.values() {
            hosts.push(HostView {
                identity: peer.identity.clone(),
                error: peer.error.clone(),
            });
            if peer.error.is_some() {
                continue;
            }
            for mut project in peer.projects.clone() {
                project.source = Some(Source {
                    instance: peer.identity.instance.clone(),
                    name: peer.identity.name.clone(),
                    root: project.root.clone(),
                });
                project.root = format!("peer:{}:{}", peer.identity.instance, project.root);
                project.name = format!("{} · {}", peer.identity.name, project.name);
                projects.push(project);
            }
        }
        (hosts, projects)
    }
}

impl Peer {
    pub async fn call(&self, operation: Operation, timeout: Duration) -> Result<Outcome> {
        if self.identity_conflict {
            bail!(
                "Conflicting host identity; repair cloned identities and remove the conflicting known-peers.json entry"
            );
        }
        if self.identity.version != VERSION {
            bail!("Unsupported peer protocol version");
        }
        tokio::time::timeout(timeout, async {
            let endpoint = format!(
                "{}/aide/api/ws",
                self.origin
                    .replacen("https:", "wss:", 1)
                    .replacen("http:", "ws:", 1)
            );
            let (mut socket, _) = tokio_tungstenite::connect_async(endpoint).await?;
            let request = Request {
                version: VERSION,
                instance: self.identity.instance.clone(),
                request_id: uuid::Uuid::new_v4().to_string(),
                operation,
            };
            socket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    serde_json::to_string(&request)?.into(),
                ))
                .await?;
            while let Some(message) = socket.next().await {
                if let tokio_tungstenite::tungstenite::Message::Text(text) = message? {
                    let response: Response = serde_json::from_str(&text)?;
                    if response.request_id != request.request_id {
                        continue;
                    }
                    return match response.outcome {
                        Outcome::Error { message } => Err(anyhow::anyhow!(message)),
                        outcome => Ok(outcome),
                    };
                }
            }
            bail!("Peer disconnected; refresh status before retrying")
        })
        .await
        .context("No response; refresh task status before retrying")?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn identity_conflict_survives_repeated_refreshes() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let home = Home::new(Some(temp.path().into()))?;
        let identity = crate::peer::Host::new(home.clone())?.identity;
        let peers = Peers::new(home, "other".into())?;
        peers.entries.write().await.insert(
            identity.instance.clone(),
            Peer {
                identity: identity.clone(),
                identity_conflict: true,
                origin: "http://127.0.0.1:1".into(),
                projects: vec![],
                error: Some("Conflicting host identity".into()),
            },
        );
        for _ in 0..2 {
            peers.refresh().await;
            assert!(peers.get(&identity.instance).await.is_err());
            assert_eq!(
                peers.views().await.0[0].error.as_deref(),
                Some("Conflicting host identity")
            );
        }
        let entry = peers.entries.read().await[&identity.instance].clone();
        let restored: Peer = serde_json::from_str(&serde_json::to_string(&entry)?)?;
        assert!(restored.identity_conflict);
        assert!(
            restored
                .call(Operation::List, Duration::from_secs(1))
                .await
                .unwrap_err()
                .to_string()
                .contains("Conflicting")
        );
        Ok(())
    }
}
