# oh-my-pi 对 XGent MVP 的借鉴分析

> 本文档基于 [oh-my-pi (omp)](./oh-my-pi-study.md) 的架构学习，分析对 XGent MVP 阶段的借鉴价值、需要实现的功能、以及需要做的架构/代码调整优化。
>
> 状态：分析报告 · 2026-07-16

---

## 0. 总体判断

omp 是一个极其成熟的 coding agent，其架构经过大量真实使用打磨。XGent 虽然技术栈不同（Rust + Bevy vs TypeScript + Bun），但 agent 核心逻辑高度可复用。关键借鉴分三层：

1. **直接可移植的设计模式**（agent loop、工具系统、会话模型）——架构层面的模式可直接用 Rust 实现
2. **MVP 需要但可简化的能力**（compaction、MCP、提示词管理）——MVP 简化版，留好接口
3. **MVP 不做但架构需预留的能力**（子 agent、LSP、DAP、记忆）——trait 抽象，不阻塞

**核心原则**：借鉴设计模式，不照搬复杂度。omp 有 32 个工具、40+ provider、55k 行 Rust native，XGent MVP 只需 4 个工具、3 个 provider、零 native 绑定。

---

## 1. Agent Loop — 借鉴与调整

### 1.1 omp 的做法

双层 while 循环：外层 follow-up/aside，内层 tool-call + steering。`stopReason` 不决定循环是否继续，`toolCalls` 是否存在才决定。中断信号三层分层（external > steering > IRC）。Push-based EventStream。

### 1.2 XGent MVP 应借鉴

| 模式 | 借鉴方式 | 优先级 |
|:---|:---|:---|
| **双层循环架构** | 外层 follow-up + 内层 tool-call，分离两个关注点 | MVP |
| **EventStream push-based 事件模型** | tokio mpsc channel + Bevy Event 双层桥接 | MVP |
| **中断信号分层** | external abort + steering abort（MVP 不需要 IRC） | MVP |
| **Snapshot 不可变性** | 消息经 channel 回 ECS 前深拷贝（`Arc<AgentMessage>` 即可） | MVP |
| **coerceToolResult 边界规范化** | untyped JSON 结果进入系统处强制类型安全 | MVP |
| **消费/非消费队列分离** | peek 不消费，dequeue 才消费 | P1 |
| **Soft tool requirement** | reminder-then-escalate | P1 |
| **Pause gate** | 进程级全局暂停 | P1 |
| **Owned dialect** | 不支持原生 function calling 的模型 | P2 |
| **AppendOnlyContext + StablePrefix** | prefix cache 优化 | P1 |

### 1.3 具体调整

**xgent_agent 当前设计的 AgentLoopSystem 需调整**：

```
// 当前设计（step9-xgent-agent.md）
AgentLoopSystem
  → ContextBuilder（取项目上下文）
  → ProviderClient（经 IPC 调 daemon）
  → 流式 chunk 经 channel 回灌 ECS（DeltaEvent）
  → 若响应含 ToolCall → ToolCallEvent → ConfirmSystem → ToolExecSystem
  → 回灌 ConversationSystem → 继续 AgentLoop

// 借鉴 omp 后调整
AgentLoopSystem（双层循环）
  外层：follow-up/aside 消息驱动
    内层：tool-call + steering
      → syncContextBeforeModelCall()（刷新 systemPrompt/tools）
      → streamAssistantResponse()
          → transformContext → convertToLlm → normalizeForProvider
          → ProviderClient（经 IPC 调 daemon）
          → 流式 chunk 经 channel 回灌 ECS（DeltaEvent）
          → stopReason 判定（toolCalls 存在才继续）
      → executeToolCalls()
          → 并发调度（shared/exclusive）
          → beforeToolCall hook → tool.execute(signal, onUpdate) → coerceToolResult → afterToolCall hook
          → ConfirmSystem 介入（NeedsConfirmation 时）
      → emitTurnEnd()
      → 轮询 steering → pendingMessages
  → endAgentStream
```

**关键调整点**：

1. **AgentMessage vs LLM Message 分离**：定义 `AgentMessage`（agent 内部消息，可含 UI-only 类型）与 `Message`（LLM 可理解的消息）。`convert_to_llm()` 在调用 LLM 前过滤/转换。这在 `xgent_core` 的类型设计中需体现。

2. **StopReason 语义**：`ChatEvent::Done(reason)` 的 `reason` 不决定循环继续——`tool_calls.is_empty()` 才决定。当前设计需调整 `ChatEvent` 的 `Done` 语义。

3. **流式可中断消费**：`streamAssistantResponse` 用 `tokio::select!` 实现 `Promise.race` 等效——流式 chunk 与 abort signal 竞争。

### 1.4 架构影响

```
xgent_core: 新增 AgentMessage 类型（Message + CustomMessages 扩展）
xgent_agent: AgentLoop 改为双层循环，新增 EventStream（tokio mpsc 封装）
xgent_agent: 新增 tool execution 生命周期 hooks（before/after）
```

---

## 2. 工具系统 — 借鉴与调整

### 2.1 omp 的做法

`AgentTool` 接口：name/label/description/parameters(Zod)/execute + approval/concurrency/loadMode/intent/interruptible。三层 tier（read/write/exec）× 三种 mode（always-ask/write/yolo）。shared/exclusive 并发调度。BM25 工具发现。ToolError 抛出式错误处理。ToolResultBuilder 链式构造。

### 2.2 XGent MVP 应借鉴

| 模式 | 借鉴方式 | 优先级 |
|:---|:---|:---|
| **Tool trait 设计** | `fn execute(&self, input, ctx, signal, on_update) -> ToolResult` | MVP |
| **Approval 三级 tier** | read/write/exec，当前设计已有 Approved/NeedsConfirmation/Denied | MVP（已设计） |
| **Approval mode 矩阵** | always-ask/write/yolo × tier，比当前三级更灵活 | P1 |
| **动态 approval** | `approval(args) -> ToolTier`，按参数决议（如 bash 检测危险命令） | P1 |
| **Concurrency shared/exclusive** | WriteFile 声明 exclusive，ReadFile/SearchFiles 声明 shared | P1 |
| **ToolError 抛出式错误** | 用 Rust `Result<ToolResult, ToolError>`，ToolError 可自定义 LLM 可见文本 | MVP |
| **coerceToolResult** | 第三方/MCP 工具结果在边界处强制规范化 | MVP |
| **ToolResultBuilder** | 链式构造，支持 isError/useless/meta 标记 | P1 |
| **工具超时配置表** | 每工具 default/min/max，clampTimeout 钳制 | P1 |
| **工具发现（BM25）** | discoverable 工具按需激活 | P2 |
| **Intent tracing** | 工具调用时声明意图 | P2 |

### 2.3 具体调整

**xgent_tools 当前设计的 Tool trait 需调整**：

```rust
// 当前设计（step7-xgent-tools.md）
trait Tool {
    fn id(&self) -> &str;
    fn schema(&self) -> ToolSchema;
    fn policy(&self) -> SecurityPolicy;       // Approved / NeedsConfirmation / Denied
    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolResult;
}

// 借鉴 omp 后调整
trait Tool {
    fn id(&self) -> &str;
    fn label(&self) -> &str;                  // UI 显示名
    fn description(&self) -> &str;
    fn schema(&self) -> ToolSchema;           // JSON Schema（给 LLM）
    
    // 安全策略：从静态三级改为动态 tier + mode
    fn approval(&self, args: &Value) -> ToolApproval;
    
    // 并发模式
    fn concurrency(&self, args: &Value) -> Concurrency { Concurrency::Shared }
    
    // 可中断性（MVP 不需要 IRC，但保留接口）
    fn interruptible(&self) -> bool { false }
    
    // 执行：signal + on_update 回调
    async fn execute(
        &self,
        tool_call_id: &str,
        params: Value,
        signal: AbortSignal,
        on_update: Option<&dyn Fn(ToolResult)>,
        ctx: &ToolCtx,
    ) -> Result<ToolResult, ToolError>;
}

// 安全策略改进
enum ToolTier { Read, Write, Exec }
enum ApprovalMode { AlwaysAsk, Write, Yolo }  // 比 Approved/NeedsConfirmation/Denied 更灵活

type ToolApproval = ToolTier 
    | ToolTierAndReason { tier: ToolTier, reason: Option<String>, override: bool }
    | fn(args: &Value) -> ToolApprovalDecision;
```

**关键调整点**：

1. **从静态 SecurityPolicy 改为动态 ToolApproval**：当前 `fn policy(&self) -> SecurityPolicy` 不够灵活。omp 的 bash 工具会检测 `rm -rf /` 等危险模式后升级到 exec + override。XGent 应支持 `fn approval(&self, args) -> ToolApproval`。

2. **execute 签名增加 signal + on_update**：当前 `async fn execute(&self, input, ctx) -> ToolResult` 缺少中断信号和流式更新。工具执行可能很慢（如 RunCommand），需要可中断 + 流式输出预览。

3. **ToolError 类型**：新增 `ToolError` 类型，`execute` 返回 `Result<ToolResult, ToolError>`。ToolError 可自定义 `render()` 给 LLM 的文本。非工具代码异常统一包装为 `isError: true` 的 ToolResult。

4. **并发调度**：MVP 的 4 个工具中 WriteFile/RunCommand 声明 exclusive（串行），ReadFile/SearchFiles 声明 shared（并行）。

### 2.4 MVP 工具清单调整

| 工具 | omp 对应 | tier | concurrency | 备注 |
|:---|:---|:---|:---|:---|
| ReadFile | read | read | shared | 已设计 |
| WriteFile | write | write | exclusive | 已设计，改 exclusive |
| SearchFiles | search | read | shared | 已设计 |
| RunCommand | bash | exec | exclusive | 已设计，改 exclusive |

MVP 不做：edit/hashline、ast_edit、lsp、debug、task、browser、web_search、MCP tools。

---

## 3. 会话管理 — 借鉴与调整

### 3.1 omp 的做法

JSONL append-only + 树形分支（id/parentId）。14 种 entry 类型。同步 append + 异步 drain。Title slot 固定 256 字节。多后端抽象（File/SQLite/Redis）。大内容处理（截断 + blob 外置 + 签名保护）。YieldQueue 后台事件延迟注入。ToolChoiceQueue 强制工具选择。

### 3.2 XGent MVP 应借鉴

| 模式 | 借鉴方式 | 优先级 |
|:---|:---|:---|
| **JSONL append-only 会话** | 替代当前 SQLite，更简单、更可恢复 | MVP（建议改） |
| **树形分支（id/parentId）** | 天然支持 session forking | P1 |
| **同步 append** | 方法返回时即持久化 | MVP |
| **Entry 类型设计** | message/compaction/model_change/thinking_level_change | MVP |
| **大内容截断** | 超长工具输出截断 + 占位符 | P1 |
| **签名块保护** | provider 的加密/签名内容不可截断 | P1 |
| **YieldQueue** | 后台事件延迟注入 | P1 |
| **ToolChoiceQueue** | 强制工具选择 | P2 |
| **多后端抽象** | SessionStorage trait | P2 |
| **Title slot** | 固定字节原地更新 | P2 |

### 3.3 具体调整

**当前设计（architecture.md 第 6.4 节）**：会话历史用 SQLite，存平台路径下 `sessions.db`。

**建议改为 JSONL append-only**：

理由：
1. **更简单**：JSONL 比 SQLite 少一个依赖，崩溃只丢最后一行（SQLite 崩溃可能损坏整个库）。
2. **天然 branching**：树形 id/parentId 比 SQLite 的外键更自然地支持 session forking/branching。
3. **可读性**：JSONL 可直接 `cat`/`grep` 调试，SQLite 需要 sqlite3 CLI。
4. **omp 验证**：omp 从早期 SQLite 迁移到 JSONL，实践证明可行。

**保留 SQLite 用于**：
- 元数据索引（session 列表、标题搜索、status 推导）
- prompt 历史（FTS5 全文搜索）
- 模型使用统计（token/cost）

```rust
// xgent_core 新增会话类型

// 会话文件格式：<platform_path>/xgent/sessions/<dir_encoded>/<timestamp>_<id>.jsonl
// 每行一个 JSON entry

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum SessionEntry {
    Header(SessionHeader),
    Message(SessionMessage),
    Compaction(CompactionEntry),
    ModelChange(ModelChangeEntry),
    // MVP 只需以上 4 种，其余 P1 追加
}

struct SessionHeader {
    id: String,
    version: u32,  // 当前 v1
    cwd: String,
    timestamp: DateTime<Utc>,
    title: Option<String>,
}

struct SessionMessage {
    id: String,
    parent_id: Option<String>,  // 树形结构
    timestamp: DateTime<Utc>,
    message: AgentMessage,
}

struct CompactionEntry {
    id: String,
    parent_id: String,
    timestamp: DateTime<Utc>,
    summary: String,
    first_kept_entry_id: String,
    tokens_before: u64,
}
```

### 3.4 会话恢复

MVP 实现最小恢复：
1. 解析 JSONL 文件
2. 沿 leaf→root 路径遍历 entries
3. 处理 CompactionEntry（summary 替代旧消息）
4. 重建 AgentMessage[]

---

## 4. 上下文压缩 — 借鉴与调整

### 4.1 omp 的做法

多触发条件（手动/溢出/阈值/mid-turn/idle）。LLM 摘要 + 工具输出修剪 + useless 结果 elision + shake（外科式上下文移除）。Snapcompact（bitmap 图片归档）。分支摘要。Display transcript（压缩不视觉重启）。

### 4.2 XGent MVP 应借鉴

| 模式 | 借鉴方式 | 优先级 |
|:---|:---|:---|
| **阈值触发压缩** | 成功 turn 后上下文超阈值自动压缩 | P1 |
| **手动压缩** | `/compact` 命令 | P1 |
| **工具输出修剪** | 旧工具输出用占位符替代 | P1 |
| **LLM 摘要** | 将旧消息序列化为文本 → LLM 生成摘要 | P1 |
| **Cut-point 逻辑** | 不在 toolResult 处切割 | P1 |
| **Display transcript** | 压缩不视觉重启对话 | P1 |
| **Useless-result elision** | 零匹配搜索结果标记 useless | P2 |
| **Snapcompact** | bitmap 归档 | 不做 |
| **Mid-turn 维护** | 工具循环中压缩 | P2 |

### 4.3 具体调整

**MVP 不实现 compaction**。MVP 阶段对话通常不会超长，且支持中断/重试（用户可手动开新会话）。但在 `xgent_agent` 中预留 `CompactionProvider` trait：

```rust
trait CompactionProvider {
    fn should_compact(&self, messages: &[AgentMessage], model: &Model) -> bool;
    async fn compact(&self, messages: &[AgentMessage], model: &Model) -> Result<CompactionResult>;
}

struct CompactionResult {
    summary: String,
    kept_messages: Vec<AgentMessage>,
    tokens_before: u64,
}
```

P1 实现默认 `LlmCompactionProvider`（LLM 摘要）。

---

## 5. Provider 抽象 — 借鉴与调整

### 5.1 omp 的做法

统一 `AssistantMessageEvent` 事件流（start/text_delta/thinking_delta/toolcall_delta/done/error）。40+ provider 适配器。Partial JSON 流式解析（throttled）。Stream 超时保护（first event + idle）。Auth retry（a/b/c 三 key 轮换）。Model catalog（内嵌 models.json）。

### 5.2 XGent MVP 应借鉴

| 模式 | 借鉴方式 | 优先级 |
|:---|:---|:---|
| **统一 ChatEvent 事件流** | Delta(text)/ToolCall/Thinking/Done(usage)/Error | MVP（已设计） |
| **Partial JSON 流式解析** | 工具调用参数流式解析（throttled） | P1 |
| **Stream 超时保护** | first event + idle timeout | P1 |
| **Auth retry** | API key 轮换 | P1 |
| **Model catalog** | 内嵌模型列表 + 元数据 | MVP |
| **Thinking/Reasoning** | Effort 级别统一接口 | P1 |
| **Tool Choice** | auto/none/any/required | MVP |
| **Service Tier** | flex/scale/priority | P2 |
| **Owned dialect** | 不支持原生 function calling 的模型 | P2 |
| **40+ provider** | MVP 只需 3-5 个 | MVP（简化） |

### 5.3 具体调整

**xgent_provider 当前设计的 LlmProvider trait 基本合理**，但需调整：

```rust
// 当前设计（step5-xgent-provider.md）
trait LlmProvider {
    fn id(&self) -> &str;
    fn list_models(&self) -> Vec<ModelInfo>;
    fn chat(&self, req: ChatRequest) -> ChatStream;  // tokio mpsc Receiver of ChatEvent
    fn health_check(&self) -> Result<()>;
}

// 借鉴 omp 后调整
trait LlmProvider {
    fn id(&self) -> &str;
    fn list_models(&self) -> Vec<ModelInfo>;
    
    // 增加 stream 超时配置
    fn chat(&self, req: ChatRequest, options: &StreamOptions) -> ChatStream;
    
    fn health_check(&self) -> Result<()>;
}

// ChatEvent 调整
enum ChatEvent {
    Start { model: Model },
    TextStart,
    TextDelta { delta: String },
    TextEnd,
    ThinkingStart,
    ThinkingDelta { delta: String },
    ThinkingEnd,
    ToolCallStart { index: usize },
    ToolCallDelta { index: usize, partial_json: String },
    ToolCallEnd { index: usize, tool_call: ToolCall },
    Done { reason: StopReason, usage: Usage },
    Error { error: LlmError },
}

enum StopReason {
    Stop,           // 正常结束
    ToolUse,        // 需要执行工具
    Length,         // max_tokens 截断
    Aborted,        // 被中断
    Error,          // 错误
}
```

**关键调整点**：

1. **ChatEvent 细化**：当前设计的 `Delta(text)` / `ToolCall(...)` / `Done(usage)` / `Error` 太粗。借鉴 omp 的细粒度事件（text_start/delta/end、toolcall_start/delta/end），UI 可以更精确地渲染流式内容。

2. **StopReason 语义**：`Done` 需携带 `reason`（Stop/ToolUse/Length/Aborted/Error），agent loop 根据 `tool_calls.is_empty()` 决定是否继续，而非仅看 reason。

3. **MVP provider 清单**：
   - `OpenAiCompatProvider`（OpenAI compatible：OpenAI、DeepSeek、Ollama 兼容模式）
   - `AnthropicProvider`（Anthropic 原生）
   - `CustomApiProvider`（自定义 endpoint/headers/body 模板）
   - Response API 和更多 provider 留 P1

---

## 6. 系统提示词 — 借鉴与调整

### 6.1 omp 的做法

静态 .md 模板 + Handlebars + 并行数据收集 + 超时降级 + 去重。上下文文件多源发现（AGENTS.md/CLAUDE.md 等 8 种格式）。@import 展开。Sticky rules（RULES.md）。Capability 控制反转发现系统。

### 6.2 XGent MVP 应借鉴

| 模式 | 借鉴方式 | 优先级 |
|:---|:---|:---|
| **提示词模板化** | .md 文件 + 模板引擎（Rust 用 `tera` 或 `askama`） | MVP |
| **编译期内联** | `include_str!` 内嵌 .md 文件 | MVP |
| **并行构建** | 各数据源并行加载 | P1 |
| **上下文文件发现** | AGENTS.md 自动发现 + 注入 | P1 |
| **@import 展开** | 上下文文件中 `@path` 内联引入 | P2 |
| **Sticky rules** | RULES.md 作为 always-apply rule | P2 |
| **Capability 系统** | 控制反转发现 | P2 |
| **超时降级** | 每步有超时和 fallback | P1 |
| **去重** | 字节级 + 段落级 | P1 |

### 6.3 具体调整

**MVP 最小实现**：

```rust
// xgent_agent: 系统提示词构建

// 模板文件：crates/xgent_agent/prompts/system.md（include_str! 内联）
// 动态变量：工具列表、环境信息、项目上下文

struct SystemPromptBuilder {
    template: &'static str,  // include_str!("prompts/system.md")
}

impl SystemPromptBuilder {
    fn build(&self, ctx: &PromptContext) -> Vec<String> {
        // MVP：简单模板渲染
        // P1：并行加载上下文文件 + 超时降级 + 去重
        vec![
            self.render_template(ctx),           // 主提示词
            self.render_project_context(ctx),     // 项目环境信息
        ]
    }
}
```

**MVP 提示词内容**（参考 omp 的 system-prompt.md 结构）：
- ROLE：定位为 AI code agent
- TOOL POLICY：工具使用规则（先读后写、确认机制）
- EXECUTION WORKFLOW：工作流程（理解→计划→执行→验证）
- DELIVERY CONTRACT：交付契约（不做半成品、不编造）

**P1：上下文文件发现**：支持项目根目录 `AGENTS.md` / `.xgent/AGENTS.md` 自动发现和注入。

---

## 7. MCP 集成 — 借鉴与预留

### 7.1 omp 的做法

三种传输（stdio/http/sse）。MCPManager 并行连接 + 延迟工具回退 + 重连断路器。工具桥接为 CustomTool。SQLite 工具缓存（config hash 失效）。OAuth 自动发现 + PKCE。

### 7.2 XGent MVP

MCP 是 P1 功能（F-13），MVP 不实现。但架构需预留：

```rust
// xgent_tools: MCP 预留接口

trait McpTransport: Send + Sync {
    async fn request(&self, method: &str, params: Value) -> Result<Value>;
    async fn notify(&self, method: &str, params: Value);
    async fn close(&self);
    fn is_connected(&self) -> bool;
}

// MCP 工具通过 Tool trait 接入，统一调度
struct McpTool {
    name: String,
    server: String,
    transport: Arc<dyn McpTransport>,
}

impl Tool for McpTool { ... }
```

P1 实现：stdio 传输（最常用）、工具桥接、基本连接管理。

---

## 8. 其他借鉴点

### 8.1 hashline 编辑模式

omp 的 hashline（content-hash 行锚点 patch）是一个重要的设计创新，显著减少编辑错误和 token 消耗。

**XGent MVP 不需要**（MVP 只有 WriteFile 全量写），但 P1 内置编辑器（F-11）上线时应考虑：
- 简化版 hashline（Rust 实现）
- 或 str_replace 模式（Aider 风格）
- 选择取决于模型兼容性和实测效果

### 8.2 子 Agent（task 工具）

omp 的 task 工具 fan out 子 agent，支持 schema-validated 结果。

**XGent MVP 不做**，但 `xgent_agent` 的 AgentLoop 设计应保证可实例化多个独立 agent（ECS 中每个 agent entity 独立）。P2 可加 task 工具。

### 8.3 TTSR（Time-Traveling Stream Rules）

规则在模型偏离时激活，中断流注入规则重试。

**XGent P1 可考虑**：规则系统（RULES.md → always-apply rule），但 MVP 不做流中断注入。

### 8.4 Advisor（第二模型审查）

**XGent P2**：可作为可选插件，但架构需保证 agent loop 可附加 advisor hook。

### 8.5 Native 绑定

omp 用 Rust native 实现 ripgrep/glob/find/brush，避免 fork-exec。

**XGent 用 Rust 全栈，天然无此问题**——直接用 `grep` crate / `ignore` crate（ripgrep 底层）/ `std::process::Command`。

---

## 9. 架构设计调整建议汇总

### 9.1 xgent_core 调整

| 调整 | 说明 |
|:---|:---|
| **新增 AgentMessage 类型** | Message + 可扩展的自定义消息类型，`convert_to_llm()` 转换 |
| **ChatEvent 细化** | text_start/delta/end、toolcall_start/delta/end、thinking_start/delta/end |
| **StopReason 枚举** | Stop/ToolUse/Length/Aborted/Error |
| **会话存储改 JSONL** | 从 SQLite 改为 JSONL append-only + 树形分支 |
| **ToolError 类型** | 工具执行错误类型，可自定义 LLM 可见文本 |
| **ToolTier 枚举** | Read/Write/Exec（替代静态 SecurityPolicy） |

### 9.2 xgent_agent 调整

| 调整 | 说明 |
|:---|:---|
| **AgentLoop 双层循环** | 外层 follow-up + 内层 tool-call |
| **EventStream（tokio mpsc 封装）** | push-based 事件流，消费者 async iterate |
| **中断信号分层** | external abort + steering abort |
| **工具执行生命周期 hooks** | before_tool_call / after_tool_call |
| **coerceToolResult** | 边界规范化 |
| **SystemPromptBuilder** | 模板化提示词构建 |
| **CompactionProvider trait（预留）** | P1 实现 LLM 摘要 |

### 9.3 xgent_tools 调整

| 调整 | 说明 |
|:---|:---|
| **Tool trait 签名调整** | 增加 signal + on_update，approval 改为动态 |
| **ToolApproval 动态** | `fn approval(&self, args) -> ToolApproval` |
| **Concurrency 声明** | shared/exclusive |
| **ToolResultBuilder** | 链式构造 result |
| **McpTransport trait（预留）** | P1 实现 |

### 9.4 xgent_provider 调整

| 调整 | 说明 |
|:---|:---|
| **ChatEvent 细化** | 对齐 omp 的细粒度事件 |
| **Stream 超时保护** | first event + idle timeout |
| **Partial JSON 流式解析** | 工具调用参数 throttled 解析 |
| **Model catalog** | 内嵌模型列表 + 元数据 |

### 9.5 会话存储调整

| 调整 | 说明 |
|:---|:---|
| **从 SQLite 改为 JSONL** | 会话历史用 JSONL append-only |
| **SQLite 保留用于** | 元数据索引、prompt 历史、模型使用统计 |
| **Entry 类型** | MVP：Header/Message/Compaction/ModelChange |
| **同步 append** | 方法返回时即持久化 |

---

## 10. MVP 实现优先级建议

基于 omp 的借鉴，对 XGent MVP 实现顺序的调整建议：

### 10.1 高优先级调整（影响 MVP 可用性）

1. **AgentMessage 类型设计**（xgent_core）：Message + 可扩展自定义类型，`convert_to_llm()` 转换。这是 agent loop 的基础。

2. **ChatEvent 细化**（xgent_core + xgent_provider）：细粒度流式事件，UI 精确渲染。

3. **AgentLoop 双层循环**（xgent_agent）：外层 follow-up + 内层 tool-call。

4. **Tool trait 签名**（xgent_tools）：增加 signal + on_update + 动态 approval + concurrency。

5. **会话存储改 JSONL**（xgent_core）：从 SQLite 改为 JSONL append-only。

### 10.2 中优先级调整（MVP 可用但体验更好）

6. **系统提示词模板化**（xgent_agent）：.md 文件 + include_str!。

7. **ToolError + coerceToolResult**（xgent_tools）：错误处理规范化。

8. **EventStream**（xgent_agent）：tokio mpsc 封装的 push-based 事件流。

9. **中断信号分层**（xgent_agent）：external + steering。

10. **Snapshot 不可变性**（xgent_agent）：Arc<AgentMessage>。

### 10.3 P1 预留接口（不实现但 trait 预留）

11. **CompactionProvider trait**（xgent_agent）。
12. **McpTransport trait**（xgent_tools）。
13. **ApprovalMode 矩阵**（xgent_tools）：always-ask/write/yolo × tier。
14. **Stream 超时保护**（xgent_provider）。
15. **上下文文件发现**（xgent_agent）：AGENTS.md。

---

## 11. 风险与注意事项

### 11.1 不要过度借鉴

omp 是一个极其成熟的项目，有很多经过大量使用打磨的复杂机制（snapcompact、TTSR、advisor、owned dialect、Harmony 泄漏处理等）。**XGent MVP 不应引入这些复杂度**。借鉴的是设计模式，不是实现细节。

### 11.2 技术栈差异

| 方面 | omp (TS/Bun) | XGent (Rust/Bevy) |
|:---|:---|:---|
| 异步 | Promise / async-await | tokio / async-await |
| 事件流 | EventStream class | tokio mpsc channel + Bevy Event |
| 类型系统 | TypeScript（declaration merging 扩展） | Rust（enum + trait，不支持 declaration merging） |
| 模板 | Handlebars（.md + import text） | tera/askama 或 include_str! + 手动替换 |
| Schema | Zod | serde_json + JSON Schema |
| 持久化 | Bun.file / bun:sqlite | std::fs / rusqlite |
| 进程模型 | 单进程（Bun） | 多进程（UI + daemon） |

**AgentMessage 扩展**：omp 用 TypeScript declaration merging 扩展自定义消息类型。Rust 不支持，需用 enum + 可扩展 trait 替代：

```rust
// Rust 方案：enum + trait
// xgent_core 定义基础 enum，xgent_agent 可通过 newtype 包装扩展
// 或用 trait object 方式：trait AgentMessage { fn to_llm(&self) -> Option<Message>; }
```

### 11.3 多进程差异

omp 是单进程架构。XGent 是多进程（UI + daemon）。agent loop 在 UI 侧，provider 在 daemon 侧。所有 agent loop 内对 provider 的调用都经 IPC。这意味着：

- omp 的 `streamSimple()` 直接调用变为 XGent 的 `ProviderClient::chat()` 经 IPC 调 daemon
- omp 的 `streamAssistantResponse()` 的 `for await (event of stream)` 变为 XGent 的从 tokio mpsc Receiver 消费
- IPC 层增加了一层间接，但不改变 agent loop 的核心逻辑

### 11.4 Bevy ECS 差异

omp 的 EventStream 消费者是 TUI 渲染器。XGent 的消费者是 Bevy ECS System。agent loop 在 tokio task 运行，结果经 channel 回 ECS：

```
tokio task (agent loop)
  → tokio mpsc channel
    → Bevy System（每帧 poll channel）
      → Bevy Event（DeltaEvent 等）
        → UI System（渲染）
```

这比 omp 的直接 subscribe 多一层，但保证了 ECS 的数据驱动原则和可测试性。

---

## 12. 总结

oh-my-pi 的核心价值在于它验证了一套 agent 架构模式在生产中的有效性。XGent MVP 应重点借鉴：

1. **Agent loop 双层循环** — 分离 follow-up 和 tool-call 两个关注点
2. **AgentMessage/Message 分离** — 允许 UI-only 消息类型，LLM 调用前转换
3. **细粒度流式事件** — UI 精确渲染
4. **动态 Tool Approval** — 按参数决议安全级别
5. **JSONL 会话存储** — 比 SQLite 更简单、更可恢复
6. **工具执行生命周期 hooks** — before/after 可拦截/修改
7. **coerceToolResult 边界规范化** — 防止持久化损坏
8. **中断信号分层 + 消费/非消费队列分离** — abort 后消息不丢失
9. **系统提示词模板化** — .md 文件 + 编译期内联
10. **Snapshot 不可变性** — UI/持久化/工具调度看到一致视图

**不借鉴**：snapcompact、TTSR、advisor、owned dialect、Harmony 处理、BM25 工具发现、多后端存储、Capability 系统——这些是 omp 的成熟期优化，XGent MVP 不需要。
