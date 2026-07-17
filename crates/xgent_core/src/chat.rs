use serde::{Deserialize, Serialize};

/// 消息角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// 单条对话消息（LLM 层类型，provider 接收的格式）。
///
/// 结构化 content（对齐 Anthropic 协议原生形态，见 ADR-0005）。
/// OpenAiCompat 的 `message_to_json` 按 role 展开为 OpenAI 协议形态
/// （assistant+ToolCall→content+tool_calls 顶层字段；Tool→role:tool+content+tool_call_id）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl ChatMessage {
    /// 便捷构造：纯文本消息（System/User 常用）。
    pub fn text(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }
}

/// 消息内容块（`ChatMessage.content` 与 `AssistantMessage.content` 共用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ContentBlock {
    /// 文本块
    Text { text: String },
    /// 工具调用（assistant 发起）
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
    /// 工具结果（tool role 消息携带）
    ToolResult {
        tool_call_id: String,
        content: String,
        is_error: bool,
    },
    /// 图片块（MVP 无 UI 上传，类型定义保留）
    Image { data: String, mime_type: String },
}

/// Agent 内部消息类型（agent 层）。
///
/// 借鉴 omp 的 AgentMessage 设计：LLM 可理解的消息 + UI-only 扩展类型。
/// `convert_to_llm()` 在调用 LLM 前过滤 UI-only 类型，保留结构化 content。
/// 见 ADR-0005。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum AgentMessage {
    User(UserMessage),
    Assistant(AssistantMessage),
    ToolResult(ToolResultMessage),
    /// 系统通知（UI-only，不发给 LLM）
    Notification(NotificationMessage),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserMessage {
    pub content: Vec<ContentBlock>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub content: Vec<ContentBlock>,
    pub model: Option<String>,
    pub usage: Option<TokenUsage>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultMessage {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationMessage {
    pub text: String,
    pub timestamp: u64,
}

/// 将 `AgentMessage[]` 转换为 LLM 可理解的 `ChatMessage[]`。
///
/// 只过滤 UI-only 消息（Notification），保留结构化 content（不压扁为字符串）。
/// 见 ADR-0005。
pub fn convert_to_llm(messages: &[AgentMessage]) -> Vec<ChatMessage> {
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
            AgentMessage::Notification(_) => None, // UI-only，过滤
        })
        .collect()
}

/// 一次 chat 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    /// provider id，如 "openai"
    pub provider: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    /// 工具 schema（MVP 可先 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolSchema>>,
}

/// 错误分类，按"用户可采取的行动"划分（见 CONTEXT.md「ErrorKind」）。
///
/// UI 不感知 HTTP 状态码——daemon 侧负责把 `ProviderError::Api{status,body}`
/// 映射到 `AuthFailed`（401/403）或 `ProviderError`（其余）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// provider 未就绪（闸门拦截：未配/缺字段），引导开 settings_panel
    NotConfigured,
    /// provider 鉴权失败（API key 错/失效），引导检查 key
    AuthFailed,
    /// 连接/超时，可重试
    Network,
    /// SSE/JSON 解析失败，可重试
    StreamParse,
    /// provider 返回非鉴权类错误，含原始 message 供排查
    ProviderError,
}

/// provider 流式输出的事件。
///
/// 借鉴 omp 的细粒度事件设计，UI 可精确渲染流式内容。
/// 事件序列：`Start → (TextStart→TextDelta*→TextEnd | ThinkingStart→ThinkingDelta*→ThinkingEnd |
///                   ToolCallStart→ToolCallDelta*→ToolCallEnd)* → Done | Error`。
///
/// 用 `#[serde(tag = "type")]` 使 JSON-RPC notification 可据 `type` 字段分发，
/// daemon 侧透传整个 ChatEvent JSON，不解析内部结构（见 ADR-0006）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatEvent {
    /// 流开始
    Start {
        model: String,
    },

    // —— 文本块 ——
    TextStart,
    TextDelta {
        text: String,
    },
    TextEnd,

    // —— 推理块（thinking models，MVP 不发射，变体预留给 Anthropic 适配器）——
    ThinkingStart,
    ThinkingDelta {
        text: String,
    },
    ThinkingEnd,

    // —— 工具调用块 ——
    /// 工具调用开始（按 index 聚合分块，首次出现该 index 时发射）
    ToolCallStart {
        index: u32,
        id: String,
        name: String,
    },
    /// 工具调用参数增量（原始 partial JSON 字符串，MVP 不做 throttled 解析）
    ToolCallDelta {
        index: u32,
        partial_json: String,
    },
    /// 工具调用结束（全量 args）
    ToolCallEnd {
        index: u32,
        args: serde_json::Value,
    },

    /// 流结束
    Done {
        reason: StopReason,
        usage: TokenUsage,
    },
    /// 出错
    Error {
        kind: ErrorKind,
        message: String,
    },
}

/// 流结束原因。
///
/// agent loop 不依赖 reason 决定是否继续——`tool_calls.is_empty()` 才决定（对齐 omp）。
/// reason 供 UI 展示与错误恢复参考（如 Length 后是否重试）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    /// 正常结束
    Stop,
    /// 需要执行工具
    ToolUse,
    /// max_tokens 截断
    Length,
    /// 被中断
    Aborted,
    /// 错误
    Error,
}

/// token 用量
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt: u32,
    pub completion: u32,
}

/// 工具 schema 占位（step7 完善）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_serializes_lowercase() {
        let j = serde_json::to_string(&Role::Assistant).unwrap();
        assert_eq!(j, r#""assistant""#);
        let r: Role = serde_json::from_str(r#""tool""#).unwrap();
        assert_eq!(r, Role::Tool);
    }

    #[test]
    fn chat_message_text_helper_roundtrip() {
        let m = ChatMessage::text(Role::User, "hello");
        let j = serde_json::to_string(&m).unwrap();
        let m2: ChatMessage = serde_json::from_str(&j).unwrap();
        assert_eq!(m2.role, Role::User);
        assert_eq!(m2.content.len(), 1);
        assert!(matches!(&m2.content[0], ContentBlock::Text { text } if text == "hello"));
    }

    #[test]
    fn chat_request_roundtrip() {
        let req = ChatRequest {
            provider: "openai".into(),
            model: "gpt-4".into(),
            messages: vec![ChatMessage::text(Role::System, "you are helpful")],
            tools: None,
        };
        let j = serde_json::to_string(&req).unwrap();
        // tools 为 None 时不序列化
        assert!(!j.contains(r#"tools"#));
        let req2: ChatRequest = serde_json::from_str(&j).unwrap();
        assert_eq!(req2.provider, "openai");
        assert_eq!(req2.messages.len(), 1);
        assert!(req2.tools.is_none());
    }

    #[test]
    fn convert_to_llm_filters_notification_and_preserves_structure() {
        let msgs = vec![
            AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text { text: "hi".into() }],
                timestamp: 1,
            }),
            AgentMessage::Notification(NotificationMessage {
                text: "UI-only".into(),
                timestamp: 2,
            }),
            AgentMessage::Assistant(AssistantMessage {
                content: vec![
                    ContentBlock::Text {
                        text: "let me read".into(),
                    },
                    ContentBlock::ToolCall {
                        id: "call_1".into(),
                        name: "read_file".into(),
                        args: serde_json::json!({"path": "/x"}),
                    },
                ],
                model: None,
                usage: None,
                timestamp: 3,
            }),
            AgentMessage::ToolResult(ToolResultMessage {
                tool_call_id: "call_1".into(),
                tool_name: "read_file".into(),
                content: "file content".into(),
                is_error: false,
                timestamp: 4,
            }),
        ];
        let llm = convert_to_llm(&msgs);
        // Notification 被过滤
        assert_eq!(llm.len(), 3);
        // User 保留结构
        assert_eq!(llm[0].role, Role::User);
        assert!(matches!(&llm[0].content[0], ContentBlock::Text { text } if text == "hi"));
        // Assistant 保留 ToolCall 结构（不压扁为字符串）
        assert_eq!(llm[1].role, Role::Assistant);
        assert_eq!(llm[1].content.len(), 2);
        assert!(
            matches!(&llm[1].content[1], ContentBlock::ToolCall { name, .. } if name == "read_file")
        );
        // ToolResult 转 Role::Tool + ToolResult block
        assert_eq!(llm[2].role, Role::Tool);
        assert!(
            matches!(&llm[2].content[0], ContentBlock::ToolResult { tool_call_id, content, .. } if tool_call_id == "call_1" && content == "file content")
        );
    }

    #[test]
    fn chat_event_text_delta_roundtrip() {
        let e = ChatEvent::TextDelta { text: "hi".into() };
        let j = serde_json::to_string(&e).unwrap();
        let e2: ChatEvent = serde_json::from_str(&j).unwrap();
        assert!(matches!(e2, ChatEvent::TextDelta { text } if text == "hi"));
        assert!(j.contains(r#""type":"textDelta""#));
    }

    #[test]
    fn chat_event_start_roundtrip() {
        let e = ChatEvent::Start {
            model: "gpt-4".into(),
        };
        let j = serde_json::to_string(&e).unwrap();
        let e2: ChatEvent = serde_json::from_str(&j).unwrap();
        assert!(matches!(e2, ChatEvent::Start { model } if model == "gpt-4"));
        assert!(j.contains(r#""type":"start""#));
    }

    #[test]
    fn chat_event_tool_call_start_roundtrip() {
        let e = ChatEvent::ToolCallStart {
            index: 0,
            id: "call_1".into(),
            name: "read_file".into(),
        };
        let j = serde_json::to_string(&e).unwrap();
        let e2: ChatEvent = serde_json::from_str(&j).unwrap();
        match e2 {
            ChatEvent::ToolCallStart { index, id, name } => {
                assert_eq!(index, 0);
                assert_eq!(id, "call_1");
                assert_eq!(name, "read_file");
            }
            _ => panic!("expected ToolCallStart"),
        }
        assert!(j.contains(r#""type":"toolCallStart""#));
    }

    #[test]
    fn chat_event_tool_call_end_roundtrip() {
        let e = ChatEvent::ToolCallEnd {
            index: 0,
            args: serde_json::json!({"path": "/x"}),
        };
        let j = serde_json::to_string(&e).unwrap();
        let e2: ChatEvent = serde_json::from_str(&j).unwrap();
        match e2 {
            ChatEvent::ToolCallEnd { index, args } => {
                assert_eq!(index, 0);
                assert_eq!(args, serde_json::json!({"path": "/x"}));
            }
            _ => panic!("expected ToolCallEnd"),
        }
    }

    #[test]
    fn chat_event_done_with_reason_roundtrip() {
        let e = ChatEvent::Done {
            reason: StopReason::ToolUse,
            usage: TokenUsage {
                prompt: 10,
                completion: 5,
            },
        };
        let j = serde_json::to_string(&e).unwrap();
        let e2: ChatEvent = serde_json::from_str(&j).unwrap();
        match e2 {
            ChatEvent::Done { reason, usage } => {
                assert_eq!(reason, StopReason::ToolUse);
                assert_eq!(usage.prompt, 10);
                assert_eq!(usage.completion, 5);
            }
            _ => panic!("expected Done"),
        }
        assert!(j.contains(r#""reason":"toolUse""#));
    }

    #[test]
    fn chat_event_error_roundtrip() {
        let e = ChatEvent::Error {
            kind: ErrorKind::Network,
            message: "boom".into(),
        };
        let j = serde_json::to_string(&e).unwrap();
        let e2: ChatEvent = serde_json::from_str(&j).unwrap();
        assert!(
            matches!(e2, ChatEvent::Error { kind: ErrorKind::Network, message } if message == "boom")
        );
    }

    #[test]
    fn stop_reason_serde() {
        assert_eq!(
            serde_json::to_string(&StopReason::Stop).unwrap(),
            r#""stop""#
        );
        assert_eq!(
            serde_json::to_string(&StopReason::ToolUse).unwrap(),
            r#""toolUse""#
        );
        assert_eq!(
            serde_json::to_string(&StopReason::Length).unwrap(),
            r#""length""#
        );
        assert_eq!(
            serde_json::to_string(&StopReason::Aborted).unwrap(),
            r#""aborted""#
        );
        assert_eq!(
            serde_json::to_string(&StopReason::Error).unwrap(),
            r#""error""#
        );
    }

    #[test]
    fn token_usage_default() {
        let u = TokenUsage::default();
        assert_eq!(u.prompt, 0);
        assert_eq!(u.completion, 0);
    }
}
