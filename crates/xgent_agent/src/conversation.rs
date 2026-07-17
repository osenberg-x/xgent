//! 会话状态 Resource。

use bevy::prelude::*;
use xgent_core::chat::{
    AgentMessage, AssistantMessage, ContentBlock, NotificationMessage, ToolResultMessage,
    UserMessage,
};
use xgent_core::ids::SessionId;

/// 会话状态。
#[derive(Resource, Debug)]
pub struct Conversation {
    /// 会话 id
    pub id: SessionId,
    /// 消息历史（agent 层 AgentMessage，调用 LLM 前经 convert_to_llm 转换）
    pub messages: Vec<AgentMessage>,
    /// 当前状态
    pub status: ConversationStatus,
    /// 流式累加中的助手回复
    pub current_assistant_text: String,
    /// 会话 JSONL 持久化句柄（None 表示未开启持久化，见 ADR-0008）。
    /// 首次用户输入时由 agent_poll_system 打开并写入 Header。
    pub session_store: Option<crate::session_store::SessionStore>,
}

impl Default for Conversation {
    fn default() -> Self {
        Self {
            id: SessionId(1),
            messages: Vec::new(),
            status: ConversationStatus::Idle,
            current_assistant_text: String::new(),
            session_store: None,
        }
    }
}

impl Conversation {
    /// 追加用户消息。
    pub fn push_user(&mut self, text: &str) {
        self.messages.push(AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            timestamp: 0,
        }));
    }

    /// 把累加的助手回复固化进历史。
    pub fn finalize_assistant(&mut self) {
        if !self.current_assistant_text.is_empty() {
            let text = std::mem::take(&mut self.current_assistant_text);
            self.messages
                .push(AgentMessage::Assistant(AssistantMessage {
                    content: vec![ContentBlock::Text { text }],
                    model: None,
                    usage: None,
                    timestamp: 0,
                }));
        }
    }

    /// 打开会话 JSONL 存储并写入 Header entry（见 ADR-0008）。
    ///
    /// 仅在 `session_store` 为 None 时执行（会话首次开始）。
    /// 失败不阻塞对话——记录到 stderr，存储保持 None。
    pub fn ensure_session_store(&mut self, project_root: &std::path::Path) {
        if self.session_store.is_some() {
            return;
        }
        let path = crate::session_store::session_file_path(project_root, &self.id.to_string());
        match crate::session_store::SessionStore::open(path) {
            Ok(mut store) => {
                let header =
                    xgent_core::session::SessionEntry::Header(xgent_core::session::SessionHeader {
                        id: self.id.to_string(),
                        version: 1,
                        cwd: project_root.to_string_lossy().into_owned(),
                        timestamp: crate::session_store::now_ms(),
                        title: None,
                    });
                if let Err(e) = store.append(&header) {
                    eprintln!("[session] 写入 Header 失败: {e}");
                    return;
                }
                self.session_store = Some(store);
            }
            Err(e) => {
                eprintln!("[session] 打开会话存储失败: {e}");
            }
        }
    }

    /// 把最后一条 Assistant 消息持久化为 JSONL Message entry。
    ///
    /// 在 `finalize_assistant` 之后调用。消息 id 用 `消息序号`，parent_id 为 None（MVP 线性）。
    pub fn persist_last_assistant(&mut self) {
        let Some(store) = self.session_store.as_mut() else {
            return;
        };
        // 找最后一条 Assistant 消息
        let Some(idx) = self
            .messages
            .iter()
            .rposition(|m| matches!(m, AgentMessage::Assistant(_)))
        else {
            return;
        };
        let AgentMessage::Assistant(msg) = &self.messages[idx] else {
            return;
        };
        let entry =
            xgent_core::session::SessionEntry::Message(xgent_core::session::SessionMessage {
                id: format!("{}-msg-{}", self.id, idx),
                parent_id: None,
                timestamp: crate::session_store::now_ms(),
                message: AgentMessage::Assistant(AssistantMessage {
                    content: msg.content.clone(),
                    model: msg.model.clone(),
                    usage: msg.usage.clone(),
                    timestamp: msg.timestamp,
                }),
            });
        if let Err(e) = store.append(&entry) {
            eprintln!("[session] 写入 Message 失败: {e}");
        }
    }

    /// 追加工具结果消息（工具执行完成后调用）。
    pub fn push_tool_result(
        &mut self,
        tool_call_id: &str,
        tool_name: &str,
        content: &str,
        is_error: bool,
    ) {
        self.messages
            .push(AgentMessage::ToolResult(ToolResultMessage {
                tool_call_id: tool_call_id.to_string(),
                tool_name: tool_name.to_string(),
                content: content.to_string(),
                is_error,
                timestamp: 0,
            }));
    }

    /// 追加 UI-only 通知消息（不发给 LLM）。
    pub fn push_notification(&mut self, text: &str) {
        self.messages
            .push(AgentMessage::Notification(NotificationMessage {
                text: text.to_string(),
                timestamp: 0,
            }));
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
