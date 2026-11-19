# Running Aide across machines

This implementation uses an externally installed Tailscale client. Each host owns its tasks; your dashboard host aggregates peers and launches its own terminal windows. All devices allowed to reach the private endpoint are trusted to manage tasks. Browser-origin checks remain enabled. Do not expose Aide through Funnel or a public listener.

## Prepare a Mac and Linux VM

For the complete machine setup, personal configuration, authentication, and source-build procedure, follow [Linux machine setup](LINUX_SETUP.md).

Use a Linux VM with its own disk and checkout. UTM with an Ubuntu image matching the Mac CPU architecture is suitable; 4 CPUs, 8 GB RAM and 60 GB storage are reasonable development defaults.

1. Install Tailscale on the Mac and Linux VM and sign both into the same account. See [Linux installation](https://tailscale.com/docs/install/linux).
2. Enable Tailscale SSH on the Linux VM with `sudo tailscale set --ssh`. Configure the tailnet SSH policy to allow your identity to access the intended Linux account. See [Tailscale SSH](https://tailscale.com/docs/features/tailscale-ssh).
3. Check `ssh <linux-user>@<vm-name>` from the Mac. Ordinary OpenSSH configuration and keys also work; Aide does not manage SSH credentials.
4. Install Rust, Just, Git, jj, tmux, fish, and Codex on the VM; configure Codex and VCS credentials there. Alacritty is needed only on desktop hosts that launch windows. See the root README for the existing development setup and installing Just.
5. Check out this version on each host and run `just install-aide`. The dashboard build requires Trunk and the wasm32 Rust target (`just setup` installs/checks Rust build dependencies).
6. Create a project using `aide init /path/to/project --repo /path/to/project/repository` on each task host.

## Run privately

Configure the private route once on each host (use sudo on Linux):

```sh
tailscale serve --bg --https=443 --set-path=/aide http://127.0.0.1:49173
```

Then run Aide in a foreground terminal:

```sh
aide --tailnet --no-open
```

The Tailscale executable must be on PATH. Complete any HTTPS/Serve enrollment prompt during the one-time setup. Aide defaults to loopback port 49173 and reads the existing Serve configuration without modifying it or requiring operator privileges. Missing or mismatched routes, shadowing subroutes, and enabled Funnel produce an error. If using `--port` to override the default, configure the route to match. `--port 0` remains available for an ephemeral loopback-only development server.

Visit `https://<machine>.<tailnet>.ts.net/aide/`, including from the same Mac. `tailscale serve status` shows the address. The printed loopback URL is also available for troubleshooting. Stopping Aide leaves its Serve route configured, but unavailable until Aide restarts.

The header shows each machine's name, `(attention.ready)` counts, and a connection dot. Click a machine to filter tasks; click it again to show all machines. Disconnected machines retain their names but show no task counts. Tasks remains on the left; the top-right cog opens the dashboard host's AI configuration (independent of the task filter).

Aide probes the visible Tailscale device list periodically and refreshes known hosts' task snapshots. Discovery can take roughly 30 seconds plus probe time. Known unavailable hosts remain listed as disconnected with no stale tasks/counts. `AIDE_NAME` overrides the displayed machine name. `AIDE_DESKTOP=1` enables desktop launching (default on macOS); `AIDE_DESKTOP=0` disables it. A Linux desktop can opt in explicitly.

Use the Mac dashboard for terminal windows on the Mac. Local tasks attach directly; VM tasks run the same Aide attachment command through SSH. The peer advertises its executable and Aide-home paths, avoiding reliance on the SSH shell's PATH. A headless dashboard can manage tasks but cannot open a desktop window; CLI attachment still works.

## SSH destination mapping

By default, Aide uses the peer's Tailscale DNS name and your normal SSH configuration. Configure `User` for that hostname in `~/.ssh/config` when the Linux username differs. Alternatively, create `~/.aide/ssh-peers.json` mapping instance IDs (shown by `aide peer list`) to SSH aliases:

```json
{
  "PEER-INSTANCE-ID": "aide-vm"
}
```

The alias belongs in your ordinary SSH configuration. The SSH account must own the peer's Aide home and tasks. Host verification/authentication prompts occur in the terminal. A closed terminal disconnects SSH without completing the task.

## Open remote workspaces in Zed

Use the Mac dashboard's **Zed** shortcut beside **Terminal**. Install this Aide version on both the dashboard host and task host, and make the `zed` CLI available on the dashboard service's PATH. No Zed desktop installation is needed on the task host.

Zed reuses Aide's SSH destination mapping above. Put custom ports, identity files and jump hosts in `~/.ssh/config` under the selected alias. The SSH account must access the task owner's workspace. Zed opens its window on the dashboard host, connects over SSH, and manages its matching remote server. Host-key/password prompts and later connection errors appear in Zed. For restricted remote internet access, configure Zed's `upload_binary_over_ssh` option.

Aide validates the current remote workspace before launching and unparks the task after a successful CLI request. “Requested Zed” confirms the launch request, not an established SSH session. A failed activation acknowledgement after launch is reported separately; inspect Zed and refresh task state before retrying. Headless dashboards omit Zed and Terminal; remote Review remains unavailable.

## CLI

Keep the dashboard running with `--tailnet` to discover peers. CLI commands use its remembered peer addresses and query fresh state:

```sh
aide peer list
aide task list --all-peers
aide task list --peer aide-vm
aide task attach example --peer aide-vm --project ProjectName
aide task start example --peer aide-vm --project ProjectName --repo repository --new-window
aide task park example --peer aide-vm --project ProjectName
aide task cleanup example --peer aide-vm --project ProjectName
```

Peer selectors accept a unique name or instance ID. Project selectors accept a unique project name or host-local root. Remote PR link/unlink/list/refresh and recovery use the task host's configuration. Cleanup requires the same explicit confirmation as local cleanup. Remote workspace-only start and guided-review routing are not supported in this slice. Use the dashboard Zed button for remote editing.

After a timeout, execution may still be underway or complete. Inspect task state before retrying. A retry is a new request: matching startup reuses existing work, cleanup continues partial removal, parking sets an explicit state, and recovery reuses healthy components. No commands are automatically replayed after reconnecting.

## Automatic startup

Configure Tailscale Serve first, then use the repository recipes (Python 3 and Just required):

```sh
just install-aide          # build/install the binary and embedded dashboard
just aide-service-install # install/enable a user LaunchAgent or systemd unit
just aide-service-status
```

After updating your checkout to the version you want:

```sh
just upgrade-aide         # build/install successfully, then restart the service
```

`upgrade-aide` does not fetch or change your checkout. A failed build/install leaves the running service alone. `install-aide` remains a binary-only installation for machines without a service. `aide-service-restart` restarts an existing service without rebuilding; on macOS it signals only the dashboard with SIGINT and lets launchd KeepAlive restart it, preserving task sessions. Check `aide-service-status` after a restart because startup can fail if Tailscale is unavailable or its route is wrong.

Service setup uses the templates in `examples/`, substitutes your home on macOS, and preserves existing service files and custom settings on subsequent runs. It expects the binary at `~/.cargo/bin/aide` and defaults to port 49173, `--tailnet`, and the normal Aide home. If you use custom Cargo install paths or service settings, edit the service definition. Installing the service does not stop an unrelated foreground dashboard; stop it first if it owns the same home or port.

On Linux this enables the systemd user service. For startup without a login, an administrator must still run `sudo loginctl enable-linger <user>`. Logs: `journalctl --user -u aide -f`.

On macOS the LaunchAgent starts in your logged-in desktop session, not before login. Logs: `~/Library/Logs/aide.log` and `aide-error.log`. Serve does not start Aide itself. Tailscale enrollment, private Serve configuration, no-Funnel policy, and SSH authentication remain external setup steps.

## Validation status

Controlled Rust tests cover request correlation, lost-response reconciliation, host identity checks, routed peer actions, and discarding disconnected data. Browser acceptance covers the `/aide/` prefix, headless controls, and connection loss alongside existing task flows. See `plans/REMOTES_PLAN.md` for the latest actual results and remaining gaps. Live Mac/Linux acceptance passed on 2026-11-19: private Serve, automatic discovery, remote creation and parking, SSH/tmux attachment, native Codex recording, service restarts, VM reboot recovery, and cleanup. Cloud-host testing remains separate.

### Prevent accidental public exposure

Keep the `funnel` node attribute unavailable to Aide hosts in the Tailscale policy. Remove it from any applicable `nodeAttrs` grants (including broad `autogroup:member` grants); preserve unrelated attributes and access rules. Aide checks for Funnel at startup, not continuously. Do not enable Funnel later while Aide is running. See [Tailscale Funnel policy requirements](https://tailscale.com/docs/features/tailscale-funnel).

Only host trusted applications on other paths of the same Serve hostname: browser origins do not isolate `/aide` from sibling paths. A detected duplicate Aide identity stays blocked, including CLI calls; repair the cloned host identity and remove its conflicting entry from `known-peers.json` while Aide is stopped before rediscovery.
