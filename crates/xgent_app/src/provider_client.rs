//! ProviderClient 的 IPC 实现：经 daemon 调 provider 池。
//!
//! `chat`：调 `provider.chat` 拿 stream_id，订阅 IPC 通知，过滤该 stream_id 的
//! `provider.*` 通知转成 [`ChatEvent`]，发到 mpsc channel 供 agent bridge 消费。

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;
use xgent_agent::bridge::ProviderClient;
use xgent_core::chat::{ChatEvent, ChatRequest, TokenUsage};
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
    ) -> Result<(StreamId, mpsc::Receiver<ChatEvent>), String> {
        let params = serde_json::to_value(&req).map_err(|e| e.to_string())?;
        let result = self
            .ipc
            .call_ok(xgent_core::methods::PROVIDER_CHAT, params)
            .await
            .map_err(|e| e.to_string())?;
        let stream_id: u64 = result["stream_id"]
            .as_u64()
            .ok_or_else(|| "响应缺少 stream_id".to_string())?;
        let stream_id = StreamId(stream_id);

        // 订阅通知，过滤该 stream 的 provider.* 通知转 ChatEvent
        let mut rx = self.ipc.subscribe();
        let (tx, chat_rx) = mpsc::channel::<ChatEvent>(64);
        let target_sid = stream_id.0;
        tokio::spawn(async move {
            while let Ok(notif) = rx.recv().await {
                let sid = notif.params["stream_id"].as_u64();
                if sid != Some(target_sid) {
                    continue;
                }
                let ev = match notif.method.as_str() {
                    notifications::PROVIDER_DELTA => {
                        let text = notif.params["text"].as_str().unwrap_or("").to_string();
                        Some(ChatEvent::Delta { text })
                    }
                    notifications::PROVIDER_TOOL_CALL => {
                        let id = notif.params["id"].as_str().unwrap_or("").to_string();
                        let name = notif.params["name"].as_str().unwrap_or("").to_string();
                        let args = notif.params["args"].clone();
                        Some(ChatEvent::ToolCall { id, name, args })
                    }
                    notifications::PROVIDER_DONE => {
                        let usage: TokenUsage =
                            serde_json::from_value(notif.params["usage"].clone())
                                .unwrap_or_default();
                        Some(ChatEvent::Done { usage })
                    }
                    notifications::PROVIDER_ERROR => {
                        let message = notif.params["message"]
                            .as_str()
                            .unwrap_or("未知错误")
                            .to_string();
                        Some(ChatEvent::Error { message })
                    }
                    _ => None,
                };
                if let Some(ev) = ev
                    && tx.send(ev).await.is_err()
                {
                    break;
                }
            }
        });

        Ok((stream_id, chat_rx))
    }
}
