//! 工具抽象 trait 与相关类型。
//!
//! 定义统一的工具接口（id、schema、tier、approval、并发声明、异步可中断执行、
//! 人类可读摘要）。工具是纯异步逻辑，不依赖 Bevy；Bevy 桥接放 xgent_agent。
//!
//! 设计对齐 ADR-0007：删除旧 `fn policy() -> SecurityPolicy`，改为
//! `tier()` + `approval_for(input)` + `concurrency()`，`execute` 签名加入
//! `CancellationToken` 与 `on_update` 回调，返回 `Result<ToolResult, ToolError>`。

use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use thiserror::Error;
use xgent_core::chat::ToolSchema;
use xgent_settings_core::project::ToolPolicyConfig;

/// 工具执行上下文：项目根、工具策略配置等。
#[derive(Debug, Clone)]
pub struct ToolCtx {
    /// 项目根目录（绝对路径）
    pub project_root: PathBuf,
    /// 工具策略配置（approved / denied 覆盖）
    pub tool_policy: ToolPolicyConfig,
}

/// 工具执行结果。
///
/// `is_error` 取代旧 `success`（语义反转，对齐 omp 的 isError）：
/// 逻辑失败（如文件不存在）用 `is_error: true` 表达，不抛 `ToolError`。
/// `ToolError::Aborted` 仅用于中断等需 agent loop 走 abort 路径的异常。
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// 给 LLM 的文本结果
    pub output: String,
    /// 是否为逻辑失败（语义反转：true 表示失败）
    pub is_error: bool,
    /// 副作用通知（用于多客户端文件状态同步）
    pub side_effect: Option<SideEffect>,
}

/// 工具副作用。
#[derive(Debug, Clone)]
pub enum SideEffect {
    /// 写入了文件（绝对路径）
    FileWritten(PathBuf),
    /// 运行了命令
    CommandRun(String),
}

/// 工具并发声明。
///
/// - `Shared`：只读工具，可与其它 Shared 工具并行执行。
/// - `Exclusive`：写/执行工具，必须串行（等前序完成）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Concurrency {
    /// 共享并发（只读）
    Shared,
    /// 独占并发（写/执行）
    Exclusive,
}

/// 工具分层。
///
/// 用于 `resolve_policy` 推导运行时 [`SecurityPolicy`]。MVP 阶段
/// Read/Write/Exec 全映射为 `NeedsConfirmation`；P1 引入 ApprovalMode 后
/// Read 在 yolo 模式下可自动批准。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTier {
    /// 只读（如 read_file、search_files）
    Read,
    /// 写入（如 write_file）
    Write,
    /// 执行（如 run_command）
    Exec,
}

/// 工具执行错误。
///
/// `Aborted` 透传给 agent loop 走 abort 路径；`Failed`/`Timeout` 视为
/// 异常失败（MVP 下也回灌 LLM）。工具的逻辑失败（如文件不存在）不
/// 用 `ToolError`，而返回 `Ok(ToolResult { is_error: true, .. })`。
#[derive(Debug, Clone, Error)]
pub enum ToolError {
    /// 工具内部异常失败
    #[error("{0}")]
    Failed(String),
    /// 被中断（CancellationToken cancel 或子进程被 kill）
    #[error("aborted")]
    Aborted,
    /// 执行超时（参数为秒数）
    #[error("timeout after {0}s")]
    Timeout(u64),
}

/// 工具流式更新回调。
///
/// 长时工具（如 run_command 的 stdout 增量）通过此回调推送中间 `ToolResult`
/// 给调用方。MVP 阶段 executor 传 `None`。
pub type ToolUpdateCallback = Box<dyn Fn(ToolResult) + Send + Sync>;

/// 工具安全策略级别（运行时决议结果）。
///
/// 不再是 Tool trait 方法的返回值，而由 `resolve_policy` 综合
/// `ToolTier` + `approval_for(input)` + 配置推导得出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityPolicy {
    /// 自动执行
    Approved,
    /// 弹窗确认后执行
    NeedsConfirmation,
    /// 拒绝
    Denied,
}

/// 工具抽象 trait。
///
/// 对齐 ADR-0007：删除 `fn policy()`，新增 `tier()` / `approval_for()` /
/// `concurrency()`，`execute` 加入 `signal` 与 `on_update`，返回
/// `Result<ToolResult, ToolError>`。
#[async_trait]
pub trait Tool: Send + Sync {
    /// 工具 id，如 `"read_file"`
    fn id(&self) -> &str;

    /// 工具的 JSON schema（OpenAI function calling 格式）
    fn schema(&self) -> ToolSchema;

    /// 工具分层（静态默认）。
    fn tier(&self) -> ToolTier;

    /// 按输入动态决议分层（默认返回 `tier()`）。
    ///
    /// 工具可 override 以对特定危险输入返回更高 tier（如 run_command
    /// 检测 `rm -rf` 始终返回 Exec）。`resolve_policy` 用此结果推导策略。
    fn approval_for(&self, _input: &Value) -> ToolTier {
        self.tier()
    }

    /// 并发声明（默认 `Shared`）。
    fn concurrency(&self) -> Concurrency {
        Concurrency::Shared
    }

    /// 对输入生成人类可读摘要，用于确认弹窗展示。
    fn summarize(&self, input: &Value) -> String;

    /// 异步执行工具。
    ///
    /// - `signal`：中断信号，cancel 后工具应尽快返回 `ToolError::Aborted`。
    /// - `on_update`：流式更新回调（MVP 传 `None`）。
    /// - 逻辑失败返回 `Ok(ToolResult { is_error: true, .. })`；
    ///   中断/超时等异常返回 `Err(ToolError::...)`。
    async fn execute(
        &self,
        input: Value,
        ctx: &ToolCtx,
        signal: tokio_util::sync::CancellationToken,
        on_update: Option<&ToolUpdateCallback>,
    ) -> Result<ToolResult, ToolError>;
}
