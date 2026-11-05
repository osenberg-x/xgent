//! MCP stdio 传输：子进程 + 换行分隔 JSON-RPC + 请求路由。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as AsyncMutex, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use xgent_settings_core::McpServerConfig;

use super::{McpError, McpTransport};

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, McpError>>>>>;
type BoxWriter = Box<dyn AsyncWrite + Send + Unpin>;
const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

struct Inner {
    pending: Pending,
    writer: AsyncMutex<BoxWriter>,
    closed: AtomicBool,
    next_id: AtomicU64,
    server_id: String,
    timeout: Duration,
}

impl Inner {
    fn fail_all(&self, reason: &str) {
        let mut pending = self.pending.lock();
        for (_, sender) in pending.drain() {
            let _ = sender.send(Err(McpError::Transport(reason.to_string())));
        }
    }

    fn mark_closed(&self, reason: &str) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            self.fail_all(reason);
        }
    }

    async fn write_value(&self, value: &Value) -> Result<(), McpError> {
        let mut line =
            serde_json::to_vec(value).map_err(|error| McpError::Protocol(error.to_string()))?;
        line.push(b'\n');
        let mut writer = self.writer.lock().await;
        writer
            .write_all(&line)
            .await
            .map_err(|error| McpError::Transport(error.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|error| McpError::Transport(error.to_string()))
    }
}

struct PendingGuard<'a> {
    inner: &'a Inner,
    key: String,
}

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        self.inner.pending.lock().remove(&self.key);
    }
}

/// 基于 stdio 的 MCP 传输。
pub struct StdioTransport {
    inner: Arc<Inner>,
    child: Mutex<Option<Child>>,
    reader_task: Mutex<Option<JoinHandle<()>>>,
}

/// Windows 的 `CreateProcessW` 不会按 PATHEXT 解析 `npx` 这类无扩展名命令；
/// 在 PATH/PATHEXT 中查找 `.cmd`/`.bat`/`.exe`，其他平台原样返回。
fn resolve_program(command: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let path = Path::new(command);
        if path.extension().is_some() || path.is_absolute() || path.components().count() > 1 {
            return path.to_path_buf();
        }
        if let Some(found) = find_in_path(command, std::env::var_os("PATH").as_deref()) {
            return found;
        }
    }
    PathBuf::from(command)
}

#[cfg(windows)]
fn find_in_path(command: &str, path_var: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    find_in_path_with(command, path_var?, &pathext)
}

#[cfg(windows)]
fn find_in_path_with(command: &str, path_var: &std::ffi::OsStr, pathext: &str) -> Option<PathBuf> {
    let extensions: Vec<&str> = pathext
        .split(';')
        .map(str::trim)
        .filter(|ext| !ext.is_empty())
        .collect();
    for dir in std::env::split_paths(path_var) {
        for extension in &extensions {
            let candidate = dir.join(format!("{command}{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

impl StdioTransport {
    /// 启动一个 MCP stdio 子进程。
    pub fn spawn(
        config: &McpServerConfig,
        project_root: &Path,
        timeout: Duration,
    ) -> Result<Arc<Self>, McpError> {
        let program = resolve_program(&config.command);
        let mut command = Command::new(&program);
        command.args(&config.args);
        command.envs(&config.env);
        if let Some(cwd) = &config.cwd {
            let path = Path::new(cwd);
            command.current_dir(if path.is_absolute() {
                path.to_path_buf()
            } else {
                project_root.join(path)
            });
        }
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            McpError::Spawn(format!(
                "启动 MCP Server `{}`（{}）失败：{error}",
                config.id, config.command
            ))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Spawn(format!("MCP Server `{}` 未提供 stdin", config.id)))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Spawn(format!("MCP Server `{}` 未提供 stdout", config.id)))?;
        if let Some(stderr) = child.stderr.take() {
            spawn_stderr_logger(config.id.clone(), stderr);
        }
        let transport = Self::from_io(stdout, stdin, timeout, config.id.clone());
        *transport.child.lock() = Some(child);
        Ok(transport)
    }

    /// 用任意读写流构造（测试与自定义传输复用）。
    pub fn from_io<R, W>(
        reader: R,
        writer: W,
        timeout: Duration,
        server_id: impl Into<String>,
    ) -> Arc<Self>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let inner = Arc::new(Inner {
            pending: Arc::new(Mutex::new(HashMap::new())),
            writer: AsyncMutex::new(Box::new(writer)),
            closed: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            server_id: server_id.into(),
            timeout,
        });
        let reader_inner = inner.clone();
        let reader_task = tokio::spawn(async move {
            read_loop(reader_inner, reader).await;
        });
        Arc::new(Self {
            inner,
            child: Mutex::new(None),
            reader_task: Mutex::new(Some(reader_task)),
        })
    }

    /// 带自定义超时的请求。
    pub async fn request_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, McpError> {
        if self.inner.closed.load(Ordering::SeqCst) {
            return Err(McpError::Transport("MCP 连接已关闭".to_string()));
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let key = id.to_string();
        let (tx, rx) = oneshot::channel();
        self.inner.pending.lock().insert(key.clone(), tx);
        let _guard = PendingGuard {
            inner: &self.inner,
            key: key.clone(),
        };
        // EOF 可能在首次检查与 insert 之间发生；插入后复检，避免等到超时。
        if self.inner.closed.load(Ordering::SeqCst) {
            return Err(McpError::Transport("MCP 连接已关闭".to_string()));
        }
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(error) = self.inner.write_value(&message).await {
            return Err(error);
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(McpError::Transport("响应通道已关闭".to_string())),
            Err(_) => Err(McpError::Timeout(timeout.as_secs().max(1))),
        }
    }

    /// 可取消请求；取消时向 Server 发送 `notifications/cancelled`。
    pub async fn request_cancellable(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Value, McpError> {
        if self.inner.closed.load(Ordering::SeqCst) {
            return Err(McpError::Transport("MCP 连接已关闭".to_string()));
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let key = id.to_string();
        let (tx, rx) = oneshot::channel();
        self.inner.pending.lock().insert(key.clone(), tx);
        let _guard = PendingGuard {
            inner: &self.inner,
            key: key.clone(),
        };
        if self.inner.closed.load(Ordering::SeqCst) {
            return Err(McpError::Transport("MCP 连接已关闭".to_string()));
        }
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(error) = self.inner.write_value(&message).await {
            return Err(error);
        }
        let timer = tokio::time::sleep(timeout);
        tokio::pin!(timer);
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                let _ = self.inner.write_value(&json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/cancelled",
                    "params": {"requestId": id, "reason": "cancelled"},
                })).await;
                return Err(McpError::Cancelled);
            }
            _ = &mut timer => {
                return Err(McpError::Timeout(timeout.as_secs().max(1)));
            }
            response = rx => response,
        };
        match result {
            Ok(result) => result,
            Err(_) => Err(McpError::Transport("响应通道已关闭".to_string())),
        }
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        self.request_with_timeout(method, params, self.inner.timeout)
            .await
    }

    async fn request_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, McpError> {
        StdioTransport::request_with_timeout(self, method, params, timeout).await
    }

    async fn request_cancellable(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Value, McpError> {
        StdioTransport::request_cancellable(self, method, params, timeout, cancel).await
    }

    async fn notify(&self, method: &str, params: Value) {
        let message = json!({"jsonrpc": "2.0", "method": method, "params": params});
        if let Err(error) = self.inner.write_value(&message).await {
            tracing::warn!(server = %self.inner.server_id, %error, "MCP 通知发送失败");
        }
    }

    async fn close(&self) {
        self.inner.mark_closed("MCP 传输已关闭");
        if let Some(task) = self.reader_task.lock().take() {
            task.abort();
        }
        let child = self.child.lock().take();
        if let Some(mut child) = child {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        }
    }

    fn is_connected(&self) -> bool {
        !self.inner.closed.load(Ordering::SeqCst)
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        self.inner.mark_closed("MCP 传输已释放");
        if let Some(task) = self.reader_task.get_mut().take() {
            task.abort();
        }
        if let Some(mut child) = self.child.get_mut().take() {
            let _ = child.start_kill();
        }
    }
}

async fn read_loop<R>(inner: Arc<Inner>, reader: R)
where
    R: AsyncRead + Send + Unpin,
{
    let mut lines = BufReader::new(reader);
    loop {
        match read_line_capped(&mut lines, MAX_MESSAGE_BYTES).await {
            Ok(Some(line)) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<Value>(line) {
                    Ok(value) => handle_incoming(&inner, value).await,
                    Err(error) => {
                        tracing::warn!(
                            server = %inner.server_id,
                            %error,
                            "MCP Server 发送了无法解析的 JSON 行"
                        );
                    }
                }
            }
            Ok(None) => {
                inner.mark_closed("MCP Server 已关闭连接");
                break;
            }
            Err(error) => {
                inner.mark_closed(&format!("读取 MCP Server 输出失败：{error}"));
                break;
            }
        }
    }
}

/// 读取一行但限制内存；超过上限直接报错并断开，避免恶意/异常 Server 撑爆进程。
async fn read_line_capped<R>(reader: &mut R, cap: usize) -> std::io::Result<Option<String>>
where
    R: AsyncBufRead + Unpin,
{
    let mut output = Vec::new();
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            if output.is_empty() {
                return Ok(None);
            }
            return Ok(Some(String::from_utf8_lossy(&output).into_owned()));
        }
        if let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            if output.len() + newline > cap {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "MCP 消息超过大小上限",
                ));
            }
            output.extend_from_slice(&buffer[..=newline]);
            reader.consume(newline + 1);
            return Ok(Some(String::from_utf8_lossy(&output).into_owned()));
        }
        if output.len() + buffer.len() > cap {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "MCP 消息超过大小上限",
            ));
        }
        let len = buffer.len();
        output.extend_from_slice(buffer);
        reader.consume(len);
    }
}

async fn handle_incoming(inner: &Arc<Inner>, value: Value) {
    let id_value = value.get("id").cloned();
    let method = value.get("method").and_then(Value::as_str);
    match (id_value, method) {
        (Some(id_value), None) => {
            let Some(key) = id_key(&id_value) else {
                tracing::debug!(server = %inner.server_id, "MCP 响应 id 类型非法");
                return;
            };
            let Some(sender) = inner.pending.lock().remove(&key) else {
                tracing::debug!(server = %inner.server_id, id = %key, "MCP 收到未知请求 id 的响应");
                return;
            };
            if let Some(error) = value.get("error") {
                let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("未知错误");
                let _ = sender.send(Err(McpError::Protocol(format!(
                    "MCP JSON-RPC 错误 {code}: {message}"
                ))));
            } else {
                let result = value.get("result").cloned().unwrap_or(Value::Null);
                let _ = sender.send(Ok(result));
            }
        }
        (Some(id_value), Some(method)) => {
            // Server 主动请求：只支持 ping，其余明确回 Method not found。
            // 回显原始 id（JSON-RPC 允许 number/string）。
            let response = if method == "ping" {
                json!({"jsonrpc": "2.0", "id": id_value, "result": {}})
            } else {
                json!({
                    "jsonrpc": "2.0",
                    "id": id_value,
                    "error": {"code": -32601, "message": format!("Method not found: {method}")},
                })
            };
            let _ = inner.write_value(&response).await;
        }
        (None, Some(method)) => {
            tracing::debug!(server = %inner.server_id, method, "MCP 通知");
        }
        (None, None) => {
            tracing::debug!(server = %inner.server_id, "MCP 收到无 id/无 method 的消息");
        }
    }
}

fn id_key(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => Some(number.to_string()),
        Value::String(text) => Some(format!("s:{text}")),
        _ => None,
    }
}

fn spawn_stderr_logger<R>(server_id: String, stderr: R)
where
    R: AsyncRead + Send + Unpin + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line: String = line.chars().take(2048).collect();
            // stderr 可能带凭据；默认降到 debug，避免 WARN 落盘时泄露。
            tracing::debug!(server = %server_id, "MCP stderr: {line}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

    fn pair(
        timeout: Duration,
    ) -> (
        Arc<StdioTransport>,
        BufReader<tokio::io::ReadHalf<DuplexStream>>,
        tokio::io::WriteHalf<DuplexStream>,
    ) {
        let (client, peer) = tokio::io::duplex(64 * 1024);
        let (client_read, client_write) = tokio::io::split(client);
        let (peer_read, peer_write) = tokio::io::split(peer);
        let transport = StdioTransport::from_io(client_read, client_write, timeout, "test");
        (transport, BufReader::new(peer_read), peer_write)
    }

    async fn write_json<W: AsyncWrite + Unpin>(writer: &mut W, value: Value) {
        let mut line = serde_json::to_vec(&value).expect("序列化");
        line.push(b'\n');
        writer.write_all(&line).await.expect("写响应");
    }

    #[tokio::test]
    async fn request_routes_response_by_id() {
        let (transport, mut reader, mut writer) = pair(Duration::from_secs(1));
        let peer = tokio::spawn(async move {
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("读请求");
            let value: Value = serde_json::from_str(line.trim()).expect("解析请求");
            assert_eq!(value["method"], "ping");
            write_json(
                &mut writer,
                json!({"jsonrpc": "2.0", "id": value["id"], "result": {"ok": true}}),
            )
            .await;
        });
        let result = transport
            .request_with_timeout("ping", json!({}), Duration::from_secs(1))
            .await
            .expect("响应");
        assert_eq!(result["ok"], true);
        peer.await.expect("peer");
    }

    #[tokio::test]
    async fn out_of_order_responses_still_route() {
        let (transport, mut reader, mut writer) = pair(Duration::from_secs(1));
        let peer = tokio::spawn(async move {
            let mut first = String::new();
            let mut second = String::new();
            reader.read_line(&mut first).await.expect("第一行");
            reader.read_line(&mut second).await.expect("第二行");
            let first: Value = serde_json::from_str(first.trim()).expect("first");
            let second: Value = serde_json::from_str(second.trim()).expect("second");
            write_json(
                &mut writer,
                json!({"jsonrpc": "2.0", "id": second["id"], "result": "second"}),
            )
            .await;
            write_json(
                &mut writer,
                json!({"jsonrpc": "2.0", "id": first["id"], "result": "first"}),
            )
            .await;
        });
        let (a, b) = tokio::join!(
            transport.request_with_timeout("a", json!({}), Duration::from_secs(1)),
            transport.request_with_timeout("b", json!({}), Duration::from_secs(1)),
        );
        assert_eq!(a.expect("a"), json!("first"));
        assert_eq!(b.expect("b"), json!("second"));
        peer.await.expect("peer");
    }

    #[tokio::test]
    async fn timeout_fails_and_marks_request_done() {
        let (transport, _reader, _writer) = pair(Duration::from_millis(30));
        let result = transport
            .request_with_timeout("slow", json!({}), Duration::from_millis(30))
            .await;
        assert!(matches!(result, Err(McpError::Timeout(_))));
    }

    #[tokio::test]
    async fn eof_fails_pending_and_marks_disconnected() {
        let (transport, reader, writer) = pair(Duration::from_secs(1));
        drop(reader);
        drop(writer);
        for _ in 0..100 {
            if !transport.is_connected() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(!transport.is_connected(), "对端关闭后应标记断开");
        let result = transport
            .request_with_timeout("ping", json!({}), Duration::from_secs(1))
            .await;
        assert!(matches!(result, Err(McpError::Transport(_))));
    }

    #[tokio::test]
    async fn server_ping_is_answered_and_unknown_method_is_rejected() {
        let (transport, mut reader, mut writer) = pair(Duration::from_secs(1));
        let peer = tokio::spawn(async move {
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("读请求");
            let request: Value = serde_json::from_str(line.trim()).expect("请求");
            write_json(
                &mut writer,
                json!({"jsonrpc": "2.0", "id": 700, "method": "ping"}),
            )
            .await;
            let mut ping = String::new();
            reader.read_line(&mut ping).await.expect("ping 响应");
            let ping: Value = serde_json::from_str(ping.trim()).expect("ping");
            assert_eq!(ping["result"], json!({}));
            write_json(
                &mut writer,
                json!({"jsonrpc": "2.0", "id": 701, "method": "unknown/server"}),
            )
            .await;
            let mut unknown = String::new();
            reader.read_line(&mut unknown).await.expect("未知响应");
            let unknown: Value = serde_json::from_str(unknown.trim()).expect("unknown");
            assert_eq!(unknown["error"]["code"], -32601);
            write_json(
                &mut writer,
                json!({"jsonrpc": "2.0", "id": request["id"], "result": "done"}),
            )
            .await;
        });
        let result = transport
            .request_with_timeout("call", json!({}), Duration::from_secs(1))
            .await
            .expect("最终响应");
        assert_eq!(result, json!("done"));
        peer.await.expect("peer");
    }

    #[cfg(windows)]
    #[test]
    fn windows_resolves_cmd_via_pathext() {
        let temp = tempfile::tempdir().expect("临时目录");
        let path = temp.path().join("npx.cmd");
        std::fs::write(&path, "").expect("写 cmd");
        let found = find_in_path_with("npx", temp.path().as_os_str(), ".CMD").expect("应找到");
        assert!(found.is_file(), "应解析到真实 cmd 文件");
        assert_eq!(
            found
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_lowercase),
            Some("npx.cmd".to_string())
        );
        assert_eq!(resolve_program("npx.cmd"), PathBuf::from("npx.cmd"));
    }

    #[tokio::test]
    async fn string_id_server_request_gets_matching_response() {
        let (transport, mut reader, mut writer) = pair(Duration::from_secs(1));
        let peer = tokio::spawn(async move {
            let mut request = String::new();
            reader.read_line(&mut request).await.expect("读请求");
            let request: Value = serde_json::from_str(request.trim()).expect("请求");
            write_json(
                &mut writer,
                json!({"jsonrpc": "2.0", "id": "abc", "method": "ping"}),
            )
            .await;
            let mut ping = String::new();
            reader.read_line(&mut ping).await.expect("ping 响应");
            let ping: Value = serde_json::from_str(ping.trim()).expect("ping");
            assert_eq!(ping["id"], "abc");
            write_json(
                &mut writer,
                json!({"jsonrpc": "2.0", "id": request["id"], "result": "done"}),
            )
            .await;
        });
        let result = transport
            .request_with_timeout("call", json!({}), Duration::from_secs(1))
            .await
            .expect("最终响应");
        assert_eq!(result, json!("done"));
        peer.await.expect("peer");
    }

    #[tokio::test]
    async fn oversized_line_is_rejected_without_unbounded_read() {
        let (mut client, mut peer) = tokio::io::duplex(1024);
        peer.write_all(&vec![b'x'; 64]).await.expect("写大行");
        let mut reader = BufReader::new(&mut client);
        let error = read_line_capped(&mut reader, 16).await.expect_err("应拒绝");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn cancellation_sends_notification_and_returns_cancelled() {
        let (transport, mut reader, mut writer) = pair(Duration::from_secs(1));
        let peer = tokio::spawn(async move {
            let mut request = String::new();
            reader.read_line(&mut request).await.expect("请求");
            let mut cancelled = String::new();
            reader.read_line(&mut cancelled).await.expect("取消通知");
            let cancelled: Value = serde_json::from_str(cancelled.trim()).expect("取消");
            assert_eq!(cancelled["method"], "notifications/cancelled");
            write_json(
                &mut writer,
                json!({"jsonrpc": "2.0", "id": cancelled["params"]["requestId"], "result": "late"}),
            )
            .await;
        });
        let token = CancellationToken::new();
        let cancel = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            cancel.cancel();
        });
        let result = transport
            .request_cancellable("call", json!({}), Duration::from_secs(1), &token)
            .await;
        assert!(matches!(result, Err(McpError::Cancelled)));
        peer.await.expect("peer");
    }
}
