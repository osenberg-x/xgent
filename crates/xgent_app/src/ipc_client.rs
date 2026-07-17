//! JSON-RPC 客户端：经本地 IPC 通道（Unix domain socket / Windows named pipe）与 daemon 通信。
//!
//! 协议：换行分隔的 JSON-RPC 2.0（每行一个 Request/Response/Notification）。
//! 读 IPC 通道在 tokio task 长驻：有 id 的 Response 唤醒对应 pending oneshot；
//! 无 id 的 Notification 经 broadcast 推送给订阅者。

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, broadcast, oneshot};
use xgent_core::proto::{Notification, Request, Response};

/// IPC 客户端：经本地通道调用 daemon。
#[derive(Clone)]
pub struct IpcClient {
    /// 写请求的通道端（互斥）
    writer: Arc<Mutex<Pin<Box<dyn AsyncWrite + Send>>>>,
    /// 待响应请求的 oneshot 表：id -> sender
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Response>>>>,
    /// 通知广播：所有订阅者都能收到
    notif_tx: broadcast::Sender<Notification>,
    /// 下一个请求 id
    next_id: Arc<AtomicU64>,
}

impl IpcClient {
    /// 连接到已运行的 daemon。
    ///
    /// - Unix：经 Unix domain socket 连接
    /// - Windows：经 named pipe 连接
    pub async fn connect(socket_path: &std::path::Path) -> Result<Self> {
        let (reader, writer) = connect_stream(socket_path).await?;

        let (notif_tx, _) = broadcast::channel::<Notification>(128);
        let pending = Arc::new(Mutex::new(HashMap::<u64, oneshot::Sender<Response>>::new()));

        // 读循环 task
        {
            let notif_tx = notif_tx.clone();
            let pending = pending.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(reader);
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
                            // 先尝试解析为 Response（有 id），否则 Notification（无 id）
                            if let Ok(resp) = serde_json::from_str::<Response>(trimmed) {
                                if let Some(tx) = pending.lock().await.remove(&resp.id) {
                                    let _ = tx.send(resp);
                                }
                                continue;
                            }
                            if let Ok(notif) = serde_json::from_str::<Notification>(trimmed) {
                                let _ = notif_tx.send(notif);
                                continue;
                            }
                            tracing::warn!("无法解析 IPC 行: {}", trimmed);
                        }
                        Err(e) => {
                            tracing::warn!("IPC 读错误: {e}");
                            break;
                        }
                    }
                }
            });
        }

        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            pending,
            notif_tx,
            next_id: Arc::new(AtomicU64::new(1)),
        })
    }

    /// 订阅通知流。多个订阅者各自独立。
    pub fn subscribe(&self) -> broadcast::Receiver<Notification> {
        self.notif_tx.subscribe()
    }

    /// 发起 JSON-RPC 请求，等待响应。
    pub async fn call(&self, method: &str, params: serde_json::Value) -> Result<Response> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let req = Request::new(id, method, params);
        let line = serde_json::to_string(&req)?;

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        {
            let mut writer = self.writer.lock().await;
            writer.write_all(line.as_bytes()).await?;
            writer.write_all(b"\n").await?;
        }

        let resp = rx.await.map_err(|_| anyhow::anyhow!("响应通道关闭"))?;
        Ok(resp)
    }

    /// 便捷：调用并断言成功，返回 result。
    pub async fn call_ok(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let resp = self.call(method, params).await?;
        if let Some(err) = resp.error {
            anyhow::bail!("daemon 调用 {method} 失败 [{}]: {}", err.code, err.message);
        }
        Ok(resp.result.unwrap_or(serde_json::Value::Null))
    }
}

/// 跨平台 IPC 连接：返回已 split 的读写两半。
///
/// - Unix：`UnixStream::connect` + `into_split()`
/// - Windows：`NamedPipeClient::connect` + `tokio::io::split()`
#[cfg(unix)]
async fn connect_stream(
    socket_path: &std::path::Path,
) -> Result<(
    Pin<Box<dyn AsyncRead + Send>>,
    Pin<Box<dyn AsyncWrite + Send>>,
)> {
    let stream = tokio::net::UnixStream::connect(socket_path).await?;
    let (read_half, write_half) = stream.into_split();
    Ok((Box::pin(read_half), Box::pin(write_half)))
}

/// 跨平台 IPC 连接：返回已 split 的读写两半。
///
/// - Unix：`UnixStream::connect` + `into_split()`
/// - Windows：`NamedPipeClient::connect` + `tokio::io::split()`
#[cfg(windows)]
async fn connect_stream(
    pipe_name: &std::path::Path,
) -> Result<(
    Pin<Box<dyn AsyncRead + Send>>,
    Pin<Box<dyn AsyncWrite + Send>>,
)> {
    use tokio::io;
    use tokio::net::windows::named_pipe::ClientOptions;

    // named pipe 客户端可能需要重试：daemon 刚拉起时 pipe 尚未就绪
    let stream = ClientOptions::new()
        .open(pipe_name)
        .map_err(|e| anyhow::anyhow!("连接 named pipe 失败: {} ({})", pipe_name.display(), e))?;

    let (read_half, write_half) = io::split(stream);
    Ok((Box::pin(read_half), Box::pin(write_half)))
}
