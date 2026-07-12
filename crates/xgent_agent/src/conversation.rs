//! 会话状态 Resource。

use bevy::prelude::*;
use xgent_core::chat::{ChatMessage, Role};
use xgent_core::ids::SessionId;

/// 会话状态。
#[derive(Resource, Debug)]
pub struct Conversation {
    /// 会话 id
    pub id: SessionId,
    /// 消息历史
    pub messages: Vec<ChatMessage>,
    /// 当前状态
    pub status: ConversationStatus,
    /// 流式累加中的助手回复
    pub current_assistant_text: String,
}

impl Default for Conversation {
    fn default() -> Self {
        Self {
            id: SessionId(1),
            messages: Vec::new(),
            status: ConversationStatus::Idle,
            current_assistant_text: String::new(),
        }
    }
}

impl Conversation {
    /// 追加用户消息。
    pub fn push_user(&mut self, text: &str) {
        self.messages.push(ChatMessage {
            role: Role::User,
            content: text.to_string(),
        });
    }

    /// 把累加的助手回复固化进历史。
    pub fn finalize_assistant(&mut self) {
        if !self.current_assistant_text.is_empty() {
            let text = std::mem::take(&mut self.current_assistant_text);
            self.messages.push(ChatMessage {
                role: Role::Assistant,
                content: text,
            });
        }
    }
}

/// 对话状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConversationStatus {
    /// 等待用户输入
    #[default]
    Idle,
    /// 等待 provider 响应
    Thinking,
    /// 接收流式 delta
    Streaming,
    /// 执行工具中
    ToolRunning,
    /// 等待用户确认
    Confirming,
    /// 中断中
    Aborting,
    /// 出错
    Error,
}
