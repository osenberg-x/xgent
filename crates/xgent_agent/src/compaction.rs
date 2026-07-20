//! 对话压缩（compaction）。
//!
//! 长对话 token 接近上下文窗口时，把较早的消息段摘要为一段 summary 文本，
//! 保留最近若干轮消息，降低后续请求输入体积。借鉴 omp `compaction.ts`。
//!
//! 触发：`should_compact(context_tokens, context_window, settings)`——
//! `context_tokens > context_window - reserve`，reserve = max(15% window, 16384)。
//!
//! 算法：`find_cut_point` 从尾部累积 token，超过 `keep_recent_tokens` 时
//! 回退到最近合法 cut point（user/assistant 消息边界，保证 turn 完整）。
//! `LlmCompactor` 调 provider 生成摘要，替换被压缩段为单条 summary user 消息。
//!
//! provider 上报的 `TokenUsage.prompt` 是触发依据；本地 `estimate_messages_tokens`
//! 作为 floor（防止 provider 报告被压缩扩展 deflate 后漏触发，对齐 omp
//! `compactionContextTokens`）。

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;
use xgent_core::chat::{
    AgentMessage, ChatEvent, ChatMessage, ChatRequest, ContentBlock, ErrorKind, Role,
};

use crate::bridge::ProviderClient;
use crate::tokenizer::estimate_messages_tokens;

/// 默认 reserve token（对齐 omp `DEFAULT_RESERVE_TOKENS`）。
pub const DEFAULT_RESERVE_TOKENS: u32 = 16384;

/// 默认 reserve 占上下文窗口比例。
const DEFAULT_RESERVE_RATIO: f64 = 0.15;

/// 默认 compaction 触发阈值百分比（占窗口）。
const DEFAULT_THRESHOLD_PERCENT: u8 = 80;

/// 压缩结果：摘要文本 + 保留的关键消息。
#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// 对被压缩消息段的摘要说明。
    pub summary: String,
    /// 显式保留（不压缩）的消息，通常为最近若干轮或含未完成工具调用的消息。
    pub kept_messages: Vec<AgentMessage>,
    /// 压缩前对话 token 估算（审计用）。
    pub tokens_before: u32,
}

/// 压缩失败错误。
#[derive(Debug, thiserror::Error)]
pub enum CompactionError {
    /// 压缩过程失败，附带上游原因描述。
    #[error("{0}")]
    Failed(String),
    /// provider 调用失败。
    #[error("provider 错误: {kind:?} - {message}")]
    Provider { kind: ErrorKind, message: String },
    /// 摘要为空（provider 未返回有效文本）。
    #[error("摘要生成返回空文本")]
    EmptySummary,
}

/// 压缩配置。
///
/// - `enabled`：是否启用 compaction（关闭则 `should_compact` 恒 false）。
/// - `reserve_tokens`：预留 token 数（None 用 `max(15% window, DEFAULT_RESERVE_TOKENS)`）。
/// - `threshold_percent`：触发阈值占窗口百分比（None 用 80%）。
#[derive(Debug, Clone)]
pub struct CompactionSettings {
    pub enabled: bool,
    pub reserve_tokens: Option<u32>,
    pub threshold_percent: Option<u8>,
}

impl Default for CompactionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            reserve_tokens: None,
            threshold_percent: None,
        }
    }
}

/// 有效 reserve：`max(15% window, 配置或默认)`。
pub fn effective_reserve_tokens(context_window: u32, settings: &CompactionSettings) -> u32 {
    let proportional = (context_window as f64 * DEFAULT_RESERVE_RATIO).floor() as u32;
    let configured = settings.reserve_tokens.unwrap_or(DEFAULT_RESERVE_TOKENS);
    proportional.max(configured)
}

/// 解析触发阈值 token 数。
///
/// 优先用百分比（默认 80%），确保阈值严格小于窗口（`min(window - 1, ...)`）。
pub fn resolve_threshold_tokens(context_window: u32, settings: &CompactionSettings) -> u32 {
    let pct = settings
        .threshold_percent
        .unwrap_or(DEFAULT_THRESHOLD_PERCENT);
    let clamped = pct.min(99).max(1) as u32;
    // 阈值 = window * pct/100，但不超过 window - reserve
    let by_percent = context_window * clamped / 100;
    let by_reserve =
        context_window.saturating_sub(effective_reserve_tokens(context_window, settings));
    by_percent
        .min(by_reserve)
        .min(context_window.saturating_sub(1))
}

/// 判断是否需要压缩。
///
/// `context_tokens` 取 `max(provider 报告, 本地估算)`（见模块文档）。
pub fn should_compact(
    context_tokens: u32,
    context_window: u32,
    settings: &CompactionSettings,
) -> bool {
    if !settings.enabled || context_window == 0 {
        return false;
    }
    context_tokens > resolve_threshold_tokens(context_window, settings)
}

/// 综合触发 token：`max(provider 报告, 本地估算)`。
pub fn compaction_context_tokens(
    provider_context_tokens: u32,
    stored_conversation_estimate: u32,
) -> u32 {
    provider_context_tokens.max(stored_conversation_estimate)
}

/// 找到压缩 cut point：保留最近 `keep_recent_tokens` token 对应的消息起始索引。
///
/// 从尾部累积 token，超过预算时回退到「累积超标处」的下一条 user/assistant
/// 消息边界（保证 turn 完整，不切断工具调用配对）。
///
/// 返回值是 `kept_messages` 的起始索引；全量未超标时返回 0（不压缩）。
pub fn find_cut_point(messages: &[AgentMessage], keep_recent_tokens: u32) -> usize {
    if messages.is_empty() {
        return 0;
    }
    let mut accumulated: u32 = 0;
    let mut overflow_idx: Option<usize> = None;

    // 从尾部向前累积，找到第一个超过预算的位置
    for (i, msg) in messages.iter().enumerate().rev() {
        let tokens = crate::tokenizer::estimate_message_tokens(msg);
        accumulated = accumulated.saturating_add(tokens);
        if accumulated >= keep_recent_tokens {
            overflow_idx = Some(i);
            break;
        }
    }

    let Some(overflow) = overflow_idx else {
        // 全量未超标
        return 0;
    };

    // 从 overflow 处向后找最近的 user/assistant 边界（turn 起点）
    // 跳过 ToolResult（不能从工具结果中间切，会破坏 tool_use/tool_result 配对）
    for i in overflow..messages.len() {
        match &messages[i] {
            AgentMessage::User(_) | AgentMessage::Assistant(_) => return i,
            _ => continue,
        }
    }
    // 找不到边界（尾部全是 tool result）——保守不切
    0
}

/// 摘要专用 system prompt。
const SUMMARIZATION_SYSTEM_PROMPT: &str = r#"你是对话摘要助手。把给定的对话历史压缩为简洁的摘要，保留：
1. 用户的核心需求与已确认的决策
2. 已完成的工具调用及其关键结果（文件路径、命令输出要点）
3. 未解决的问题与下一步计划
丢弃寒暄、重复内容与冗余的工具输出细节。用中文，不超过 500 字。"#;

/// LLM 摘要 compactor：调 provider 生成摘要。
///
/// 复用 agent loop 的 `ProviderClient`（与对话同 provider/model），
/// 构造一次性 chat 请求，消费流式响应聚合文本。
pub struct LlmCompactor {
    provider: Arc<dyn ProviderClient>,
    provider_id: String,
    model: String,
}

impl LlmCompactor {
    pub fn new(provider: Arc<dyn ProviderClient>, provider_id: String, model: String) -> Self {
        Self {
            provider,
            provider_id,
            model,
        }
    }

    /// 调 provider 生成摘要文本。
    ///
    /// 把 `to_summarize` 转为 LLM 消息，加摘要 system prompt，
    /// 消费流式响应聚合为最终文本。
    async fn generate_summary(
        &self,
        to_summarize: &[AgentMessage],
        cancel_token: &tokio_util::sync::CancellationToken,
    ) -> Result<String, CompactionError> {
        let mut messages = convert_to_llm_messages(to_summarize);
        messages.insert(
            0,
            ChatMessage::text(Role::System, SUMMARIZATION_SYSTEM_PROMPT),
        );
        messages.push(ChatMessage::text(
            Role::User,
            "请把上述对话历史压缩为摘要。",
        ));

        let req = ChatRequest {
            provider: self.provider_id.clone(),
            model: self.model.clone(),
            messages,
            tools: None,
        };

        let (_sid, mut stream) = self
            .provider
            .chat(req)
            .await
            .map_err(|(kind, message)| CompactionError::Provider { kind, message })?;

        let mut summary = String::new();
        loop {
            tokio::select! {
                ev = stream.recv() => {
                    match ev {
                        Some(ChatEvent::TextDelta { text }) => summary.push_str(&text),
                        Some(ChatEvent::Done { .. }) => break,
                        Some(ChatEvent::Error { kind, message }) => {
                            return Err(CompactionError::Provider { kind, message });
                        }
                        Some(_) => {}
                        None => break,
                    }
                }
                _ = cancel_token.cancelled() => {
                    return Err(CompactionError::Failed("压缩被中断".into()));
                }
            }
        }

        let trimmed = summary.trim();
        if trimmed.is_empty() {
            return Err(CompactionError::EmptySummary);
        }
        Ok(trimmed.to_string())
    }
}

#[async_trait]
impl CompactionProvider for LlmCompactor {
    fn should_compact(&self, messages: &[AgentMessage], model: &str) -> bool {
        // LlmCompactor 不自带 context_window 知识，由 agent loop 用模块级
        // `should_compact` + provider 上报 usage 判断。此方法保留 trait 兼容，
        // 用本地估算 + 默认 128k 窗口作粗判。
        let context_window = 128_000;
        let estimate = estimate_messages_tokens(messages);
        let settings = CompactionSettings::default();
        let _ = model;
        crate::compaction::should_compact(estimate, context_window, &settings)
    }

    async fn compact(
        &self,
        messages: &[AgentMessage],
        _model: &str,
    ) -> Result<CompactionResult, CompactionError> {
        let cancel_token = tokio_util::sync::CancellationToken::new();
        self.compact_with_cancel(messages, &cancel_token).await
    }
}

impl LlmCompactor {
    /// 带中断 token 的压缩（agent loop 用，abort 时取消）。
    pub async fn compact_with_cancel(
        &self,
        messages: &[AgentMessage],
        cancel_token: &tokio_util::sync::CancellationToken,
    ) -> Result<CompactionResult, CompactionError> {
        let tokens_before = estimate_messages_tokens(messages);

        // 保留最近 25% token（对齐 omp keepRecentTokens 语义）
        let keep_recent = tokens_before / 4;
        let cut = find_cut_point(messages, keep_recent);

        if cut == 0 {
            // 全量未超标或找不到切点——不压缩
            return Ok(CompactionResult {
                summary: String::new(),
                kept_messages: messages.to_vec(),
                tokens_before,
            });
        }

        let to_summarize = &messages[..cut];
        let kept = messages[cut..].to_vec();

        let summary = self.generate_summary(to_summarize, cancel_token).await?;

        Ok(CompactionResult {
            summary,
            kept_messages: kept,
            tokens_before,
        })
    }
}

/// 把 AgentMessage 转为 LLM ChatMessage（复用 xgent_core::convert_to_llm 语义，
/// 但此处内联以避免 Notification 等被过滤后丢失上下文标记）。
fn convert_to_llm_messages(messages: &[AgentMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .filter_map(|msg| match msg {
            AgentMessage::User(m) => Some(ChatMessage {
                role: Role::User,
                content: m.content.clone(),
            }),
            AgentMessage::Assistant(m) => Some(ChatMessage {
                role: Role::Assistant,
                content: m.content.clone(),
            }),
            AgentMessage::ToolResult(m) => Some(ChatMessage {
                role: Role::Tool,
                content: vec![ContentBlock::ToolResult {
                    tool_call_id: m.tool_call_id.clone(),
                    content: m.content.clone(),
                    is_error: m.is_error,
                }],
            }),
            AgentMessage::Notification(_) => None,
        })
        .collect()
}

/// 对话压缩 provider。
///
/// 职责：判断当前会话是否需要压缩，以及在需要时将消息段压缩为
/// [`CompactionResult`]。实现可为本地启发式策略或调用 LLM 摘要。
///
/// 实现需 `Send + Sync` 以便跨 tokio 任务与 Bevy 系统共享。
#[async_trait]
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

/// 把压缩结果应用为新的消息序列：summary 作为首条 user 消息 + kept_messages。
///
/// summary 非空时包装为 `[summary context]` 标记的 user 消息，让模型知道
/// 前文已被压缩。summary 为空时直接返回 kept（即未压缩）。
pub fn apply_compaction(result: CompactionResult) -> Vec<AgentMessage> {
    if result.summary.is_empty() {
        return result.kept_messages;
    }
    let mut out = Vec::with_capacity(result.kept_messages.len() + 1);
    out.push(AgentMessage::User(xgent_core::chat::UserMessage {
        content: vec![ContentBlock::Text {
            text: format!("[前序对话摘要]\n{}", result.summary),
        }],
        timestamp: 0,
    }));
    out.extend(result.kept_messages);
    out
}

// 静默未使用导入警告（mpsc 在 generate_summary 未直接用，保留以备扩展）
#[allow(unused_imports)]
use mpsc as _mpsc;

#[cfg(test)]
mod tests {
    use super::*;
    use xgent_core::chat::{AssistantMessage, ContentBlock, UserMessage};

    fn user_text(text: &str) -> AgentMessage {
        AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text { text: text.into() }],
            timestamp: 0,
        })
    }

    fn assistant_text(text: &str) -> AgentMessage {
        AgentMessage::Assistant(AssistantMessage {
            content: vec![ContentBlock::Text { text: text.into() }],
            model: None,
            usage: None,
            timestamp: 0,
        })
    }

    #[test]
    fn should_compact_below_threshold_false() {
        let settings = CompactionSettings::default();
        // 窗口 100k, 用 50k → 阈值 80k → 不触发
        assert!(!should_compact(50_000, 100_000, &settings));
    }

    #[test]
    fn should_compact_above_threshold_true() {
        let settings = CompactionSettings::default();
        // 窗口 100k, 用 85k → 阈值 80k → 触发
        assert!(should_compact(85_000, 100_000, &settings));
    }

    #[test]
    fn should_compact_disabled_never() {
        let settings = CompactionSettings {
            enabled: false,
            ..Default::default()
        };
        assert!(!should_compact(99_999, 100_000, &settings));
    }

    #[test]
    fn should_compact_floor_by_local_estimate() {
        // provider 报告被 deflate 到 10k，本地估算 90k → 应触发
        let ctx = compaction_context_tokens(10_000, 90_000);
        let settings = CompactionSettings::default();
        assert!(should_compact(ctx, 100_000, &settings));
    }

    #[test]
    fn find_cut_point_empty() {
        assert_eq!(find_cut_point(&[], 1000), 0);
    }

    #[test]
    fn find_cut_point_under_budget_no_cut() {
        // 3 条小消息，预算 10000 → 全量未超标，返回 0
        let msgs = vec![user_text("a"), assistant_text("b"), user_text("c")];
        assert_eq!(find_cut_point(&msgs, 10_000), 0);
    }

    #[test]
    fn find_cut_point_cuts_at_user_boundary() {
        // 构造：user(大) + assistant(小) + user(小) + assistant(小)
        // 预算很小，应从最后一个 user 边界切
        let big = "x".repeat(200);
        let msgs = vec![
            user_text(&big),
            assistant_text("ok"),
            user_text("hi"),
            assistant_text("there"),
        ];
        let cut = find_cut_point(&msgs, 5);
        // cut 不应为 0（需要压缩），且应落在 user 边界
        assert!(cut > 0);
        assert!(matches!(
            &msgs[cut],
            AgentMessage::User(_) | AgentMessage::Assistant(_)
        ));
    }

    #[test]
    fn find_cut_point_never_cuts_mid_toolresult() {
        // 尾部是 ToolResult 时不应从中间切
        let msgs = vec![
            user_text("do something"),
            AgentMessage::ToolResult(xgent_core::chat::ToolResultMessage {
                tool_call_id: "1".into(),
                tool_name: "bash".into(),
                content: "output".into(),
                is_error: false,
                timestamp: 0,
            }),
        ];
        let cut = find_cut_point(&msgs, 1);
        // 找不到 user/assistant 边界 → 返回 0（不切）
        assert_eq!(cut, 0);
    }

    #[test]
    fn apply_compaction_empty_summary_returns_kept() {
        let kept = vec![user_text("a")];
        let result = CompactionResult {
            summary: String::new(),
            kept_messages: kept.clone(),
            tokens_before: 100,
        };
        assert_eq!(apply_compaction(result), kept);
    }

    #[test]
    fn apply_compaction_prepends_summary() {
        let kept = vec![user_text("a")];
        let result = CompactionResult {
            summary: "已完成 X".into(),
            kept_messages: kept,
            tokens_before: 100,
        };
        let out = apply_compaction(result);
        assert_eq!(out.len(), 2);
        // 首条应为 summary user 消息
        match &out[0] {
            AgentMessage::User(u) => match &u.content[0] {
                ContentBlock::Text { text } => assert!(text.contains("已完成 X")),
                _ => panic!("expected text block"),
            },
            _ => panic!("expected user message"),
        }
    }

    #[test]
    fn resolve_threshold_respects_reserve() {
        // 窗口 100k, 默认 reserve = max(15k, 16384) = 16384
        // 默认阈值 80% = 80k, 但 window-reserve = 83616 → 取 min = 80k
        let settings = CompactionSettings::default();
        let t = resolve_threshold_tokens(100_000, &settings);
        assert_eq!(t, 80_000);
    }

    #[test]
    fn resolve_threshold_clamped_below_window() {
        let settings = CompactionSettings {
            threshold_percent: Some(99),
            ..Default::default()
        };
        let t = resolve_threshold_tokens(100_000, &settings);
        // 99% = 99000, window-reserve = 83616 → min = 83616
        assert!(t < 100_000);
        assert_eq!(t, 83_616);
    }
}
