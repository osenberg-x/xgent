//! 单个 UI 客户端连接的会话。
//!
//! 注册客户端、循环读取 JSON-RPC 消息（按行）、分发到对应 handler。
//! 所有输出（Response 与 Notification）经统一 writer task 写回 socket，
//! 避免读写半边竞争。连接断开时注销并触发退出计时。

use std::pin::Pin;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use xgent_core::config::{ConfigReadRequest, ConfigWriteRequest};
use xgent_core::fs::WatchRequest;
use xgent_core::methods;
use xgent_core::notifications;
use xgent_core::proto::{Notification, Request, Response, RpcError};

use crate::server::Shared;

/// 已连接的 IPC 流的读写两半（trait object，跨平台抽象）。
///
/// - Unix：`tokio::net::UnixStream::into_split()` 得到 `OwnedReadHalf` / `OwnedWriteHalf`
/// - Windows：`tokio::net::windows::NamedPipeServer` 经 `tokio::io::split()` 得到两半
///
/// 统一装箱为 trait object，`Session` 不关心底层具体类型。
pub struct ConnStream {
    pub read: Pin<Box<dyn AsyncRead + Send>>,
    pub write: Pin<Box<dyn AsyncWrite + Send>>,
}

/// 写回客户端的一行消息（Response 或 Notification）。
enum Outgoing {
    Response(Response),
    Notification(Notification),
}

impl Outgoing {
    /// 序列化为 JSON 行文本（含尾部换行）。
    fn to_json_line(&self) -> String {
        let s = match self {
            Outgoing::Response(r) => serde_json::to_string(r).unwrap_or_default(),
            Outgoing::Notification(n) => serde_json::to_string(n).unwrap_or_default(),
        };
        format!("{s}\n")
    }
}

/// 单个客户端会话。
pub struct Session {
    stream: ConnStream,
    shared: Shared,
}

impl Session {
    pub fn new(stream: ConnStream, shared: Shared) -> Self {
        Self { stream, shared }
    }

    /// 处理整个连接生命周期。
    pub async fn handle(self) {
        let mut reader = BufReader::new(self.stream.read);
        let mut writer = self.stream.write;

        // 统一输出 channel：所有 Response/Notification 经此发送给 writer task
        let (out_tx, mut out_rx) = mpsc::channel::<Outgoing>(128);

        // 注册客户端，把其通知推送端绑到 out_tx（经 Notification 转发）
        let client_id = {
            let mut reg = self.shared.registry.write().await;
            // 把 out_tx 克隆一份作为该客户端的通知 sender
            reg.register(out_tx.clone().into_notification_sender())
        };
        self.shared.lifecycle.on_connect();

        // writer task：消费 out_rx，逐行写回 socket
        let writer_task = tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                let line = msg.to_json_line();
                if writer.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
        });

        // 按行读取请求/通知
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(req) = serde_json::from_str::<Request>(trimmed) {
                        let resp = dispatch_request(req, &self.shared, client_id).await;
                        let _ = out_tx.send(Outgoing::Response(resp)).await;
                    }
                    // 通知（无 id）MVP 暂不处理
                }
                Err(e) => {
                    tracing::warn!("读取失败: {e}");
                    break;
                }
            }
        }

        // 注销
        {
            let mut reg = self.shared.registry.write().await;
            reg.unregister(client_id);
        }
        self.shared.watcher.unwatch_client(client_id).await;
        self.shared.lifecycle.on_disconnect().await;
        // 关闭输出 channel，让 writer task 自然结束
        drop(out_tx);
        let _ = writer_task.await;
    }
}

/// 适配：把 `mpsc::Sender<Outgoing>` 转成 `mpsc::Sender<Notification>`。
///
/// provider_pool/chat 通过 registry 的 `sender_for` 拿到此 sender 推送
/// 通知；这里实际是同一个 out_tx 的克隆，通知会被 writer task 写回。
/// 由于 `Outgoing` 与 `Notification` 不同类型，用一个轻量适配器转发。
trait IntoNotificationSender {
    fn into_notification_sender(self) -> mpsc::Sender<Notification>;
}

impl IntoNotificationSender for mpsc::Sender<Outgoing> {
    fn into_notification_sender(self) -> mpsc::Sender<Notification> {
        // 创建新 channel，spawn task 把 Notification 转 Outgoing 转发。
        let (n_tx, mut n_rx) = mpsc::channel::<Notification>(128);
        let out_tx = self;
        tokio::spawn(async move {
            while let Some(n) = n_rx.recv().await {
                if out_tx.send(Outgoing::Notification(n)).await.is_err() {
                    break;
                }
            }
        });
        n_tx
    }
}

/// 分发请求到对应 handler。
async fn dispatch_request(
    req: Request,
    shared: &Shared,
    client_id: xgent_core::ids::ClientId,
) -> Response {
    match req.method.as_str() {
        methods::CONFIG_READ => config_read(req, shared).await,
        methods::CONFIG_WRITE => config_write(req, shared, client_id).await,
        methods::FS_WATCH => fs_watch(req, shared, client_id).await,
        methods::PROVIDER_LIST_MODELS => provider_list_models(req, shared).await,
        methods::PROVIDER_CHAT => provider_chat(req, shared, client_id).await,
        _ => Response::err(
            req.id,
            RpcError::new(
                xgent_core::proto::METHOD_NOT_FOUND,
                format!("未知方法: {}", req.method),
                None,
            ),
        ),
    }
}

/// config.read
async fn config_read(req: Request, shared: &Shared) -> Response {
    let params: ConfigReadRequest = match serde_json::from_value(req.params.clone()) {
        Ok(p) => p,
        Err(e) => {
            return Response::err(
                req.id,
                RpcError::new(xgent_core::proto::INVALID_PARAMS, e.to_string(), None),
            );
        }
    };
    let cfg = shared.config.read().await;
    let value = cfg.read(&params.key);
    Response::ok(req.id, value)
}

/// config.write：写入并广播 config.changed
async fn config_write(
    req: Request,
    shared: &Shared,
    client_id: xgent_core::ids::ClientId,
) -> Response {
    let params: ConfigWriteRequest = match serde_json::from_value(req.params.clone()) {
        Ok(p) => p,
        Err(e) => {
            return Response::err(
                req.id,
                RpcError::new(xgent_core::proto::INVALID_PARAMS, e.to_string(), None),
            );
        }
    };
    let changed = {
        let mut cfg = shared.config.write().await;
        match cfg.write(&params.key, params.value) {
            Ok(c) => c,
            Err(e) => {
                return Response::err(
                    req.id,
                    RpcError::new(xgent_core::proto::INTERNAL_ERROR, e.to_string(), None),
                );
            }
        }
    };
    // 广播给所有客户端（排除来源）
    let notif = Notification::new(
        notifications::CONFIG_CHANGED,
        serde_json::to_value(&changed).unwrap_or_default(),
    );
    let reg = shared.registry.read().await;
    reg.broadcast_all(notif, Some(client_id));
    Response::ok(req.id, serde_json::json!({"ok": true}))
}

/// fs.watch：订阅项目
async fn fs_watch(req: Request, shared: &Shared, client_id: xgent_core::ids::ClientId) -> Response {
    let params: WatchRequest = match serde_json::from_value(req.params.clone()) {
        Ok(p) => p,
        Err(e) => {
            return Response::err(
                req.id,
                RpcError::new(xgent_core::proto::INVALID_PARAMS, e.to_string(), None),
            );
        }
    };
    {
        let mut reg = shared.registry.write().await;
        reg.subscribe(client_id, params.project_root.clone());
    }
    if let Err(e) = shared.watcher.watch(params.project_root, client_id).await {
        return Response::err(
            req.id,
            RpcError::new(xgent_core::proto::INTERNAL_ERROR, e.to_string(), None),
        );
    }
    Response::ok(req.id, serde_json::json!({"ok": true}))
}

/// provider.listModels
async fn provider_list_models(req: Request, shared: &Shared) -> Response {
    #[derive(serde::Deserialize)]
    struct Params {
        provider: String,
    }
    let params: Params = match serde_json::from_value(req.params.clone()) {
        Ok(p) => p,
        Err(e) => {
            return Response::err(
                req.id,
                RpcError::new(xgent_core::proto::INVALID_PARAMS, e.to_string(), None),
            );
        }
    };
    match shared.pool.get(&params.provider).await {
        Ok(p) => match p.list_models().await {
            Ok(models) => Response::ok(req.id, serde_json::to_value(models).unwrap_or_default()),
            Err(e) => Response::err(
                req.id,
                RpcError::new(xgent_core::proto::INTERNAL_ERROR, e.to_string(), None),
            ),
        },
        Err(e) => Response::err(
            req.id,
            RpcError::new(xgent_core::proto::INVALID_PARAMS, e, None),
        ),
    }
}

/// provider.chat：发起流式对话
async fn provider_chat(
    req: Request,
    shared: &Shared,
    client_id: xgent_core::ids::ClientId,
) -> Response {
    let chat_req: xgent_core::chat::ChatRequest = match serde_json::from_value(req.params.clone()) {
        Ok(r) => r,
        Err(e) => {
            return Response::err(
                req.id,
                RpcError::new(xgent_core::proto::INVALID_PARAMS, e.to_string(), None),
            );
        }
    };
    let sender = {
        let reg = shared.registry.read().await;
        reg.sender_for(client_id)
    };
    let Some(sender) = sender else {
        return Response::err(
            req.id,
            RpcError::new(
                xgent_core::proto::INTERNAL_ERROR,
                "客户端未注册".to_string(),
                None,
            ),
        );
    };
    match shared.pool.chat(chat_req, client_id, sender).await {
        Ok(stream_id) => Response::ok(req.id, serde_json::json!({"stream_id": stream_id.0})),
        Err(e) => Response::err(
            req.id,
            RpcError::new(xgent_core::proto::INTERNAL_ERROR, e, None),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outgoing_response_to_json_line() {
        let r = Outgoing::Response(Response::ok(1, serde_json::json!({"ok": true})));
        let line = r.to_json_line();
        assert!(line.ends_with('\n'));
        assert!(line.contains(r#""id":1"#));
        assert!(line.contains(r#""result""#));
    }

    #[test]
    fn outgoing_notification_to_json_line() {
        let n = Outgoing::Notification(Notification::new(
            notifications::FS_CHANGED,
            serde_json::json!({}),
        ));
        let line = n.to_json_line();
        assert!(line.ends_with('\n'));
        // 通知无 id 字段
        assert!(!line.contains(r#""id""#));
        assert!(line.contains(r#""fs.changed""#));
    }
}
