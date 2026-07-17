//! 对话压缩（compaction）预留 trait。
//!
//! 长对话在 token 接近上下文窗口时，由 provider 将历史消息摘要为一段
//! `summary`，并保留少量关键消息（`kept_messages`），以降低后续请求的输入体积。
//!
//! 本模块仅声明 [`CompactionProvider`] trait 与相关类型，具体策略（基于 token
//! 计数、LLM 摘要、滑动窗口等）留待后续实现，P2 阶段不接入 Agent Loop。

use xgent_core::chat::AgentMessage;

/// 压缩结果：摘要文本 + 保留的关键消息。
#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// 对被压缩消息段的摘要说明。
    pub summary: String,
    /// 显式保留（不压缩）的消息，通常为最近若干轮或含未完成工具调用的消息。
    pub kept_messages: Vec<AgentMessage>,
}

/// 压缩失败错误。
#[derive(Debug, thiserror::Error)]
pub enum CompactionError {
    /// 压缩过程失败，附带上游原因描述。
    #[error("{0}")]
    Failed(String),
}

/// 对话压缩 provider。
///
/// 职责：判断当前会话是否需要压缩，以及在需要时将消息段压缩为
/// [`CompactionResult`]。实现可为本地启发式策略或调用 LLM 摘要。
///
/// 实现需 `Send + Sync` 以便跨 tokio 任务与 Bevy 系统共享。
pub trait CompactionProvider: Send + Sync {
    /// 判断给定消息序列在指定模型下是否需要压缩。
    ///
    /// `model` 用于按模型的上下文窗口与计价策略决策。
    fn should_compact(&self, messages: &[AgentMessage], model: &str) -> bool;

    /// 执行压缩，返回摘要与保留消息。
    ///
    /// `messages` 为当前完整消息序列，`model` 为目标模型名。
    async fn compact(
        &self,
        messages: &[AgentMessage],
        model: &str,
    ) -> Result<CompactionResult, CompactionError>;
}
