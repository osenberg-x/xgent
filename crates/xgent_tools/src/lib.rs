//! xgent_tools — Agent 可调用的工具体系。
//!
//! 提供工具抽象 [`Tool`] trait、内置工具（ReadFile/WriteFile/SearchFiles/RunCommand）、
//! 安全策略分级（Approved/NeedsConfirmation/Denied）与执行器 [`ToolExecutor`]。
//! P1 新增 [`EditorTool`]（UI-only Tier，agent 驱动编辑器动作，默认 Approved）。
//!
//! 不依赖 Bevy——工具是纯异步逻辑。Bevy 桥接放 xgent_agent。

pub mod atomic;
pub mod builtins;
pub mod checkpoint;
pub mod confirm;
pub mod editor_tool;
pub mod executor;
pub mod mcp;
pub mod path;
pub mod security;
pub mod tool;

/// glob 模式 → 正则的路径匹配（`search_files` 的 `glob` 参数用）。
pub(crate) mod glob;

pub use builtins::{EditFile, ReadFile, RunCommand, SearchFiles, WriteFile};
pub use checkpoint::{
    CheckpointEntry, CheckpointStore, SkipReason, TurnKey, TurnManifest, UndoReport, fnv1a64,
};
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

/// 内置工具的运行参数（来自项目配置 `[tools]`，R3-17）。
///
/// 缺省值即本次转配置前各处的硬编码常量，因此「不写配置」与「改前行为」等价。
/// 工具不直接读配置层，保持 `Tool` 契约与配置类型解耦。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolParams {
    /// `run_command` 最长执行秒数（原 `run_command::TIMEOUT_SECS`）。
    pub command_timeout_secs: u64,
    /// `run_command` 捕获输出上限字节（原 `MAX_TOOL_OUTPUT_BYTES`）。
    pub command_max_output_bytes: usize,
    /// 等待用户确认的秒数（原 `executor` 内 `Duration::from_secs(300)`）。
    pub confirm_timeout_secs: u64,
}

impl Default for ToolParams {
    fn default() -> Self {
        use xgent_settings_core::project::defaults as d;
        Self {
            command_timeout_secs: d::COMMAND_TIMEOUT_SECS,
            command_max_output_bytes: d::COMMAND_MAX_OUTPUT_BYTES,
            confirm_timeout_secs: d::CONFIRM_TIMEOUT_SECS,
        }
    }
}

impl ToolParams {
    /// 由项目配置的 `[tools]` 节派生（`0` 已解析为内置缺省）。
    pub fn from_tool_config(cfg: &xgent_settings_core::ToolConfig) -> Self {
        Self {
            command_timeout_secs: cfg.effective_command_timeout_secs(),
            command_max_output_bytes: cfg.effective_command_max_output_bytes(),
            confirm_timeout_secs: cfg.effective_confirm_timeout_secs(),
        }
    }
}

/// 默认内置工具集合。
pub fn default_tools(params: &ToolParams) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(ReadFile),
        Arc::new(WriteFile),
        Arc::new(EditFile),
        Arc::new(SearchFiles),
        Arc::new(RunCommand::with_params(params)),
    ]
}

/// 工具输出上限（字节）：所有读取类工具共享的天花板。
///
/// 单次工具调用不能突破此上限——超限即撑爆 LLM 上下文并触发连锁压缩。
/// `run_command` 的捕获上限可按项目配置 `[tools]` 收窄，缺省与此同值。
pub const MAX_TOOL_OUTPUT_BYTES: usize = 64 * 1024;

/// 把工具输出截断到 [`MAX_TOOL_OUTPUT_BYTES`] 内：见 [`truncate_output_to`]。
pub fn truncate_output(s: &str) -> String {
    truncate_output_to(s, MAX_TOOL_OUTPUT_BYTES)
}

/// 把工具输出截断到 `cap_bytes` 内：保留开头与结尾，中间以
/// 截断标记衔接（结尾常是报错位置，比只留头部更有用）。
///
/// 进入截断分支的判据是**字节**数（`s.len()`），而头尾保留按**字符**数算：
/// CJK 输出 65537 字节仅约 2.2 万字符，可能少于保留额（32768 字符），
/// `total - keep` 会下溢（debug panic 会打死 agent 任务）——须双重判据。
pub fn truncate_output_to(s: &str, cap_bytes: usize) -> String {
    let keep = cap_bytes / 2;
    let total = s.chars().count();
    if s.len() <= cap_bytes || total <= keep {
        return s.to_string();
    }
    // 按字符边界截断，避免切在 UTF-8 中间
    let head: String = s.chars().take(keep).collect();
    let tail: String = s.chars().skip(total - keep).collect();
    format!("{head}\n[... 输出过长，中间部分已截断 ...]\n{tail}")
}
