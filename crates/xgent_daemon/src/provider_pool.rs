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

/// 建流失败的错误：携带跨进程重建重试行为所需的全部信息。
///
/// 三个字段都必须过 IPC——只传 message 会让 UI 误判 kind 而触发无意义重试；
/// 只传 kind 会丢掉 `Retry-After`，使限流退避失效。
#[derive(Debug, Clone)]
pub struct ProviderCallError {
    pub kind: xgent_core::chat::ErrorKind,
    pub message: String,
    /// 服务端 `Retry-After`（秒）
    pub retry_after_secs: Option<u64>,
}

/// Provider 连接池。
pub struct ProviderPool {
    /// provider id → 实例（按配置中的 providers map key 标识）
    providers: RwLock<HashMap<String, Arc<dyn LlmProvider>>>,
    /// 全局配置引用（用于按需构造 provider）
    config: Arc<RwLock<ConfigCoordinator>>,
    /// daemon 自身的 StreamId 生成计数器
    stream_counter: std::sync::atomic::AtomicU64,
    /// 活跃流：stream_id → 该流的取消令牌。
    ///
    /// 用户 abort 时经 `provider.cancel` 查到令牌并 cancel，推送 task 在
    /// `cancelled()` 分支退出并 drop 接收端，上游 HTTP 请求随之取消——
    /// 否则被放弃的流会继续被消费到自然结束，token 照常计费（R1-2）。
    ///
    /// 用 `Arc<RwLock<..>>`：推送 task 需持有它，以便退出时移除自己的条目，
    /// 否则每次流结束都泄漏一条登记。
    active_streams: Arc<RwLock<HashMap<StreamId, tokio_util::sync::CancellationToken>>>,
}

impl ProviderPool {
    /// 构造。
    pub fn new(config: Arc<RwLock<ConfigCoordinator>>) -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
            config,
            stream_counter: std::sync::atomic::AtomicU64::new(1),
            active_streams: Arc::new(RwLock::new(HashMap::new())),
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
    /// 从配置构造并缓存。缓存可被 [`Self::invalidate`] 驱逐——config.write
    /// 触碰 providers.* 时必须失效，否则改 api_key/api_base 后旧实例
    /// （旧凭据）继续被使用，直到 daemon 重启。
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

    /// 驱逐全部缓存的 provider 实例（下次 `get` 按最新配置重建）。
    pub async fn invalidate(&self) {
        self.providers.write().await.clear();
    }

    /// 流式对话：调用 `provider.chat()`，把每个 [`ChatEvent`] 转成
    /// IPC notification 推送回客户端的 sender。
    ///
    /// 返回 `StreamId`。建流阶段错误（如 401/缺配置）返回
    /// `(ErrorKind, message)`，由 session 编码进 RPC error.data 透传给 UI
    /// （修复之前 map_err(to_string) 丢失 ErrorKind 导致 UI 误判为
    /// Network 触发无意义重试的 bug）。流中错误仍以 ChatEvent::Error 推送。
    pub async fn chat(
        &self,
        req: ChatRequest,
        _client: ClientId,
        sender: tokio::sync::mpsc::Sender<Notification>,
    ) -> Result<StreamId, ProviderCallError> {
        let provider_id = req.provider.clone();
        let provider = self
            .get(&provider_id)
            .await
            .map_err(|msg| ProviderCallError {
                kind: xgent_core::chat::ErrorKind::NotConfigured,
                message: msg,
                retry_after_secs: None,
            })?;
        let stream_id = self.next_stream_id();
        let (_, mut stream) = provider.chat(req).await.map_err(|e| ProviderCallError {
            kind: e.to_error_kind(),
            message: e.to_string(),
            // Retry-After 必须跨 IPC 抵达重试层，否则限流下永远按客户端
            // 退避重试——只会更快撞上限（R1-7 / A3）。
            retry_after_secs: e.retry_after_secs(),
        })?;
        let sid = stream_id;
        // 登记取消令牌，使 `provider.cancel` 能停掉本流（R1-2）
        let cancel = tokio_util::sync::CancellationToken::new();
        self.active_streams
            .write()
            .await
            .insert(sid, cancel.clone());
        // task 持有登记表，退出时移除自己的条目（否则每次流结束泄漏一条）
        let streams = self.active_streams.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    // 用户 abort：立即停止消费并退出 task，上游请求随之取消
                    _ = cancel.cancelled() => {
                        tracing::info!(stream_id = sid.0, "流被客户端取消");
                        break;
                    }
                    ev = stream.recv() => match ev {
                        Some(ev) => {
                            let notif = chat_event_to_notification(sid, ev);
                            if sender.send(notif).await.is_err() {
                                // 客户端已断开，停止推送
                                break;
                            }
                        }
                        None => break,
                    },
                }
            }
            // 流结束（正常/取消/断开）：移除登记
            streams.write().await.remove(&sid);
        });
        Ok(stream_id)
    }

    /// 取消指定流（用户 abort）。
    ///
    /// 流不存在或已结束视为成功——取消是幂等的，重复取消不应报错。
    pub async fn cancel(&self, stream_id: StreamId) -> bool {
        match self.active_streams.write().await.remove(&stream_id) {
            Some(token) => {
                token.cancel();
                tracing::info!(stream_id = stream_id.0, "已取消流");
                true
            }
            None => false,
        }
    }

    /// 当前活跃流数量（测试与诊断用）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn active_stream_count(&self) -> usize {
        self.active_streams.read().await.len()
    }

    /// 登记一条活跃流（仅测试用）。
    #[cfg(test)]
    pub async fn register_stream_for_test(
        &self,
        stream_id: StreamId,
        token: tokio_util::sync::CancellationToken,
    ) {
        self.active_streams.write().await.insert(stream_id, token);
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
