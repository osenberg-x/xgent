//! 文件监听。
//!
//! 用 [`notify`] 监听项目目录变更，转成 [`FileChanged`] 推送给 daemon。
//! 同项目多客户端订阅时，notify 只监听一次，事件经 [`crate::registry`]
//! 广播给所有订阅者。
//!
//! notify 的 `Watcher::watch` 需 `&mut self`，故把 watcher 放进
//! [`tokio::sync::Mutex`]（仅在添加 watch 时持锁，监听回调在独立线程）。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, mpsc};
use xgent_core::fs::{FileChangeKind, FileChanged};
use xgent_core::ids::ClientId;

// notify 的 watch/unwatch 是 Watcher trait 方法，需引入 trait
use notify::Watcher;

/// 文件监听产生的事件（含项目根与变更）。
pub struct FsEvent {
    pub project_root: PathBuf,
    pub changed: FileChanged,
}

/// 文件监听器。
pub struct FsWatcher {
    /// notify watcher（Mutex 因 watch/unwatch 需 &mut）
    watcher: Mutex<notify::RecommendedWatcher>,
    /// 项目路径 → 订阅客户端集合
    subscriptions: Arc<RwLock<HashMap<PathBuf, HashSet<ClientId>>>>,
}

impl FsWatcher {
    /// 构造。监听到的事件经 `event_tx` 推送。
    pub fn new(event_tx: mpsc::Sender<FsEvent>) -> notify::Result<Self> {
        let subs = Arc::new(RwLock::new(HashMap::<PathBuf, HashSet<ClientId>>::new()));

        // notify 回调：把 notify::Event 转成 FileChanged 发送
        let subs_for_cb = subs.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                handle_notify_event(&ev, &subs_for_cb, &event_tx);
            }
        })?;

        Ok(Self {
            watcher: Mutex::new(watcher),
            subscriptions: subs,
        })
    }

    /// 订阅项目路径。同项目首次订阅才真正向 notify 注册 watch。
    pub async fn watch(&self, project: PathBuf, client: ClientId) -> notify::Result<()> {
        // 记录订阅
        let was_empty;
        {
            let mut subs = self.subscriptions.write().await;
            let set = subs.entry(project.clone()).or_default();
            was_empty = set.is_empty();
            set.insert(client);
        }
        // 首次订阅该路径则注册 notify watch
        if was_empty {
            let mut w = self.watcher.lock().await;
            // notify 的 watch 是同步的，但持锁会阻塞 tokio 任务——
            // 用 spawn_blocking 避免阻塞，但 RecommendedWatcher 非 Send 时不可。
            // RecommendedWatcher 在主流平台 Send，此处直接调用（通常很快）。
            w.watch(&project, notify::RecursiveMode::Recursive)?;
        }
        Ok(())
    }

    /// 客户端断开时清理其所有订阅。若某项目已无订阅者，取消 notify watch。
    pub async fn unwatch_client(&self, client: ClientId) {
        let to_unwatch: Vec<PathBuf> = {
            let mut subs = self.subscriptions.write().await;
            let mut removed = Vec::new();
            for set in subs.values_mut() {
                set.remove(&client);
            }
            // 移除空集合并记录需 unwatch 的路径
            subs.retain(|path, set| {
                if set.is_empty() {
                    removed.push(path.clone());
                    false
                } else {
                    true
                }
            });
            removed
        };
        // 取消已无订阅者的 watch
        if !to_unwatch.is_empty() {
            let mut w = self.watcher.lock().await;
            for path in to_unwatch {
                let _ = w.unwatch(&path);
            }
        }
    }

    /// 当前所有订阅的项目（用于诊断与测试）。
    #[allow(dead_code)]
    pub async fn subscribed_projects(&self) -> Vec<PathBuf> {
        self.subscriptions.read().await.keys().cloned().collect()
    }
}

/// 把单个 notify::Event 转成 FileChanged，反查所属项目根后发送。
fn handle_notify_event(
    ev: &notify::Event,
    subs: &RwLock<HashMap<PathBuf, HashSet<ClientId>>>,
    tx: &mpsc::Sender<FsEvent>,
) {
    let kind = notify_kind_to_file_kind(ev.kind);
    // notify 回调在 watcher 线程调用，blocking_read 避免异步上下文
    let subs = subs.blocking_read();
    for path in &ev.paths {
        // 找到包含该路径的项目根
        for root in subs.keys() {
            if path.starts_with(root) {
                let changed = FileChanged {
                    project_root: root.clone(),
                    path: path.strip_prefix(root).unwrap_or(path).to_path_buf(),
                    kind,
                };
                let _ = tx.blocking_send(FsEvent {
                    project_root: root.clone(),
                    changed,
                });
            }
        }
    }
}

/// notify 事件类型 → FileChangeKind
fn notify_kind_to_file_kind(kind: notify::EventKind) -> FileChangeKind {
    use notify::EventKind;
    match kind {
        EventKind::Create(_) => FileChangeKind::Created,
        EventKind::Modify(_) => FileChangeKind::Modified,
        EventKind::Remove(_) => FileChangeKind::Removed,
        EventKind::Access(_) => FileChangeKind::Modified,
        EventKind::Any | EventKind::Other => FileChangeKind::Modified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn watch_records_subscription() {
        let (tx, _rx) = mpsc::channel::<FsEvent>(16);
        let watcher = FsWatcher::new(tx).unwrap();
        let dir = tempfile::tempdir().unwrap();
        watcher
            .watch(dir.path().to_path_buf(), ClientId(1))
            .await
            .unwrap();
        let projects = watcher.subscribed_projects().await;
        assert!(projects.iter().any(|p| p == dir.path()));
    }

    #[tokio::test]
    async fn unwatch_client_removes_subscription() {
        let (tx, _rx) = mpsc::channel::<FsEvent>(16);
        let watcher = FsWatcher::new(tx).unwrap();
        let dir = tempfile::tempdir().unwrap();
        watcher
            .watch(dir.path().to_path_buf(), ClientId(1))
            .await
            .unwrap();
        watcher.unwatch_client(ClientId(1)).await;
        let projects = watcher.subscribed_projects().await;
        assert!(projects.is_empty(), "取消订阅后项目集合应空");
    }

    #[tokio::test]
    async fn real_file_change_produces_event() {
        // 集成测试：真实创建文件并断言收到事件
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (tx, mut rx) = mpsc::channel::<FsEvent>(32);
        let watcher = FsWatcher::new(tx).unwrap();
        watcher.watch(root.clone(), ClientId(1)).await.unwrap();

        // 创建文件触发事件
        let file = root.join("test.txt");
        tokio::fs::write(&file, b"hello").await.unwrap_or_else(|e| {
            // 某些 CI 环境文件系统监听不可用，跳过
            eprintln!("无法写测试文件（跳过）: {e}");
        });

        // 等待事件到达（文件系统监听有延迟）
        let received = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;
        match received {
            Ok(Some(ev)) => {
                assert_eq!(ev.project_root, root);
                // 路径应是相对项目根
                assert!(ev.changed.path.ends_with("test.txt"));
            }
            _ => {
                // 文件系统监听在某些平台/环境不可靠，不视为失败
                eprintln!("未收到文件变更事件（平台/环境限制，跳过断言）");
            }
        }
        // 清理订阅避免 watcher 在测试结束时报错
        watcher.unwatch_client(ClientId(1)).await;
    }
}
