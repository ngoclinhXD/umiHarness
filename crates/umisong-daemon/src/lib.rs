pub mod git_ops;
pub mod handoff;
pub mod pty_auth;
pub mod server;
pub mod store;

use anyhow::Result;
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(".umisong-harness")
}

pub fn default_socket_path() -> PathBuf {
    data_dir().join("umisongd.sock")
}

pub fn default_db_path() -> PathBuf {
    data_dir().join("umisong.db")
}

pub fn default_worktrees_dir() -> PathBuf {
    data_dir().join("worktrees")
}

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub socket_path: PathBuf,
    pub db_path: PathBuf,
    pub worktrees_dir: PathBuf,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path(),
            db_path: default_db_path(),
            worktrees_dir: default_worktrees_dir(),
        }
    }
}

pub async fn run_daemon(config: DaemonConfig) -> Result<()> {
    server::serve(config).await
}
