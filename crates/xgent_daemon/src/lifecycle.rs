//! daemon 生命周期管理。
//!
//! 维护客户端连接计数，所有客户端断开后启动延迟退出计时，
//! 避免快速重启抖动。新连接到达时取消计时。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

/// 延迟退出的时间窗口。最后一个客户端断开后等待此时长再退出，
/// 以便用户快速重开项目时复用同一 daemon。
pub const SHUTDOWN_DELAY: Duration = Duration::from_secs(30);

/// daemon 生命周期状态。
pub struct Lifecycle {
    /// 当前连接的客户端数
    client_count: AtomicU64,
    /// 延迟退出计时句柄（None 表示无计时）
    shutdown_handle: Mutex<Option<JoinHandle<()>>>,
    /// 关闭信号，用于通知 main 循环退出
    shutdown_tx: tokio::sync::mpsc::Sender<()>,
}

impl Lifecycle {
    /// 构造。`shutdown_tx` 在该退出时被触发一次。
    pub fn new(shutdown_tx: tokio::sync::mpsc::Sender<()>) -> Self {
        Self {
            client_count: AtomicU64::new(0),
            shutdown_handle: Mutex::new(None),
            shutdown_tx,
        }
    }

    /// 客户端连接。计数 +1，取消任何挂起的退出计时。
    pub fn on_connect(&self) -> u64 {
        let n = self.client_count.fetch_add(1, Ordering::SeqCst) + 1;
        // 取消退出计时
        if let Ok(mut guard) = self.shutdown_handle.try_lock()
            && let Some(handle) = guard.take()
        {
            handle.abort();
        }
        n
    }

    /// 客户端断开。计数 -1，归零则启动延迟退出计时。
    pub async fn on_disconnect(&self) -> u64 {
        let n = self.client_count.fetch_sub(1, Ordering::SeqCst);
        let n = n.saturating_sub(1);
        if n == 0 {
            // 启动延迟退出
            let tx = self.shutdown_tx.clone();
            let mut guard = self.shutdown_handle.lock().await;
            // 取消已有计时
            if let Some(h) = guard.take() {
                h.abort();
            }
            let handle = tokio::spawn(async move {
                tokio::time::sleep(SHUTDOWN_DELAY).await;
                let _ = tx.send(()).await;
            });
            *guard = Some(handle);
        }
        n
    }

    /// 当前客户端数（主要用于测试与诊断）。
    #[allow(dead_code)]
    pub fn count(&self) -> u64 {
        self.client_count.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connect_increments_disconnect_decrements() {
        let (tx, _rx) = tokio::sync::mpsc::channel::<()>(1);
        let lc = Lifecycle::new(tx);
        assert_eq!(lc.count(), 0);
        assert_eq!(lc.on_connect(), 1);
        assert_eq!(lc.on_connect(), 2);
        assert_eq!(lc.count(), 2);
        assert_eq!(lc.on_disconnect().await, 1);
        assert_eq!(lc.count(), 1);
    }

    #[tokio::test]
    async fn last_disconnect_triggers_shutdown_after_delay() {
        // 用极短延迟验证逻辑：直接构造延迟为 0 的情况不便，
        // 故验证“末个客户端断开后确实触发关闭信号”。
        // 用 tokio::time::pause + advance 推进虚拟时间。
        tokio::time::pause();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);
        let lc = Lifecycle::new(tx);
        lc.on_connect();
        lc.on_disconnect().await; // 启动退出计时
        // 推进虚拟时间超过延迟
        tokio::time::advance(SHUTDOWN_DELAY * 2).await;
        let received = rx.recv().await;
        assert!(received.is_some(), "应在延迟后收到关闭信号");
    }

    #[tokio::test]
    async fn reconnect_cancels_shutdown() {
        tokio::time::pause();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);
        let lc = Lifecycle::new(tx);
        lc.on_connect();
        lc.on_disconnect().await; // 启动退出计时
        // 模拟重连，取消计时
        lc.on_connect();
        // 推进时间远超延迟，确认不会收到关闭信号
        tokio::time::advance(SHUTDOWN_DELAY * 3).await;
        let received = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        assert!(received.is_err(), "重连应取消退出计时");
    }
}
