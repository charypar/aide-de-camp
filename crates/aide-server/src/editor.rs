use crate::{attachment, database::Home, peers::Peer, terminal};
use aide_core::{
    Task,
    peer::{Operation, Outcome},
};
use anyhow::{Context, Result, bail};
use std::{path::PathBuf, time::Duration};

pub struct RemoteEditor {
    executable: PathBuf,
    destination: String,
}

impl RemoteEditor {
    pub fn new(home: &Home, peer: &Peer) -> Result<Self> {
        Ok(Self {
            executable: terminal::executable("zed")
                .context("Install the Zed CLI on the dashboard host")?,
            destination: attachment::destination(home, peer)?,
        })
    }

    fn url(destination: &str, workspace: &str) -> Result<String> {
        let (user, host) = destination
            .rsplit_once('@')
            .map_or((None, destination), |(user, host)| (Some(user), host));
        let simple = |s: &str| {
            !s.is_empty()
                && !s.starts_with('-')
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        };
        if user.is_some_and(|user| !simple(user)) {
            bail!("Invalid SSH user; configure a host alias in ssh-peers.json");
        }
        let host = if let Ok(ip) = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::Ipv6Addr>()
        {
            format!("[{ip}]")
        } else if simple(host) {
            host.to_owned()
        } else {
            bail!("Invalid SSH host; configure ports and keys through an SSH host alias");
        };
        if !workspace.starts_with('/') || workspace.contains('\0') {
            bail!("Remote workspace must be an absolute path");
        }
        let mut path = String::new();
        for byte in workspace.bytes() {
            if byte.is_ascii_alphanumeric() || b"/._-~".contains(&byte) {
                path.push(char::from(byte));
            } else {
                use std::fmt::Write;
                write!(path, "%{byte:02X}")?;
            }
        }
        // A directory suffix prevents a final :number from becoming a line position.
        if !path.ends_with('/') {
            path.push('/');
        }
        let user = user.map(|user| format!("{user}@")).unwrap_or_default();
        Ok(format!("ssh://{user}{host}{path}"))
    }

    async fn launch(&self, workspace: &str) -> Result<()> {
        let url = Self::url(&self.destination, workspace)?;
        let mut command = tokio::process::Command::new(&self.executable);
        command.arg(url).kill_on_drop(true);
        let status = tokio::time::timeout(Duration::from_secs(15), command.status())
            .await
            .context("Zed launch timed out; check Zed before retrying")?
            .context("Launch Zed on the dashboard host")?;
        if !status.success() {
            bail!("Zed launcher failed ({status})");
        }
        Ok(())
    }

    pub async fn open(&self, peer: &Peer, project: &str, task: &Task) -> Result<()> {
        // Validate locally before contacting the owner; never send a desktop command there.
        Self::url(&self.destination, "/")?;
        let operation = |unpark| Operation::EditorWorkspace {
            project: project.into(),
            id: task.id.clone(),
            key: task.key.clone(),
            unpark,
        };
        let outcome = peer
            .call(operation(false), Duration::from_secs(10))
            .await
            .context(
                "Resolve remote workspace; both hosts need an Aide version with remote Zed support",
            )?;
        let Outcome::Workspace { path } = outcome else {
            bail!("Unexpected remote workspace response");
        };
        self.launch(&path).await?;
        let outcome = peer
            .call(operation(true), Duration::from_secs(10))
            .await
            .context(
                "Zed launch requested, but task activation was not confirmed; refresh task status",
            )?;
        if !matches!(outcome, Outcome::Workspace { .. }) {
            bail!("Zed launch requested, but task activation returned an unexpected response");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn remote_launch_uses_fresh_workspace_and_preserves_parking_on_failure() -> Result<()> {
        use aide_core::peer::Request;
        use axum::{
            Router,
            extract::{State, WebSocketUpgrade},
            routing::get,
        };
        use futures_util::StreamExt;
        use std::{
            future::IntoFuture,
            os::unix::fs::PermissionsExt,
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
            },
        };

        let (temp, home, project) = crate::workspace::tests::fixture()?;
        let task = crate::workspace::start(&project, "remote-editor", None)?;
        let host = crate::peer::Host::new(home)?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}", listener.local_addr()?);
        let lose_ack = Arc::new(AtomicBool::new(false));
        let router = Router::new()
            .route(
                "/aide/api/ws",
                get(
                    |State((host, lose_ack)): State<(crate::peer::Host, Arc<AtomicBool>)>,
                     ws: WebSocketUpgrade| async move {
                        ws.on_upgrade(move |mut socket| async move {
                            if let Some(Ok(axum::extract::ws::Message::Text(text))) =
                                socket.next().await
                            {
                                let request: Request = serde_json::from_str(&text).unwrap();
                                if matches!(
                                    request.operation,
                                    Operation::EditorWorkspace { unpark: true, .. }
                                ) && lose_ack.load(Ordering::SeqCst)
                                {
                                    return;
                                }
                                let response = host.respond(request).await;
                                let _ = socket
                                    .send(axum::extract::ws::Message::Text(
                                        serde_json::to_string(&response).unwrap().into(),
                                    ))
                                    .await;
                            }
                        })
                    },
                ),
            )
            .with_state((host.clone(), lose_ack.clone()));
        let server = tokio::spawn(axum::serve(listener, router).into_future());
        let peer = Peer {
            identity: host.identity,
            origin,
            identity_conflict: false,
            projects: vec![],
            error: None,
        };
        let executable = temp.path().join("zed");
        let output = temp.path().join("arguments");
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
                output.display()
            ),
        )?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
        let editor = RemoteEditor {
            executable: executable.clone(),
            destination: "me@vm-alias".into(),
        };
        let root = project.root.to_str().unwrap();
        project.set_parked(&task.id, true)?;
        let mut cached = task.clone();
        cached.workspace = "stale cached path".into();
        editor.open(&peer, root, &cached).await?;
        assert_eq!(
            std::fs::read_to_string(&output)?.trim(),
            RemoteEditor::url("me@vm-alias", &task.workspace)?
        );
        assert!(!project.is_parked(&task.id)?);

        std::fs::remove_file(&output)?;
        project.set_parked(&task.id, true)?;
        cached.key = "wrong-key".into();
        assert!(editor.open(&peer, root, &cached).await.is_err());
        assert!(!output.exists());
        assert!(project.is_parked(&task.id)?);
        std::fs::write(&executable, "#!/bin/sh\nexit 1\n")?;
        assert!(editor.open(&peer, root, &task).await.is_err());
        assert!(project.is_parked(&task.id)?);

        std::fs::write(&executable, "#!/bin/sh\nexit 0\n")?;
        lose_ack.store(true, Ordering::SeqCst);
        assert!(
            editor
                .open(&peer, root, &task)
                .await
                .unwrap_err()
                .to_string()
                .contains("activation was not confirmed")
        );
        assert!(project.is_parked(&task.id)?);
        lose_ack.store(false, Ordering::SeqCst);
        editor.open(&peer, root, &task).await?;
        assert!(!project.is_parked(&task.id)?);

        server.abort();
        let _ = server.await;
        assert!(editor.open(&peer, root, &task).await.is_err());
        Ok(())
    }

    #[test]
    fn ssh_urls_preserve_paths_and_reject_ambiguous_destinations() -> Result<()> {
        assert_eq!(
            RemoteEditor::url("viktor@linux-vm", "/home/viktor/Cat Herder/é%#?:123")?,
            "ssh://viktor@linux-vm/home/viktor/Cat%20Herder/%C3%A9%25%23%3F%3A123/"
        );
        assert_eq!(
            RemoteEditor::url("me@2001:db8::1", "/tmp/task")?,
            "ssh://me@[2001:db8::1]/tmp/task/"
        );
        assert_eq!(
            RemoteEditor::url("[::1]", "/tmp/task")?,
            "ssh://[::1]/tmp/task/"
        );
        for bad in [
            "",
            "-oProxyCommand=evil",
            "host/path",
            "host?query",
            "user:password@host",
            "me@@host",
            "host:2222",
            "host\nother",
        ] {
            assert!(RemoteEditor::url(bad, "/tmp/task").is_err(), "{bad}");
        }
        assert!(RemoteEditor::url("host", "relative/path").is_err());
        Ok(())
    }

    #[tokio::test]
    async fn launcher_reports_missing_and_failed_executables_and_passes_one_argument() -> Result<()>
    {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir()?;
        let executable = temp.path().join("zed");
        let editor = RemoteEditor {
            executable: executable.clone(),
            destination: "vm".into(),
        };
        assert!(editor.launch("/tmp/task").await.is_err());
        std::fs::write(&executable, "#!/bin/sh\nexit 7\n")?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
        assert!(
            editor
                .launch("/tmp/task")
                .await
                .unwrap_err()
                .to_string()
                .contains("launcher failed")
        );
        std::fs::write(
            &executable,
            "#!/bin/sh\n[ \"$#\" = 1 ] && [ \"$1\" = 'ssh://vm/tmp/space%20%24%28touch%20oops%29/' ]\n",
        )?;
        editor.launch("/tmp/space $(touch oops)").await?;
        Ok(())
    }
}
