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
        let result = self
            .ipc
            .call_ok(xgent_core::methods::PROVIDER_CHAT, params)
            .await
            .map_err(|e| (xgent_core::chat::ErrorKind::Network, e.to_string()))?;
        let stream_id: u64 = result["stream_id"].as_u64().ok_or_else(|| {
            (
                xgent_core::chat::ErrorKind::StreamParse,
                "响应缺少 stream_id".to_string(),
            )
        })?;
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
                    notifications::PROVIDER_EVENT => {
                        // daemon 透传整个 ChatEvent JSON（见 ADR-0006），反序列化
                        match serde_json::from_value::<ChatEvent>(notif.params["event"].clone()) {
                            Ok(ev) => Some(ev),
                            Err(_) => {
                                // 未知事件类型或畸形 JSON：跳过（向前兼容）
                                None
                            }
                        }
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
