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

/// 对话出错（agent → UI）。
#[derive(Message)]
pub struct ErrorMessage {
    pub kind: xgent_core::chat::ErrorKind,
    pub message: String,
}
