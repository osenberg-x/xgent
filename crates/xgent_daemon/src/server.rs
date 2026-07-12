//! daemon 共享状态与 IPC 服务端。
//!
//! 持有客户端注册表、provider 池、配置协调器、文件监听器、生命周期。
//! 监听本地 socket，每个连接 spawn 一个 [`Session`] task。
//!
//! 跨平台：macOS/Linux 用 Unix domain socket；Windows 用 named pipe
//! （MVP 阶段 Windows 支持后续补，当前 Unix 优先）。

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use xgent_core::proto::Notification;

use crate::config_store::ConfigCoordinator;
use crate::fs_watcher::{FsEvent, FsWatcher};
use crate::lifecycle::Lifecycle;
use crate::provider_pool::ProviderPool;
use crate::registry::ClientRegistry;
use crate::session::Session;

/// daemon 共享状态（所有 Session task 共享）。
#[derive(Clone)]
pub struct Shared {
    pub registry: Arc<RwLock<ClientRegistry>>,
    pub pool: Arc<ProviderPool>,
    pub config: Arc<RwLock<ConfigCoordinator>>,
    pub watcher: Arc<FsWatcher>,
    pub lifecycle: Arc<Lifecycle>,
    /// 文件监听事件接收端（由 server task 消费并广播）
    pub fs_events: Arc<tokio::sync::Mutex<mpsc::Receiver<FsEvent>>>,
}

/// IPC 服务端。
pub struct Daemon {
    pub shared: Shared,
    pub socket_path: PathBuf,
    shutdown_rx: tokio::sync::mpsc::Receiver<()>,
}

impl Daemon {
    /// 构造 daemon，加载配置、初始化各子系统。
    pub fn new(socket_path: PathBuf) -> anyhow::Result<Self> {
        let config =
            ConfigCoordinator::load().map_err(|e| anyhow::anyhow!("加载全局配置失败: {e}"))?;
        let config = Arc::new(RwLock::new(config));
        let pool = Arc::new(ProviderPool::new(config.clone()));
        let registry = Arc::new(RwLock::new(ClientRegistry::new()));
        let (fs_tx, fs_rx) = mpsc::channel::<FsEvent>(64);
        let watcher = Arc::new(FsWatcher::new(fs_tx)?);
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);
        let lifecycle = Arc::new(Lifecycle::new(shutdown_tx));

        let shared = Shared {
            registry,
            pool,
            config,
            watcher,
            lifecycle,
            fs_events: Arc::new(tokio::sync::Mutex::new(fs_rx)),
        };

        Ok(Self {
            shared,
            socket_path,
            shutdown_rx,
        })
    }

    /// 运行 daemon：监听 socket，accept 连接，spawn session task。
    pub async fn run(self) -> anyhow::Result<()> {
        let shared = self.shared.clone();

        // 文件监听事件消费 task：收到 FileChanged 后广播给订阅该项目的客户端
        let shared_for_fs = shared.clone();
        tokio::spawn(async move {
            loop {
                let ev = {
                    let mut rx = shared_for_fs.fs_events.lock().await;
                    rx.recv().await
                };
                let Some(ev) = ev else { break };
                let notif = Notification::new(
                    xgent_core::notifications::FS_CHANGED,
                    serde_json::to_value(&ev.changed).unwrap_or_default(),
                );
                let reg = shared_for_fs.registry.read().await;
                reg.broadcast_to_project(&ev.project_root, notif, None);
            }
        });

        // 绑定 socket
        bind_and_serve(&self.socket_path, shared.clone()).await?;

        // 等待关闭信号（所有客户端断开后 lifecycle 延迟触发）
        let mut shutdown_rx = self.shutdown_rx;
        let _ = shutdown_rx.recv().await;
        tracing::info!("daemon 收到关闭信号，退出中");
        // 清理 socket 文件
        let _ = std::fs::remove_file(&self.socket_path);
        Ok(())
    }
}

/// 绑定 socket 并 accept 连接，spawn session task。
#[cfg(unix)]
async fn bind_and_serve(socket_path: &std::path::Path, shared: Shared) -> anyhow::Result<()> {
    use tokio::net::UnixListener;
    // 清理可能残留的旧 socket 文件
    let _ = std::fs::remove_file(socket_path);
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(socket_path)?;

    let shared_clone = shared.clone();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let shared = shared_clone.clone();
                    tokio::spawn(async move {
                        let session = Session::new(stream, shared);
                        session.handle().await;
                    });
                }
                Err(e) => {
                    tracing::warn!("accept 失败: {e}");
                }
            }
        }
    });
    Ok(())
}

#[cfg(not(unix))]
async fn bind_and_serve(_socket_path: &std::path::Path, _shared: Shared) -> anyhow::Result<()> {
    anyhow::bail!("Windows named pipe 支持待实现（MVP 阶段）")
}
