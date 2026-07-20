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
#[derive(Message)]
pub struct DoneMessage;

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
