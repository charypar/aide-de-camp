//! The picker and conversation remain native Codex. This connection adapter
//! commits the first fork's identity before exposing its response to the UI.
use crate::{conversations::ForkSelection, database::Project, rpc::Rpc, runtime::Runtime};
use aide_core::Task;
use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt, stream::FuturesUnordered};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    io::{self, IsTerminal, Write},
    path::PathBuf,
    time::Duration,
};
use tokio::net::{UnixListener, UnixStream};
use tokio_tungstenite::tungstenite::Message;

pub(crate) struct NativePicker {
    runtime: Runtime,
    root: PathBuf,
    task: Task,
    pinned: Cell<bool>,
    selecting: Cell<bool>,
}

impl NativePicker {
    pub fn new(runtime: Runtime, root: PathBuf, task: Task) -> Self {
        Self {
            runtime,
            root,
            task,
            pinned: Cell::new(false),
            selecting: Cell::new(false),
        }
    }

    pub async fn run(&self) -> Result<()> {
        if !io::stdin().is_terminal() {
            bail!("The native picker requires a terminal; pass a conversation ID instead");
        }
        crate::activity::CodexProcess::ensure_no_ui(&self.task)?;
        let directory = tempfile::Builder::new()
            .prefix("pick-")
            .tempdir_in(self.runtime.home.path.join("run"))?;
        let socket = directory.path().join("ui");
        let listener = UnixListener::bind(&socket)?;
        let _terminal = TerminalState::capture()?;
        // This parent and its native UI child form one managed Codex process tree.
        crate::activity::CodexProcess::remember()?;
        let mut command = tokio::process::Command::new(
            std::env::var_os("AIDE_CODEX").unwrap_or_else(|| "codex".into()),
        );
        command
            .args(["fork", "--all", "--remote"])
            .arg(format!("unix://{}", socket.display()))
            .args(["--cd", &self.task.workspace, "--no-alt-screen"])
            .current_dir(&self.task.workspace)
            .kill_on_drop(true);
        let mut interrupt =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
        let mut child = command.spawn().context("Open native Codex fork picker")?;
        let result = async {
            let mut connections = FuturesUnordered::new();
            loop {
                tokio::select! {
                    status = child.wait() => {
                        if !status?.success() {
                            bail!("Native Codex exited unsuccessfully");
                        }
                        return Ok(());
                    }
                    accepted = listener.accept() => {
                        let (stream, _) = accepted?;
                        connections.push(self.connection(stream));
                    }
                    Some(result) = connections.next(), if !connections.is_empty() => result?,
                    _ = interrupt.recv() => bail!("Native picker interrupted"),
                    _ = terminate.recv() => bail!("Native picker terminated"),
                    _ = hangup.recv() => bail!("Native picker terminal closed"),
                }
            }
        }
        .await;
        // Only this UI child is ours; the task's independent backend survives.
        if child.try_wait()?.is_none() {
            child.kill().await?;
        }
        result
    }

    async fn connection(&self, stream: UnixStream) -> Result<()> {
        let mut client = tokio::time::timeout(
            Duration::from_secs(5),
            tokio_tungstenite::accept_async(stream),
        )
        .await??;
        let upstream = tokio::time::timeout(
            Duration::from_secs(5),
            UnixStream::connect(self.runtime.socket(&self.task)),
        )
        .await??;
        let (mut backend, _) = tokio::time::timeout(
            Duration::from_secs(5),
            tokio_tungstenite::client_async("ws://localhost/", upstream),
        )
        .await??;
        let mut pending: Option<PendingFork<'_>> = None;
        loop {
            tokio::select! {
                message = client.next() => {
                    let Some(message) = connected_message(message)? else {
                        return Ok(());
                    };
                    let mut message = message;
                    if let Message::Text(text) = &message {
                        let mut value: Value = serde_json::from_str(text)?;
                        if !self.pinned.get() && value["method"] == "thread/fork" {
                            match self.prepare(&mut value).await {
                                Ok(prepared) => {
                                    pending = Some(prepared);
                                    message = Message::Text(value.to_string().into());
                                }
                                Err(error) => {
                                    client.send(error_response(&value["id"], &error)).await?;
                                    continue;
                                }
                            }
                        } else if !self.pinned.get() && starts_conversation(&value) {
                            let error = anyhow::anyhow!(
                                "Choose a conversation to fork first; cancellation leaves this task unchanged"
                            );
                            client.send(error_response(&value["id"], &error)).await?;
                            continue;
                        }
                    }
                    backend.send(message).await?;
                }
                message = backend.next() => {
                    let Some(message) = connected_message(message)? else {
                        return Ok(());
                    };
                    let mut message = message;
                    if let Message::Text(text) = &message {
                        let value: Value = serde_json::from_str(text)?;
                        if pending.as_ref().is_some_and(|p| is_response(&value, &p.request_id)) {
                            let selected = pending.take().unwrap();
                            if value.get("error").is_none() {
                                let project = Project::open(&self.root)?;
                                if let Err(error) = self.runtime.finish_fork(&project, &selected.selection, &value["result"]) {
                                    client.send(error_response(&selected.request_id, &error)).await?;
                                    return Err(error.context("Cannot pin native selection; conversation UI stopped"));
                                }
                                self.pinned.set(true);
                            }
                            // Publish only after binding succeeds. Error replies retain
                            // the old binding; reverse RPC and unrelated IDs stay intact.
                            message = Message::Text(value.to_string().into());
                        }
                    }
                    client.send(message).await?;
                }
                _ = async {
                    match &pending {
                        Some(p) => tokio::time::sleep_until(p.deadline).await,
                        None => std::future::pending::<()>().await,
                    }
                } => bail!("Native fork timed out; check the task binding before retrying"),
            }
        }
    }

    async fn prepare(&self, request: &mut Value) -> Result<PendingFork<'_>> {
        if self.selecting.replace(true) {
            bail!("A conversation selection is already in progress");
        }
        let claim = SelectionClaim(&self.selecting);
        let source = request["params"]["threadId"]
            .as_str()
            .context("Native fork has no source conversation ID")?;
        let request_id = request
            .get("id")
            .filter(|id| id.is_string() || id.is_number())
            .context("Native fork has no request ID")?
            .clone();
        let project = Project::open(&self.root)?;
        let mut rpc = Rpc::connect(&self.runtime.socket(&self.task)).await?;
        let selection = self
            .runtime
            .prepare_fork(
                &project,
                &self.task,
                &mut rpc,
                source,
                Some(std::process::id()),
            )
            .await?;
        let params = request["params"]
            .as_object_mut()
            .context("Invalid fork parameters")?;
        params.insert("cwd".into(), selection.params["cwd"].clone());
        params.insert(
            "developerInstructions".into(),
            selection.params["developerInstructions"].clone(),
        );
        params.insert("ephemeral".into(), json!(false));
        Ok(PendingFork {
            selection,
            request_id,
            _claim: claim,
            deadline: tokio::time::Instant::now() + Duration::from_secs(30),
        })
    }
}

struct PendingFork<'a> {
    selection: ForkSelection,
    request_id: Value,
    _claim: SelectionClaim<'a>,
    deadline: tokio::time::Instant,
}

struct SelectionClaim<'a>(&'a Cell<bool>);
impl Drop for SelectionClaim<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

fn is_response(value: &Value, id: &Value) -> bool {
    value.get("method").is_none() && value.get("id") == Some(id)
}

fn starts_conversation(value: &Value) -> bool {
    matches!(
        value["method"].as_str(),
        Some("thread/start" | "thread/resume" | "turn/start")
    )
}

fn error_response(id: &Value, error: &anyhow::Error) -> Message {
    Message::Text(
        json!({"id":id,"error":{"code":-32000,"message":format!("{error:#}")}})
            .to_string()
            .into(),
    )
}

fn connected_message(
    message: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
) -> Result<Option<Message>> {
    use tokio_tungstenite::tungstenite::{Error, error::ProtocolError};
    match message {
        None
        | Some(Ok(Message::Close(_)))
        | Some(Err(
            Error::ConnectionClosed
            | Error::AlreadyClosed
            | Error::Protocol(ProtocolError::ResetWithoutClosingHandshake),
        )) => Ok(None),
        Some(message) => Ok(Some(message?)),
    }
}

/// Restore shell input even if a broken connection forces us to stop the TUI.
struct TerminalState(libc::termios);
impl TerminalState {
    fn capture() -> Result<Self> {
        let mut state = std::mem::MaybeUninit::uninit();
        // tcgetattr initializes the buffer on success; stdin is a terminal here.
        unsafe {
            if libc::tcgetattr(libc::STDIN_FILENO, state.as_mut_ptr()) != 0 {
                return Err(io::Error::last_os_error().into());
            }
            Ok(Self(state.assume_init()))
        }
    }
}
impl Drop for TerminalState {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.0);
        }
        let _ = io::stdout().write_all(b"\x1b[0m\x1b[?25h\x1b[?2004l\x1b[?1004l");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Home;

    #[tokio::test]
    async fn native_transport_preserves_reverse_requests_and_cancellation_without_starting_a_thread()
    -> Result<()> {
        let temp = tempfile::tempdir_in("/tmp")?;
        let home = Home::new(Some(temp.path().join("home")))?;
        let runtime = Runtime::new(&home);
        let task = Task {
            id: "task".into(),
            key: "0123456789abcdef0123456789abcdef".into(),
            repository: "repo".into(),
            workspace: temp.path().display().to_string(),
            lifecycle: aide_core::Lifecycle::Open,
            desired_running: true,
            thread_id: Some("original".into()),
            native_path: None,
            recording: None,
            recovered: false,
            error: None,
            created: 0,
        };
        let backend_listener = UnixListener::bind(runtime.socket(&task))?;
        let picker = NativePicker::new(runtime, temp.path().into(), task);
        let ui_socket = temp.path().join("ui");
        let ui_listener = UnixListener::bind(&ui_socket)?;
        let relay = async {
            let (stream, _) = ui_listener.accept().await?;
            picker.connection(stream).await
        };
        let backend = async {
            let (stream, _) = backend_listener.accept().await?;
            let mut socket = tokio_tungstenite::accept_async(stream).await?;
            let request: Value = serde_json::from_str(socket.next().await.unwrap()?.to_text()?)?;
            assert_eq!(
                request,
                json!({"id":7,"method":"thread/list","params":{"cursor":"page-2"}})
            );
            let approval = json!({"id":7,"method":"item/commandExecution/requestApproval","params":{"opaque":"preserve"}});
            assert!(!is_response(&approval, &json!(7)));
            socket
                .send(Message::Text(approval.to_string().into()))
                .await?;
            let answer: Value = serde_json::from_str(socket.next().await.unwrap()?.to_text()?)?;
            assert_eq!(answer, json!({"id":7,"result":{"decision":"decline"}}));
            socket
                .send(Message::Text(
                    json!({"id":7,"result":{"data":[],"nextCursor":"page-3"}})
                        .to_string()
                        .into(),
                ))
                .await?;
            // The adapter must reject the UI's "new session" fallback locally.
            // It must never reach this backend after picker cancellation.
            assert!(connected_message(socket.next().await)?.is_none());
            anyhow::Ok(())
        };
        let native = async {
            let stream = UnixStream::connect(&ui_socket).await?;
            let (mut socket, _) =
                tokio_tungstenite::client_async("ws://localhost/", stream).await?;
            socket
                .send(Message::Text(
                    json!({"id":7,"method":"thread/list","params":{"cursor":"page-2"}})
                        .to_string()
                        .into(),
                ))
                .await?;
            let approval: Value = serde_json::from_str(socket.next().await.unwrap()?.to_text()?)?;
            assert_eq!(approval["params"]["opaque"], "preserve");
            socket
                .send(Message::Text(
                    json!({"id":7,"result":{"decision":"decline"}})
                        .to_string()
                        .into(),
                ))
                .await?;
            let result: Value = serde_json::from_str(socket.next().await.unwrap()?.to_text()?)?;
            assert_eq!(result["result"]["nextCursor"], "page-3");
            socket
                .send(Message::Text(
                    json!({"id":8,"method":"thread/start","params":{}})
                        .to_string()
                        .into(),
                ))
                .await?;
            let rejected: Value = serde_json::from_str(socket.next().await.unwrap()?.to_text()?)?;
            assert_eq!(rejected["id"], 8);
            assert!(rejected.get("error").is_some());
            socket.close(None).await?;
            anyhow::Ok(())
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::try_join!(relay, backend, native)
        })
        .await??;
        assert!(!picker.pinned.get());
        assert!(!picker.selecting.get());
        Ok(())
    }
}
