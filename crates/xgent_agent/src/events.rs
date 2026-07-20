//! agent 与 UI 之间的 ECS 消息契约（缓冲队列）。
//!
//! 所有 agent 与 UI 通信只通过 Message（缓冲事件）通信，禁止直接方法调用
//! （架构硬约束 5.2：Messages 是缓冲队列，Events 是即时通知）。
//!
//! 这些消息作为缓冲队列，由系统每帧消费。

use bevy::prelude::*;
use xgent_tools::confirm::{ConfirmDecision, ConfirmRequest};

/// 用户输入消息（UI → agent）。
#[derive(Message)]
pub struct UserInputMessage {
    pub text: String,
}

/// 中断当前对话（UI → agent）。
#[derive(Message)]
pub struct AbortMessage;

/// 新建会话（UI → agent）：清空当前会话，开始新会话。
///
/// 重置 Conversation（新 SessionId、清空 messages、新 session_store），
/// 并发 `SessionClearedMessage` 让 UI 清空消息列表。
/// 仅 Idle/Error 状态接受（忙碌时忽略，避免丢失进行中的对话）。
#[derive(Message)]
pub struct NewSessionMessage;

/// Steering 消息：用户在 agent 执行中插话（UI → agent，注入到当前对话）。
#[derive(Message)]
pub struct SteeringMessage {
    pub text: String,
}

/// Follow-up 消息：agent 停止后注入后续消息继续对话（UI → agent）。
#[derive(Message)]
pub struct FollowUpMessage {
    pub text: String,
}

/// provider 流式 delta（agent → UI）。
#[derive(Message)]
pub struct DeltaMessage {
    pub text: String,
}

/// 工具调用开始（agent → UI，展示工具执行中）。
#[derive(Message)]
pub struct ToolCallMessage {
    pub tool_id: String,
    pub input: serde_json::Value,
}

/// 工具执行完成（agent → UI）。
#[derive(Message)]
pub struct ToolResultMessage {
    pub tool_id: String,
    pub output: String,
    /// 是否为逻辑失败（语义反转：true 表示失败）
    pub is_error: bool,
}

/// 需要用户确认（agent → UI，触发弹窗）。
#[derive(Message)]
pub struct ConfirmRequestMessage(pub ConfirmRequest);

/// 用户确认决策（UI → agent）。
#[derive(Message)]
pub struct ConfirmDecisionMessage {
    pub decision: ConfirmDecision,
}

/// 对话完成（agent → UI）。
///
/// `usage` 与 `model` 来自 provider 的流式 Done 事件，供 UI 累加真实 token
/// 用量（修复之前读空 `current_assistant_text` 导致 token 永远为 0 的 bug）。
#[derive(Message)]
pub struct DoneMessage {
    /// 本次 stream 的 token 用量（来自 provider）
    pub usage: Option<xgent_core::chat::TokenUsage>,
    /// 生成该回复的模型名
    pub model: Option<String>,
}

/// 即将重试（agent → UI）。
///
/// agent loop 因可重试错误触发自动重试前发射。
/// UI 据此清空当前半截助手文本并展示"重试中(第 N 次)"。
#[derive(Message)]
pub struct RetryMessage {
    /// 即将进行的重试序号（1-based）
    pub attempt: u32,
    /// 是否为无限重试模式
    pub infinite: bool,
    /// 上次失败的错误类型
    pub kind: xgent_core::chat::ErrorKind,
    /// 上次失败的错误消息
    pub last_error: String,
}

/// 对话出错（agent → UI）。
#[derive(Message)]
pub struct ErrorMessage {
    pub kind: xgent_core::chat::ErrorKind,
    pub message: String,
}

/// 对话已压缩（agent → UI）。
///
/// compaction 触发后发射。UI 据此提示用户「前序对话已摘要」。
#[derive(Message)]
pub struct CompactedMessage {
    /// 压缩前 token 估算
    pub tokens_before: u32,
    /// 压缩后保留消息 token 估算
    pub tokens_after: u32,
}

/// 会话已清空（agent → UI）：新建会话后通知 UI 清空消息列表。
#[derive(Message)]
pub struct SessionClearedMessage;
