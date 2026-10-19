mod agent;
mod app;
mod browser;
mod cli;
mod commands;
mod fixture;
mod http;
mod lock;
mod progress;
mod session;
mod state_dir;
mod store;
mod vcs;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
