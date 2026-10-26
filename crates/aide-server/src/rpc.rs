use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio::net::UnixStream;
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

pub struct Rpc {
    socket: WebSocketStream<UnixStream>,
    sequence: u64,
}
impl Rpc {
    pub async fn connect(path: &Path) -> Result<Self> {
        tokio::time::timeout(Duration::from_secs(5), async {
            let stream = UnixStream::connect(path).await?;
            let (socket, _) = tokio_tungstenite::client_async("ws://localhost/", stream).await?;
            let mut rpc = Self { socket, sequence: 0 };
            rpc.call("initialize", json!({"clientInfo":{"name":"aide","version":"0.1.0"},"capabilities":{"experimentalApi":true}})).await?;
            rpc.socket.send(Message::Text(json!({"method":"initialized"}).to_string().into())).await?;
            Ok(rpc)
        }).await.context("Codex connection timed out")?
    }
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.sequence += 1;
        let id = self.sequence;
        tokio::time::timeout(Duration::from_secs(30), async {
            self.socket
                .send(Message::Text(
                    json!({"id":id,"method":method,"params":params})
                        .to_string()
                        .into(),
                ))
                .await?;
            while let Some(message) = self.socket.next().await {
                if let Message::Text(text) = message? {
                    let value: Value = serde_json::from_str(&text)?;
                    if value.get("method").is_none() && value["id"] == id {
                        if value.get("error").is_some() {
                            bail!("Codex {method}: {}", value["error"]);
                        }
                        return Ok(value["result"].clone());
                    }
                    // Never answer native approvals. The terminal is their owner.
                }
            }
            bail!("Codex disconnected")
        })
        .await
        .context("Codex request timed out")?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    #[tokio::test]
    #[ignore = "Uses installed Codex and native configuration; starts only an idle conversation"]
    async fn live_codex_observer() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let socket = temp.path().join("codex.sock");
        let mut server = tokio::process::Command::new("codex")
            .args([
                "app-server",
                "--listen",
                &format!("unix://{}", socket.display()),
            ])
            .current_dir(temp.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut client = loop {
            if let Ok(client) = Rpc::connect(&socket).await {
                break client;
            }
            if server.try_wait()?.is_some() {
                bail!("Backend failed to start");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let value = client
            .call("thread/start", json!({"cwd":temp.path()}))
            .await?;
        let id = value["thread"]["id"].as_str().unwrap();
        let path = value["thread"]["path"].as_str();
        println!("Native idle thread {id}; recording: {path:?}");
        let mut observer = Rpc::connect(&socket).await?;
        let read = observer.call("thread/read", json!({"threadId":id})).await?;
        assert_eq!(read["thread"]["status"]["type"], "idle");
        drop(observer);
        let still_idle = client.call("thread/read", json!({"threadId":id})).await?;
        assert_eq!(still_idle["thread"]["status"]["type"], "idle");
        let mcp = client.call("mcpServerStatus/list", json!({})).await?;
        println!(
            "Native MCP configuration loaded: {} entries",
            mcp["data"].as_array().map_or(0, Vec::len)
        );
        server.kill().await?;
        Ok(())
    }
}
