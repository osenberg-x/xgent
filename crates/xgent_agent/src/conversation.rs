//! 会话状态 Resource。

use bevy::prelude::*;
use serde_json;
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
    /// 重置会话：生成新 SessionId、清空消息与累加文本、重置状态。
    ///
    /// `session_store` 置 None（下次首次对话时 `ensure_session_store` 重新打开）。
    /// 用于「新建会话」功能。
    pub fn reset(&mut self) {
        // 用当前时间戳作为新 SessionId（保证全局唯一，对齐 pi 的 snowflake 思路简化版）
        let ts = crate::session_store::now_ms();
        self.id = SessionId(ts);
        self.messages.clear();
        self.current_assistant_text.clear();
        self.status = ConversationStatus::Idle;
        self.session_store = None;
    }

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
    ///
    /// `usage` 与 `model` 来自 provider 的流式 Done 事件（经 `AgentEvent::Done` 传递），
    /// 写入 AssistantMessage 供持久化与 UI token 统计（修复 usage 永远为 None 的 bug）。
    pub fn finalize_assistant(
        &mut self,
        usage: Option<xgent_core::chat::TokenUsage>,
        model: Option<String>,
    ) {
        if !self.current_assistant_text.is_empty() {
            let text = std::mem::take(&mut self.current_assistant_text);
            self.messages
                .push(AgentMessage::Assistant(AssistantMessage {
                    content: vec![ContentBlock::Text { text }],
                    model,
                    usage,
                    timestamp: crate::session_store::now_ms(),
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
        let path = crate::session_store::session_file_path(&self.id.to_string());
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

    /// 持久化 compaction 记录（append 一条 `CompactionEntry`，不重写历史）。
    ///
    /// JSONL 是 append-only：被压缩的历史消息 entry 保留在文件中，
    /// 恢复会话时读到 `CompactionEntry` 即知前文已被摘要为 `summary`，
    /// 上下文重建为「summary + CompactionEntry 之后的 kept 消息」。
    pub fn persist_compaction(&mut self, summary: &str, first_kept_id: &str, tokens_before: u32) {
        let Some(store) = self.session_store.as_mut() else {
            return;
        };
        let entry =
            xgent_core::session::SessionEntry::Compaction(xgent_core::session::CompactionEntry {
                id: format!("{}-compaction-{}", self.id, crate::session_store::now_ms()),
                parent_id: String::new(),
                timestamp: crate::session_store::now_ms(),
                summary: summary.to_string(),
                first_kept_id: first_kept_id.to_string(),
                tokens_before,
            });
        if let Err(e) = store.append(&entry) {
            eprintln!("[session] 写入 Compaction 失败: {e}");
        }
    }

    /// 持久化错误记录（append 一条 `ErrorEntry`，不进消息历史）。
    ///
    /// 错误本身不进 `conv.messages`（不发给 LLM），但持久化为独立 entry，
    /// 便于恢复会话时看到失败点（修复错误未持久化的 bug）。
    pub fn persist_error(&mut self, kind: xgent_core::chat::ErrorKind, message: &str) {
        let Some(store) = self.session_store.as_mut() else {
            return;
        };
        let entry = xgent_core::session::SessionEntry::Error(xgent_core::session::ErrorEntry {
            id: format!("{}-error-{}", self.id, crate::session_store::now_ms()),
            parent_id: String::new(),
            timestamp: crate::session_store::now_ms(),
            kind,
            message: message.to_string(),
        });
        if let Err(e) = store.append(&entry) {
            eprintln!("[session] 写入 Error 失败: {e}");
        }
    }

    /// 追加 assistant 的 tool_call 消息（工具开始执行时调用）。
    ///
    /// 与 [`push_tool_result`] 配对：assistant 发起 tool_call → tool 返回结果。
    /// 两者都进 `conv.messages`，下次 StartLoop 时 `convert_to_llm` 生成
    /// 符合 OpenAI 协议的消息序列（tool_call 后跟 tool result，配对完整）。
    /// 修复之前 conv.messages 缺 tool_call 导致 tool result 孤儿、
    /// 多轮工具调用后 LLM 请求被 OpenAI 拒绝的 bug。
    pub fn push_tool_call(&mut self, call_id: &str, tool_name: &str, args: &serde_json::Value) {
        self.messages
            .push(AgentMessage::Assistant(AssistantMessage {
                content: vec![ContentBlock::ToolCall {
                    id: call_id.to_string(),
                    name: tool_name.to_string(),
                    args: args.clone(),
                }],
                model: None,
                usage: None,
                timestamp: crate::session_store::now_ms(),
            }));
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
