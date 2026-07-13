//! daemon 探测、拉起与连接。
//!
//! 启动时探测本地 daemon socket，未运行则 fork 拉起 `xgent_daemon` 进程，
//! 等待 socket 就绪后重试连接。

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use xgent_settings_core::paths::daemon_socket_path;

use crate::ipc_client::IpcClient;

/// 探测 daemon，未运行则拉起，返回已连接的 IPC 客户端。
pub async fn connect_or_spawn_daemon() -> Result<IpcClient> {
    let path = daemon_socket_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    // 先尝试直连
    if let Ok(c) = IpcClient::connect(&path).await {
        tracing::info!("已连接到运行中的 daemon: {}", path.display());
        return Ok(c);
    }
    // 未运行 → 拉起 daemon 进程
    spawn_daemon_process(&path)?;
    wait_for_socket(&path).await?;
    let client = IpcClient::connect(&path)
        .await
        .with_context(|| format!("daemon 拉起后仍无法连接: {}", path.display()))?;
    tracing::info!("已拉起 daemon 并连接: {}", path.display());
    Ok(client)
}

/// 启动 xgent_daemon 子进程。
fn spawn_daemon_process(_socket_path: &Path) -> Result<()> {
    // 优先用环境变量指定的 daemon 路径，其次尝试同目录的 xgent_daemon 二进制，
    // 最后回退到 PATH 中的 xgent_daemon。
    let exe = std::env::var("XGENT_DAEMON_BIN").ok();
    let exe = exe
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("xgent_daemon")))
                .map(|p| p.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "xgent_daemon".to_string());

    tracing::info!("拉起 daemon: {exe}");
    std::process::Command::new(&exe)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("无法启动 daemon 进程: {exe}"))?;
    Ok(())
}

/// 轮询等待 socket 文件出现且可连接。
async fn wait_for_socket(path: &Path) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if path.exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    anyhow::bail!("等待 daemon socket 超时: {}", path.display())
}
