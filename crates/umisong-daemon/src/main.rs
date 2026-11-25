use anyhow::Result;
use tracing_subscriber::EnvFilter;
use umisong_daemon::{DaemonConfig, run_daemon};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = DaemonConfig::default();
    tracing::info!(
        socket = %config.socket_path.display(),
        db = %config.db_path.display(),
        "Starting umisongd daemon"
    );

    run_daemon(config).await
}
