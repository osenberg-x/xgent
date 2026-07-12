//! 客户端注册表。
//!
//! 维护 UI 客户端连接：分配 [`ClientId`]、记录每个客户端的通知发送端与
//! 已订阅的项目路径。提供按项目广播与全局广播能力。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tokio::sync::mpsc;
use xgent_core::ids::ClientId;
use xgent_core::proto::Notification;

/// 单个客户端的注册条目。
pub struct ClientEntry {
    /// 推送通知给该客户端的发送端
    pub sender: mpsc::Sender<Notification>,
    /// 已订阅的项目根目录集合
    pub subscribed_projects: HashSet<PathBuf>,
}

/// 客户端注册表。
pub struct ClientRegistry {
    /// 客户端 id → 条目
    clients: HashMap<ClientId, ClientEntry>,
    /// 下一个 ClientId（从 1 起，0 保留给 daemon 自身）
    next_id: u64,
}

impl ClientRegistry {
    /// 构造空注册表。
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
            next_id: 1,
        }
    }

    /// 注册新客户端，返回分配的 [`ClientId`]。
    pub fn register(&mut self, sender: mpsc::Sender<Notification>) -> ClientId {
        let id = ClientId(self.next_id);
        self.next_id += 1;
        self.clients.insert(
            id,
            ClientEntry {
                sender,
                subscribed_projects: HashSet::new(),
            },
        );
        id
    }

    /// 注销客户端。
    pub fn unregister(&mut self, id: ClientId) {
        self.clients.remove(&id);
    }

    /// 订阅指定项目路径。
    pub fn subscribe(&mut self, id: ClientId, project: PathBuf) {
        if let Some(entry) = self.clients.get_mut(&id) {
            entry.subscribed_projects.insert(project);
        }
    }

    /// 获取某客户端已订阅的项目集合（克隆，避免持锁）。
    #[allow(dead_code)]
    pub fn subscribed(&self, id: ClientId) -> HashSet<PathBuf> {
        self.clients
            .get(&id)
            .map(|e| e.subscribed_projects.clone())
            .unwrap_or_default()
    }

    /// 获取某客户端的通知发送端克隆（用于 provider.chat 推送）。
    pub fn sender_for(&self, id: ClientId) -> Option<mpsc::Sender<Notification>> {
        self.clients.get(&id).map(|e| e.sender.clone())
    }

    /// 当前已注册客户端数。
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.clients.len()
    }

    /// 是否为空。
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    /// 广播给订阅某项目的所有客户端（排除来源 `exclude`）。
    ///
    /// 发送失败（客户端已断开）的客户端会被静默跳过；调用方可周期性清理。
    pub fn broadcast_to_project(
        &self,
        project: &std::path::Path,
        notif: Notification,
        exclude: Option<ClientId>,
    ) {
        for (id, entry) in &self.clients {
            if Some(*id) == exclude {
                continue;
            }
            if entry.subscribed_projects.contains(project) {
                // try_send：非阻塞，满了就跳过，避免阻塞广播循环
                let _ = entry.sender.try_send(notif.clone());
            }
        }
    }

    /// 广播给所有客户端（排除 `exclude`）。
    pub fn broadcast_all(&self, notif: Notification, exclude: Option<ClientId>) {
        for (id, entry) in &self.clients {
            if Some(*id) == exclude {
                continue;
            }
            let _ = entry.sender.try_send(notif.clone());
        }
    }
}

impl Default for ClientRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xgent_core::notifications::CONFIG_CHANGED;
    use xgent_core::proto::Notification;

    fn channel() -> (mpsc::Sender<Notification>, mpsc::Receiver<Notification>) {
        mpsc::channel(8)
    }

    #[test]
    fn register_assigns_incrementing_ids() {
        let mut reg = ClientRegistry::new();
        let (tx, _rx) = channel();
        let a = reg.register(tx);
        let (tx, _rx) = channel();
        let b = reg.register(tx);
        assert_eq!(a, ClientId(1));
        assert_eq!(b, ClientId(2));
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn unregister_removes_client() {
        let mut reg = ClientRegistry::new();
        let (tx, _rx) = channel();
        let id = reg.register(tx);
        assert_eq!(reg.len(), 1);
        reg.unregister(id);
        assert!(reg.is_empty());
    }

    #[test]
    fn subscribe_and_query() {
        let mut reg = ClientRegistry::new();
        let (tx, _rx) = channel();
        let id = reg.register(tx);
        reg.subscribe(id, PathBuf::from("/proj"));
        let subs = reg.subscribed(id);
        assert!(subs.contains(std::path::Path::new("/proj")));
    }

    #[tokio::test]
    async fn broadcast_to_project_reaches_subscribers() {
        let mut reg = ClientRegistry::new();
        let (tx1, mut rx1) = channel();
        let c1 = reg.register(tx1);
        reg.subscribe(c1, PathBuf::from("/proj"));
        let (tx2, mut rx2) = channel();
        let c2 = reg.register(tx2);
        reg.subscribe(c2, PathBuf::from("/proj"));
        // 未订阅的客户端
        let (tx3, mut rx3) = channel();
        let _c3 = reg.register(tx3);

        reg.broadcast_to_project(
            std::path::Path::new("/proj"),
            Notification::new(CONFIG_CHANGED, serde_json::json!({})),
            None,
        );

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
        assert!(rx3.try_recv().is_err());
    }

    #[tokio::test]
    async fn broadcast_excludes_source() {
        let mut reg = ClientRegistry::new();
        let (tx1, mut rx1) = channel();
        let c1 = reg.register(tx1);
        reg.subscribe(c1, PathBuf::from("/proj"));
        let (tx2, mut rx2) = channel();
        let c2 = reg.register(tx2);
        reg.subscribe(c2, PathBuf::from("/proj"));

        reg.broadcast_to_project(
            std::path::Path::new("/proj"),
            Notification::new(CONFIG_CHANGED, serde_json::json!({})),
            Some(c1),
        );
        assert!(rx1.try_recv().is_err(), "源应被排除");
        assert!(rx2.try_recv().is_ok());
        // 排除来源的广播不应影响 c2
        let _ = c2;
    }

    #[tokio::test]
    async fn broadcast_all_reaches_everyone() {
        let mut reg = ClientRegistry::new();
        let (tx1, mut rx1) = channel();
        let _c1 = reg.register(tx1);
        let (tx2, mut rx2) = channel();
        let _c2 = reg.register(tx2);

        reg.broadcast_all(
            Notification::new(CONFIG_CHANGED, serde_json::json!({})),
            None,
        );
        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }
}
