//! xgent_daemon — XGent 全局守护进程（瘦后台，纯 tokio，不依赖 Bevy）。
//!
//! 职责（MVP 范围）：IPC 服务端、provider 连接池、全局配置协调、
//! 文件监听、多客户端文件状态同步、生命周期管理。

mod config_store;
mod fs_watcher;
mod lifecycle;
mod provider_pool;
mod registry;
mod server;
mod session;

use std::path::PathBuf;
use xgent_settings_core::paths::daemon_socket_path;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let socket_path: PathBuf = daemon_socket_path();
    tracing::info!("xgent_daemon 启动，socket: {}", socket_path.display());

    let daemon = server::Daemon::new(socket_path)?;
    daemon.run().await
}
