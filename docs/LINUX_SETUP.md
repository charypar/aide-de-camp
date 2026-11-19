# Linux machine setup for Aide

This is the repeatable setup for Viktor's Linux task hosts, based on the Ubuntu ARM64 VM setup on 2026-11-19. It covers the machine, personal tools, and Aide installation. Actual cross-machine acceptance is tracked separately in [REMOTES_PLAN.md](../plans/REMOTES_PLAN.md); installing the tools alone does not establish that remote operations work.

Use your ordinary Linux account, with sudo access for system packages. Commands run **on Linux** unless marked Mac. Blocks marked `bash` require Bash; run `bash` first if your login shell is fish. Blocks marked `fish` require fish. Replace hostnames and usernames for each machine.

## 1. Machine and system packages

For a Mac-hosted VM, use a Linux image matching the Mac's CPU architecture and a private VM disk. Around 4 CPUs, 8 GB RAM, and 60 GB storage is a useful starting point. Source builds work with less memory but take longer; limit Cargo parallelism on small VMs. A cloud server follows the same setup.

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev ca-certificates \
  curl unzip git openssh-client fish tmux sqlite3 nodejs npm python3
```

Personal shell/editor tools, optional for Aide itself:

```bash
sudo apt install -y eza zoxide starship just neovim ripgrep fzf
```

Package availability varies by Ubuntu release. Check `apt-cache policy <package>` if one is missing. Just can also be installed with `cargo install --locked just` after installing Rust. The tested VM provides Node 22; check the current Codex package's Node requirement before using an older distro package.

## 2. Tailscale and SSH into the machine

Install Tailscale using its [Linux instructions](https://tailscale.com/docs/install/linux), then enroll this machine in the same personal tailnet as the Mac:

```bash
sudo tailscale up --hostname=linux-vm
sudo tailscale set --ssh
tailscale status
tailscale ip -4
```

Sign in using the URL printed by Tailscale. Configure the tailnet SSH policy to allow your identity to log into the intended Linux account. [Tailscale SSH](https://tailscale.com/docs/features/tailscale-ssh) handles tailnet SSH itself; it does not require an OpenSSH daemon. Ordinary OpenSSH is also supported if you prefer managing its keys and service.

On the **Mac**:

```bash
tailscale ping linux-vm
ssh viktor@linux-vm
```

If the host is offline, use the VM console to check:

```bash
systemctl is-active tailscaled
curl -I --max-time 10 https://tailscale.com
sudo journalctl -u tailscaled -n 50 --no-pager
tailscale netcheck
```

The public website being reachable does not prove the coordination service is reachable. Test `curl -Iv --max-time 15 https://controlplane.tailscale.com` if the logs show coordination timeouts. Restarting `tailscaled` recovered our VM connection; diagnose recurring failures rather than repeatedly restarting it.

## 3. GitHub SSH key and fish login shell

Create a distinct key on this host. Keep an existing key if one is already configured; do not overwrite it.

```bash
ssh-keygen -t ed25519 -C "viktor@linux-vm"
cat ~/.ssh/id_ed25519.pub
```

Add the public key under [GitHub SSH keys](https://github.com/settings/keys). Verify:

```bash
ssh -T git@github.com
```

The expected greeting says authentication succeeded but GitHub does not provide shell access; exit status 1 is normal for that greeting. This key authenticates Linux to GitHub, independently of Tailscale SSH from the Mac.

```bash
chsh -s /usr/bin/fish
```

Reconnect over SSH to enter fish.

## 4. Rust, jj, and Trunk

Install Rust using [rustup](https://rustup.rs):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
sh /tmp/rustup-init.sh -y --profile minimal
```

In fish:

```fish
source ~/.cargo/env.fish
rustup component add rustfmt clippy rust-analyzer
rustup target add wasm32-unknown-unknown
cargo install --locked --bin jj jj-cli
cargo install --locked trunk
# If Just was unavailable through apt:
# cargo install --locked just
```

`cargo install --locked trunk` is sufficient; a special binary installer is not required. On a small machine use `env CARGO_BUILD_JOBS=2 cargo install --locked trunk`. Official architecture-specific [jj releases](https://github.com/jj-vcs/jj/releases) and [Trunk releases](https://github.com/trunk-rs/trunk/releases) are faster alternatives; verify their published checksums. The VM's configured apt repositories had no `jj`, `jj-cli`, or `jujutsu` package.

If compilation reports `Disk quota exceeded` while the main disk has free space, check `df -h /tmp`: Cargo installation can fill a small temporary filesystem. Put build temporary files on the main disk and retry:

```fish
mkdir -p ~/.cache/aide-build-tmp
env TMPDIR="$HOME/.cache/aide-build-tmp" CARGO_BUILD_JOBS=2 cargo install --locked trunk
```

The same `TMPDIR` override can be used with `just install-aide`.

## 5. PATH and personal configuration

Keep user-installed tools available in both interactive shells and SSH commands. Create `~/.config/fish/conf.d/linux-host.fish` with:

```fish
fish_add_path --prepend --path "$HOME/.local/bin" "$HOME/.cargo/bin"
set -gx EDITOR nvim
set -gx VISUAL nvim
```

Reload with `exec fish -l`, or reconnect. Check `type -a gh` and `command -v codex jj cargo` when the wrong binary or an old version is selected. Existing shells do not automatically reload changed configuration.

Clone the personal configuration separately, preserving existing machine state:

```fish
jj git clone --colocate git@github.com:charypar/config.git ~/dotfiles
```

Back up existing destination files before linking. On the test VM the source lives in `~/dotfiles`, with these links under `~/.config`:

| Destination | Source under `~/dotfiles` |
| --- | --- |
| `fish/config.fish`, `fish/functions`, `fish/conf.d`, `fish/completions`, `fish/fish_plugins` | Corresponding `fish/` entries |
| `jj`, `nvim`, `starship.toml`, `tmux-config` | Corresponding entries |
| `gh/config.yml` | `gh/config.yml` |
| `~/.tmux.conf` | `tmux-config/tmux.conf` |

Keep the VM's own `fish/fish_variables` and authenticate gh separately; do not link the checkout's `gh/hosts.yml`. If replacing `fish/conf.d` with a link, move your `linux-host.fish` into the linked directory too. No Codex credentials or Mac-specific state need copying.

Linux adjustments applied during this setup:

- Guard `/opt/homebrew/bin/rv` invocations with `if test -x /opt/homebrew/bin/rv`; keep Rust's `~/.cargo/bin` on PATH.
- Keep Starship, eza (the `ls` wrapper), and zoxide installed for the supplied fish configuration.
- Have `_fish_tmux_plugin_run` immediately call `command tmux $argv` and return its status when `not status is-interactive`. This avoids terminal-color errors in SSH commands.
- Set gh's `git_protocol` to `ssh`.
- Preserve jj's identity, aliases, and revset preferences. Set Git identity too:

```fish
git config --global user.name "Viktor Charypar"
git config --global user.email "charypar@gmail.com"
git config --global core.editor nvim
```

These Linux adjustments and the Neovim fixes below were local edits during setup, not yet published to the config repository. A fresh clone may still need them.

### tmux and Neovim

Install TPM in `~/.tmux/plugins/tpm` using the [TPM instructions](https://github.com/tmux-plugins/tpm). Start tmux, then press prefix + `I` (capital i) to install the configured sensible defaults, Dracula theme, and vim-tmux-navigator plugins. The default prefix is Ctrl-b. Verify mouse support, pane/window indices starting at 1, and fish as the default shell. To keep the machine name distinct at the far right, both machines now use this setting before TPM loads:

```tmux
set -g @dracula-time-format "%a %d/%m %I:%M %p #[bg=#bd93f9,fg=#282a36,bold] #h "
```

Reload with `command tmux source-file ~/.tmux.conf`.

Open Neovim and run `:Lazy restore` to install its locked plugins. Configuration fixes found during setup:

- Replace legacy `require('lspconfig').SERVER.setup(...)` with Neovim 0.11's `vim.lsp.config(...)` and `vim.lsp.enable(...)`.
- Install `rust-analyzer` in the active Rust toolchain; a rustup shim existing on PATH does not mean its component is installed.
- Correct `highligt` to `highlight` in the Tree-sitter setup.
- Configure autotag using `require('nvim-ts-autotag').setup({})`.
- Keep the legacy Tree-sitter configuration on its `master` branch. Its locked installer requires the 0.25 CLI for Swift generation:

```fish
npm install --global --prefix ~/.local tree-sitter-cli@0.25.10
```

Load a file to activate the lazy Tree-sitter plugin before running `:TSInstallSync swift`. Revisit that CLI pin when updating the Neovim plugins. Neovim is a personal preference, not an Aide runtime dependency.

## 6. GitHub CLI and Codex authentication

Install gh using the [official GitHub CLI apt repository instructions](https://github.com/cli/cli/blob/trunk/docs/install_linux.md#debian), rather than relying on an old Ubuntu package. An official release binary in `~/.local/bin` is also supported, but needs manual updates.

The VM's distro package was 2.46.0, which failed browser authentication with `slow_down`. GitHub identifies this as fixed in [2.87.0 and later](https://github.com/cli/cli/issues/13124). We installed 2.105.0; check the selected executable before login:

```fish
type -a gh
gh --version
env GH_BROWSER=echo gh auth login --hostname github.com --git-protocol ssh --web
```

Open the printed URL on the **Mac**, enter the fresh device code, and leave the Linux command running. The `echo` browser override prevents attempts to open a graphical browser on the server.

```fish
npm install --global --prefix ~/.local @openai/codex
codex login --device-auth
```

Again, complete the browser step on the Mac. Confirm locally without displaying credentials:

```fish
gh auth status
codex login status
```

## 7. Aide checkout, build, and project

Layout on Linux:

```text
~/AideDeCamp/                    project root
  aide-de-camp/                  main repository checkout
  aide-de-camp-tasks/            task workspaces, created by Aide
  .aide/                        project bookkeeping, created by Aide
```

```fish
jj git clone --colocate git@github.com:charypar/aide-de-camp.git ~/AideDeCamp/aide-de-camp
cd ~/AideDeCamp/aide-de-camp
# For the current remote-support acceptance test:
jj new wip/remotes-exploration@origin
just setup
env CARGO_BUILD_JOBS=2 just install-aide
aide doctor
aide init ~/AideDeCamp --repo ~/AideDeCamp/aide-de-camp
```

For a future stable installation, select the intended released revision instead of the WIP bookmark. The dashboard assets are embedded during installation. Build the checkout on Linux; a Mac executable will not run there.

## 8. Private hosting and startup

Configure the route once as an administrator, using Aide’s default loopback port:

```bash
sudo tailscale serve --bg --https=443 --set-path=/aide http://127.0.0.1:49173
```

This route persists across restarts. Aide reads the configuration and checks that it matches its listener; it never changes Tailscale settings. No operator privilege is required. If changing Aide’s `--port`, update the Serve route to match. Port 49173 is an application default, not a registered service port; it can be overridden if occupied.

Enable HTTPS certificates/Serve in the tailnet administration UI if required. Run in the foreground first:

```fish
aide --tailnet --no-open
```

Use `tailscale serve status` to find `https://<machine>.<tailnet>.ts.net/aide/`. Keep the trailing slash. Serve is private; do not enable Funnel. Aide trusts devices permitted to reach its endpoint by the tailnet policy. See [remote access](REMOTES.md) for peer discovery, SSH destination mapping, and timeout semantics.

After foreground access works, install the [systemd user unit](../examples/aide.service):

```fish
mkdir -p ~/.config/systemd/user
cp ~/AideDeCamp/aide-de-camp/examples/aide.service ~/.config/systemd/user/aide.service
systemctl --user daemon-reload
systemctl --user enable --now aide
journalctl --user -u aide -f
```

Stop the foreground Aide process before starting the unit. The unit must include both `~/.cargo/bin` and `~/.local/bin` in PATH: systemd does not load fish configuration. Its `KillMode=process` deliberately leaves task tmux/Codex processes alive when stopping the dashboard. For startup at boot without an interactive login, an administrator runs `sudo loginctl enable-linger viktor` (substitute the actual account).

## 9. Acceptance checks

From the Mac, verify the full private hostname works over SSH as well as the short name. Configure its Linux `User` in `~/.ssh/config` if necessary; Aide defaults to the discovered full Tailscale DNS name.

With both hosts running Aide through Serve:

1. Discover the VM automatically using `aide peer list` and the dashboard machine selector.
2. Create a disposable remote task and verify it belongs to the Linux project.
3. Open a terminal on the Mac, confirm SSH attaches to the Linux tmux session, detach, and reconnect to the same task.
4. Park/unpark and reload the dashboard; verify the saved state.
5. Stop/restart the VM's Aide service; confirm disconnected hosts show no stale tasks, then return with fresh data.
6. Clean up only the disposable task and verify its retained history.
7. Verify service and task recovery after a planned reboot before relying on unattended operation.

Keep a record of which checks actually passed. The Mac/Linux VM journey above passed on 2026-11-19, including an actual reboot and unattended service startup. See the remote plan for the exact verification record and remaining cloud-host checks.

## Routine service setup and upgrades

After the one-time Tailscale route and source checkout are ready, run `just install-aide` followed by `just aide-service-install`. The latter installs/enables the systemd user service while preserving an existing unit. Lingering still requires the administrator step above. Run `just aide-service-status` to inspect it.

For later updates, update the checkout with jj, then run `just upgrade-aide`. This builds and installs that checkout and restarts the service only after a successful installation. Use `just aide-service-restart` when only a restart is needed. These recipes also support the macOS LaunchAgent.
