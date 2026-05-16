use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// chat 请求
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub message: Vec<ChatMessage>,
    pub stream: bool,
    /// OpenAI function calling 工具定义
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    /// 控制是否允许工具调用
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, message: Vec<ChatMessage>) -> Self {
        Self {
            model: model.into(),
            message,
            stream: true,
            tools: None,
            tool_choice: None,
        }
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = Some(tools);
        self
    }
}

/// 聊天消息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role")]
pub struct ChatMessage {
    #[serde(rename = "system")]
    System { content: String },
    #[serde(rename = "user")]
    User { content: String },
    #[serde(rename = "assistant")]
    Assistant {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ToolCall>>,
    },
    #[serde(rename = "tool")]
    Tool {
        content: String,
        tool_call_id: String,
    },
}

/// 工具调用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

/// 函数调用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: serde_json::Value,  // JSON 格式的参数
}

/// 工具定义（提供给 LLM 的）
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

/// 函数定义
#[derive(Debug, Clone, Serialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,  // JSON Schema
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ToolChoice {
    Auto(String),
    None(String),
}

/// Token 用量
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

/// 流式响应事件
#[derive(Debug, Clone)]
pub enum ChatStreamEvent {
    /// 增量文本输出
    Delta { content: String },
    /// LLM 发起工具调用
    ToolCall { id: String, name: String, arguments: String }
    /// 流结束
    Done { usage: TokenUsage },
}

/// SSE 流式响应包装
///
/// 内部是一个 tokio mpsc channel，由 HTTP 响应解析协程写入
/// 由消费者 （Agent 对话循环）读取。
pub struct ChatStream {
    rx: tokio::sync::mpsc::Receiver<Result<ChatStreamEvent, ProviderError>>,
}

impl ChatStream {
    pub fn new(rx: tokio::sync::mpsc::Receiver<Result<ChatStreamEvent, ProviderError>>) -> Self {
        Self { rx }
    }

    /// 读取下一个事件（异步）
    pub async fn next(&mut self) -> Option<Result<ChatStreamEvent, ProviderError>> {
        self.rx.recv().await
    }

    /// 非阻塞读取（用于 Bevy System 中 poll）
    pub fn try_next(&mut self) -> Option<Result<ChatStreamEvent, ProviderError>> {
        self.rx.try_recv().ok()
    }
}
