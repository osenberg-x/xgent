# Step 1: xgent_core

## 模块职责

承载 UI 进程与守护进程（daemon）共享的类型与协议契约：错误类型、IPC 协议（JSON-RPC 方法与通知）、provider 事件类型、文件变更事件、配置变更事件、其他跨进程共享的数据结构。

这是整个项目的"契约层"，先定义清楚，后续 crate 才能稳定实现。

## 前置依赖

无。这是最底层 crate。

## 目标文件结构

```
crates/xgent_core/
├── Cargo.toml
└── src/
    ├── lib.rs          # 模块导出
    ├── error.rs        # 统一错误类型
    ├── proto.rs        # JSON-RPC 协议：请求/响应/通知
    ├── methods.rs      # IPC 方法枚举与通知枚举
    ├── chat.rs         # ChatRequest / ChatEvent / ChatStream 类型
    ├── fs.rs           # FileChanged 事件、项目订阅类型
    ├── config.rs       # 配置读写请求/响应/变更通知
    └── ids.rs          # 共享 ID 类型（SessionId、ClientId、StreamId 等）
```

## Cargo.toml

```toml
[package]
name = "xgent_core"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
```

说明：core 不依赖 tokio/reqwest 等重型依赖，保持轻量；只定义类型与协议。

## 关键类型与接口

### 1. error.rs — 统一错误类型

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum XgentError {
    #[error("ipc error: {0}")]
    Ipc(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("tool error: {0}")]
    Tool(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type XgentResult<T> = Result<T, XgentError>;
```

### 2. ids.rs — 共享 ID 类型

```rust
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u64);

/// provider 流式对话的流 ID，用于关联 chunk 通知与请求
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamId(pub u64);

impl fmt::Display for ClientId { /* ... */ }
// SessionId、StreamId 同理
```

### 3. chat.rs — 对话、消息类型与流式事件

本模块是 ADR-0005（双层消息类型）与 ADR-0006（细粒度流式事件）的落地点，包含四组类型：
LLM 层 `ChatMessage`/`ContentBlock`、Agent 层 `AgentMessage` 及其子 struct、
`convert_to_llm` 转换函数、`ChatEvent`/`StopReason` 流式事件。

```rust
use serde::{Deserialize, Serialize};

/// 消息角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role { System, User, Assistant, Tool }

/// LLM 层消息（provider 接收的格式）。
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
    pub fn text(role: Role, text: impl Into<String>) -> Self { /* ... */ }
}

/// 消息内容块（`ChatMessage.content` 与 `AssistantMessage.content` 共用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ContentBlock {
    Text { text: String },
    /// 工具调用（assistant 发起）
    ToolCall { id: String, name: String, args: serde_json::Value },
    /// 工具结果（tool role 消息携带）
    ToolResult { tool_call_id: String, content: String, is_error: bool },
    /// 图片块（MVP 无 UI 上传，类型定义保留）
    Image { data: String, mime_type: String },
}

/// Agent 内部消息类型（agent 层）。
///
/// 借鉴 omp 的 AgentMessage：LLM 可理解的消息 + UI-only 扩展类型。
/// `convert_to_llm` 在调用 LLM 前过滤 UI-only 类型，保留结构化 content。
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
pub struct UserMessage { pub content: Vec<ContentBlock>, pub timestamp: u64 }

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
pub struct NotificationMessage { pub text: String, pub timestamp: u64 }

/// 将 `AgentMessage[]` 转换为 LLM 可理解的 `ChatMessage[]`。
///
/// 只过滤 UI-only 消息（Notification），保留结构化 content（不压扁为字符串）。
/// 见 ADR-0005。
pub fn convert_to_llm(messages: &[AgentMessage]) -> Vec<ChatMessage> {
    messages.iter().filter_map(|msg| match msg {
        AgentMessage::User(m) => Some(ChatMessage { role: Role::User, content: m.content.clone() }),
        AgentMessage::Assistant(m) => Some(ChatMessage { role: Role::Assistant, content: m.content.clone() }),
        AgentMessage::ToolResult(m) => Some(ChatMessage {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_call_id: m.tool_call_id.clone(),
                content: m.content.clone(),
                is_error: m.is_error,
            }],
        }),
        AgentMessage::Notification(_) => None, // UI-only，过滤
    }).collect()
}

/// 一次 chat 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub provider: String,            // provider id，如 "openai"
    pub model: String,
    pub messages: Vec<ChatMessage>, // 已经过 convert_to_llm 转换
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolSchema>>,
}

/// 错误分类，按"用户可采取的行动"划分（见 ADR-0003）。
///
/// UI 不感知 HTTP 状态码——daemon 侧把 `ProviderError::Api{status,body}`
/// 映射到 `AuthFailed`（401/403）或 `ProviderError`（其余）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    NotConfigured,   // provider 未就绪（闸门拦截：未配/缺字段），引导开 settings_panel
    AuthFailed,      // API key 错/失效，引导检查 key
    Network,         // 连接/超时，可重试
    StreamParse,     // SSE/JSON 解析失败，可重试
    ProviderError,   // provider 返回非鉴权类错误，含原始 message 供排查
}

/// provider 流式输出的事件（细粒度，见 ADR-0006）。
///
/// 事件序列：`Start → (TextStart→TextDelta*→TextEnd |
///                   ThinkingStart→ThinkingDelta*→ThinkingEnd |
///                   ToolCallStart→ToolCallDelta*→ToolCallEnd)* → Done | Error`。
///
/// 用 `#[serde(tag = "type")]` 使 JSON-RPC notification 可据 `type` 字段分发，
/// daemon 侧透传整个 ChatEvent JSON，不解析内部结构。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatEvent {
    /// 流开始
    Start { model: String },

    // —— 文本块 ——
    TextStart,
    TextDelta { text: String },
    TextEnd,

    // —— 推理块（thinking models，MVP 不发射，变体预留给 Anthropic 适配器）——
    ThinkingStart,
    ThinkingDelta { text: String },
    ThinkingEnd,

    // —— 工具调用块（按 index 聚合分块）——
    ToolCallStart { index: u32, id: String, name: String },
    /// 参数增量（原始 partial JSON 字符串，MVP 不做 throttled 解析）
    ToolCallDelta { index: u32, partial_json: String },
    /// 工具调用结束（全量 args）
    ToolCallEnd { index: u32, args: serde_json::Value },

    /// 流结束
    Done { reason: StopReason, usage: TokenUsage },
    /// 出错
    Error { kind: ErrorKind, message: String },
}

/// 流结束原因（见 ADR-0006）。
///
/// agent loop 不依赖 reason 决定是否继续——`tool_calls.is_empty()` 才决定（对齐 omp）。
/// reason 供 UI 展示与错误恢复参考（如 Length 后是否重试）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StopReason { Stop, ToolUse, Length, Aborted, Error }

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage { pub prompt: u32, pub completion: u32 }

/// 工具 schema 占位（step7 完善）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema { pub name: String, pub description: String, pub input_schema: serde_json::Value }
```

> 注：旧版（MVP 初版）`ChatEvent` 仅 4 变体 `Delta/ToolCall/Done/Error`、`ChatMessage` 仅 `role+content:String`，
> 已按 ADR-0005/0006 clean cutover 删除，不留兼容别名。

### 4. proto.rs — JSON-RPC 2.0 契约

```rust
use serde::{Deserialize, Serialize};

/// JSON-RPC 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: &'static str,  // "2.0"
    pub id: u64,
    pub method: String,
    pub params: serde_json::Value,
}

/// JSON-RPC 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub result: Option<serde_json::Value>,
    pub error: Option<RpcError>,
}

/// JSON-RPC 通知（无 id，单向）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: &'static str,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}
```

### 5. methods.rs — IPC 方法与通知枚举

```rust
use crate::{chat::ChatEvent, fs::FileChanged, config::ConfigChanged};

/// UI → daemon 的方法名
pub mod methods {
    pub const PROVIDER_CHAT: &str = "provider.chat";
    pub const PROVIDER_LIST_MODELS: &str = "provider.listModels";
    pub const CONFIG_READ: &str = "config.read";
    pub const CONFIG_WRITE: &str = "config.write";
    pub const FS_WATCH: &str = "fs.watch";
}

/// daemon → UI 的通知名
pub mod notifications {
    pub const PROVIDER_DELTA: &str = "provider.delta";       // ChatEvent::TextDelta 等（细粒度事件统一经 provider.event 透传，见 ADR-0006）
    pub const PROVIDER_TOOL_CALL: &str = "provider.toolCall";
    pub const PROVIDER_DONE: &str = "provider.done";
    pub const PROVIDER_ERROR: &str = "provider.error";
    pub const FS_CHANGED: &str = "fs.changed";
    pub const CONFIG_CHANGED: &str = "config.changed";
    pub const PEER_FILE_CHANGED: &str = "peer.fileChanged";
}
```

### 6. fs.rs — 文件变更事件

```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChanged {
    pub project_root: PathBuf,
    pub path: PathBuf,         // 相对或绝对，step5 约定
    pub kind: FileChangeKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FileChangeKind { Created, Modified, Removed, Renamed }

/// 订阅项目路径的请求参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRequest {
    pub project_root: PathBuf,
}
```

### 7. config.rs — 配置读写

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigReadRequest {
    pub scope: ConfigScope,
    pub key: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ConfigScope { Global, Project }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChanged {
    pub scope: ConfigScope,
    pub key: String,
    pub value: serde_json::Value,
}
```

## 实现要点

1. **纯类型，无逻辑**：core 只定义数据结构与协议常量，不包含运行时逻辑（不连 socket、不调度）。
2. **serde 派生**：所有跨进程类型 derive `Serialize`/`Deserialize`，JSON-RPC 经 serde_json 序列化。
3. **ID 生成**：ID 生成逻辑放各使用方（daemon 生成 StreamId、UI 生成请求 id），core 只定义类型与 Display。
4. **不引入 Bevy**：core 保持与 Bevy 无关，使 daemon 可不依赖 Bevy（daemon 是纯 tokio 服务）。
5. **ChatEvent 用 tag 枚举**：`#[serde(tag = "type")]` 使 JSON-RPC notification 可据 `type` 字段分发，扩展新事件类型不破坏旧客户端。
6. **方法/通知名用常量**：避免字符串硬编码不一致。

## 验证方法

1. **编译检查**：
   ```bash
   cargo check -p xgent_core
   ```
2. **serde 往返测试**：对每个跨进程类型写 `serialize → deserialize` 往返测试，确保 JSON 序列化稳定。
   ```rust
   #[test]
   fn chat_event_roundtrip() {
       let e = ChatEvent::TextDelta { text: "hi".into() };
       let j = serde_json::to_string(&e).unwrap();
       let e2: ChatEvent = serde_json::from_str(&j).unwrap();
       assert!(matches!(e2, ChatEvent::TextDelta { .. }));
   }
   ```
3. **协议契约测试**：构造一个 `Request`/`Response`/`Notification`，序列化后断言 JSON 结构符合 JSON-RPC 2.0。

## 完成后下一步

xgent_core 完成后 → 可并行实现 **xui_i18n**（i18n trait，无依赖，见 step2）与 **xgent_settings_core**（配置纯类型，依赖 core，见 step3）。
