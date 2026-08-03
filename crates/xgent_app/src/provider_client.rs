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
    ) -> Result<(StreamId, mpsc::Receiver<ChatEvent>), (xgent_core::chat::ErrorKind, String)> {
        let params = serde_json::to_value(&req)
            .map_err(|e| (xgent_core::chat::ErrorKind::ProviderError, e.to_string()))?;
        // 先订阅通知，再发起 chat 请求——避免 daemon 推送 task 在
        // call_ok 返回前已发通知、subscribe 后到的竞态导致首批事件丢失
        // （修复 broadcast 无历史缓存致 subscribe 前通知丢失的 bug）。
        let mut rx = self.ipc.subscribe();
        let resp = self
            .ipc
            .call(xgent_core::methods::PROVIDER_CHAT, params)
            .await
            .map_err(|e| (xgent_core::chat::ErrorKind::Network, e.to_string()))?;
        let result = match resp.error {
            Some(err) => {
                // 从 RPC error.data 恢复 ErrorKind（修复之前 call_ok 扁平化
                // 为 anyhow String、UI 误判 Network 触发无意义重试的 bug）。
                let kind = err
                    .data
                    .and_then(|d| serde_json::from_value::<xgent_core::chat::ErrorKind>(d).ok())
                    .unwrap_or(xgent_core::chat::ErrorKind::ProviderError);
                return Err((kind, err.message));
            }
            None => resp.result.unwrap_or(serde_json::Value::Null),
        };
        let stream_id: u64 = result["stream_id"].as_u64().ok_or_else(|| {
            (
                xgent_core::chat::ErrorKind::StreamParse,
                "响应缺少 stream_id".to_string(),
            )
        })?;
        let stream_id = StreamId(stream_id);

        // 消费 task：过滤该 stream 的 provider.* 通知转 ChatEvent
        let (tx, chat_rx) = mpsc::channel::<ChatEvent>(64);
        let target_sid = stream_id.0;
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(notif) => {
                        let sid = notif.params["stream_id"].as_u64();
                        if sid != Some(target_sid) {
                            continue;
                        }
                        let ev = match notif.method.as_str() {
                            // daemon 透传整个 ChatEvent JSON（见 ADR-0006），反序列化
                            notifications::PROVIDER_EVENT => {
                                serde_json::from_value::<ChatEvent>(notif.params["event"].clone())
                                    .ok()
                            }
                            _ => None,
                        };
                        if let Some(ev) = ev
                            && tx.send(ev).await.is_err()
                        {
                            break;
                        }
                    }
                    // Lagged：订阅者消费慢于生产者，溢出了一批旧通知。
                    // 不能退出循环——否则 provider 事件流静默断开，agent 侧
                    // stream.recv() 返回 None 触发 StreamParse 重试，可能反复
                    // Lagged 陷入死循环。改为 continue 接收后续新通知
                    // （溢出的旧事件已丢，agent 侧若收到不完整流会自行重试）。
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    // Closed：所有 sender（ipc 读循环）已退出，流结束。
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        Ok((stream_id, chat_rx))
    }
}
