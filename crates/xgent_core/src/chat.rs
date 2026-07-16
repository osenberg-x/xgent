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

/// 单条对话消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
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

/// provider 流式输出的事件
///
/// 用 `#[serde(tag = "type")]` 使 JSON-RPC notification 可据 `type` 字段分发，
/// 扩展新事件类型不破坏旧客户端。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatEvent {
    /// 增量文本
    Delta { text: String },
    /// 工具调用
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
    /// 流结束
    Done { usage: TokenUsage },
    /// 出错
    Error {
        /// 错误分类
        kind: ErrorKind,
        /// 可读错误原文
        message: String,
    },
}

/// token 用量
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    fn chat_message_roundtrip() {
        let m = ChatMessage {
            role: Role::User,
            content: "hello".into(),
        };
        let j = serde_json::to_string(&m).unwrap();
        let m2: ChatMessage = serde_json::from_str(&j).unwrap();
        assert_eq!(m2.role, Role::User);
        assert_eq!(m2.content, "hello");
    }

    #[test]
    fn chat_request_roundtrip() {
        let req = ChatRequest {
            provider: "openai".into(),
            model: "gpt-4".into(),
            messages: vec![ChatMessage {
                role: Role::System,
                content: "you are helpful".into(),
            }],
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
    fn chat_event_delta_roundtrip() {
        let e = ChatEvent::Delta { text: "hi".into() };
        let j = serde_json::to_string(&e).unwrap();
        let e2: ChatEvent = serde_json::from_str(&j).unwrap();
        assert!(matches!(e2, ChatEvent::Delta { text } if text == "hi"));
        // tag 用 camelCase 字段名 type
        assert!(j.contains(r#""type":"delta""#));
    }

    #[test]
    fn chat_event_tool_call_roundtrip() {
        let e = ChatEvent::ToolCall {
            id: "call_1".into(),
            name: "read_file".into(),
            args: serde_json::json!({"path": "/x"}),
        };
        let j = serde_json::to_string(&e).unwrap();
        let e2: ChatEvent = serde_json::from_str(&j).unwrap();
        match e2 {
            ChatEvent::ToolCall { id, name, args } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "read_file");
                assert_eq!(args, serde_json::json!({"path": "/x"}));
            }
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn chat_event_done_roundtrip() {
        let e = ChatEvent::Done {
            usage: TokenUsage {
                prompt: 10,
                completion: 5,
            },
        };
        let j = serde_json::to_string(&e).unwrap();
        let e2: ChatEvent = serde_json::from_str(&j).unwrap();
        match e2 {
            ChatEvent::Done { usage } => {
                assert_eq!(usage.prompt, 10);
                assert_eq!(usage.completion, 5);
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn chat_event_error_roundtrip() {
        let e = ChatEvent::Error {
            kind: ErrorKind::Network,
            message: "boom".into(),
        };
        let j = serde_json::to_string(&e).unwrap();
        let e2: ChatEvent = serde_json::from_str(&j).unwrap();
        assert!(matches!(e2, ChatEvent::Error { kind: ErrorKind::Network, message } if message == "boom"));
    }

    #[test]
    fn token_usage_default() {
        let u = TokenUsage::default();
        assert_eq!(u.prompt, 0);
        assert_eq!(u.completion, 0);
    }
}
