# Step 9: xgent_agent

## 模块职责

Agent 核心引擎，组合 provider/tools/context 并接入 Bevy ECS：

1. **Agent Loop（对话循环）**：构建上下文 → 调 provider → 解析响应 → 若有工具调用则执行（经确认）→ 回灌结果 → 循环。
2. **Conversation 状态**：会话消息历史、中断/重试、流式累加。
3. **ECS 桥接**：把异步 provider/tools/context 经 tokio channel 桥接到 Bevy 系统；对话状态作为 Bevy Resource；通过 Events/Messages 与 UI 通信（禁止直接方法调用）。
4. **事件契约**：定义 agent 对外暴露的 Events（Delta、ToolCall、Done、Error、ConfirmRequest）与接收的 Messages（用户输入、中断、确认决策）。

## 前置依赖

- xgent_core（ChatRequest、ChatEvent、错误类型）
- xgent_provider（LlmProvider trait、ChatStream）—— UI 侧经 IPC 客户端调用，不直接实例化
- xgent_tools（Tool、ToolExecutor、SecurityPolicy、ConfirmRequest）
- xgent_context（ContextProvider、ContextQuery）
- xgent_settings（ProjectConfigRes、GlobalConfigRes）
- xgent_settings_core（ContextStrategy、用于构造 ContextProvider）

## 目标文件结构

```
crates/xgent_agent/
├── Cargo.toml
└── src/
    ├── lib.rs              # XgentAgentPlugin + 模块导出
    ├── conversation.rs    # Conversation Resource（消息历史、状态）
    ├── agent_loop.rs       # AgentLoopSystem：驱动对话循环
    ├── bridge.rs          # tokio 桥接：ProviderClient、ToolExecutor、ContextProvider 的异步 task 与 ECS channel
    ├── events.rs          # agent 对外 Events 与 Messages
    └── format.rs          # chat 格式化：ChatRequest 构造
```

## Cargo.toml

```toml
[package]
name = "xgent_agent"
version = "0.1.0"
edition = "2024"

[dependencies]
bevy = { workspace = true }
xgent_core = { path = "../xgent_core" }
xgent_provider = { path = "../xgent_provider" }
xgent_tools = { path = "../xgent_tools" }
xgent_context = { path = "../xgent_context" }
xgent_settings = { path = "../xgent_settings" }
xgent_settings_core = { path = "../xgent_settings_core" }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
async-trait = { workspace = true }
thiserror = { workspace = true }
```

说明：agent 依赖 Bevy——它是 ECS 与异步逻辑的桥梁层。UI 侧使用。这里也引入 IPC 客户端（调 daemon）的轻量封装，但重连接逻辑可放 xgent_app 或单独模块。

## 关键类型与接口

### 1. events.rs — ECS 事件契约

```rust
use bevy::prelude::*;
use xgent_core::chat::{ChatEvent, ChatMessage};
use xgent_tools::confirm::ConfirmRequest;

/// 用户输入消息（UI → agent）
#[derive(Event)]
pub struct UserInputEvent { pub text: String }

/// 中断当前对话（UI → agent）
#[derive(Event)]
pub struct AbortEvent;

/// provider 流式 delta（agent → UI）
#[derive(Event)]
pub struct DeltaEvent { pub text: String }

/// 工具调用开始（agent → UI，展示工具执行中）
#[derive(Event)]
pub struct ToolCallEvent { pub tool_id: String, pub input: serde_json::Value }

/// 工具执行完成（agent → UI）
#[derive(Event)]
pub struct ToolResultEvent { pub tool_id: String, pub output: String, pub success: bool }

/// 需要用户确认（agent → UI，触发弹窗）
#[derive(Event)]
pub struct ConfirmRequestEvent(pub ConfirmRequest);

/// 用户确认决策（UI → agent）
#[derive(Event)]
pub struct ConfirmDecisionEvent { pub decision: ConfirmDecision }

/// 对话完成（agent → UI）
#[derive(Event)]
pub struct DoneEvent;

/// 对话出错（agent → UI）
#[derive(Event)]
pub struct ErrorEvent(pub String);
```

### 2. conversation.rs — 会话状态 Resource

```rust
use bevy::prelude::*;
use xgent_core::chat::{ChatMessage, Role};
use xgent_core::ids::SessionId;

#[derive(Resource)]
pub struct Conversation {
    pub id: SessionId,
    pub messages: Vec<ChatMessage>,
    pub status: ConversationStatus,
    pub current_assistant_text: String,  // 流式累加中的助手回复
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConversationStatus {
    Idle,        // 等待用户输入
    Thinking,    // 等待 provider 响应
    Streaming,   // 接收流式 delta
    ToolRunning, // 执行工具中
    Confirming,  // 等待用户确认
    Aborting,    // 中断中
    Error,
}
```

### 3. bridge.rs — tokio 桥接

```rust
use bevy::prelude::*;
use tokio::sync::mpsc;
use xgent_core::chat::{ChatRequest, ChatEvent};
use xgent_core::ids::StreamId;

/// 桥接 Resource：持有 tokio runtime handle + 与异步任务的 channel
#[derive(Resource)]
pub struct AgentBridge {
    pub runtime: tokio::runtime::Runtime,  // 或 Handle
    /// 发往 agent 异步任务的命令
    pub cmd_tx: mpsc::Sender<AgentCommand>,
}

/// 命令（ECS → 异步任务）
pub enum AgentCommand {
    StartLoop { req: ChatRequest },
    Abort,
    ConfirmDecision(ConfirmDecision),
}

/// 异步任务 → ECS 的事件 channel
pub enum AgentEvent {
    Delta(String),
    ToolCall { tool_id: String, input: serde_json::Value },
    ToolResult { tool_id: String, output: String, success: bool },
    ConfirmRequest(ConfirmRequest),
    Done,
    Error(String),
}
```

**桥接模式**：
- ECS 系统（每帧）从 `event_rx` 轮询事件，转成 Bevy Event。
- ECS 系统把用户输入/确认决策经 `cmd_tx` 发给异步任务。
- 异步任务（tokio spawn）：调 provider client → 收 ChatEvent → 转成 AgentEvent 发回 channel；遇工具调用则调 ToolExecutor（含确认流程）。

### 4. agent_loop.rs — 对话循环系统

```rust
use bevy::prelude::*;

/// 每帧轮询桥接 channel，分发事件到 ECS
pub fn agent_poll_system(
    bridge: ResMut<AgentBridge>,
    mut conv: ResMut<Conversation>,
    mut delta: EventWriter<DeltaEvent>,
    mut tool_call: EventWriter<ToolCallEvent>,
    mut tool_result: EventWriter<ToolResultEvent>,
    mut confirm: EventWriter<ConfirmRequestEvent>,
    mut done: EventWriter<DoneEvent>,
    mut error: EventWriter<ErrorEvent>,
    mut user_input: EventReader<UserInputEvent>,
    mut abort: EventReader<AbortEvent>,
    mut decision: EventReader<ConfirmDecisionEvent>,
) {
    // 1. 处理用户输入：构造 ChatRequest，经 bridge.cmd_tx 发 StartLoop
    //    - 构造时调 ContextProvider::retrieve 获取上下文（异步，可另起 task）
    // 2. 处理 abort：发 Abort 命令
    // 3. 处理确认决策：发 ConfirmDecision 命令
    // 4. 轮询 bridge 的事件 channel（非阻塞 try_recv）
    //    - Delta -> conv.current_assistant_text += text; 发 DeltaEvent
    //    - ToolCall -> conv.status=ToolRunning; 发 ToolCallEvent
    //    - ToolResult -> 发 ToolResultEvent（UI 展示结果）
    //    - ConfirmRequest -> conv.status=Confirming; 发 ConfirmRequestEvent
    //    - Done -> 把 current_assistant_text 存入 messages; conv.status=Idle; 发 DoneEvent
    //    - Error -> conv.status=Error; 发 ErrorEvent
}
```

### 5. format.rs — ChatRequest 构造

```rust
use xgent_core::chat::{ChatRequest, ChatMessage, Role};
use xgent_context::provider::ContextResult;

pub fn build_request(
    messages: &[ChatMessage],
    context: &ContextResult,
    provider: &str,
    model: &str,
    tools: Option<Vec<ToolSchema>>,
) -> ChatRequest {
    // 1. system message：agent 角色 + 可用工具说明
    // 2. 把 context.tree_summary 与 chunks 注入 system 或 context messages
    // 3. 组装 messages
    ChatRequest { /* ... */ }
}
```

### 6. lib.rs — Plugin

```rust
use bevy::prelude::*;

pub struct XgentAgentPlugin;

impl Plugin for XgentAgentPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_event::<UserInputEvent>()
            .add_event::<AbortEvent>()
            .add_event::<DeltaEvent>()
            .add_event::<ToolCallEvent>()
            .add_event::<ToolResultEvent>()
            .add_event::<ConfirmRequestEvent>()
            .add_event::<ConfirmDecisionEvent>()
            .add_event::<DoneEvent>()
            .add_event::<ErrorEvent>()
            .init_resource::<Conversation>()
            .init_resource::<AgentBridge>()  // 启动 tokio runtime 与 channel
            .add_systems(Update, agent_poll_system);
    }
}
```

## 实现要点

1. **agent loop 放 UI 侧**：每客户端独立对话循环，daemon 只做 provider 池。Conversation 是 UI 进程本地状态。
2. **异步桥接**：tokio runtime 作为 Bevy Resource（`AgentBridge`），ECS 系统每帧非阻塞轮询 channel。provider/tools/context 的异步调用都在 tokio task，结果经 channel 回 ECS。**这是关键桥接模式**。
3. **ECS 通信硬约束**：agent 与 UI 之间只通过 Events（即时）通信，不直接方法调用。用户输入、确认决策是 UI→agent 的 Event；delta、工具状态、完成、错误是 agent→UI 的 Event。
4. **流式累加**：`current_assistant_text` 在 delta 期间累加，Done 时存入 messages 历史。
5. **工具执行桥接**：ToolExecutor 在 tokio task 执行（异步），确认流程经 ConfirmRequest 事件回 ECS 弹窗，决策经 ConfirmDecisionEvent 回 task。task 等待 oneshot channel。
6. **上下文检索**：每次新对话轮，在构造 ChatRequest 前异步调 `ContextProvider::retrieve`，结果注入 system message。检索是异步 task，不阻塞 ECS 帧。
7. **中断**：Abort 命令通过 channel 通知 task；task 检测到 abort 信号后取消 provider 流（drop stream receiver）。
8. **会话持久化**：Done 时把 messages 存 SQLite（经 settings_core 的 sessions_db_path），MVP 可先内存，持久化在 step11/12 完善。
9. **不直接依赖 daemon 连接**：agent 通过 `ProviderClient`（IPC 封装）调 daemon，此封装可放 agent crate 或 xgent_app。MVP 先放 agent，用 trait 抽象使未来可换本地直连实现（单进程调试时）。

## 验证方法

1. **编译检查**：
   ```bash
   cargo check -p xgent_agent
   ```
2. **桥接测试**：mock 一个 ProviderClient（不连 daemon，本地假流式输出），驱动 agent loop，断言 DeltaEvent 序列与 DoneEvent。
3. **工具调用测试**：mock provider 返回 ToolCall，agent 调 ToolExecutor，断言 ToolCallEvent +（若需确认）ConfirmRequestEvent + ToolResultEvent。
4. **中断测试**：对话中发 AbortEvent，断言 agent 停止并状态回到 Idle。
5. **上下文注入测试**：mock ContextProvider 返回固定 chunks，断言 ChatRequest 的 messages 含上下文。

## 完成后下一步

xgent_agent 完成后 → 实现 **xui**（通用 Bevy UI 组件库），它被 xgent_ui 依赖。
