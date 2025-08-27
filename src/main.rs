use anyhow::Result;
use tracing::{error, info, warn};

mod auth;
mod config;
mod diff;
mod docs;
mod drive;
mod errors;
mod rpc;

use config::Config;
use rpc::JsonRpcServer;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Starting Cedar daemon");

    let config = Config::load().await?;
    info!("Configuration loaded");

    let mut server = JsonRpcServer::new(config).await?;
    info!("JSON-RPC server initialized on {}", server.address());

    let shutdown_signal = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C signal handler");
        warn!("Received shutdown signal");
    };

    tokio::select! {
        result = server.run() => {
            match result {
                Ok(_) => info!("Server shut down gracefully"),
                Err(e) => error!("Server error: {}", e),
            }
        }
        _ = shutdown_signal => {
            info!("Shutting down Cedar daemon");
        }
    }

    Ok(())
}
