//! Native harness transport. Product state and rendering deliberately live outside this module.
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::process::Stdio;
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, mpsc, oneshot},
};

type Reply = oneshot::Sender<Result<Value>>;
pub struct Agent {
    input: Mutex<ChildStdin>,
    pending: Arc<Mutex<HashMap<u64, Reply>>>,
    sequence: AtomicU64,
    child: Mutex<Child>,
}
impl Agent {
    pub async fn start(directory: &Path) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<Value>)> {
        let mut child = Command::new("codex")
            .arg("app-server")
            .current_dir(directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        let pending: Arc<Mutex<HashMap<u64, Reply>>> = Arc::default();
        let p = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(output).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    if value.get("method").is_none() {
                        if let Some(id) = value["id"].as_u64()
                            && let Some(reply) = p.lock().await.remove(&id)
                        {
                            let result = if let Some(e) = value.get("error") {
                                Err(anyhow!("Codex: {e}"))
                            } else {
                                Ok(value["result"].clone())
                            };
                            let _ = reply.send(result);
                        }
                    } else {
                        let _ = tx.send(value);
                    }
                }
            }
            for (_, reply) in p.lock().await.drain() {
                let _ = reply.send(Err(anyhow!("Codex app server disconnected")));
            }
            let _ = tx.send(json!({"method":"transport/closed"}));
        });
        let agent = Arc::new(Self {
            input: Mutex::new(input),
            pending,
            sequence: AtomicU64::new(1),
            child: Mutex::new(child),
        });
        agent.call("initialize",json!({"clientInfo":{"name":"aide_review","version":"0.1.0"},"capabilities":{"experimentalApi":true}})).await?;
        agent
            .write(json!({"method":"initialized","params":{}}))
            .await?;
        Ok((agent, rx))
    }
    async fn write(&self, value: Value) -> Result<()> {
        let mut w = self.input.lock().await;
        w.write_all(format!("{value}\n").as_bytes()).await?;
        w.flush().await?;
        Ok(())
    }
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        if let Err(e) = self
            .write(json!({"id":id,"method":method,"params":params}))
            .await
        {
            self.pending.lock().await.remove(&id);
            return Err(e);
        }
        match tokio::time::timeout(std::time::Duration::from_secs(90), rx).await {
            Ok(r) => r?,
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(anyhow!("Codex request timed out: {method}"))
            }
        }
    }
    pub async fn respond(&self, id: Value, response: Value) -> Result<()> {
        self.write(json!({"id":id,"result":response})).await
    }
    pub async fn is_alive(&self) -> bool {
        self.child.lock().await.try_wait().ok().flatten().is_none()
    }
    pub async fn stop(&self) {
        let _ = self.child.lock().await.kill().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn turn(
        agent: &Agent,
        events: &mut mpsc::UnboundedReceiver<Value>,
        thread: &str,
        text: &str,
    ) -> Result<(String, bool)> {
        agent
            .call(
                "turn/start",
                json!({"threadId":thread,"input":[{"type":"text","text":text}]}),
            )
            .await?;
        let mut final_text = String::new();
        let mut mcp = false;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
        loop {
            let e = tokio::time::timeout_at(deadline, events.recv())
                .await?
                .ok_or_else(|| anyhow!("EOF"))?;
            if e.get("id").is_some() {
                let method = e["method"].as_str().unwrap_or("");

                let response = if method.ends_with("requestApproval") {
                    json!({"decision":"decline"})
                } else if method.contains("elicitation")
                    && e["params"]["serverName"] == "playwright"
                    && e["params"]["_meta"]["tool_params"]["action"] == "list"
                {
                    json!({"action":"accept", "content":{}})
                } else if method.contains("elicitation") {
                    json!({"action":"cancel"})
                } else {
                    json!({"answers":{}})
                };
                agent.respond(e["id"].clone(), response).await?;
            }
            if e["method"] == "item/completed" {
                let item = &e["params"]["item"];
                if item["type"] == "agentMessage" {
                    final_text = item["text"].as_str().unwrap_or("").into();
                }
                if item["type"] == "mcpToolCall" {
                    println!("MCP tool {}: {}", item["tool"], item["status"]);
                    mcp = item["status"] == "completed";
                }
            }
            if e["method"] == "turn/completed" {
                if e["params"]["turn"]["status"] != "completed" {
                    return Err(anyhow!("Turn failed: {}", e["params"]["turn"]["error"]));
                }
                return Ok((final_text, mcp));
            }
        }
    }
    #[tokio::test]
    #[ignore = "Uses the installed Codex account and configured MCPs"]
    async fn live_codex() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let directory = temp.path();
        std::fs::write(
            directory.join("math.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
        )?;
        let (agent, mut events) = Agent::start(directory).await?;
        let result = agent
            .call(
                "thread/start",
                json!({"cwd":directory,"sandbox":"read-only","approvalPolicy":"on-request"}),
            )
            .await?;
        let thread = result["thread"]["id"].as_str().unwrap().to_string();
        let servers = agent.call("mcpServerStatus/list", json!({})).await?;
        assert!(
            servers["data"]
                .as_array()
                .is_some_and(|s| s.iter().any(|s| s["name"] == "playwright"))
        );
        println!("Native configuration: Playwright MCP discovered");
        let prompt = format!(
            "{}\n\nRead math.rs. Narrate this diff using one exact annotated snippet:\ndiff --git a/math.rs b/math.rs\n@@ -1 +1 @@\n-pub fn add(a: i32, b: i32) -> i32 {{ a - b }}\n+pub fn add(a: i32, b: i32) -> i32 {{ a + b }}\nRemember the review marker ORCHARD. Do not modify files.",
            include_str!("../../../skills/review-walkthrough/SKILL.md")
        );
        let (guide, _) = turn(&agent, &mut events, &thread, &prompt).await?;
        assert!(guide.contains("math.rs"));
        assert!(guide.contains("```"));
        let workspace = review_core::Workspace {
            fingerprint: String::new(),
            diff: String::new(),
            vcs: "git".into(),
            files: vec![review_core::SourceFile {
                path: "math.rs".into(),
                old: Some("pub fn add(a: i32, b: i32) -> i32 { a - b }\n".into()),
                new: Some("pub fn add(a: i32, b: i32) -> i32 { a + b }\n".into()),
            }],
        };
        let (_, warnings) = crate::document::render(&guide, &workspace);
        assert!(warnings.is_empty(), "{warnings:?}\n{guide}");
        println!("Guide generated ({} characters)", guide.len());
        let (answer,mcp)=turn(&agent,&mut events,&thread,"Selected code: math.rs:1-1 (new). Does this add or subtract? Also call the configured Playwright browser_tabs tool with action list as a read-only MCP check; do not navigate or change tabs.").await?;
        assert!(!answer.is_empty());
        println!("Follow-up: {answer}");
        println!("Code reference answered; native MCP call observed: {mcp}");
        assert!(mcp, "Expected an actual MCP tool call");
        agent.stop().await;
        drop(agent);
        let (agent, mut events) = Agent::start(directory).await?;
        agent.call("thread/resume",json!({"threadId":thread,"cwd":directory,"sandbox":"read-only","approvalPolicy":"on-request"})).await?;
        let (answer,_)=turn(&agent,&mut events,&thread,"Selected guide text: 'Narrate this diff'. What review marker did I ask you to remember, and what operation did the code change to? Answer briefly.").await?;
        assert!(answer.contains("ORCHARD"));
        println!("Native session resumed with history; prose-reference follow-up answered");
        agent.stop().await;
        Ok(())
    }
}

/// The review app consumes these events; native request payloads remain available for harness-specific UI.
pub enum HarnessEvent {
    Request(review_core::AgentRequest),
    MessageDelta {
        id: String,
        text: String,
    },
    Message {
        id: String,
        text: String,
        final_answer: bool,
    },
    Activity(String),
    TurnStarted(String),
    TurnFinished {
        status: review_core::MessageStatus,
        error: Option<String>,
    },
    Disconnected,
    Error(String),
    Other,
}
pub fn normalize(event: &Value) -> HarnessEvent {
    let method = event["method"].as_str().unwrap_or("");
    let p = &event["params"];
    if let Some(id) = event.get("id") {
        return HarnessEvent::Request(review_core::AgentRequest {
            id: id.clone(),
            method: method.into(),
            params: p.clone(),
        });
    }
    match method {
        "item/agentMessage/delta" => HarnessEvent::MessageDelta {
            id: p["itemId"].as_str().unwrap_or("").into(),
            text: p["delta"].as_str().unwrap_or("").into(),
        },
        "item/completed" => {
            let item = &p["item"];
            if item["type"] == "agentMessage" {
                HarnessEvent::Message {
                    id: item["id"].as_str().unwrap_or("").into(),
                    text: item["text"].as_str().unwrap_or("").into(),
                    final_answer: item["phase"].as_str().is_none_or(|p| p == "final_answer"),
                }
            } else {
                HarnessEvent::Activity(format!(
                    "{}: {}",
                    item["type"].as_str().unwrap_or("tool"),
                    item
                ))
            }
        }
        "item/started" if p["item"]["type"] != "agentMessage" => HarnessEvent::Activity(format!(
            "Started {}",
            p["item"]["type"].as_str().unwrap_or("tool")
        )),
        "turn/started" => HarnessEvent::TurnStarted(p["turn"]["id"].as_str().unwrap_or("").into()),
        "turn/completed" => {
            let native = p["turn"]["status"].as_str().unwrap_or("<missing>");
            let (status, unknown) = match native {
                "completed" => (review_core::MessageStatus::Complete, None),
                "interrupted" => (review_core::MessageStatus::Interrupted, None),
                "failed" => (review_core::MessageStatus::Failed, None),
                other => (
                    review_core::MessageStatus::Failed,
                    Some(format!("Unknown Codex turn status: {other}")),
                ),
            };
            HarnessEvent::TurnFinished {
                status,
                error: unknown.or_else(|| {
                    p["turn"]
                        .get("error")
                        .filter(|e| !e.is_null())
                        .map(ToString::to_string)
                }),
            }
        }
        "transport/closed" => HarnessEvent::Disconnected,
        "error" => HarnessEvent::Error(p.to_string()),
        m if m.starts_with("mcpServer/") => HarnessEvent::Activity(format!("{method}: {p}")),
        _ => HarnessEvent::Other,
    }
}
impl Agent {
    pub async fn session(&self, thread: Option<&str>, directory: &str) -> Result<String> {
        let r = if let Some(thread) = thread {
            self.call("thread/resume",json!({"threadId":thread,"cwd":directory,"sandbox":"read-only","approvalPolicy":"on-request"})).await?
        } else {
            self.call(
                "thread/start",
                json!({"cwd":directory,"sandbox":"read-only","approvalPolicy":"on-request"}),
            )
            .await?
        };
        Ok(r["thread"]["id"]
            .as_str()
            .ok_or_else(|| anyhow!("Harness returned no session ID"))?
            .to_owned())
    }
    pub async fn send(&self, thread: &str, text: &str) -> Result<Option<String>> {
        let r = self
            .call(
                "turn/start",
                json!({"threadId":thread,"input":[{"type":"text","text":text}]}),
            )
            .await?;
        Ok(r["turn"]["id"].as_str().map(str::to_owned))
    }
    pub async fn interrupt(&self, thread: &str, turn: &str) -> Result<()> {
        self.call("turn/interrupt", json!({"threadId":thread,"turnId":turn}))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;
    use review_core::MessageStatus;
    #[test]
    fn unknown_or_missing_native_status_is_a_failure_not_success() {
        for status in [json!("new-status"), Value::Null] {
            match normalize(&json!({"method":"turn/completed","params":{"turn":{"status":status}}}))
            {
                HarnessEvent::TurnFinished { status, error } => {
                    assert_eq!(status, MessageStatus::Failed);
                    assert!(error.unwrap().contains("Unknown Codex turn status"));
                }
                _ => panic!("Expected a terminal event"),
            }
        }
    }
}
