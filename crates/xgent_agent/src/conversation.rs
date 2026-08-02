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
    /// 本轮待固化的 assistant tool_call 块（批量累积）。
    ///
    /// OpenAI 协议要求一条 assistant 消息可含多个 tool_calls，后跟多条 tool role
    /// 消息。ToolCall 事件逐个到达时累积到此；首个 ToolResult 到达时把累积的
    /// blocks 作为一条 AssistantMessage 固化到 messages，再追写 tool result。
    /// 修复之前逐个 push_tool_call 把一条 assistant 拆成多条破坏协议的 bug。
    pub pending_tool_calls: Vec<ContentBlock>,
}

impl Default for Conversation {
    fn default() -> Self {
        Self {
            id: SessionId(1),
            messages: Vec::new(),
            status: ConversationStatus::Idle,
            current_assistant_text: String::new(),
            session_store: None,
            pending_tool_calls: Vec::new(),
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
        self.pending_tool_calls.clear();
        self.status = ConversationStatus::Idle;
        self.session_store = None;
    }

    /// 追加用户消息。
    pub fn push_user(&mut self, text: &str) {
        self.messages.push(AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            timestamp: crate::session_store::now_ms(),
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

    /// 累积 assistant 的 tool_call 块（工具开始执行时调用）。
    ///
    /// 不立即 push assistant 消息，而是把 ToolCall 块追加到 `pending_tool_calls`。
    /// 当首个 [`push_tool_result`] 到达时，把累积的 blocks 连同本轮流式文本
    /// （`current_assistant_text`）作为**一条** AssistantMessage 固化到 messages，
    /// 再追写 tool result。这样一轮多个 tool_calls 生成符合 OpenAI 协议的
    /// `assistant(text + tool_call_1 + ... + tool_call_N) → tool(r1) → ... → tool(rN)`，
    /// 修复之前逐个 push 把一条 assistant 拆成多条破坏协议的 bug。
    pub fn push_tool_call(&mut self, call_id: &str, tool_name: &str, args: &serde_json::Value) {
        self.pending_tool_calls.push(ContentBlock::ToolCall {
            id: call_id.to_string(),
            name: tool_name.to_string(),
            args: args.clone(),
        });
    }

    /// 把累积的 `pending_tool_calls` 固化为一条 AssistantMessage。
    ///
    /// 在首个 [`push_tool_result`] 到达时调用：把本轮流式文本（若有）作为首个
    /// Text 块前置，后接所有 ToolCall 块，组装成单条 AssistantMessage push 到
    /// messages，并清空 pending 与 current_assistant_text。对应 bridge 侧 req
    /// 回灌时「首个 tool_call 携带本轮文本块」的语义。
    fn flush_pending_tool_calls(&mut self) {
        if self.pending_tool_calls.is_empty() {
            return;
        }
        let mut content: Vec<ContentBlock> = Vec::with_capacity(self.pending_tool_calls.len() + 1);
        if !self.current_assistant_text.is_empty() {
            let text = std::mem::take(&mut self.current_assistant_text);
            content.push(ContentBlock::Text { text });
        }
        content.extend(self.pending_tool_calls.drain(..));
        self.messages
            .push(AgentMessage::Assistant(AssistantMessage {
                content,
                model: None,
                usage: None,
                timestamp: crate::session_store::now_ms(),
            }));
    }

    /// 追加工具结果消息（工具执行完成后调用）。
    ///
    /// 首次调用时先把累积的 tool_call 块固化为一条 AssistantMessage
    /// （见 [`flush_pending_tool_calls`]），保证 assistant tool_call 与
    /// 后续 tool result 配对，且多个 tool_call 不被拆成多条 assistant 消息。
    pub fn push_tool_result(
        &mut self,
        tool_call_id: &str,
        tool_name: &str,
        content: &str,
        is_error: bool,
    ) {
        self.flush_pending_tool_calls();
        self.messages
            .push(AgentMessage::ToolResult(ToolResultMessage {
                tool_call_id: tool_call_id.to_string(),
                tool_name: tool_name.to_string(),
                content: content.to_string(),
                is_error,
                timestamp: crate::session_store::now_ms(),
            }));
    }

    /// 追加 UI-only 通知消息（不发给 LLM）。
    pub fn push_notification(&mut self, text: &str) {
        self.messages
            .push(AgentMessage::Notification(NotificationMessage {
                text: text.to_string(),
                timestamp: crate::session_store::now_ms(),
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
