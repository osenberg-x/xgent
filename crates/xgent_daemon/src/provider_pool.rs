//! Provider 连接池。
//!
//! 持有各 provider 的 [`LlmProvider`] 实例（按配置构造，复用连接）。
//! 流式对话由 daemon task 消费 [`ChatEvent`]，转成 JSON-RPC notification
//! 推送回客户端。

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use xgent_core::chat::{ChatEvent, ChatRequest};
use xgent_core::ids::{ClientId, StreamId};
use xgent_core::notifications;
use xgent_core::proto::Notification;
use xgent_provider::{LlmProvider, build_provider};

use crate::config_store::ConfigCoordinator;

/// Provider 连接池。
pub struct ProviderPool {
    /// provider id → 实例（按配置中的 providers map key 标识）
    providers: RwLock<HashMap<String, Arc<dyn LlmProvider>>>,
    /// 全局配置引用（用于按需构造 provider）
    config: Arc<RwLock<ConfigCoordinator>>,
    /// daemon 自身的 StreamId 生成计数器
    stream_counter: std::sync::atomic::AtomicU64,
}

impl ProviderPool {
    /// 构造。
    pub fn new(config: Arc<RwLock<ConfigCoordinator>>) -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
            config,
            stream_counter: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// 生成新的 StreamId。
    fn next_stream_id(&self) -> StreamId {
        StreamId(
            self.stream_counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        )
    }

    /// 获取或创建 provider 实例。
    ///
    /// 按 `id`（对应全局配置 `providers` map 的 key）查找；不存在则
    /// 从配置构造并缓存。
    pub async fn get(&self, id: &str) -> Result<Arc<dyn LlmProvider>, String> {
        {
            let map = self.providers.read().await;
            if let Some(p) = map.get(id) {
                return Ok(p.clone());
            }
        }
        // 读配置构造
        let cfg = self.config.read().await;
        let provider_cfg = cfg
            .config()
            .providers
            .get(id)
            .cloned()
            .ok_or_else(|| format!("配置中无 provider: {id}"))?;
        drop(cfg);
        let provider: Arc<dyn LlmProvider> = Arc::from(build_provider(&provider_cfg));
        let mut map = self.providers.write().await;
        map.insert(id.to_string(), provider.clone());
        Ok(provider)
    }

    /// 流式对话：调用 `provider.chat()`，把每个 [`ChatEvent`] 转成
    /// IPC notification 推送回客户端的 sender。
    ///
    /// 返回 `(StreamId, ())`。错误以 [`ChatEvent::Error`] 形式推送。
    pub async fn chat(
        &self,
        req: ChatRequest,
        _client: ClientId,
        sender: tokio::sync::mpsc::Sender<Notification>,
    ) -> Result<StreamId, String> {
        let provider_id = req.provider.clone();
        let provider = self.get(&provider_id).await?;
        let stream_id = self.next_stream_id();
        let (_, mut stream) = provider.chat(req).await.map_err(|e| e.to_string())?;
        let sid = stream_id;
        tokio::spawn(async move {
            while let Some(ev) = stream.recv().await {
                let notif = chat_event_to_notification(sid, ev);
                if sender.send(notif).await.is_err() {
                    // 客户端已断开，停止推送
                    break;
                }
            }
        });
        Ok(stream_id)
    }
}

/// 把 [`ChatEvent`] 转成对应的 IPC notification。
fn chat_event_to_notification(stream_id: StreamId, ev: ChatEvent) -> Notification {
    let (method, value) = match &ev {
        ChatEvent::Delta { text } => (
            notifications::PROVIDER_DELTA,
            serde_json::json!({ "stream_id": stream_id.0, "text": text }),
        ),
        ChatEvent::ToolCall { id, name, args } => (
            notifications::PROVIDER_TOOL_CALL,
            serde_json::json!({ "stream_id": stream_id.0, "id": id, "name": name, "args": args }),
        ),
        ChatEvent::Done { usage } => (
            notifications::PROVIDER_DONE,
            serde_json::json!({ "stream_id": stream_id.0, "usage": usage }),
        ),
        ChatEvent::Error { message } => (
            notifications::PROVIDER_ERROR,
            serde_json::json!({ "stream_id": stream_id.0, "message": message }),
        ),
    };
    Notification::new(method, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xgent_core::chat::{ChatEvent, TokenUsage};
    use xgent_core::ids::StreamId;

    #[test]
    fn delta_event_to_notification() {
        let ev = ChatEvent::Delta { text: "hi".into() };
        let n = chat_event_to_notification(StreamId(7), ev);
        assert_eq!(n.method, "provider.delta");
        assert_eq!(n.params["stream_id"], 7);
        assert_eq!(n.params["text"], "hi");
    }

    #[test]
    fn done_event_to_notification() {
        let ev = ChatEvent::Done {
            usage: TokenUsage {
                prompt: 5,
                completion: 3,
            },
        };
        let n = chat_event_to_notification(StreamId(1), ev);
        assert_eq!(n.method, "provider.done");
        assert_eq!(n.params["usage"]["prompt"], 5);
        assert_eq!(n.params["usage"]["completion"], 3);
    }

    #[test]
    fn error_event_to_notification() {
        let ev = ChatEvent::Error {
            message: "boom".into(),
        };
        let n = chat_event_to_notification(StreamId(9), ev);
        assert_eq!(n.method, "provider.error");
        assert_eq!(n.params["message"], "boom");
    }

    #[test]
    fn tool_call_event_to_notification() {
        let ev = ChatEvent::ToolCall {
            id: "call_1".into(),
            name: "read".into(),
            args: serde_json::json!({"path": "/x"}),
        };
        let n = chat_event_to_notification(StreamId(2), ev);
        assert_eq!(n.method, "provider.toolCall");
        assert_eq!(n.params["id"], "call_1");
        assert_eq!(n.params["name"], "read");
        assert_eq!(n.params["args"]["path"], "/x");
    }

    #[tokio::test]
    async fn get_unknown_provider_errors() {
        let cfg = ConfigCoordinator::with_config(Default::default());
        let pool = ProviderPool::new(Arc::new(RwLock::new(cfg)));
        match pool.get("nonexistent").await {
            Err(msg) => assert!(msg.contains("无 provider")),
            Ok(_) => panic!("应返回错误"),
        }
    }

    #[tokio::test]
    async fn next_stream_id_increments() {
        let cfg = ConfigCoordinator::with_config(Default::default());
        let pool = ProviderPool::new(Arc::new(RwLock::new(cfg)));
        let a = pool.next_stream_id();
        let b = pool.next_stream_id();
        assert!(b.0 > a.0);
    }
}
