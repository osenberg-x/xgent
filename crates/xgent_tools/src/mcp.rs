//! MCP（Model Context Protocol）传输层抽象。
//!
//! 本模块定义 [`McpTransport`] trait 与 [`McpError`] 错误类型，用于抽象外部 MCP 服务端
//! 的通信通道（stdio / SSE / WebSocket 等）。当前为 P2 预留，仅声明 trait，
//! 不提供具体实现；后续阶段将在此 trait 基础上接入具体传输。

use async_trait::async_trait;
use serde_json::Value;

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
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    /// 底层传输错误：连接中断、IO 失败、超时等。
    #[error("{0}")]
    Transport(String),

    /// 协议层错误：JSON-RPC 格式错误、方法未找到、参数不合法等。
    #[error("{0}")]
    Protocol(String),
}
