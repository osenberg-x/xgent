# Step 5: xgent_provider

## 模块职责

LLM Provider 的抽象层与具体适配器：

1. **抽象 trait `LlmProvider`**：统一 provider 接口（id、列模型、流式对话、健康检查）。
2. **ChatStream**：流式输出的异步 channel 类型。
3. **适配器实现**：
   - `OpenAiCompatProvider`：OpenAI compatible 接口（OpenAI、DeepSeek、Ollama 兼容模式等）。
   - `ResponseApiProvider`：Response API 风格（占位，MVP 先实现 OpenAiCompat）。
   - `AnthropicProvider`：Anthropic 原生（占位）。
   - `CustomApiProvider`：用户自定义 endpoint/headers/body 模板（占位）。
4. **provider 解析**：从 settings 的 ProviderConfig 构造具体 provider 实例。

## 前置依赖

- xgent_core（ChatRequest / ChatEvent / ChatMessage / Role / TokenUsage / ToolSchema / 错误类型）
- xgent_settings_core（ProviderConfig / ProviderKind）

## 目标文件结构

```
crates/xgent_provider/
├── Cargo.toml
└── src/
    ├── lib.rs                # 模块导出 + 构造函数
    ├── provider.rs          # LlmProvider trait + ChatStream
    ├── openai_compat.rs     # OpenAI compatible 适配器
    ├── response_api.rs      # Response API 适配器（占位）
    ├── anthropic.rs         # Anthropic 适配器（占位）
    ├── custom.rs            # 自定义 API 适配器（占位）
    └── sse.rs               # SSE 解析辅助
```

## Cargo.toml

```toml
[package]
name = "xgent_provider"
version = "0.1.0"
edition = "2024"

[dependencies]
xgent_core = { path = "../xgent_core" }
xgent_settings_core = { path = "../xgent_settings_core" }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
reqwest = { workspace = true }
async-trait = { workspace = true }
eventsource-stream = { workspace = true }
thiserror = { workspace = true }
futures-core = { workspace = true }
```

说明：不依赖 Bevy——provider 是纯异步逻辑，daemon 侧使用；UI 侧经 IPC 调用，不直接实例化 provider。这是保持 daemon 可纯 tokio 的关键。

## 关键类型与接口

### 1. provider.rs — 抽象 trait

```rust
use async_trait::async_trait;
use tokio::sync::mpsc;
use xgent_core::chat::{ChatRequest, ChatEvent};
use xgent_core::ids::StreamId;

/// 流式对话的接收端
pub type ChatStream = mpsc::Receiver<ChatEvent>;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// provider 唯一标识，如 "openai"、"ollama"
    fn id(&self) -> &str;

    /// 列出可用模型
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;

    /// 流式对话。返回 (StreamId, ChatStream)，调用方据 StreamId 关联后续 chunk
    /// （注：daemon 侧 StreamId 用于 IPC 路由；本地直调可忽略）
    async fn chat(&self, req: ChatRequest) -> Result<(StreamId, ChatStream), ProviderError>;

    /// 健康检查
    async fn health_check(&self) -> Result<(), ProviderError>;
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_window: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("network: {0}")]
    Network(String),
    #[error("api: status {status}, body: {body}")]
    Api { status: u16, body: String },
    #[error("stream parse: {0}")]
    Stream(String),
    #[error("config: {0}")]
    Config(String),
}
```

### 2. openai_compat.rs — OpenAI compatible 适配器

```rust
use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc;
use xgent_core::chat::{ChatRequest, ChatEvent};
use xgent_core::ids::StreamId;
use crate::provider::{LlmProvider, ModelInfo, ProviderError, ChatStream};

pub struct OpenAiCompatProvider {
    id: String,            // "openai" / "deepseek" / "ollama" 等
    api_base: String,      // "https://api.openai.com/v1"
    api_key: String,
    client: Client,        // reqwest，复用连接池
}

impl OpenAiCompatProvider {
    pub fn new(id: String, api_base: String, api_key: String) -> Self { /* ... */ }
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    fn id(&self) -> &str { &self.id }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        // GET {api_base}/models，带 Authorization: Bearer {api_key}
        // 解析 data[].id
    }

    async fn chat(&self, req: ChatRequest) -> Result<(StreamId, ChatStream), ProviderError> {
        // POST {api_base}/chat/completions，stream=true
        // body: model + messages + tools(optional) + stream
        // 解析 SSE：每个 data: 行是 JSON delta
        //   choices[0].delta.content -> ChatEvent::Delta
        //   choices[0].delta.tool_calls -> ChatEvent::ToolCall
        //   finish_reason=="stop" -> ChatEvent::Done(usage)
        // 用 mpsc::channel，spawn task 把 SSE event 转成 ChatEvent 发送
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        // list_models 成功即健康
    }
}
```

### 3. sse.rs — SSE 解析辅助

```rust
use eventsource_stream::Eventsource;
use futures_core::Stream;
use tokio_stream::StreamExt;
use serde_json::Value;

/// 把 HTTP 响应体的字节流转成 SSE 事件流
pub async fn parse_sse_stream(
    body: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>>,
) -> impl Stream<Item = Result<Value, ProviderError>> {
    body.eventsource()
        .map(|ev| match ev {
            Ok(e) => serde_json::from_str::<Value>(&e.data).map_err(ProviderError::from),
            Err(e) => Err(ProviderError::Stream(e.to_string())),
        })
}
```

### 4. response_api.rs / anthropic.rs / custom.rs — 占位

```rust
// 占位结构，trait impl 返回 ProviderError::Config("not implemented yet")
// 在 P1 迭代中完善
```

### 5. lib.rs — 构造函数

```rust
use xgent_settings_core::ProviderConfig;
use xgent_settings_core::ProviderKind;

pub fn build_provider(cfg: &ProviderConfig) -> Box<dyn LlmProvider> {
    match cfg.kind {
        ProviderKind::OpenAiCompat | ProviderKind::Ollama => {
            Box::new(OpenAiCompatProvider::new(...))
        }
        ProviderKind::ResponseApi => Box::new(ResponseApiProvider::new(...)),
        ProviderKind::Anthropic => Box::new(AnthropicProvider::new(...)),
        ProviderKind::Custom => Box::new(CustomApiProvider::new(...)),
    }
}
```

## 实现要点

1. **不依赖 Bevy**：provider 是纯异步 trait，daemon 侧持有实例池，UI 侧经 IPC 调用。
2. **连接复用**：每个 provider 实例持有一个 `reqwest::Client`（自带连接池），避免每次请求新建连接。
3. **SSE 流式**：用 `eventsource-stream` 把 reqwest 的 `bytes_stream()` 转 SSE 事件，再 parse JSON delta，经 `mpsc::channel` 转成 `ChatEvent`。
4. **工具调用解析**：OpenAI 的 `tool_calls` 字段在 delta 里分块到达，需按 `tool_call.index` 聚合，完整后发 `ChatEvent::ToolCall`。MVP 可先支持文本 delta，工具调用在 step7（xgent_tools）后完善。
5. **错误分层**：`ProviderError` 区分网络/API/流解析/配置，便于上层重试与提示。
6. **占位策略**：ResponseApi/Anthropic/Custom 先占位返回未实现，trait 与构造框架就位，后续迭代补实现，不阻塞 MVP（MVP 主用 OpenAiCompat，可对接 OpenAI/Ollama）。
7. **StreamId**：本地直调用不到，但 trait 返回它使 daemon 侧 IPC 路由自然；本地测试可忽略。

## 验证方法

1. **编译检查**：
   ```bash
   cargo check -p xgent_provider
   ```
2. **mock SSE 测试**：构造一段 OpenAI 风格 SSE 响应文本，喂给 `parse_sse_stream` 的测试版（直接用 `futures::stream::iter`），断言解析出正确的 ChatEvent 序列。
3. **真实 provider 测试**（手动/可选，需 API key）：配 OpenAI 或本地 Ollama，`list_models` 与 `chat` 流式输出验证。无 key 时跳过。
4. **构造函数测试**：从 ProviderConfig 构造各 provider，断言类型正确。

## 完成后下一步

xgent_provider 完成后 → 实现 **xgent_daemon**（守护进程：provider 池 + 全局配置 + 文件监听 + 多客户端同步），它组合 core/provider/settings_core。
