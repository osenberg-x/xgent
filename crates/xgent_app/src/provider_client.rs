//! ProviderClient 的 IPC 实现：经 daemon 调 provider 池。
//!
//! `chat`：调 `provider.chat` 拿 stream_id，订阅 IPC 通知，过滤该 stream_id 的
//! `provider.*` 通知转成 [`ChatEvent`]，发到 mpsc channel 供 agent bridge 消费。

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;
use xgent_agent::bridge::ProviderClient;
use xgent_core::chat::{ChatEvent, ChatRequest};
use xgent_core::ids::StreamId;
use xgent_core::notifications;

use crate::ipc_client::IpcClient;

/// 经 IPC 调 daemon provider 池的 ProviderClient 实现。
pub struct IpcProviderClient {
    ipc: Arc<IpcClient>,
}

impl IpcProviderClient {
    pub fn new(ipc: Arc<IpcClient>) -> Self {
        Self { ipc }
    }
}

#[async_trait]
impl ProviderClient for IpcProviderClient {
    async fn chat(
        &self,
        req: ChatRequest,
    ) -> Result<
        (StreamId, mpsc::Receiver<ChatEvent>),
        (xgent_core::chat::ErrorKind, String, Option<u64>),
    > {
        let params = serde_json::to_value(&req).map_err(|e| {
            (
                xgent_core::chat::ErrorKind::ProviderError,
                e.to_string(),
                None,
            )
        })?;
        // 先订阅通知，再发起 chat 请求——避免 daemon 推送 task 在
        // call_ok 返回前已发通知、subscribe 后到的竞态导致首批事件丢失
        // （修复 broadcast 无历史缓存致 subscribe 前通知丢失的 bug）。
        let mut rx = self.ipc.subscribe();
        let resp = self
            .ipc
            .call(xgent_core::methods::PROVIDER_CHAT, params)
            .await
            .map_err(|e| (xgent_core::chat::ErrorKind::Network, e.to_string(), None))?;
        let result = match resp.error {
            Some(err) => {
                // 从 RPC error.data 恢复 ErrorKind 与 Retry-After（修复之前
                // call_ok 扁平化为 anyhow String、UI 误判 Network 触发无意义重试）。
                //
                // 兼容两种 data 形态：新版是 `{kind, retry_after_secs}` 对象，
                // 旧版 daemon 只发裸 kind 字符串。
                let (kind, retry_after_secs) = match err.data.as_ref() {
                    Some(v) if v.is_object() => (
                        serde_json::from_value::<xgent_core::chat::ErrorKind>(v["kind"].clone())
                            .unwrap_or(xgent_core::chat::ErrorKind::ProviderError),
                        v["retry_after_secs"].as_u64(),
                    ),
                    Some(v) => (
                        serde_json::from_value::<xgent_core::chat::ErrorKind>(v.clone())
                            .unwrap_or(xgent_core::chat::ErrorKind::ProviderError),
                        None,
                    ),
                    None => (xgent_core::chat::ErrorKind::ProviderError, None),
                };
                return Err((kind, err.message, retry_after_secs));
            }
            None => resp.result.unwrap_or(serde_json::Value::Null),
        };
        let stream_id: u64 = result["stream_id"].as_u64().ok_or_else(|| {
            (
                xgent_core::chat::ErrorKind::StreamParse,
                "响应缺少 stream_id".to_string(),
                None,
            )
        })?;
        let stream_id = StreamId(stream_id);

        // 消费 task：过滤该 stream 的 provider.* 通知转 ChatEvent
        let (tx, chat_rx) = mpsc::channel::<ChatEvent>(64);
        let target_sid = stream_id.0;
        tokio::spawn(async move {
            // 空闲兜底：daemon 侧流有 first(30s)/idle(60s) 超时，正常情况
            // 90s 内必有事件。超限说明 daemon 侧异常终止且 Done/Error 丢失
            // （如 UI/daemon 版本偏差导致事件反序列化失败被丢弃）——退出
            // task，chat_rx 关闭让 agent 侧按流断开路径处理（StreamParse 重试）。
            const IDLE_LIMIT: std::time::Duration = std::time::Duration::from_secs(180);
            loop {
                match tokio::time::timeout(IDLE_LIMIT, rx.recv()).await {
                    Err(_) => {
                        tracing::warn!(
                            "provider 事件流 180s 无事件，消费 task 退出（stream {target_sid}）"
                        );
                        break;
                    }
                    // Closed：所有 sender（ipc 读循环）已退出，流结束
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
                    // Lagged：订阅者消费慢于生产者，溢出了一批旧通知。
                    // 不能退出循环——否则 provider 事件流静默断开，agent 侧
                    // stream.recv() 返回 None 触发 StreamParse 重试，可能反复
                    // Lagged 陷入死循环。改为 continue 接收后续新通知
                    // （溢出的旧事件已丢，agent 侧若收到不完整流会自行重试）。
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                    Ok(Ok(notif)) => {
                        let sid = notif.params["stream_id"].as_u64();
                        if sid != Some(target_sid) {
                            continue;
                        }
                        let ev = match notif.method.as_str() {
                            // daemon 透传整个 ChatEvent JSON（见 ADR-0006），反序列化
                            notifications::PROVIDER_EVENT => {
                                match serde_json::from_value::<ChatEvent>(
                                    notif.params["event"].clone(),
                                ) {
                                    Ok(ev) => Some(ev),
                                    Err(e) => {
                                        // 未知事件类型（版本偏差）：丢弃但必须可观测，
                                        // 否则丢的是 Done 时消费 task 永久滞留
                                        tracing::warn!("provider 事件反序列化失败（忽略）: {e}");
                                        None
                                    }
                                }
                            }
                            _ => None,
                        };
                        if let Some(ev) = ev {
                            let terminated =
                                matches!(ev, ChatEvent::Done { .. } | ChatEvent::Error { .. });
                            if tx.send(ev).await.is_err() {
                                break;
                            }
                            if terminated {
                                // Done/Error 是流的终止事件（chat.rs 事件序列约定）：
                                // 转发后退出，否则 task 常驻挂到 IpcClient drop
                                // （每次对话泄漏一个 task，并加剧 broadcast Lagged）
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok((stream_id, chat_rx))
    }

    /// 经 `provider.cancel` 把取消送到 daemon。
    ///
    /// daemon 侧据 stream_id 查到该流的取消令牌并 cancel，推送 task 退出并
    /// drop provider 接收端，上游 HTTP 请求随之取消——用户按中断后不再产生
    /// token。取消幂等：流已结束也不报错（R1-2）。
    async fn cancel(&self, stream_id: StreamId) {
        let params = serde_json::json!({ "stream_id": stream_id.0 });
        if let Err(e) = self
            .ipc
            .call_ok(xgent_core::methods::PROVIDER_CANCEL, params)
            .await
        {
            tracing::warn!(stream_id = stream_id.0, "取消流失败: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use xgent_core::chat::ErrorKind;
    use xgent_core::proto::RpcError;

    fn err_with_data(data: serde_json::Value) -> xgent_core::proto::Response {
        xgent_core::proto::Response::err(
            1,
            RpcError::new(xgent_core::proto::INTERNAL_ERROR, "boom", Some(data)),
        )
    }

    /// A3/R1-7：daemon 把 `{kind, retry_after_secs}` 编进 error.data，
    /// UI 侧必须能把两者都还原出来——否则 Retry-After 在 IPC 边界被丢弃。
    #[test]
    fn rpc_error_data_object_yields_kind_and_retry_after() {
        let r = err_with_data(serde_json::json!({
            "kind": "rateLimited",
            "retry_after_secs": 7,
        }));
        let d = r.error.unwrap().data.unwrap();
        let kind: ErrorKind = serde_json::from_value(d["kind"].clone()).unwrap();
        assert_eq!(kind, ErrorKind::RateLimited);
        assert_eq!(d["retry_after_secs"].as_u64(), Some(7));
    }

    /// 兼容旧版 daemon：只发裸 kind 字符串。
    #[test]
    fn rpc_error_data_bare_kind_still_parses() {
        let r = err_with_data(serde_json::json!("rateLimited"));
        let d = r.error.unwrap().data.unwrap();
        let kind: ErrorKind = serde_json::from_value(d).unwrap();
        assert_eq!(kind, ErrorKind::RateLimited);
    }

    #[test]
    fn rpc_error_data_null_yields_provider_error() {
        let r = xgent_core::proto::Response::err(
            1,
            RpcError::new(xgent_core::proto::INTERNAL_ERROR, "boom", None),
        );
        assert!(r.error.unwrap().data.is_none());
    }
}
