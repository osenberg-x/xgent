# 基于 oh-my-pi 借鉴的 MVP 优化方案

> 本文档基于 [oh-my-pi 学习报告](../notes/oh-my-pi-study.md) 与 [借鉴分析](../notes/oh-my-pi-borrowing-analysis.md)，对比当前项目已有代码实现，给出落地的架构与代码调整方案。
>
> 状态：优化方案 · 2026-07-16

---

## 0. 当前项目状态

项目已有完整 12 个 crate 的骨架实现，`cargo check --workspace` 通过。核心模块状态：

| crate | 状态 | 关键差距 |
|:---|:---|:---|
| xgent_core | ✅ 已实现 | ChatEvent 太粗（只有 Delta/ToolCall/Done/Error），无 StopReason，ChatMessage 只有 role+content |
| xui_i18n | ✅ 已实现 | — |
| xgent_settings_core | ✅ 已实现 | — |
| xgent_settings | ✅ 已实现 | — |
| xgent_provider | ✅ 已实现 | OpenAiCompat 已有 SSE 解析，无 Stream 超时、无 partial JSON 流式解析 |
| xgent_daemon | ✅ 已实现 | IPC 完整，无会话历史持久化（sessions.db 路径已定义但未使用） |
| xgent_tools | ✅ 已实现 | Tool trait 有静态 SecurityPolicy，无 signal/on_update/concurrency/ToolError |
| xgent_context | ✅ 已实现 | OnDemand 方案 A 已实现 |
| xgent_agent | ✅ 已实现 | 单层循环（run_conversation），无双层循环、无 steering、无 abort signal |
| xui | ✅ 已实现 | — |
| xgent_ui | ✅ 已实现 | — |
| xgent_app | ✅ 已实现 | — |

**核心判断**：项目已有可用骨架，但 agent 核心逻辑（loop、工具、会话）距 omp 借鉴的设计模式有明显差距。本方案聚焦这些差距，按优先级分批落地。

---

## 1. 优化项总览

| # | 优化项 | 影响 crate | 优先级 | 复杂度 |
|:---|:---|:---|:---|:---|
| O1 | ChatEvent 细化 + StopReason | xgent_core, xgent_provider, xgent_daemon, xgent_agent | P0 | 中 |
| O2 | AgentMessage 类型体系 | xgent_core, xgent_agent | P0 | 中 |
| O3 | Tool trait 增强（signal + on_update + concurrency + ToolError） | xgent_tools, xgent_agent | P0 | 高 |
| O4 | Agent Loop 双层循环 + abort signal | xgent_agent | P0 | 高 |
| O5 | 会话持久化 JSONL | xgent_core, xgent_agent | P1 | 中 |
| O6 | 系统提示词模板化 | xgent_agent | P1 | 低 |
| O7 | 工具 Approval 动态化 | xgent_tools, xgent_settings_core | P1 | 低 |
| O8 | Provider 流式增强 | xgent_provider | P1 | 中 |
| O9 | Compaction trait 预留 | xgent_agent | P2 | 低 |
| O10 | MCP transport trait 预留 | xgent_tools | P2 | 低 |

---

## 2. O1: ChatEvent 细化 + StopReason

### 2.1 问题

当前 `ChatEvent` 只有 4 个变体：

```rust
// crates/xgent_core/src/chat.rs — 当前
pub enum ChatEvent {
    Delta { text: String },
    ToolCall { id: String, name: String, args: serde_json::Value },
    Done { usage: TokenUsage },
    Error { kind: ErrorKind, message: String },
}
```

差距：
- **无 StopReason**：`Done` 不区分 Stop/ToolUse/Length/Aborted，agent loop 无法判断为何结束
- **无细粒度事件**：UI 无法区分 text_start/text_end、toolcall_start/toolcall_delta/toolcall_end
- **ToolCall 是一次性全量**：无流式参数增量（partial JSON），长参数时 UI 无反馈
- **无 Thinking 事件**：不支持推理模型

### 2.2 方案

```rust
// crates/xgent_core/src/chat.rs — 调整后

/// provider 流式输出的事件。
///
/// 借鉴 omp 的细粒度事件设计，UI 可精确渲染流式内容。
/// 事件序列：Start → (TextStart→TextDelta*→TextEnd |
///                   ThinkingStart→ThinkingDelta*→ThinkingEnd |
///                   ToolCallStart→ToolCallDelta*→ToolCallEnd)* → Done | Error
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatEvent {
    /// 流开始
    Start { model: String },

    // —— 文本块 ——
    TextStart,
    TextDelta { text: String },
    TextEnd,

    // —— 推理块（thinking models）——
    ThinkingStart,
    ThinkingDelta { text: String },
    ThinkingEnd,

    // —— 工具调用块 ——
    ToolCallStart { index: u32, id: String, name: String },
    ToolCallDelta { index: u32, partial_json: String },
    ToolCallEnd { index: u32, args: serde_json::Value },

    /// 流结束
    Done { reason: StopReason, usage: TokenUsage },
    /// 出错
    Error { kind: ErrorKind, message: String },
}

/// 流结束原因。
///
/// agent loop 不依赖 reason 决定是否继续——`tool_calls.is_empty()` 才决定。
/// reason 供 UI 展示和错误恢复参考。
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
```

### 2.3 影响范围

| 文件 | 调整 |
|:---|:---|
| `xgent_core/src/chat.rs` | ChatEvent 重构，新增 StopReason |
| `xgent_provider/src/openai_compat.rs` | SSE 解析发射细粒度事件 |
| `xgent_daemon/src/session.rs` | IPC notification 转发新事件类型 |
| `xgent_agent/src/bridge.rs` | AgentEvent 对齐，`run_conversation` 消费新事件 |
| `xgent_agent/src/agent_loop.rs` | `handle_agent_event` 处理新事件 |
| `xgent_agent/src/events.rs` | 新增 ThinkingMessage、ToolCallUpdateMessage 等 Bevy Message |
| `xgent_ui/src/chat_panel.rs` | 渲染 thinking 块、工具调用流式预览 |

### 2.4 兼容策略

- **OpenAI compatible 适配器**：SSE delta 中 `choices[0].delta.content` → TextDelta；`choices[0].delta.tool_calls` 按 index 聚合 → ToolCallStart/Delta/End；`finish_reason` 映射 StopReason
- **MVP 先不发射 Thinking 事件**：OpenAI compatible 不解析 reasoning，Thinking 事件留 P1 给 Anthropic 适配器
- **ToolCallDelta 的 partial JSON**：MVP 先不做 throttled 解析，仅在 ToolCallEnd 发全量 args。ToolCallDelta 发原始 partial_json 字符串，UI 可选展示

---

## 3. O2: AgentMessage 类型体系

### 3.1 问题

当前 `ChatMessage` 只有 `role: Role` + `content: String`，无法表达：
- 多模态内容（图片、工具调用内联在 assistant 消息中）
- 工具结果消息（role=Tool + tool_call_id + content）
- UI-only 消息类型（通知、artifact、压缩摘要等不发给 LLM 的消息）

**协议级 bug**：当前 `openai_compat.rs` 的 `message_to_json` 对 `Role::Tool` 只发 `{role, content}`，**缺 OpenAI 协议要求的 `tool_call_id` 字段**——连单轮 tool calling 都会被 OpenAI 拒绝（400）。

omp 通过 `AgentMessage`（含 UI-only 类型）+ `convertToLlm` 转换解决，但 **omp 的 LLM 层 `Message` 也是结构化的**（content 是 `ContentBlock[]`），不是纯字符串。`convertToLlm` 只过滤 UI-only 类型，保留结构化 content。

> **此方案已由 [ADR-0005](../decisions/0005-chatmessage-结构化-agentmessage-双层类型.md) 定案**：ChatMessage 改为结构化（role + Vec<ContentBlock>），对齐 omp 实际做法。原方案"保留 ChatMessage 为 content:String"违背 omp，已废弃。

### 3.2 方案

```rust
// crates/xgent_core/src/chat.rs — 调整后

/// LLM 层消息类型（provider 接收的格式）。
///
/// 结构化 content（对齐 Anthropic 协议原生形态）。
/// OpenAiCompat 的 message_to_json 按 role 展开为 OpenAI 协议形态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

/// Agent 内部消息类型。
///
/// 借鉴 omp 的 AgentMessage 设计：LLM 可理解的消息 + UI-only 扩展类型。
/// `convert_to_llm()` 在调用 LLM 前过滤 UI-only 类型，保留结构化 content。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum AgentMessage {
    User(UserMessage),
    Assistant(AssistantMessage),
    ToolResult(ToolResultMessage),
    /// 系统通知（UI-only，不发给 LLM）
    Notification(NotificationMessage),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub content: Vec<ContentBlock>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub content: Vec<ContentBlock>,
    pub model: Option<String>,
    pub usage: Option<TokenUsage>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultMessage {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationMessage {
    pub text: String,
    pub timestamp: u64,
}

/// 内容块（ChatMessage.content 与 AssistantMessage.content 共用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ContentBlock {
    Text { text: String },
    ToolCall { id: String, name: String, args: serde_json::Value },
    ToolResult { tool_call_id: String, content: String, is_error: bool },
    Image { data: String, mime_type: String },
}

/// 将 AgentMessage[] 转换为 LLM 可理解的 ChatMessage[]。
///
/// 只过滤 UI-only 消息（Notification），保留结构化 content（不压扁为字符串）。
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
```

**OpenAiCompat 的 message_to_json 按 role 展开**（关键：协议正确性）：

```rust
fn message_to_json(m: &ChatMessage) -> Value {
    match m.role {
        Role::System | Role::User => json!({
            "role": role_str(m.role),
            "content": blocks_to_text(&m.content),  // Text 块拼接
        }),
        Role::Assistant => {
            // OpenAI 协议：tool_calls 是顶层字段，非 content 内
            let text: String = m.content.iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                }).collect::<Vec<_>>().join("");
            let tool_calls: Vec<Value> = m.content.iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolCall { id, name, args } => Some(json!({
                        "id": id, "type": "function",
                        "function": { "name": name, "arguments": args.to_string() }
                    })),
                    _ => None,
                }).collect();
            let mut v = json!({ "role": "assistant", "content": text });
            if !tool_calls.is_empty() {
                v["tool_calls"] = json!(tool_calls);
            }
            v
        }
        Role::Tool => {
            // OpenAI 协议：tool role 消息必须带 tool_call_id
            let (tool_call_id, content) = m.content.iter()
                .find_map(|b| match b {
                    ContentBlock::ToolResult { tool_call_id, content, .. } =>
                        Some((tool_call_id.clone(), content.clone())),
                    _ => None,
                }).unwrap_or_default();
            json!({ "role": "tool", "content": content, "tool_call_id": tool_call_id })
        }
    }
}
```

### 3.3 影响范围

| 文件 | 调整 |
|:---|:---|
| `xgent_core/src/chat.rs` | ChatMessage 结构化（role+Vec<ContentBlock>）+ ContentBlock + AgentMessage 体系 + convert_to_llm |
| `xgent_provider/src/openai_compat.rs` | message_to_json 重写（按 role 展开 tool_calls/tool_call_id） |
| `xgent_agent/src/conversation.rs` | Conversation.messages 从 `Vec<ChatMessage>` 改为 `Vec<AgentMessage>` |
| `xgent_agent/src/bridge.rs` | run_conversation 调用前 convert_to_llm |
| `xgent_agent/src/format.rs` | build_request 使用 AgentMessage + convert_to_llm |

### 3.4 兼容策略

- **ChatMessage 结构化**：role + Vec<ContentBlock>，对齐 Anthropic 协议原生形态。OpenAiCompat 的 message_to_json 负责 OpenAI 协议形态展开（tool_calls 顶层字段、tool_call_id）。
- **AgentMessage 是 agent 层类型**：Conversation 持有 AgentMessage[]，调用 LLM 前经 convert_to_llm 过滤 UI-only + 保留结构。
- **MVP 先支持 Text + ToolCall + ToolResult 三种 ContentBlock**：Image 类型定义保留但 MVP 无图片输入 UI，OpenAiCompat 遇 Image block 报 `ProviderError::Config`。
- **Notification 消息**：MVP 用于 UI 显示系统通知（如"provider 已连接"、"会话已恢复"），不发给 LLM。
- **协议正确性优先**：此方案修复当前 message_to_json 缺 tool_call_id 的 bug，使多轮 tool calling 协议合法。
---

## 4. O3: Tool trait 增强

### 4.1 问题

当前 Tool trait：

```rust
// crates/xgent_tools/src/tool.rs — 当前
#[async_trait]
pub trait Tool: Send + Sync {
    fn id(&self) -> &str;
    fn schema(&self) -> ToolSchema;
    fn policy(&self) -> SecurityPolicy { SecurityPolicy::NeedsConfirmation }
    fn summarize(&self, input: &Value) -> String;
    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolResult;
}
```

差距：
- **无 abort signal**：工具执行不可中断（RunCommand 卡住时用户无法 abort）
- **无流式更新**：长时工具（RunCommand、SearchFiles 大库）无进度反馈
- **无并发声明**：所有工具串行执行，无法并行搜索
- **无 ToolError**：错误混在 ToolResult.success 里，无法区分"工具逻辑失败"和"代码异常"
- **静态 SecurityPolicy**：无法按参数决议（如 RunCommand 检测 `rm -rf` 升级为高危）

### 4.2 方案

```rust
// crates/xgent_tools/src/tool.rs — 调整后

/// 工具并发模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Concurrency {
    /// 可与其他 shared 工具并行
    Shared,
    /// 独占执行，等前序全部完成
    Exclusive,
}

/// 工具安全分级（借鉴 omp 的 tier）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTier {
    /// 读操作，无副作用
    Read,
    /// 写操作，修改 workspace/session 状态
    Write,
    /// 执行代码/shell，高危
    Exec,
}

/// 工具执行错误。
///
/// 可自定义 `render()` 给 LLM 的文本。
/// 非 ToolError 的 panic/未捕获异常由 agent loop 兜底为 isError ToolResult。
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{0}")]
    Failed(String),
    #[error("aborted")]
    Aborted,
    #[error("timeout after {0}s")]
    Timeout(u64),
}

/// 工具执行结果。
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// 给 LLM 的文本结果
    pub output: String,
    /// 是否成功（非异常的失败，如文件不存在）
    pub is_error: bool,
    /// 副作用通知
    pub side_effect: Option<SideEffect>,
}

/// 工具流式更新回调。
pub type ToolUpdateCallback = Box<dyn Fn(ToolResult) + Send + Sync>;

/// 工具抽象 trait。
#[async_trait]
pub trait Tool: Send + Sync {
    fn id(&self) -> &str;
    fn schema(&self) -> ToolSchema;

    /// 安全分级（静态）。动态分级用 `approval_for`。
    fn tier(&self) -> ToolTier;

    /// 按参数动态决议安全分级。
    ///
    /// 默认返回 `tier()` 的静态值。
    /// 工具可 override 实现参数级分级（如 RunCommand 检测危险命令）。
    fn approval_for(&self, input: &Value) -> ToolTier {
        self.tier()
    }

    /// 并发模式。默认 Shared。
    fn concurrency(&self) -> Concurrency {
        Concurrency::Shared
    }

    /// 对输入生成人类可读摘要（确认弹窗展示）。
    fn summarize(&self, input: &Value) -> String;

    /// 异步执行工具。
    ///
    /// - `signal`：中断信号，工具应定期检查或传递给子进程
    /// - `on_update`：流式进度回调（可选）
    async fn execute(
        &self,
        input: Value,
        ctx: &ToolCtx,
        signal: tokio_util::sync::CancellationToken,
        on_update: Option<&ToolUpdateCallback>,
    ) -> Result<ToolResult, ToolError>;
}
```

### 4.3 内置工具调整

| 工具 | tier | concurrency | 关键调整 |
|:---|:---|:---|:---|
| ReadFile | Read | Shared | 传 signal（支持 abort） |
| WriteFile | Write | Exclusive | 传 signal |
| SearchFiles | Read | Shared | 传 signal + on_update（进度） |
| RunCommand | Exec | Exclusive | `approval_for` 检测危险模式 + signal 传子进程 + on_update（输出流） |

RunCommand 的动态 approval 示例：

```rust
impl Tool for RunCommand {
    fn tier(&self) -> ToolTier { ToolTier::Exec }

    fn approval_for(&self, input: &Value) -> ToolTier {
        if let Some(cmd) = input["command"].as_str() {
            // 检测高危模式
            if cmd.contains("rm -rf") || cmd.contains("sudo") || cmd.contains("mkfs") {
                return ToolTier::Exec; // 始终需确认（即使配置了 yolo）
            }
        }
        ToolTier::Exec
    }
}
```

### 4.4 SecurityPolicy 适配

当前 `SecurityPolicy`（Approved/NeedsConfirmation/Denied）保留作为**运行时决议结果**，从 `ToolTier` + 用户配置推导：

```rust
// security.rs — 调整后

/// 运行时安全策略决议。
pub fn resolve_policy(
    tool_id: &str,
    tier: ToolTier,
    input: &Value,
    tool: &dyn Tool,
    policy: &ToolPolicyConfig,
) -> SecurityPolicy {
    // 1. 配置显式 denied
    if policy.denied.iter().any(|t| t == tool_id) {
        return SecurityPolicy::Denied;
    }
    // 2. 配置显式 approved
    if policy.approved.iter().any(|t| t == tool_id) {
        return SecurityPolicy::Approved;
    }
    // 3. 动态 tier（工具可按参数升级）
    let effective_tier = tool.approval_for(input);
    // 4. 按 tier 决议（MVP 默认全部 NeedsConfirmation）
    match effective_tier {
        ToolTier::Read => SecurityPolicy::NeedsConfirmation,   // MVP 默认需确认
        ToolTier::Write => SecurityPolicy::NeedsConfirmation,
        ToolTier::Exec => SecurityPolicy::NeedsConfirmation,
    }
    // P1：引入 ApprovalMode（always-ask/write/yolo）后，Read 在 yolo 下自动批准
}
```

### 4.5 影响范围

| 文件 | 调整 |
|:---|:---|
| `xgent_tools/src/tool.rs` | Tool trait 重构 + 新增 Concurrency/ToolTier/ToolError |
| `xgent_tools/src/security.rs` | resolve_policy 接受 ToolTier + tool 引用 |
| `xgent_tools/src/executor.rs` | ToolExecutor 传 signal + on_update，并发调度 |
| `xgent_tools/src/builtins/*.rs` | 4 个内置工具实现新 trait |
| `xgent_agent/src/bridge.rs` | run_conversation 传 CancellationToken 给 executor |

### 4.6 依赖新增

```toml
# xgent_tools/Cargo.toml
tokio-util = { workspace = true }  # CancellationToken
```

---

## 5. O4: Agent Loop 双层循环 + abort signal

### 5.1 问题

当前 `run_conversation`（bridge.rs:175-227）是**单层单次循环**：
- 收到 ChatEvent → 转发 → 遇 ToolCall 执行 → 遇 Done break
- **不循环**：工具执行后不自动继续下一轮 LLM 调用
- **无 abort signal**：`AgentCommand::Abort` 只是发 Done 事件，不真正中断
- **无 steering**：用户无法在 agent 执行中插话
- **无 follow-up**：无外层循环处理后续消息

### 5.2 方案

将 `run_conversation` 改为双层循环结构：

```rust
// crates/xgent_agent/src/bridge.rs — 调整后

/// 驱动 agent 对话循环（双层）。
///
/// 外层：follow-up 消息驱动（agent 准备停止时注入新消息继续）
/// 内层：tool-call + steering（核心 prompt → LLM → tool → continue）
async fn run_agent_loop(
    provider: &Arc<dyn ProviderClient>,
    executor: &Arc<ToolExecutor>,
    ctx: &ToolCtx,
    initial_req: ChatRequest,
    event_tx: &mpsc::Sender<AgentEvent>,
    shared_confirm: &SharedConfirm,
    cancel_token: tokio_util::sync::CancellationToken,
    steering_rx: &mut mpsc::Receiver<AgentCommand>,  // steering/follow-up 消息
) {
    let mut req = initial_req;
    let mut pending_messages: Vec<ChatMessage> = Vec::new();

    // 外层循环：follow-up 驱动
    loop {
        let mut has_more_tool_calls = true;

        // 内层循环：tool-call + steering
        while has_more_tool_calls || !pending_messages.is_empty() {
            // 注入 pending messages（steering）
            if !pending_messages.is_empty() {
                req.messages.extend(pending_messages.drain(..));
            }

            // 流式调用 LLM
            let tool_calls = match stream_llm_response(
                provider, &req, event_tx, &cancel_token,
            ).await {
                Ok(tc) => tc,
                Err(e) => {
                    let _ = event_tx.send(AgentEvent::Error { kind: e.0, message: e.1 }).await;
                    return;
                }
            };

            if tool_calls.is_empty() {
                has_more_tool_calls = false;
            } else {
                // 执行工具调用
                let tool_results = execute_tool_calls(
                    executor, ctx, &tool_calls, event_tx, shared_confirm, &cancel_token,
                ).await;

                // 将工具结果回灌为新的 messages
                for (call, result) in tool_calls.into_iter().zip(tool_results) {
                    req.messages.push(ChatMessage {
                        role: Role::Assistant,
                        content: format!("{{\"tool_call\":\"{}\"}}", call.name), // 简化
                    });
                    req.messages.push(ChatMessage {
                        role: Role::Tool,
                        content: result.output,
                    });
                }
                has_more_tool_calls = true;
            }

            // 轮询 steering（非阻塞）
            while let Ok(cmd) = steering_rx.try_recv() {
                match cmd {
                    AgentCommand::Steering(msg) => pending_messages.push(msg),
                    AgentCommand::Abort => {
                        cancel_token.cancel();
                        let _ = event_tx.send(AgentEvent::Done).await;
                        return;
                    }
                    _ => {}
                }
            }
        }

        // 外层：检查 follow-up
        match steering_rx.recv().await {
            Some(AgentCommand::FollowUp(msg)) => {
                req.messages.push(msg);
                continue; // 继续外层循环
            }
            Some(AgentCommand::Abort) | None => {
                let _ = event_tx.send(AgentEvent::Done).await;
                return;
            }
            _ => {}
        }
    }
}

/// 流式调用 LLM，返回工具调用列表。
async fn stream_llm_response(
    provider: &Arc<dyn ProviderClient>,
    req: &ChatRequest,
    event_tx: &mpsc::Sender<AgentEvent>,
    cancel_token: &CancellationToken,
) -> Result<Vec<ToolCallInfo>, (ErrorKind, String)> {
    let (_sid, mut stream) = provider.chat(req.clone()).await
        .map_err(|(k, m)| (k, m))?;

    let mut tool_calls = Vec::new();
    let mut current_text = String::new();

    loop {
        tokio::select! {
            // 流式事件
            ev = stream.recv() => {
                match ev {
                    Some(ChatEvent::TextDelta { text }) => {
                        current_text.push_str(&text);
                        let _ = event_tx.send(AgentEvent::Delta(text)).await;
                    }
                    Some(ChatEvent::ToolCallEnd { index, args }) => {
                        // 收集工具调用
                        tool_calls.push(ToolCallInfo { index, args });
                    }
                    Some(ChatEvent::Done { reason, .. }) => {
                        // tool_calls 非空则继续（不管 reason）
                        return Ok(tool_calls);
                    }
                    Some(ChatEvent::Error { kind, message }) => {
                        return Err((kind, message));
                    }
                    None => return Ok(tool_calls),
                    _ => {} // 其他事件忽略（MVP）
                }
            }
            // abort 信号
            _ = cancel_token.cancelled() => {
                let _ = event_tx.send(AgentEvent::Done).await;
                return Ok(Vec::new()); // 空工具调用 → 停止
            }
        }
    }
}
```

### 5.3 AgentCommand 调整

```rust
// crates/xgent_agent/src/bridge.rs — AgentCommand 调整

pub enum AgentCommand {
    /// 启动新对话
    StartLoop { req: ChatRequest },
    /// 中断当前对话
    Abort,
    /// 确认决策
    ConfirmDecision(ConfirmDecision),
    /// Steering：用户在 agent 执行中插话（注入到当前对话）
    Steering(ChatMessage),
    /// Follow-up：agent 停止后注入后续消息
    FollowUp(ChatMessage),
}
```

### 5.4 影响范围

| 文件 | 调整 |
|:---|:---|
| `xgent_agent/src/bridge.rs` | run_conversation → run_agent_loop 双层循环 + abort |
| `xgent_agent/src/agent_loop.rs` | agent_poll_system 处理新的 AgentCommand 变体 |
| `xgent_agent/src/events.rs` | 新增 SteeringMessage Bevy Message |

### 5.5 兼容策略

- **MVP 先不实现 steering 的中断逻辑**：steering 消息在工具执行完成后注入（interruptMode = "wait"），不中断正在执行的工具
- **abort 通过 CancellationToken**：`AgentCommand::Abort` 调用 `cancel_token.cancel()`，流式消费 `tokio::select!` 检测
- **工具 abort**：CancellationToken 传给 ToolExecutor，再传给 Tool::execute 的 signal

---

## 6. O5: 会话持久化 JSONL

### 6.1 问题

当前 `Conversation` 是纯内存状态（`messages: Vec<ChatMessage>`），无持久化。`sessions_db_path()` 已定义但未使用。架构设计文档（6.4 节）说用 SQLite，但借鉴分析建议改 JSONL。

### 6.2 方案

MVP 采用 JSONL append-only，SQLite 留给元数据索引（P1）。

```rust
// crates/xgent_core/src/session.rs — 新增

/// 会话文件格式：<platform_path>/xgent/sessions/<dir_encoded>/<timestamp>_<id>.jsonl
/// 每行一个 JSON entry。

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SessionEntry {
    /// 文件首行：会话头
    Header(SessionHeader),
    /// 对话消息
    Message(SessionMessage),
    /// 模型切换
    ModelChange(ModelChangeEntry),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHeader {
    pub id: String,
    pub version: u32,
    pub cwd: String,
    pub timestamp: u64,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub id: String,
    pub parent_id: Option<String>,
    pub timestamp: u64,
    pub message: AgentMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelChangeEntry {
    pub id: String,
    pub parent_id: String,
    pub timestamp: u64,
    pub model: String,
}
```

```rust
// crates/xgent_agent/src/session_store.rs — 新增

/// 会话存储：JSONL append-only。
pub struct SessionStore {
    path: PathBuf,
    file: Option<std::fs::File>,
}

impl SessionStore {
    pub fn open(path: PathBuf) -> io::Result<Self> { /* ... */ }

    /// 同步 append 一行 JSONL。
    /// 方法返回时即持久化（不丢数据）。
    pub fn append(&mut self, entry: &SessionEntry) -> io::Result<()> {
        let line = serde_json::to_string(entry)?;
        writeln!(self.file.as_mut().unwrap(), "{line}")
    }

    /// 加载全部 entries。
    pub fn load_all(&self) -> io::Result<Vec<SessionEntry>> { /* ... */ }
}
```

### 6.3 影响范围

| 文件 | 调整 |
|:---|:---|
| `xgent_core/src/session.rs` | 新增 SessionEntry 体系 |
| `xgent_core/src/lib.rs` | 导出 session 模块 |
| `xgent_agent/src/session_store.rs` | 新增 SessionStore |
| `xgent_agent/src/conversation.rs` | Done 时调用 SessionStore.append |
| `xgent_agent/src/bridge.rs` | agent_loop_task 持有 SessionStore |

### 6.4 兼容策略

- **MVP 只持久化 message entry**：每次 assistant 消息完成（Done）时 append
- **MVP 不实现恢复**：恢复/重放留 P1（需要 buildSessionContext 逻辑）
- **MVP 不实现 compaction entry**：Compaction 留 P1

---

## 7. O6: 系统提示词模板化

### 7.1 问题

当前 `format.rs` 的 `build_request` 中系统提示词是硬编码或简单的拼接。无模板化、无项目上下文注入。

### 7.2 方案

```rust
// crates/xgent_agent/src/prompts/system.md — 新增（include_str! 内联）

// 模板内容：
// # XGent Agent
// 你是一个 AI code agent，帮助开发者完成日常编码任务。
//
// ## 工具使用规则
// - 先读后写：修改文件前先读取确认内容
// - 确认机制：高危操作需用户确认后执行
//
// ## 工作流程
// 1. 理解：明确用户需求
// 2. 计划：制定修改方案
// 3. 执行：使用工具完成修改
// 4. 验证：确认修改正确
//
// ## 交付契约
// - 不做半成品：完成任务后再回复
// - 不编造：不确定时明确说明
```

```rust
// crates/xgent_agent/src/format.rs — 调整后

const SYSTEM_PROMPT: &str = include_str!("prompts/system.md");
const PROJECT_CONTEXT_TEMPLATE: &str = include_str!("prompts/project-context.md");

pub fn build_request(
    messages: &[AgentMessage],
    context: &ContextResult,
    provider: &str,
    model: &str,
    tools: Option<Vec<ToolSchema>>,
) -> ChatRequest {
    let system = format!(
        "{system_prompt}\n\n## 项目上下文\n{project_context}",
        system_prompt = SYSTEM_PROMPT,
        project_context = render_project_context(context),
    );

    let mut chat_messages = vec![ChatMessage {
        role: Role::System,
        content: system,
    }];

    // convert_to_llm 转换
    chat_messages.extend(convert_to_llm(messages));

    ChatRequest {
        provider: provider.to_string(),
        model: model.to_string(),
        messages: chat_messages,
        tools,
    }
}
```

### 7.3 影响范围

| 文件 | 调整 |
|:---|:---|
| `xgent_agent/src/prompts/system.md` | 新增 |
| `xgent_agent/src/prompts/project-context.md` | 新增 |
| `xgent_agent/src/format.rs` | 使用 include_str! + 模板渲染 |

---

## 8. O7: 工具 Approval 动态化

### 8.1 问题

当前 `SecurityPolicy` 是静态的 `fn policy(&self) -> SecurityPolicy`，无法按参数决议。

### 8.2 方案

在 O3（Tool trait 增强）中已设计：`fn tier(&self) -> ToolTier` + `fn approval_for(&self, input: &Value) -> ToolTier`。security.rs 的 `resolve_policy` 从 ToolTier + 配置推导 SecurityPolicy。

MVP 保持"默认全部 NeedsConfirmation"，P1 引入 ApprovalMode（always-ask/write/yolo）。

### 8.3 影响范围

已包含在 O3 中。

---

## 9. O8: Provider 流式增强

### 9.1 问题

当前 OpenAiCompat 的 SSE 解析只发射 `ChatEvent::Delta` 和 `ChatEvent::ToolCall`（全量），无细粒度事件、无 Stream 超时。

### 9.2 方案

```rust
// crates/xgent_provider/src/openai_compat.rs — 调整后

async fn chat(&self, req: ChatRequest) -> Result<(StreamId, ChatStream), ProviderError> {
    let (tx, rx) = mpsc::channel(128);
    let client = self.client.clone();
    let url = format!("{}/chat/completions", self.api_base);
    let body = self.build_body(&req);

    tokio::spawn(async move {
        let result = async {
            let resp = client.post(&url)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send().await?;

            // Stream 超时：首事件 + idle
            let mut stream = resp.bytes_stream();
            let _ = tx.send(ChatEvent::Start { model: req.model.clone() }).await;

            // ... SSE 解析，发射细粒度事件 ...
            // choices[0].delta.content → TextDelta
            // choices[0].delta.tool_calls → 按 index 聚合 ToolCallStart/Delta/End
            // finish_reason → StopReason 映射
        }.await;

        match result {
            Ok(()) => { /* Done 已在循环内发送 */ }
            Err(e) => {
                let _ = tx.send(ChatEvent::Error {
                    kind: ErrorKind::Network,
                    message: e.to_string(),
                }).await;
            }
        }
    });

    Ok((StreamId::new(), rx))
}
```

### 9.3 StopReason 映射

| OpenAI finish_reason | StopReason |
|:---|:---|
| `stop` | Stop |
| `tool_calls` | ToolUse |
| `length` | Length |
| (abort) | Aborted |
| (error) | Error |

### 9.4 影响范围

| 文件 | 调整 |
|:---|:---|
| `xgent_provider/src/openai_compat.rs` | SSE 解析发射细粒度事件 + StopReason |
| `xgent_provider/src/sse.rs` | 辅助解析 tool_calls delta 聚合 |

---

## 10. O9-O10: P2 预留接口

### 10.1 CompactionProvider trait

```rust
// crates/xgent_agent/src/compaction.rs — 新增（P2 预留）

/// 上下文压缩抽象。
pub trait CompactionProvider: Send + Sync {
    fn should_compact(&self, messages: &[AgentMessage], model: &str) -> bool;
    async fn compact(&self, messages: &[AgentMessage], model: &str)
        -> Result<CompactionResult, CompactionError>;
}

pub struct CompactionResult {
    pub summary: String,
    pub kept_messages: Vec<AgentMessage>,
}
```

MVP 不实现，仅定义 trait。P1 实现 `LlmCompactionProvider`。

### 10.2 McpTransport trait

```rust
// crates/xgent_tools/src/mcp.rs — 新增（P2 预留）

/// MCP 传输层抽象。
#[async_trait]
pub trait McpTransport: Send + Sync {
    async fn request(&self, method: &str, params: Value) -> Result<Value>;
    async fn notify(&self, method: &str, params: Value);
    async fn close(&self);
    fn is_connected(&self) -> bool;
}
```

MVP 不实现，仅定义 trait。P1 实现 stdio 传输 + 工具桥接。

---

## 11. 实施顺序

### 阶段一：P0 核心 agent 能力（让 agent 真正可用）

```
O1 ChatEvent 细化 → O2 AgentMessage → O3 Tool trait 增强 → O4 Agent Loop 双层循环
```

这四项有依赖顺序：ChatEvent 是基础 → AgentMessage 依赖 ChatEvent → Tool trait 独立但 agent loop 依赖它 → Agent Loop 整合前三者。

完成后：agent 能真正多轮对话、工具调用后自动继续、可中断、有流式进度。

### 阶段二：P1 体验增强

```
O5 会话持久化 → O6 系统提示词模板化 → O7 Approval 动态化 → O8 Provider 流式增强
```

这些独立性强，可并行实施。

### 阶段三：P2 预留

```
O9 Compaction trait → O10 MCP trait
```

仅定义 trait，不实现。

---

## 12. 风险与缓解

### 12.1 ChatEvent 重构的连锁影响

ChatEvent 是跨进程协议类型（UI ↔ daemon），重构影响 daemon 的 IPC notification 转发。

**缓解**：JSON-RPC notification 用 `#[serde(tag = "type")]`，新事件类型向后兼容——旧客户端忽略未知 type。daemon 侧只需透传 JSON，不解析 ChatEvent 内部结构。

### 12.2 Tool trait 签名变化的破坏性

Tool trait 签名变化导致所有内置工具 + executor + bridge 全部需调整。

**缓解**：一次性完成，不保留旧 trait。4 个内置工具 + 1 个 executor + 1 个 bridge，影响面可控。

### 12.3 Agent Loop 双层循环的复杂度

双层循环 + abort signal + steering 是 omp 的核心复杂度来源。

**缓解**：MVP 简化版——steering 只在工具执行完成后注入（interruptMode = "wait"），不中断正在执行的工具。不实现消费/非消费队列分离、pause gate、soft tool requirement。

### 12.4 JSONL vs SQLite 决策

借鉴分析建议改 JSONL，但架构设计文档（6.4 节）说用 SQLite。

**缓解**：会话历史用 JSONL（主存储），元数据索引/prompt 历史/模型使用统计保留 SQLite（P1）。两套存储各司其职，不冲突。需更新架构设计文档 6.4 节。

---

## 13. 文档更新清单

完成本方案后需更新的文档：

| 文档 | 更新内容 |
|:---|:---|
| `doc/design/architecture.md` | 6.4 节会话存储改 JSONL；6.1 节 ChatEvent 细化；6.2 节 Tool trait 增强 |
| `doc/plans/step1-xgent-core.md` | ChatEvent 新变体、StopReason、AgentMessage 体系 |
| `doc/plans/step5-xgent-provider.md` | 流式细粒度事件、StopReason 映射 |
| `doc/plans/step7-xgent-tools.md` | Tool trait 新签名、ToolTier、Concurrency、ToolError |
| `doc/plans/step9-xgent-agent.md` | 双层循环、abort signal、steering、SessionStore |
| `doc/decisions/` | 新增 ADR：会话存储 JSONL 决策、ChatEvent 细化决策 |
