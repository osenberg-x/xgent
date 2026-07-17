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
        let provider: Arc<dyn LlmProvider> = Arc::from(build_provider(id, &provider_cfg));
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

/// 把 [`ChatEvent`] 转成对应的 IPC notification（透传整个 event JSON）。
///
/// daemon 不解析 ChatEvent 内部结构——按 ADR-0006，daemon 只透传 JSON，
/// 由 UI 侧反序列化。单一 method `provider.event`，params 含 stream_id + event。
fn chat_event_to_notification(stream_id: StreamId, ev: ChatEvent) -> Notification {
    Notification::new(
        notifications::PROVIDER_EVENT,
        serde_json::json!({
            "stream_id": stream_id.0,
            "event": ev,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use xgent_core::chat::{ChatEvent, StopReason, TokenUsage};
    use xgent_core::ids::StreamId;

    #[test]
    fn event_to_notification_透传整个_json() {
        let ev = ChatEvent::TextDelta { text: "hi".into() };
        let n = chat_event_to_notification(StreamId(7), ev);
        assert_eq!(n.method, "provider.event");
        assert_eq!(n.params["stream_id"], 7);
        // event 字段是完整 ChatEvent JSON
        assert_eq!(n.params["event"]["type"], "textDelta");
        assert_eq!(n.params["event"]["text"], "hi");
    }

    #[test]
    fn done_event_with_reason_透传() {
        let ev = ChatEvent::Done {
            reason: StopReason::ToolUse,
            usage: TokenUsage {
                prompt: 5,
                completion: 3,
            },
        };
        let n = chat_event_to_notification(StreamId(1), ev);
        assert_eq!(n.params["event"]["type"], "done");
        assert_eq!(n.params["event"]["reason"], "toolUse");
        assert_eq!(n.params["event"]["usage"]["prompt"], 5);
    }

    #[test]
    fn tool_call_start_透传() {
        let ev = ChatEvent::ToolCallStart {
            index: 0,
            id: "call_1".into(),
            name: "read_file".into(),
        };
        let n = chat_event_to_notification(StreamId(2), ev);
        assert_eq!(n.params["event"]["type"], "toolCallStart");
        assert_eq!(n.params["event"]["id"], "call_1");
        assert_eq!(n.params["event"]["name"], "read_file");
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
