mod activity;
mod attachment;
mod cli;
mod configuration;
mod conversations;
mod database;
mod gateway;
mod github;
mod http;
mod instructions;
mod native_picker;
mod operations;
mod peer;
mod peers;
mod recording;
mod remote_cli;
mod rpc;
mod runtime;
mod tailnet;
mod terminal;
mod worker;
mod workspace;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
