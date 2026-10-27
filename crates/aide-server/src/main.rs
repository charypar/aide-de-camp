mod activity;
mod cli;
mod database;
mod http;
mod recording;
mod rpc;
mod runtime;
mod terminal;
mod worker;
mod workspace;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
