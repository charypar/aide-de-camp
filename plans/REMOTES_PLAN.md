# Aide across machines

Status: implemented with controlled verification and live Mac/Linux Tailscale acceptance completed, 2026-11-19. Cloud-host latency/NAT and phone interaction remain future checks. This extends the [coordination plan](COORDINATION_PLAN.md) without defining the broader milestone 2. Installation and usage are documented in the [remote access guide](../docs/REMOTES.md).

## Implementation and verification record — 2026-11-17

Implemented shared host operations, repeat-safe startup, versioned request/result and read messages, identity checks, connection-independent execution, Serve publication with conflict/Funnel checks, Tailscale inventory discovery, persisted peer identities, dashboard aggregation/filtering/counts, disconnected-state clearing, CLI peer routing, PR operations, and a shared attachment command invoked directly or over SSH. Browser mutations have response correlation and timeout notices; gateway failures refresh peer state through reads without replaying commands. Service templates and Just recipes are included.

Wire targets use the host instance ID, canonical registered project root, and task UUID key. Project moves require refreshed discovery/listings; they are not transparent task migration. Completed human task IDs remain reserved, and mutations/attachment reject stale task keys. The display name is configurable through AIDE_NAME; desktop capability through AIDE_DESKTOP. SSH destinations are configured locally in ssh-peers.json; the host supplies its executable and Aide-home paths. Remote workspace-only creation, editor launch, guided-review routing, and phone interaction remain outside this slice.

Verification: 55 automated server tests passed (two opt-in live tests excluded from that run); the native Codex observer separately passed with seven configured MCP entries and no model prompt. The 22 existing task-dashboard browser cases passed across the broad run and focused reruns after intentional interface assertion updates. All nine PR/private-instruction browser cases and all three remote-dashboard cases passed. Server/UI builds, formatting, server/UI Clippy, diff whitespace checks, Just recipe listing, and the LaunchAgent plist syntax check passed. Headless Playwright MCP independently exercised the prefixed headless dashboard, creation, ready counts, parking, reload persistence, unparking, confirmed cleanup, and retained history. Protocol fixtures exercised separate host services over loopback WebSockets, including lost-response reconciliation, routed mutations, and disconnected-data removal.

Remaining acceptance as of 2026-11-17 (superseded by the live record below): the user is preparing a Mac-hosted Linux VM and tailnet. No real Serve route was published and no live cross-machine SSH/tmux attachment, Tailscale discovery, service installation/reboot, or Linux native Codex recovery has been verified. The systemd template has not been exercised on Linux. Loopback protocol fixtures and mocked peer UI snapshots do not establish those guarantees. The independent guided-review tool was unchanged and its selection/chat/feedback suite was not rerun.

## Setup refinement — 2026-11-19

The user selected a one-time administrator-configured Serve route, with no ongoing Tailscale operator privilege for Aide. Aide now validates and reuses that route without changing Tailscale settings. Its configurable loopback default is 49173 (not OTLP’s 4317); the external contract remains HTTPS 443 and `/aide/`. The Linux route has been configured by the user. The machine setup manual is [LINUX_SETUP.md](../docs/LINUX_SETUP.md). Live application acceptance is recorded below.

## Live acceptance record — 2026-11-19

Built and installed the WIP on Ubuntu ARM64 using Rust, Trunk, and `just install-aide`; registered `/home/viktor/AideDeCamp` with its main `aide-de-camp` checkout. Both machines use manually configured private HTTPS Serve routes to loopback port 49173. Aide requires no Tailscale operator privilege. The Mac test gateway used an isolated Aide home, leaving existing local tasks untouched.

Headless Playwright MCP exercised the real Mac Serve URL and automatically discovered Linux peer: machine selection, remote task creation, status/counts, parking/unparking, reload persistence, SSH terminal opening, disconnection with task/count removal, reconnection, and host-labelled cleanup confirmation. Two disposable tasks were completed and their workspaces removed, with conversation history and recordings retained. Native Alacritty on the Mac attached through SSH to Linux tmux; detaching and reattaching reused the task. Two short no-tools Codex turns completed and were recorded. The configured SSH override reused the already trusted `viktor@linux-vm` destination; the discovered full hostname had not yet been added to the Mac's SSH known-hosts file.

Linux testing exposed a group-writable runtime socket directory rejected by native Codex. Aide now creates/repairs `~/.aide/run` to mode 0700; a regression test failed before the fix and passed afterward, including preservation of existing files. Recovering the failed task succeeded. `aide doctor` now uses tmux's `-V` option. A second task then started successfully without recovery.

Installed and exercised the systemd user service with both user binary directories on PATH and `KillMode=process`, so task runtimes survive dashboard stops. Verified this with a tmux server created by the service itself. Enabled lingering with the user's assistance. An actual user-authorized VM reboot changed the boot ID; the private identity endpoint was available before the next SSH login. Systemd retried one initial startup while Tailscale became ready. The recovered task kept conversation ID `01a1f501-77b9-71c2-a51e-5440759489c9`, and SSH attachment displayed the original `BOOT_CHECK_OK` conversation without replaying the turn.

Latest verification: 58 server tests passed, two optional live tests excluded; server Clippy and formatting passed. All three remote browser regressions passed again after test harnesses were changed to request ephemeral ports explicitly. The broader browser suites and native observer previously passed as recorded above; they were not all rerun during this live pass. Lost-response reconciliation and partial filesystem cleanup remain covered by controlled tests rather than fault injection on the live tailnet. No cloud-host or live remote PR-provider test was performed. The Linux user service remains installed/enabled; the Mac test gateway is temporary, not a new LaunchAgent installation.

## Header refinement — 2026-11-19

Rebased the WIP onto the newer main revision containing AI configuration navigation, task collapse on parking, and latest CI status handling. The missing Tasks/settings navigation was a version mismatch. Preserved configuration request correlation alongside peer task routing; peer task APIs reject configuration requests explicitly.

Moved machine selection into the header with per-host `(attention.ready)` counts and connection dots, retained Tasks on the left, and placed the existing configuration cog at the far right. Selection toggles back to all machines and survives periodic snapshots; disconnected hosts retain names without stale tasks/counts, including gateway disconnection.

Verification: the new five-machine browser regression failed against the prior UI and passed after implementation. All 63 server tests passed (two opt-in live tests excluded), all 22 task-dashboard tests, four remote-dashboard tests, and six configuration tests passed. Server/UI Clippy, formatting, and builds passed. Headless Playwright MCP verified the new header, actual Mac/Linux discovery, machine selection, and configuration/Tasks navigation through the Mac Serve URL. Native session/reboot checks were not repeated for this UI change.

## Remote Zed — implemented 2026-11-19

The task heading offers Zed beside Terminal for local and remote tasks when the dashboard host can launch desktop applications. Zed runs on that host, matching Terminal semantics. Review remains local-only.

`RemoteEditor` reuses the instance-keyed `ssh-peers.json` override and discovered hostname fallback shared with Alacritty. Use SSH aliases for custom ports, keys and jump hosts. The local Zed CLI receives one `ssh://user@host/absolute/path/` argument, with percent-encoded path bytes and a trailing directory slash. IPv6 destinations are bracketed. No shell is involved.

The additive `editor_workspace` peer operation checks the host instance, registered project, task UUID, lifecycle, pending startup, task lock and workspace ownership. Resolution returns the fresh owner-side workspace, not a cached path. It never starts a runtime or desktop application. After the local CLI succeeds, the gateway repeats validation with an unpark acknowledgement. Missing/failing launchers leave parking intact. If acknowledgement fails after launch, the UI says launch was requested but task activation was not confirmed; no launch is automatically replayed. Cleanup can still occur between host validation and Zed opening the directory; this does not recreate an Aide task or runtime. Later SSH failures belong to Zed, so the success notice says “Requested Zed”.

Install this version on both hosts: older peers do not understand the new operation. Existing peer operations and protocol version remain compatible. Services were upgraded after implementation with user authorization; see the deployment record below.

Sources: [official remote-development documentation](https://zed.dev/docs/remote-development) and [Zed SSH URL parser](https://github.com/zed-industries/zed/blob/main/crates/zed/src/zed/open_listener.rs). Zed manages its remote server and SSH prompts. Set `upload_binary_over_ssh` in Zed's connection settings if the remote cannot download its server directly.

Verification: 70 server tests passed with two opt-in live tests excluded, followed by both attachment tests passing after adding the SSH destination regression. Coverage includes owner-side validation, launch failure, exact URL arguments, fresh workspace resolution, lost acknowledgement/retry and disconnected peers. Six focused browser cases passed across the main run and one rerun correcting macOS canonical-path expectations: remote Zed routing/error/retry/reload, headless controls, machine selection, local collapsed shortcuts and parking behavior. Playwright MCP was unavailable, so browser checks used the repository's headless Playwright runner.

Native Zed 1.29.2 connected from the Mac to `viktor@linux-vm` (Linux ARM64), started its remote server, and persisted the exact temporary workspace path containing spaces, Unicode, `%`, `#`, `?` and `:`. An isolated-profile attempt exited because Zed was already running; the successful check used a new window in the existing application. The disposable folder is `/tmp/aide-zed-check.RcQ5F4/space é%#?:123`. Interactive editing/reconnection was not tested in that initial pass. The later live dashboard check through both upgraded services is recorded below. Native Codex and guided-review flows were unchanged and not rerun.

Server/UI builds, normal server/UI Clippy and formatting passed. All-target Clippy remains blocked by the pre-existing `items_after_test_module` warning in `worker.rs`; it passes with only that lint suppressed.

## Remote Zed deployment — 2026-11-19

Committed the feature and upgrade instructions with jj as `d44ff393644876dbfa1f4009b5bc182825c5c14d`, pushed `wip/zed-buttons`, and fetched it into the clean Linux checkout. Ran `just upgrade-aide` first on Linux and then on the Mac. Both services restarted successfully; the Linux task tmux server survived. Existing service definitions, Tailscale routes and Aide homes were preserved. `AGENTS.md` now documents jj bookmark delivery and the Just upgrade/status workflow.

A live headless Playwright check through the upgraded Mac dashboard discovered a disposable Linux workspace-only task, parked it, clicked Zed, observed the launch result and unpark, and confirmed the exact workspace/SSH host/user in native Zed's saved workspace record. Reload retained the unparked state. Dashboard cleanup removed the disposable workspace and retained Completed history. No model turn was needed. This covers the actual upgraded gateway-to-owner protocol and local native Zed launch; interactive editing and connection-loss recovery in Zed remain untested.

## Experience and scope

Use the Mac's local Aide dashboard and CLI to inspect and manage tasks on the Mac and Linux hosts. Start with a Linux VM running on the Mac, using its own filesystem and project checkout. Join both hosts to the same Tailscale network and exercise actual private networking, not shared folders or a localhost-only substitute.

Each host owns its projects, task databases, workspaces, credentials, recordings, tmux sessions, and Codex processes. No database replication, workspace synchronization, task migration, or central coordinator. The first version serves one person's machines.

Local and remote control are equivalent operations reached through different interfaces/transports. Use the same validation, lifecycle rules, locking, status queries, and recovery behavior. Improve retry behavior locally as well as remotely.

Ownership determines where work executes; host capabilities determine available desktop actions; transport only determines how requests reach the owner. The local CLI may continue operating without a running service, while remote API requests require one, but both invoke the same operation implementation.

Phone interaction and remote guided-review routing remain deferred. Remote Zed launching is implemented as the follow-up described above; retain existing local actions. No browser terminal, custom URL handler, SSH credential UI, or durable command ledger.

## Service and client responsibilities

Each Aide installation can provide:

- A local task manager responsible for that host's resources.
- A private peer API exposing only that host's projects and tasks.
- A dashboard gateway that combines local state with direct queries to discovered peers.
- Desktop launching on the dashboard host when that host has desktop-launch capability.

The Mac browser connects to the Mac service. The Mac service connects to VM services. Peer APIs never return their own aggregated peer views, preventing recursive aggregation. The CLI uses the same operation layer and peer routing; ordinary local commands remain usable without requiring the dashboard where they are usable today.

“Local” means the host serving the dashboard (or running the CLI), not the browser's physical location. Compare stable host instance IDs: tasks owned by that host attach directly to tmux; tasks on another host attach through SSH. The UI identifies its dashboard host and labels desktop launches accordingly, such as “Open terminal on Mac”. Headless hosts omit desktop-launch actions. Opening the Mac dashboard from another trusted device still launches on the Mac, never on the browser device.

Headless hosts still support CLI attachment in the invoking terminal. Only creating a desktop window requires desktop-launch capability; attaching to a task does not.

Separate shared operations from the current HTTP manager and terminal-launching code. Keep transport adapters thin. Use cohesive types for host operations, peer discovery/connections, and desktop attachment; do not introduce a general distributed workflow framework.

A long-running service is now an intentional requirement for remote availability. Provide documented launchd/systemd setup. Task runtimes must continue to survive service shutdown as they do today. Reuse the existing startup recovery policy; do not replay interrupted agent turns.

## Networking, publication, and discovery

Use Tailscale for private connectivity and Tailscale Serve for HTTPS exposure. Aide binds locally; opt-in tailnet access registers a route under:

```text
https://<machine>.<tailnet>.ts.net/aide/
```

The common external contract is HTTPS on port 443 and the `/aide/` prefix. Local listener ports can differ; a stable configured default simplifies service setup. Assets, navigation, and WebSocket URLs must work under the prefix as well as the existing local entry point.

Use the Serve URL as the normal entry point even when the browser runs on the same machine. This unifies HTTPS, routing, and origin handling. Retain loopback access as an offline/troubleshooting fallback when Tailscale is unavailable; it uses the same host-based action semantics.

An administrator configures the private Serve route once. Aide reads and validates that it points to its loopback listener, and never registers or modifies routes. No ongoing Tailscale operator privilege is needed. Report missing/mismatched routes and public exposure clearly. Serve does not keep the Aide process running; the configured route survives backend restarts.

Discover candidate devices through the local Tailscale client, initially `tailscale status --json`; no administrative API key or broadcast protocol. Probe a fixed versioned identity endpoint beneath `/aide/` with bounded concurrency and timeouts. Validate the response before adding an Aide peer. Ignore unrelated devices and exclude self by instance identity. Periodically refresh discovery, independently of task refreshes.

Persist a host instance ID and an overridable display name defaulting to hostname. Remember discovered identity/address information so known disconnected peers remain visible. Do not retain stale task listings or counts for display. Detect incompatible API versions and conflicting identities rather than silently merging them. Cloned Aide homes must not silently become the same host.

Manual address overrides can be an escape hatch, not a required setup step. No manually maintained reciprocal peer lists.

## Access boundary

Agreed trust model: all devices in this personal tailnet are owned and trusted by the user. Aide relies on Tailscale authentication and access policy for remote authorization: any device permitted to reach Aide's private endpoint may manage that host's tasks. No additional Aide login, pairing, per-user permissions, or identity-header authorization layer is required. Revisit the policy if less-trusted devices or shared access are introduced.

Tailscale is installed, enrolled, and running externally on each host; Aide does not embed a VPN client or form the network itself. Keep the backend bound to loopback and publish remote access only through private Tailscale Serve, never Funnel or a public listener. Document and validate the tailnet policy and private exposure on Mac/Linux. This relies on correct host/network configuration as well as Tailscale; it is not a guarantee against compromise of a trusted device.

Retain browser-origin protections for WebSockets and browser-triggered mutations. Origin checks protect against requests from unrelated websites; they are not a second login system. Under the trusted-device policy, dashboard desktop launches are permitted through Serve and always execute on the dashboard host. Do not infer browser location from loopback addresses or proxy headers. Task-management routing must not forward a desktop-launch action to the task owner: the dashboard host launches its terminal, using SSH when the task owner differs. This supersedes the earlier local-entry-only launcher restriction.

## Task addressing and dashboard

Address a task by host instance, project identity, and stable task identity. Human task names may collide across hosts/projects; paths are host-local data and must never accidentally become local operation targets. Check whether existing project/task keys remain unique across cleanup and recreation before choosing wire identifiers.

The dashboard offers an all-machines view and a machine filter. Group tasks by machine and project; show the owning machine in task details and cleanup confirmation. Reuse current attention/ready definitions and parking rules. Machine navigation lives in the top-right header: name, `(attention.ready)` counts, and connection dot for each host (designed for up to five). Clicking a host filters tasks; clicking it again restores all machines. Disconnected names remain visible without counts, including when the gateway socket drops. Tasks stays on the left and the AI configuration cog sits at the far right; configuration belongs to the dashboard host, independently of the task filter. Browser-title counts follow the existing all-project convention extended across connected hosts, with peer connectivity shown in the machine header.

On peer disconnection, remove its task data and counts and show the offline dot with a Disconnected tooltip in the machine header. Reconnect by fetching a fresh snapshot. A reachable but incompatible or unauthorized host should have an explanatory connection state. A transport connection without a current snapshot is not sufficient to display old data as live.

Apply the same freshness rule when the browser loses its dashboard connection: remove unavailable task data/counts and show the connection failure rather than presenting the last snapshot as live. Reconnect with a fresh snapshot regardless of whether the unavailable source was local or remote.

Proposed CLI syntax, to refine against existing Clap commands:

```sh
aide peer list
aide task list --all-peers
aide task list --peer dev-vm
aide task attach task-123 --peer dev-vm --project my-project
```

## Operations and protocol

Keep versioned JSON over secure WebSockets through Serve. Reuse the existing command/event model where appropriate, adding explicit host-local read queries and response correlation. First-version management scope: list projects/tasks, get task status, start, set parked state, recover, confirmed cleanup, and existing PR link/unlink/refresh actions. PR operations use the owning host's credentials and configuration. Project registration remains host setup. Remote editor/review integration remains explicitly deferred, not inherently prohibited by the architecture.

Each action carries a connection-scoped request ID. Return one success/error result when execution finishes. Task snapshots show intermediate Starting/Closing states; no separate durable acceptance acknowledgement is required. Request IDs correlate replies, not deduplicate requests across restarts.

Execution is owned by the host while its process is running, independently of a browser or peer connection. A disconnect must not cancel an already executing mutation. A service/process crash relies on persisted task lifecycle and resource reconciliation, not a persistent queue of commands.

After a timeout or lost response, query the relevant task or list:

- Desired state reached: show that outcome.
- Still preparing/closing: show progress and continue observing.
- Partial state or persisted error: show the problem and offer the appropriate explicit retry.
- Host unreachable: show Disconnected and read fresh state after reconnection.

A read establishes current state, not proof that a particular request caused it. Reads must not trigger setup/recovery/cleanup. Audit existing snapshot observation for this boundary; ordinary monitoring bookkeeping is distinct from initiating lifecycle work.

Timeout means success is unconfirmed, not that the host rolled back or stopped. Do not automatically replay mutations. A user retry is a new request against current state. Keep reconciliation context for an in-flight UI action as needed, without introducing a durable action ledger. Browser reload can reconstruct the task view from host state.

### Required repeat behavior

| Operation | Repeated request |
| --- | --- |
| Start | Matching existing task parameters reuse the task and complete missing setup; conflicting repository/parameters fail clearly. Closed tasks are not implicitly resurrected. |
| Cleanup | Continue partial cleanup; already completed returns success. Preserve confirmation requirements and retained history. |
| Set parked | Set an explicit boolean, never toggle. Apply normal lifecycle validation. |
| Recover | Reuse healthy components and repair missing managed components; do not replace unrelated/user panes or revive closing/completed tasks. |
| PR link/unlink/refresh | Link/unlink express the desired association, with repetitions leaving it unchanged; refresh reads current provider state. Preserve existing validation and host-side polling coordination. |
| Attach | Reuse the task/runtime; each explicit invocation may create another terminal attachment. |

Existing host-local task locks reject conflicting in-progress mutations as busy. Do not hold a task lock for the lifetime of an interactive terminal session. Retryable does not mean every failure should retry automatically: invalid configuration, permission failures, and ambiguous resource ownership remain visible errors.

Audit startup and cleanup against filesystem/VCS effects as well as database state. In particular, complete partial workspace creation without replacing user work; recognize owned resources already removed during cleanup without deleting unowned paths. Preserve existing cleanup error reporting and recovery behavior.

## SSH attachment

Assume SSH access is configured externally. Prefer Tailscale SSH for the Linux VM if convenient; ordinary OpenSSH keys/configuration remain supported. No mandatory dedicated Aide Unix account. Login must resolve to the account and Aide home owning the selected task.

Use the discovered host address by default, with a local per-peer SSH destination/alias override. Reuse SSH configuration for usernames, keys, host verification, and connection options. Do not infer the remote Unix username from the local username or introduce a credential store. Authentication/host verification prompts belong in the launched terminal.

Use one Aide task-attachment command for both paths. For a task owned by the dashboard host, launch that command directly in Alacritty; for another host's task, launch it through SSH with a pseudo-terminal. CLI attachment uses the invoking terminal instead of requiring a desktop window. Both paths share task resolution, recovery, and tmux attachment logic. Resolve the exact command syntax during implementation, including executable discovery in non-interactive SSH environments and non-default Aide homes.

The shared attachment command resolves project/task identity, checks lifecycle under the existing locks, prepares missing runtime components using host-owned configuration, and attaches to tmux. Environment selection follows the operation, not whether the request traveled over a network: dashboard start/recovery uses the owning host's environment; explicit resume from a work shell uses that shell's exported environment. Do not forward the dashboard client's environment to the task host. Preserve these rules for both local and SSH attachment.

Use structured identifiers and safe argument encoding at both process and remote-shell boundaries; never execute arbitrary command text from a peer response. Validate the expected host/home/task identity, including task recreation, before attachment.

If cleanup races with attachment, attachment fails or the attached session closes; it must not resurrect the task. Closing Alacritty disconnects SSH and leaves tmux/Codex running. Terminal-launch failure after successful task creation reports a launch error and offers attach again, never another start.

## External setup versus Aide implementation

| External machine setup | Aide responsibility |
| --- | --- |
| Install/sign in to Tailscale; enroll hosts | Read device inventory and discover Aide peers |
| Enable HTTPS; configure the private Serve route once and set access policy | Validate and reuse the route without Tailscale administration privileges |
| Install Aide and its required host tools | Offer diagnostics and documented Just recipes/setup steps |
| Configure Codex/VCS credentials and register a test project | Execute tasks using host-local configuration |
| Configure working SSH/Tailscale SSH | Launch local SSH attachment; allow destination override |
| Install launchd/systemd service configuration | Run headlessly and recover existing state according to the established policy |

The user handles account sign-in and credential enrollment. Repeatable setup steps should be documented and automated where useful. Host-side task execution must not require Alacritty or another desktop application on the Linux VM.

## Implementation and verification sequence

1. **Shared operations and reads.** Extract transport-independent operations; test observable repeat behavior and read-only reconciliation first. Cover partial startup/cleanup and task identity reuse. Preserve existing CLI and browser behavior.
2. **Private host interface.** Add protocol versioning, correlated results, read queries, and connection-independent execution. Add Serve/base-path support and instance identity. Validate private exposure, tailnet access policy, origin protection, host-based desktop capabilities, and registration on Mac/Linux before treating private hosting as complete.
3. **Discovery and aggregation.** Add Tailscale inventory adapter, peer connections, machine navigation, and remote CLI selection. Test missing Tailscale, unrelated devices, identity/version conflicts, disconnected snapshots, and reconnects.
4. **Remote mutations.** Route the shared operations to peers. Inject lost responses, slow operations, connection drops, concurrent attempts, and host restarts. Verify read reconciliation and explicit retry without a durable command ledger.
5. **SSH terminal flow.** Use the shared attach command directly or through SSH. Test safe encoding, account/home resolution, session reuse, cleanup races, operation-specific environment selection, headless CLI attachment, and terminal-launch failures independently of successful task creation.
6. **Acceptance.** Use headless Playwright MCP against the real dashboard for selection, creation, parking, status, cleanup, reload, and disconnection flows. Use controlled fixtures for failure injection, and live Codex/tmux/SSH integration on the Mac-hosted Linux VM for native behavior. Report actual checks and gaps. A later cloud-VM check covers internet latency/NAT conditions absent from local virtualization.

Required acceptance journey: discover the Linux VM without adding it manually; create a remote task from the Mac dashboard; open local Alacritty attached to its remote native session; detach and return to the same session; lose a mutation response and reconcile via reads; retry partial setup/cleanup safely; disconnect/reconnect the host without stale task data; restart Aide and the VM and verify the existing recovery policy; close only the selected task while retaining its history and leaving unrelated local/remote tasks untouched.

Run relevant Rust tests, formatting/lint checks, browser tests, and live acceptance checks per slice. Keep the root justfile useful for the new setup and test workflows. This planning change itself requires documentation checks, not application tests.

## Research references

- [Tailscale Serve CLI](https://tailscale.com/docs/reference/tailscale-cli/serve): path routing, proxying, background persistence, and configuration.
- [Tailscale CLI](https://tailscale.com/docs/reference/tailscale-cli): device inventory through status JSON.
- [Tailscale SSH](https://tailscale.com/docs/features/tailscale-ssh): identity-based SSH and supported host variants.
- [Tailscale pricing](https://tailscale.com/pricing): selected as a practical fit for this small personal deployment; verify current terms during setup.

## Security review — 2026-11-19

Reviewed private ingress/origin handling, discovery and identity conflicts, SSH argument construction, peer routing, project/task validation, retry behavior, configuration access, and service setup. Fixed three findings with regressions that failed before the fixes: dashboard framing is now denied with CSP frame-ancestors and X-Frame-Options; identity conflicts remain quarantined across refreshes and serialization (including CLI calls); unregistered project requests are rejected before SQLite opening/migrations.

User chose to keep Funnel unavailable on Aide hosts. Aide only checks Serve/Funnel configuration at startup; enabling Funnel afterwards would expose the API under the current trusted-tailnet model. Remove applicable `funnel` grants from tailnet `nodeAttrs`; this external policy has not been inspected or changed by the agent. Other applications on the same HTTPS hostname share its browser origin, so only trusted applications should share that Serve host.

Remaining findings: HTTP WebSocket handlers spawn unbounded per-request work (message size and response queues do not bound operation concurrency); add bounded admission with explicit busy responses. The headless configuration finding is resolved by the follow-up below. Bounded request admission remains open. No additional login/pairing layer is proposed.

Validation of review fixes: 66 server tests passed, two optional live tests excluded; formatting and server Clippy passed. Headless Playwright MCP verified normal dashboard rendering and that the browser refused iframe embedding. These review fixes are in the workspace; the previously installed Mac/VM binaries have not yet been replaced with them.

## Headless configuration follow-up — 2026-11-19

Configuration editor actions now use the dashboard host's desktop capability in both UI and server. Headless hosts omit file/folder Zed buttons and reject direct editor-launch requests with correlated errors before executing a process. Browsing files, refresh, and copying folder paths/line references remain available; guidance directs users to SSH or their preferred remote editor. Desktop hosts keep the existing Zed workflow. Browser regressions cover both rejected launch variants (no process launched), browsing/copying/refresh/reload, and the existing desktop configuration flows.

Follow-up verification: all eight configuration browser tests passed, including both new regressions failing before implementation; server/UI builds and Clippy plus formatting passed. Installed the updated WIP (including the three review fixes) on the VM and restarted its service. Headless Playwright MCP verified the live VM configuration screen omits Zed actions, displays remote-editing guidance, and rejects a direct editor-launch request with its request ID preserved. Verified the deployed framing header as well. The installed Mac application and running Mac test gateway have not been upgraded in this follow-up.

## Local installation — 2026-11-19

Finalized the implementation change with jj and installed the release binary on the Mac. Replaced the old foreground dashboard and isolated test gateway with `dev.aide.dashboard`, a per-user LaunchAgent using the normal `~/.aide` home, explicit loopback port 49173, `--tailnet --no-open`, and dependency PATH. The existing private Serve route is unchanged. Starts on desktop login and restarts on exit; this is not a pre-login system daemon. Copied the tested Linux SSH alias into normal configuration. Verified all three existing task tmux sessions survived the migration, the service owns port 49173, and the installed CLI sees Linux connected. Headless Playwright MCP verified the normal Mac dashboard through its Serve URL.

## Service automation — 2026-11-19

Added Just recipes for per-user service installation, restart, status, and source-checkout upgrades on macOS/Linux. Existing service definitions are preserved. Upgrade builds/installs first and only then requests restart; it does not fetch source or configure Tailscale. macOS restart signals only the dashboard and relies on KeepAlive, avoiding job unloading and termination of task descendants. Python standard-library tests cover template path escaping, preserved customizations, missing binaries, and platform-specific lifecycle commands. Machine naming remains an OS configuration step; Tailscale naming is independent.

Automation verification: five standard-library tests pass, including an actual Just invocation proving install failure skips service restart. Ran service installation (preserving existing settings), full upgrade, and status on the Mac; launchd restarted successfully and all three task tmux sessions remained. Ran installation/restart/status on the Linux VM with its existing unit preserved and service active. Headless Playwright MCP verified the Mac dashboard after upgrade. Hostname has not been changed by automation.
