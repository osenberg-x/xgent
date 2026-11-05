//! MCP（Model Context Protocol）客户端：stdio 传输、协议、工具适配与生命周期。
//!
//! 首期只支持 MCP `2025-06-18` 的 stdio + tools；HTTP/SSE、resources/prompts、
//! 2026-07-28 无状态规范留后续。MCP 工具经 [`crate::Tool`] 注册进现有执行器，
//! 安全策略与确认流程保持不变。

pub mod client;
pub mod manager;
pub mod tool;
pub mod transport;

use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

pub use client::McpClient;
pub use manager::McpManager;
pub use tool::McpTool;
pub use transport::StdioTransport;

/// MCP 传输层抽象。
///
/// 抽象与 MCP 服务端之间的通信通道，屏蔽底层传输差异（stdio / SSE / WebSocket 等）。
/// 实现方需保证 `Send + Sync`，以支撑在 Bevy 异步桥接中跨任务共享。
///
/// # 方法语义
/// - [`request`](Self::request)：发起 JSON-RPC 请求，等待服务端响应并返回结果。
/// - [`notify`](Self::notify)：发送通知（无需响应），失败仅记录于日志。
/// - [`close`](Self::close)：关闭传输通道，释放底层资源。
/// - [`is_connected`](Self::is_connected)：查询当前连接状态。
///
/// 该 trait 当前为 P2 预留，尚无实现。
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// 发起 JSON-RPC 请求。
    ///
    /// `method` 为 RPC 方法名，`params` 为参数（JSON 值）。
    /// 返回服务端响应结果，失败时返回 [`McpError`]。
    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError>;

    /// 带指定超时的请求；默认实现忽略超时，由具体传输决定。
    async fn request_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, McpError> {
        let _ = timeout;
        self.request(method, params).await
    }

    /// 可取消请求；默认实现忽略取消，由具体传输实现取消通知。
    async fn request_cancellable(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Value, McpError> {
        let _ = cancel;
        self.request_with_timeout(method, params, timeout).await
    }

    /// 发送通知（无响应）。
    ///
    /// 通知失败时仅记录日志，不向上层传播错误。
    async fn notify(&self, method: &str, params: Value);

    /// 关闭传输通道，释放底层资源。
    async fn close(&self);

    /// 查询当前连接状态。
    ///
    /// 返回 `true` 表示通道可用，`false` 表示已断开或未建立。
    fn is_connected(&self) -> bool;
}

/// MCP 通信错误类型。
///
/// 区分底层传输错误与协议层错误，便于上层（如 Tool 包装层）做差异化处理。
#[derive(Debug, Clone, thiserror::Error)]
pub enum McpError {
    /// 底层传输错误：连接中断、IO 失败、超时等。
    #[error("{0}")]
    Transport(String),

    /// 协议层错误：JSON-RPC 格式错误、方法未找到、参数不合法等。
    #[error("{0}")]
    Protocol(String),

    /// 子进程启动失败。
    #[error("{0}")]
    Spawn(String),

    /// 请求超时（秒）。
    #[error("请求超时（{0}s）")]
    Timeout(u64),

    /// 调用方取消。
    #[error("请求已取消")]
    Cancelled,
}
