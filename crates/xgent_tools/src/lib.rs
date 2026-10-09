//! xgent_tools — Agent 可调用的工具体系。
//!
//! 提供工具抽象 [`Tool`] trait、内置工具（ReadFile/WriteFile/SearchFiles/RunCommand）、
//! 安全策略分级（Approved/NeedsConfirmation/Denied）与执行器 [`ToolExecutor`]。
//! P1 新增 [`EditorTool`]（UI-only Tier，agent 驱动编辑器动作，默认 Approved）。
//!
//! 不依赖 Bevy——工具是纯异步逻辑。Bevy 桥接放 xgent_agent。

pub mod atomic;
pub mod builtins;
pub mod confirm;
pub mod editor_tool;
pub mod executor;
pub mod mcp;
pub mod path;
pub mod security;
pub mod tool;

pub use builtins::{EditFile, ReadFile, RunCommand, SearchFiles, WriteFile};
pub use confirm::{ConfirmDecision, ConfirmRequest};
pub use editor_tool::{EditorCommandRequest, EditorCommandSink, EditorTool};
pub use executor::{ConfirmCallback, ToolExecutor, ToolExecutorResource};
pub use path::resolve_in_project;
pub use security::resolve_policy;
pub use tool::{
    Concurrency, SecurityPolicy, SideEffect, Tool, ToolCtx, ToolError, ToolResult, ToolTier,
    ToolUpdateCallback,
};

use std::sync::Arc;

/// 默认内置工具集合。
pub fn default_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(ReadFile),
        Arc::new(WriteFile),
        Arc::new(EditFile),
        Arc::new(SearchFiles),
        Arc::new(RunCommand),
    ]
}

/// 工具输出上限（字节）：所有读取类工具共享。
///
/// 单次工具调用不能突破此上限——超限即撑爆 LLM 上下文并触发连锁压缩。
/// 与 `run_command` 的输出上限同值，便于统一推理。
pub const MAX_TOOL_OUTPUT_BYTES: usize = 64 * 1024;

/// 把工具输出截断到 [`MAX_TOOL_OUTPUT_BYTES`] 内：保留开头与结尾，中间以
/// 截断标记衔接（结尾常是报错位置，比只留头部更有用）。
///
/// 进入截断分支的判据是**字节**数（`s.len()`），而头尾保留按**字符**数算：
/// CJK 输出 65537 字节仅约 2.2 万字符，可能少于保留额（32768 字符），
/// `total - keep` 会下溢（debug panic 会打死 agent 任务）——须双重判据。
pub fn truncate_output(s: &str) -> String {
    let keep = MAX_TOOL_OUTPUT_BYTES / 2;
    let total = s.chars().count();
    if s.len() <= MAX_TOOL_OUTPUT_BYTES || total <= keep {
        return s.to_string();
    }
    // 按字符边界截断，避免切在 UTF-8 中间
    let head: String = s.chars().take(keep).collect();
    let tail: String = s.chars().skip(total - keep).collect();
    format!("{head}\n[... 输出过长，中间部分已截断 ...]\n{tail}")
}
