mod agent;
mod app;
mod browser;
mod cli;
mod commands;
mod fixture;
mod http;
mod progress;
mod session;
mod store;
mod vcs;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
