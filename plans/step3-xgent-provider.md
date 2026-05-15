# xgent_provider — 详细编码指导

## 前置依赖

- **xgent_settings** 已完成

## 模块职责

LLM Provider 抽象层：定义 `LLMProvider` trait、实现 OpenAI 兼容适配器、管理 Provider 注册表、处理 SSE 流式响应。

---

## 目标文件结构

```
crates/xgent_provider/
├── Cargo.toml
└── src/
    ├── lib.rs            # Plugin + 公开导出
    ├── provider.rs       # LLMProvider trait, ProviderId, ModelId
    ├── chat_types.rs     # ChatRequest, ChatMessage, ChatStream, TokenUsage
    ├── openai_compat.rs  # OpenAICompatibleAdapter
    └── registry.rs       # ProviderRegistry (Resource)
```

MVP-1 暂不实现 router.rs 和 cost.rs（降级链和计费追踪留到 MVP-3）。

---

## Cargo.toml

```toml
[package]
name = "xgent_provider"
version = "0.1.0"
edition = "2024"

[dependencies]
bevy = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
reqwest = { workspace = true }
async-trait = { workspace = true }
eventsource-stream = { workspace = true }
```

---

## 各文件详细设计

### provider.rs — 核心 trait

```rust
use serde::{Deserialize, Serialize};
use std::fmt;

/// Provider 唯一标识
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderId(pub String);

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 模型标识
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelId(pub String);

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 模型信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: ModelId,
    pub display_name: String,
    pub context_window: usize,
}

/// LLM Provider 抽象 trait
///
/// 所有 Provider 必须实现此 trait。
/// Agent 引擎只依赖此 trait，不绑定具体实现。
#[async_trait::async_trait]
pub trait LLMProvider: Send + Sync {
    /// Provider 唯一标识
    fn id(&self) -> &ProviderId;

    /// 列出可用模型
    fn list_models(&self) -> Vec<ModelInfo>;

    /// 发送 Chat 请求（支持 SSE 流式）
    async fn chat(&self, request: ChatRequest) -> Result<ChatStream, ProviderError>;

    /// 健康检查
    async fn health_check(&self) -> Result<(), ProviderError>;
}

/// Provider 错误类型
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error: {status} {message}")]
    Api { status: u16, message: String },

    #[error("Stream error: {0}")]
    Stream(String),

    #[error("Rate limited, retry after {0:?}")]
    RateLimited(Option<std::time::Duration>),

    #[error("Configuration error: {0}")]
    Config(String),
}
```

**注意**：需要添加 `thiserror` 依赖到 Cargo.toml：
```toml
thiserror = "2"
```
也可以在 workspace.dependencies 中添加。

---

### chat_types.rs — 请求/响应类型

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Chat 请求
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    /// OpenAI function calling 工具定义
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    /// 控制是否允许工具调用
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        Self {
            model: model.into(),
            messages,
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
pub enum ChatMessage {
    #[serde(rename = "system")]
    System { content: String },
    #[serde(rename = "user")]
    User { content: String },
    #[serde(rename = "assistant")]
    Assistant {
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ToolCall>>,
    },
    #[serde(rename = "tool")]
    Tool {
        content: String,
        tool_call_id: String,
    },
}

/// 工具调用（LLM 发起的）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,  // 固定为 "function"
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,  // JSON string
}

/// 工具定义（提供给 LLM 的）
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,  // "function"
    pub function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,  // JSON Schema
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ToolChoice {
    Auto(String),   // "auto"
    None(String),   // "none"
}

/// Token 使用量
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
```

---

### chat_types.rs 续 — ChatStream

```rust
/// 流式响应事件
#[derive(Debug, Clone)]
pub enum ChatStreamEvent {
    /// 增量文本输出
    Delta { content: String },
    /// LLM 发起工具调用
    ToolCall { id: String, name: String, arguments: String },
    /// 流结束
    Done { usage: TokenUsage },
}

/// SSE 流式响应包装
///
/// 内部是一个 tokio mpsc channel，由 HTTP 响应解析协程写入，
/// 由消费者（Agent 对话循环）读取。
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
```

---

### openai_compat.rs — OpenAI 兼容适配器

这是 MVP-1 的核心实现。需要处理 SSE 流式响应解析。

```rust
use crate::*;
use eventsource_stream::Eventsource;
use futures_core::StreamExt;

/// OpenAI 兼容协议适配器
///
/// 支持 OpenAI / DeepSeek / 月之暗面 / 智谱 / Ollama / LM Studio 等
/// 所有提供 OpenAI Chat Completions API 兼容接口的 Provider
pub struct OpenAICompatibleAdapter {
    id: ProviderId,
    client: reqwest::Client,
    api_base: String,
    api_key: String,
    default_model: String,
}

impl OpenAICompatibleAdapter {
    pub fn new(
        id: impl Into<String>,
        api_base: impl Into<String>,
        api_key: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        Self {
            id: ProviderId(id.into()),
            client: reqwest::Client::new(),
            api_base: api_base.into(),
            api_key: api_key.into(),
            default_model: default_model.into(),
        }
    }
}

#[async_trait::async_trait]
impl LLMProvider for OpenAICompatibleAdapter {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        // MVP-1: 返回硬编码的默认模型
        vec![ModelInfo {
            id: ModelId(self.default_model.clone()),
            display_name: self.default_model.clone(),
            context_window: 128_000,
        }]
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream, ProviderError> {
        let url = format!("{}/chat/completions", self.api_base.trim_end_matches('/'));

        let mut body = serde_json::to_value(&request)
            .map_err(|e| ProviderError::Config(e.to_string()))?;
        // 确保 stream: true
        body["stream"] = serde_json::Value::Bool(true);

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                status: status.as_u16(),
                message,
            });
        }

        // 解析 SSE 流
        let (tx, rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            // 使用 eventsource-stream 解析 SSE
            let mut stream = response.bytes_stream().eventsource();

            while let Some(result) = stream.next().await {
                match result {
                    Ok(event) => {
                        if event.data == "[DONE]" {
                            let _ = tx.send(Ok(ChatStreamEvent::Done {
                                usage: TokenUsage::default(),
                            })).await;
                            break;
                        }

                        // 解析 OpenAI SSE data JSON
                        match serde_json::from_str::<serde_json::Value>(&event.data) {
                            Ok(data) => {
                                // 解析 choices[0].delta
                                if let Some(choices) = data.get("choices").and_then(|c| c.as_array()) {
                                    if let Some(choice) = choices.first() {
                                        if let Some(delta) = choice.get("delta") {
                                            // 文本内容
                                            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                                if !content.is_empty() {
                                                    if tx.send(Ok(ChatStreamEvent::Delta {
                                                        content: content.to_string(),
                                                    })).await.is_err() {
                                                        break;
                                                    }
                                                }
                                            }
                                            // 工具调用
                                            if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                                                for tc in tool_calls {
                                                    let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                                                    let function = tc.get("function");
                                                    let name = function.and_then(|f| f.get("name")).and_then(|n| n.as_str()).unwrap_or("").to_string();
                                                    let arguments = function.and_then(|f| f.get("arguments")).and_then(|a| a.as_str()).unwrap_or("{}").to_string();
                                                    if !name.is_empty() {
                                                        if tx.send(Ok(ChatStreamEvent::ToolCall { id, name, arguments })).await.is_err() {
                                                            break;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(Err(ProviderError::Stream(e.to_string()))).await;
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(ProviderError::Stream(e.to_string()))).await;
                        break;
                    }
                }
            }
        });

        Ok(ChatStream::new(rx))
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        let url = format!("{}/models", self.api_base.trim_end_matches('/'));
        let response = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ProviderError::Api {
                status: response.status().as_u16(),
                message: "Health check failed".to_string(),
            })
        }
    }
}
```

**需要额外添加的依赖**：
```toml
futures-core = "0.3"   # 用于 StreamExt trait
```
也加入 workspace.dependencies。

---

### registry.rs — Provider 注册表

```rust
use crate::*;

/// Provider 注册表（ECS Resource）
#[derive(Resource, Default)]
pub struct ProviderRegistry {
    providers: HashMap<ProviderId, Box<dyn LLMProvider>>,
    default_provider: Option<ProviderId>,
    default_model: Option<ModelId>,
}

impl ProviderRegistry {
    /// 注册一个 Provider
    pub fn register(&mut self, provider: Box<dyn LLMProvider>) {
        self.providers.insert(provider.id().clone(), provider);
    }

    /// 获取指定 Provider
    pub fn get(&self, id: &ProviderId) -> Option<&dyn LLMProvider> {
        self.providers.get(id).map(|p| p.as_ref())
    }

    /// 获取默认 Provider
    pub fn default(&self) -> Option<&dyn LLMProvider> {
        self.default_provider.as_ref().and_then(|id| self.get(id))
    }

    /// 设置默认 Provider 和 Model
    pub fn set_default(&mut self, provider: ProviderId, model: ModelId) {
        self.default_provider = Some(provider);
        self.default_model = Some(model);
    }

    /// 列出所有已注册的 Provider ID
    pub fn list_provider_ids(&self) -> Vec<ProviderId> {
        self.providers.keys().cloned().collect()
    }
}
```

---

### lib.rs — Plugin 与导出

```rust
mod provider;
mod chat_types;
mod openai_compat;
mod registry;

pub use provider::*;
pub use chat_types::*;
pub use openai_compat::*;
pub use registry::*;

use bevy::prelude::*;

pub struct XgentProviderPlugin;

impl Plugin for XgentProviderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProviderRegistry>();
    }
}
```

---

## 验证方法

1. **编译**：`cargo check -p xgent_provider`
2. **单元测试**：创建 `OpenAICompatibleAdapter`，用真实 API Key 调用 `chat()`，验证收到流式 Delta 事件
3. **Mock 测试**（推荐）：用 mock 服务器验证 SSE 解析逻辑

### 手动验证脚本

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // 需要真实 API Key，CI 中跳过
    async fn test_openai_chat_stream() {
        let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
        let provider = OpenAICompatibleAdapter::new(
            "openai",
            "https://api.openai.com/v1",
            &api_key,
            "gpt-4o-mini",
        );

        let request = ChatRequest::new(
            "gpt-4o-mini",
            vec![ChatMessage::User { content: "Say hello in 3 words".to_string() }],
        );

        let mut stream = provider.chat(request).await.expect("chat failed");
        let mut full_response = String::new();

        while let Some(event) = stream.next().await {
            match event {
                Ok(ChatStreamEvent::Delta { content }) => {
                    full_response.push_str(&content);
                    print!("{}", content);
                }
                Ok(ChatStreamEvent::Done { .. }) => break,
                Ok(ChatStreamEvent::ToolCall { .. }) => {}
                Err(e) => panic!("Stream error: {}", e),
            }
        }

        assert!(!full_response.is_empty());
    }
}
```

---

## 完成后下一步

xgent_provider 完成后 → 实现 **xgent_tools**（Tool 枚举 + SecurityPolicy + 执行器），因为 Agent 引擎依赖它来定义和执行工具调用。
