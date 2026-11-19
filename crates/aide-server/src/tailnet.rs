use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{collections::BTreeMap, time::Duration};
use tokio::process::Command;

#[derive(Debug, Deserialize)]
pub struct Device {
    #[serde(rename = "DNSName")]
    pub dns_name: String,
}

#[derive(Debug, Deserialize)]
pub struct Inventory {
    #[serde(rename = "Self")]
    pub this: Device,
    #[serde(rename = "Peer", default, deserialize_with = "peer_map")]
    pub peers: BTreeMap<String, Device>,
}

fn peer_map<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, Device>, D::Error> {
    Ok(Option::<BTreeMap<String, Device>>::deserialize(deserializer)?.unwrap_or_default())
}

impl Device {
    pub fn origin(&self) -> Result<String> {
        let dns = self.dns_name.trim_end_matches('.');
        if !dns.ends_with(".ts.net")
            || !dns
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-')
        {
            bail!("Invalid Tailscale DNS name");
        }
        Ok(format!("https://{dns}"))
    }
}

pub async fn command(args: &[&str]) -> Result<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        Command::new("tailscale")
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("Tailscale command timed out")??;
    if !output.status.success() {
        bail!("Tailscale: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(String::from_utf8(output.stdout)?)
}

pub async fn inventory() -> Result<Inventory> {
    Ok(serde_json::from_str(
        &command(&["status", "--json"]).await?,
    )?)
}

/// Use an administrator-configured private route without modifying Tailscale.
pub async fn configured_origin(port: u16) -> Result<String> {
    let origin = inventory().await?.this.origin()?;
    let target = format!("http://127.0.0.1:{port}");
    let status: serde_json::Value =
        serde_json::from_str(&command(&["serve", "status", "--json"]).await?)?;
    let host = format!("{}:443", origin.trim_start_matches("https://"));
    validate_mount(&status, &host, &target).with_context(|| {
        format!("Configure private hosting once with: sudo tailscale serve --bg --https=443 --set-path=/aide {target}")
    })?;
    Ok(origin)
}

fn validate_mount(status: &serde_json::Value, host: &str, target: &str) -> Result<()> {
    if status["AllowFunnel"]
        .as_object()
        .is_some_and(|ports| ports.values().any(|value| value.as_bool() == Some(true)))
    {
        bail!("Tailscale Funnel is enabled; disable public sharing before hosting Aide");
    }
    let mut found = false;
    if let Some(handlers) = status["Web"][host]["Handlers"].as_object() {
        for (mount, existing) in handlers {
            if mount == "/aide" || mount == "/aide/" {
                let proxy = existing["Proxy"].as_str();
                if proxy != Some(target) {
                    bail!(
                        "Tailscale Serve {mount} does not point to this Aide listener ({target})"
                    );
                }
                found = true;
            } else if mount.starts_with("/aide/") {
                bail!("An existing route beneath /aide would shadow the Aide service");
            }
        }
    }
    if !found || status["TCP"]["443"]["HTTPS"].as_bool() != Some(true) {
        bail!("No matching private HTTPS Serve route for /aide");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_hosting_requires_a_matching_preconfigured_route() {
        let host = "vm.example.ts.net:443";
        let target = "http://127.0.0.1:49173";
        assert!(validate_mount(&serde_json::json!({}), host, target).is_err());
        for mount in ["/aide", "/aide/"] {
            let config = serde_json::json!({"TCP":{"443":{"HTTPS":true}},"Web":{host:{"Handlers":{mount:{"Proxy":target},"/other":{"Proxy":"http://127.0.0.1:8000"}}}}});
            assert!(validate_mount(&config, host, target).is_ok());
        }
    }

    #[test]
    fn hosting_rejects_missing_public_or_conflicting_routes() {
        let host = "mac.example.ts.net:443";
        let target = "http://127.0.0.1:49173";
        let config = serde_json::json!({"Web": {host: {"Handlers": {"/other": {"Proxy": "http://127.0.0.1:8000"}}}}});
        assert!(validate_mount(&config, host, target).is_err());
        for handler in [
            serde_json::json!({"Path":"/tmp"}),
            serde_json::json!({"Proxy":"http://127.0.0.1:8000"}),
        ] {
            let config = serde_json::json!({"Web":{host:{"Handlers":{"/aide":handler}}}});
            assert!(validate_mount(&config, host, target).is_err());
        }
        let config = serde_json::json!({"Web":{host:{"Handlers":{"/aide":{"Proxy":"http://127.0.0.1:8000"}}}}});
        assert!(validate_mount(&config, host, target).is_err());
        assert!(
            validate_mount(
                &serde_json::json!({"AllowFunnel":{host:true}}),
                host,
                target
            )
            .is_err()
        );
        assert!(
            validate_mount(
                &serde_json::json!({"Web":{host:{"Handlers":{"/aide/api":{"Proxy":target}}}}}),
                host,
                target
            )
            .is_err()
        );
    }

    #[test]
    fn inventory_only_uses_valid_dns_destinations() -> Result<()> {
        let alone: Inventory =
            serde_json::from_str(r#"{"Self":{"DNSName":"mac.example.ts.net."},"Peer":null}"#)?;
        assert!(alone.peers.is_empty());
        let inventory: Inventory = serde_json::from_str(
            r#"{"Self":{"DNSName":"mac.example.ts.net."},"Peer":{"key":{"DNSName":"vm.example.ts.net."}}}"#,
        )?;
        assert_eq!(inventory.this.origin()?, "https://mac.example.ts.net");
        assert_eq!(
            inventory.peers["key"].origin()?,
            "https://vm.example.ts.net"
        );
        for dns_name in [
            "evil.test",
            "vm.ts.net/path",
            "user@vm.ts.net",
            "vm.ts.net:443",
        ] {
            assert!(
                Device {
                    dns_name: dns_name.into()
                }
                .origin()
                .is_err()
            );
        }
        Ok(())
    }
}
