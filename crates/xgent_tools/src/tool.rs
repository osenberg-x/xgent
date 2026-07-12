//! 工具抽象 trait 与相关类型。
//!
//! 定义统一的工具接口（id、schema、安全策略、异步执行、人类可读摘要）。
//! 工具是纯异步逻辑，不依赖 Bevy；Bevy 桥接放 xgent_agent。

use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
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
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// 给 LLM 的文本结果
    pub output: String,
    /// 是否成功
    pub success: bool,
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

/// 工具安全策略级别。
///
/// 参考成熟 code agent：信任可配置，默认所有工具（含只读）需确认。
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
#[async_trait]
pub trait Tool: Send + Sync {
    /// 工具 id，如 `"read_file"`
    fn id(&self) -> &str;

    /// 工具的 JSON schema（OpenAI function calling 格式）
    fn schema(&self) -> ToolSchema;

    /// 工具建议的默认安全策略。
    ///
    /// 内置工具均返回 [`SecurityPolicy::NeedsConfirmation`]，
    /// 用户可在配置中提升只读工具为 `Approved` 或降级危险工具为 `Denied`。
    fn policy(&self) -> SecurityPolicy {
        SecurityPolicy::NeedsConfirmation
    }

    /// 对输入生成人类可读摘要，用于确认弹窗展示。
    fn summarize(&self, input: &Value) -> String;

    /// 异步执行工具。
    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolResult;
}
