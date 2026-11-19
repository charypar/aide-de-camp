use crate::{database::Home, peers::Peer};
use aide_core::Task;
use anyhow::{Context, Result, bail};
use std::{collections::BTreeMap, process::Command};

pub fn arguments(home: &str, instance: &str, root: &str, task: &Task) -> Vec<String> {
    [
        "--home",
        home,
        "attach-target",
        "--instance",
        instance,
        "--project",
        root,
        "--id",
        &task.id,
        "--key",
        &task.key,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn destination(home: &Home, peer: &Peer) -> Result<String> {
    let overrides: BTreeMap<String, String> = match std::fs::read(home.path.join("ssh-peers.json"))
    {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => return Err(error.into()),
    };
    let destination = overrides
        .get(&peer.identity.instance)
        .cloned()
        .unwrap_or_else(|| peer.origin.trim_start_matches("https://").to_owned());
    if destination.starts_with('-')
        || destination.is_empty()
        || destination.chars().any(char::is_whitespace)
    {
        bail!("Invalid SSH destination; configure a host alias in ssh-peers.json");
    }
    Ok(destination)
}

pub fn remote_command(home: &Home, peer: &Peer, root: &str, task: &Task) -> Result<Vec<String>> {
    let destination = destination(home, peer)?;
    let mut remote = vec![peer.identity.executable.clone()];
    remote.extend(arguments(
        &peer.identity.home,
        &peer.identity.instance,
        root,
        task,
    ));
    Ok(vec![
        crate::terminal::executable("ssh")?.display().to_string(),
        "-t".into(),
        "--".into(),
        destination,
        remote
            .iter()
            .map(|arg| quote(arg))
            .collect::<Vec<_>>()
            .join(" "),
    ])
}

pub fn open_remote(home: &Home, peer: &Peer, root: &str, task: &Task) -> Result<()> {
    let args = remote_command(home, peer, root, task)?;
    let env = crate::terminal::executable("env")?;
    let title = format!("Aide · {} · {}", peer.identity.name, task.id);
    let configure = |reuse: bool| {
        let mut command = Command::new("alacritty");
        command.env_remove("TMUX").env_remove("TMUX_PANE");
        if reuse {
            command.args(["msg", "create-window"]);
        }
        command.args([
            "--title",
            &title,
            "-e",
            env.to_str()
                .context("Environment executable path is not UTF-8")?,
            "-u",
            "TMUX",
            "-u",
            "TMUX_PANE",
        ]);
        command.args(&args);
        Ok::<_, anyhow::Error>(command)
    };
    let mut opened = configure(true)?
        .status()
        .is_ok_and(|status| status.success());
    if !opened && std::env::var_os("ALACRITTY_SOCKET").is_some() {
        opened = configure(true)?
            .env_remove("ALACRITTY_SOCKET")
            .status()
            .is_ok_and(|status| status.success());
    }
    if !opened {
        configure(false)?.spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_destination_reuses_instance_override_or_discovered_hostname() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let home = Home::new(Some(temp.path().into()))?;
        let peer = Peer {
            identity: crate::peer::Host::new(home.clone())?.identity,
            origin: "https://linux.example.ts.net".into(),
            identity_conflict: false,
            projects: vec![],
            error: None,
        };
        assert_eq!(destination(&home, &peer)?, "linux.example.ts.net");
        for target in ["viktor@linux-vm", "vm-with-custom-port"] {
            std::fs::write(
                home.path.join("ssh-peers.json"),
                serde_json::to_vec(&BTreeMap::from([(&peer.identity.instance, target)]))?,
            )?;
            assert_eq!(destination(&home, &peer)?, target);
        }
        std::fs::write(home.path.join("ssh-peers.json"), "invalid JSON")?;
        assert!(destination(&home, &peer).is_err());
        Ok(())
    }

    #[test]
    fn shell_arguments_preserve_quotes_and_metacharacters() -> Result<()> {
        let text = "space ' quote $(echo wrong); newline\n";
        let output = Command::new("sh")
            .args(["-c", &format!("printf %s {}", quote(text))])
            .output()?;
        assert_eq!(String::from_utf8(output.stdout)?, text);
        Ok(())
    }
}
