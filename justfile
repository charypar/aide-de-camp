set positional-arguments
set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Show the available commands.
default:
    @just --list

# Install/check Rust development dependencies (Codex must already be configured).
setup:
    @command -v rustup >/dev/null || { echo 'Install Rust from https://rustup.rs, then rerun just setup'; exit 1; }
    rustup target add wasm32-unknown-unknown
    @command -v trunk >/dev/null || cargo install --locked trunk
    @command -v codex >/dev/null || { echo 'Install and sign into Codex before running reviews'; exit 1; }
    cargo fetch --locked

# Build the Leptos browser application.
build-ui:
    cd crates/review-ui && env -u NO_COLOR trunk build --locked

# Build the packaged CLI with embedded browser assets.
build: build-ui build-aide-ui
    cargo build --locked -p review-server -p aide-server

# Build and run against a repository.
run path=".": build
    cargo run --locked -p review-server -- --path "$1"

# Start server and UI watching; review a repository of your choice.
dev path=".": build-ui
    cargo run --locked -p xtask -- dev "$1"

# Install the CLI with its embedded UI into Cargo's bin directory.
install:
    cd crates/review-ui && env -u NO_COLOR trunk build --release --locked
    cargo install --locked --path crates/review-server --force
    cd crates/aide-ui && env -u NO_COLOR trunk build --release --locked
    cargo install --locked --path crates/aide-server --force

# Run automated Rust tests.
test:
    cargo test --locked -p review-core -p review-server -p aide-server

# Format all Rust code.
fmt:
    cargo fmt --all

# Check formatting, server code, and browser code.
check:
    cargo fmt --all -- --check
    cargo clippy --locked -p review-core -p review-server -p aide-server -p xtask -- -D warnings
    cargo clippy --locked -p review-ui -p aide-ui --target wasm32-unknown-unknown -- -D warnings

# Install browser test tooling (development only).
setup-browser:
    npm ci
    npx playwright install chromium

# Run repeatable headless browser acceptance tests.
test-browser: build
    npx playwright test

# Exercise the real Codex session and configured MCPs; uses your normal account.
test-live:
    cargo test --locked -p review-server live_codex -- --ignored --nocapture

# Check the installed harness and VCS commands.
doctor:
    cargo run --locked -p review-server -- doctor

# Build the Aide dashboard.
build-aide-ui:
    cd crates/aide-ui && env -u NO_COLOR trunk build --locked

# Start the task dashboard; existing task runtimes survive its exit.
aide: build
    target/debug/aide

# Develop the dashboard UI with Trunk watching (run aide-dev-server separately).
aide-dev-ui:
    cd crates/aide-ui && env -u NO_COLOR trunk watch --locked

# Serve development dashboard assets; rerun after Rust changes.
aide-dev-server: build-aide-ui
    cargo run --locked -p aide-server -- --assets dist-aide

# Validate native Codex observation without sending a model prompt.
test-aide-protocol:
    cargo test --locked -p aide-server live_codex_observer -- --ignored --nocapture

# Check terminal activity with native Codex and detached tmux panes (no model prompts).
test-aide-terminal-activity: build
    python3 tests/aide-live.py --terminal-activity

# Exercise native terminal conversation, fish environment, and restart recovery (two model turns).
test-aide-live: build
    python3 tests/aide-live.py

# Check status becomes idle after detaching a real terminal mid-turn (one model turn).
test-aide-detached-status: build
    python3 tests/aide-live.py --detached-status
