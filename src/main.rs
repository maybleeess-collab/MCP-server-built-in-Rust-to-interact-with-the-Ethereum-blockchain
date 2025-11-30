use dotenv::dotenv;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod config;
pub mod ethereum;
pub mod server;
pub mod tools;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    info!("Starting Ethereum Trading MCP Server...");

    let config = config::Config::from_env()?;
    let eth_client = ethereum::EthereumClient::new(&config.rpc_url, &config.private_key).await?;

    server::run(eth_client).await?;

    Ok(())
}
