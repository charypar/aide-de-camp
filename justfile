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
build: build-ui
    cargo build --locked -p review-server

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

# Run automated Rust tests.
test:
    cargo test --locked -p review-core -p review-server

# Format all Rust code.
fmt:
    cargo fmt --all

# Check formatting, server code, and browser code.
check:
    cargo fmt --all -- --check
    cargo clippy --locked -p review-core -p review-server -p xtask -- -D warnings
    cargo clippy --locked -p review-ui --target wasm32-unknown-unknown -- -D warnings

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
