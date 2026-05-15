# XGent — 下一代沉浸式 AI Code Agent

> 基于 Bevy 游戏引擎构建的沉浸式 AI 编程助手，将代码库可视化为可交互的 3D 宇宙，让开发者在"探索-构建-回溯"的闭环中高效编码。

---

## 目录

- [核心设计理念](#-核心设计理念)
- [系统架构全景](#-系统架构全景)
- [Agent 核心引擎](#-agent-核心引擎)
- [AI Provider 与协议层](#-ai-provider-与协议层)
- [ECS 数据模型与事件契约](#-ecs-数据模型与事件契约)
- [代码智能管线](#-代码智能管线)
- [特色功能详细设计](#-特色功能详细设计)
- [UI 布局与交互设计](#-ui-布局与交互设计)
- [键盘优先工作流](#-键盘优先工作流)
- [插件与扩展体系](#-插件与扩展体系)
- [安全模型](#-安全模型)
- [性能工程](#-性能工程)
- [技术栈与依赖](#-技术栈与依赖)
- [实现路线图](#-实现路线图)

---

### 🎨 核心设计理念

跳出当前主流 AI 编程助手的框架（聊天面板 + 代码编辑器），围绕四大支柱构建下一代体验：

| 支柱 | 核心命题 | 用户价值 |
|:---|:---|:---|
| **3D 可视化** | 代码不再是平面的文本列表，而是可探索的空间 | 直觉理解项目结构，一眼发现架构问题 |
| **时空回溯** | 每次操作都是可回溯的时间锚点 | 安全试错，精确定位问题引入点 |
| **数据驱动 UI** | UI 即 Agent 状态的实时投影 | 零延迟响应，交互逻辑可推理、可测试 |
| **沉浸式协作** | 人与 AI 在同一空间中工作 | AI 思考过程透明化，信任度提升 |

**设计原则**：

1. **可玩性优先**：每一次交互都应该有"操作感"——3D 飞行有惯性，节点点选有弹性反馈，时间回溯有拖拽手势。
2. **实用底线**：酷炫但不失效率。3D 视图与 2D 面板无缝切换；所有 3D 操作都有对应的键盘快捷键；核心编辑流绝不被 3D 动画阻塞。
3. **渐进式沉浸**：用户可以从纯 2D 模式起步，按需解锁 3D 层。不强制 3D，但 3D 始终是"更酷"的选项。

---

### ⚙️ 系统架构全景

#### 分层架构

```
┌───────────────────────────────────────────────────────────┐
│                     Presentation Layer                     │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │ 3D Viewport │  │ 2D Panels    │  │ Overlay/HUD      │  │
│  │ (Bevy Scene) │  │ (bevy_ui)    │  │ (Notifications)  │  │
│  └──────┬───────┘  └──────┬───────┘  └────────┬─────────┘  │
│         │                 │                    │            │
├─────────┴─────────────────┴────────────────────┴────────────┤
│                      Interaction Layer                      │
│  Input Router │ Command Palette │ Gesture Recognizer        │
├──────────────────────────────────────────────────────────────┤
│                      Application Layer                      │
│  ┌──────────┐  ┌──────────────┐  ┌───────────────────────┐ │
│  │ Agent    │  │ Timeline     │  │ Code Intelligence     │ │
│  │ Orchestr.│  │ Manager     │  │ Pipeline               │ │
│  └────┬─────┘  └──────┬──────┘  └──────────┬────────────┘ │
│       │               │                     │              │
├───────┴───────────────┴─────────────────────┴───────────────┤
│                       Infrastructure Layer                   │
│  ┌───────────────┐  ┌──────────┐  ┌──────────────────────┐  │
│  │ Provider Mgr  │  │ Protocol │  │ External Tool Bridge │  │
│  │ (模型/路由/   │  │ (MCP/ACP │  │ (MCP Server/Custom)  │  │
│  │  计费/降级)   │  │  /OpenAI)│  │                      │  │
│  └───────────────┘  └──────────┘  └──────────────────────┘  │
│  Git Bridge │ File System Watcher │ Plugin RT              │
├──────────────────────────────────────────────────────────────┤
│                       Platform Layer (Bevy ECS)              │
│  World │ Schedule │ Resources │ Events │ Messages │ Systems  │
└──────────────────────────────────────────────────────────────┘
```

#### 核心数据流

```
User Input → Input Router → Command/Query
                                  │
                    ┌─────────────┼─────────────┐
                    ▼             ▼             ▼
              Agent Engine   Code Intel     Timeline
                    │             │             │
                    ▼             ▼             ▼
            AgentActionEvent  IndexUpdate  TimeAnchor
                    │             │             │
                    ▼             │             │
            Provider Router      │             │
            ┌─────┼─────┐       │             │
            ▼     ▼     ▼       │             │
          MCP   ACP  OpenAPI    │             │
            │     │     │       │             │
            └─────┼─────┘       │             │
                  ▼             │             │
            LLM Response        │             │
                  │             │             │
                  └──────┬──────┘─────────────┘
                         ▼
                  ECS Event Bus
                         │
            ┌────────────┼────────────┐
            ▼            ▼            ▼
       3D Sync      UI Sync      Git Bridge
```

**关键约束**：所有子系统只通过 ECS Events（即时观察者）和 Messages（缓冲消息）通信，禁止直接方法调用。这确保了：
- 每个 Plugin 可以独立测试
- 可以在无 3D 渲染时运行 headless 模式
- 录制/回放只需要记录消息流
- Events 适合即时通知（观察者模式），Messages 适合批量处理（生产者-消费者模式）

---

### 🧠 Agent 核心引擎

Agent 引擎是 XGent 的心脏，负责编排 LLM 调用、工具执行和状态管理。

#### Agent 生命周期

```
Idle → Thinking → Executing → Observing → Reflecting → Done
  │                                            │
  └──────────── Cancel/Timeout ◄──────────────┘
```

每个状态转换都触发 `AgentStateTransition` Event，UI 层通过 Observer 监听并更新可视化。

#### 多 Agent 编排策略

| 模式 | 描述 | 适用场景 |
|:---|:---|:---|
| **Solo** | 单 Agent 执行任务 | 简单修改、问答 |
| **Split** | 两个 Agent 并行执行同一任务，用户选择最佳结果 | 方案对比、A/B 实验 |
| **Swarm** | 多 Agent 协作，各负责子任务 | 大型重构、跨模块修改 |
| **Adversarial** | 一个 Agent 写代码，一个 Agent review | 质量门禁 |

```rust
#[derive(Component)]
struct Agent {
    id: AgentId,
    model: ModelConfig,       // LLM 配置
    role: AgentRole,          // Coder / Reviewer / Explorer / Planner
    state: AgentState,
    context_window: usize,    // token 预算
}

#[derive(Component)]
struct AgentOrchestrator {
    mode: OrchestrationMode,
    agents: Vec<Entity>,      // 管理的 Agent 实体
    task_queue: Vec<Task>,
    consensus_strategy: ConsensusStrategy,
}
```

#### 工具系统 (Tool Use)

Agent 通过结构化的工具调用与外部世界交互：

```rust
#[derive(Message)]
struct ToolCallRequest {
    agent_id: AgentId,
    tool: Tool,
    arguments: serde_json::Value,
    call_id: String,
}

#[derive(Message)]
struct ToolCallResult {
    call_id: String,
    output: Result<serde_json::Value, ToolError>,
    duration: Duration,
}

enum Tool {
    // 文件操作
    ReadFile { path: PathBuf, range: Option<(usize, usize)> },
    WriteFile { path: PathBuf, content: String },
    SearchFiles { pattern: String, query: String },

    // 终端操作
    RunCommand { cmd: String, cwd: PathBuf, timeout: Duration },
    RunTests { filter: Option<String> },

    // Git 操作
    GitStatus,
    GitDiff { commit: Option<String> },
    GitLog { count: usize },

    // 代码智能
    GotoDefinition { file: PathBuf, line: usize, col: usize },
    FindReferences { file: PathBuf, line: usize, col: usize },
    Diagnostics { path: Option<PathBuf> },
}
```

**沙箱策略**：`WriteFile` 和 `RunCommand` 必须经过用户确认或符合安全策略才能执行。详见[安全模型](#-安全模型)。

#### 上下文管理

```
┌─────────────────────────────────────────┐
│            Context Window               │
│  ┌─────────────────────────────────┐    │
│  │ System Prompt (固定)            │    │
│  ├─────────────────────────────────┤    │
│  │ Codebase Map (压缩索引)         │    │
│  ├─────────────────────────────────┤    │
│  │ Conversation History (滑动窗口)  │    │
│  ├─────────────────────────────────┤    │
│  │ Active File Context (当前文件)   │    │
│  ├─────────────────────────────────┤    │
│  │ Tool Results (最新结果)          │    │
│  └─────────────────────────────────┘    │
└─────────────────────────────────────────┘
```

**压缩策略**：
- **Codebase Map**：将整个项目的符号表压缩为 ~2000 token 的摘要（模块层级、公开 API 签名、依赖关系）
- **滑动窗口**：对话历史保留最近 N 轮完整内容，更早的对话摘要为 1-2 句话
- **按需加载**：Agent 需要读取文件时才加载，读完后降级为摘要

---

### 🌐 AI Provider 与协议层

XGent 需要对接多种大模型提供商和 AI 协议。这一层是 Agent 引擎与外部 AI 服务之间的桥梁，负责模型路由、协议适配、计费追踪和降级容灾。

#### 设计目标

| 目标 | 描述 |
|:---|:---|
| **Provider 无关** | Agent 代码不绑定任何特定模型 API，只面向抽象接口编程 |
| **协议可插拔** | 新增一个 Provider 或协议只需实现 trait，不改核心逻辑 |
| **智能路由** | 根据任务类型、成本预算、延迟要求自动选择最优模型 |
| **透明降级** | 主 Provider 不可用时自动切换到备用，用户无感知 |

#### Provider 抽象架构

```
┌──────────────────────────────────────────────────────┐
│                    Agent Engine                       │
│              (只依赖 LLMProvider trait)                │
└──────────────────────┬───────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────┐
│                 Provider Router                      ││  ┌─────────────┐  ┌──────────────┐  ┌────────────┐  │
│  │ Model Select│  │ Cost Tracker │  │ Fallback   │  │
│  │ (智能路由)   │  │ (计费/预算)  │  │ (降级策略) │  │
│  └─────────────┘  └──────────────┘  └────────────┘  │
└──────────────────────┬───────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────┐
│              Protocol Adapter Layer                   │
│  ┌────────┐  ┌────────┐  ┌────────┐  ┌──────────┐  │
│  │  MCP   │  │  ACP   │  │ OpenAI │  │ Custom   │  │
│  │Protocol│  │Protocol│  │Compat  │  │Protocol  │  │
│  └───┬────┘  └───┬────┘  └───┬────┘  └────┬─────┘  │
└──────┼───────────┼───────────┼────────────┼──────────┘
       │           │           │            │
┌──────▼───────────▼───────────▼────────────▼──────────┐
│               Concrete Providers                     ││  OpenAI │ Anthropic │ Google │ DeepSeek │ Ollama │ ...│
└─────────────────────────────────────────────────────┘
```

#### 核心 Trait 定义

```rust
/// LLM Provider 抽象 —— 所有 Provider 必须实现
#[async_trait]
trait LLMProvider: Send + Sync {
    /// Provider 唯一标识
    fn id(&self) -> &ProviderId;
    /// Provider 显示信息
    fn info(&self) -> &ProviderInfo;
    /// 列出可用模型
    fn list_models(&self) -> Vec<ModelInfo>;
    /// 发送 Chat 请求（支持流式）
    async fn chat(&self, request: ChatRequest) -> Result<ChatStream, ProviderError>;
    /// 发送 Embedding 请求
    async fn embed(&self, request: EmbedRequest) -> Result<EmbedResponse, ProviderError>;
    /// 健康检查
    async fn health_check(&self) -> Result<(), ProviderError>;
    /// 估算 token 数
    fn count_tokens(&self, text: &str, model: &ModelId) -> usize;
}

/// Provider 元信息
struct ProviderInfo {
    name: String,               // "OpenAI", "Anthropic"
    icon: Option<Handle<Image>>, // 3D 宇宙中的 Agent 头像
    supported_features: HashSet<Feature>,  // Stream / Vision / ToolUse / ...
    rate_limits: RateLimits,
    pricing: PricingTable,      // 按模型列出价格
}

/// 模型信息
struct ModelInfo {
    id: ModelId,
    display_name: String,
    context_window: usize,
    max_output_tokens: usize,
    capabilities: HashSet<ModelCapability>,
    pricing: ModelPricing,       // input/output token 单价
}

#[derive(Hash, Eq, PartialEq)]
enum ModelCapability {
    Chat,
    Vision,           // 支持图片输入
    ToolUse,          // 支持 function calling
    StructuredOutput, // 支持 JSON mode
    Streaming,
    Embedding,
}
```

#### Provider 注册表与配置

```rust
/// Provider 注册表（ECS Resource）
#[derive(Resource)]
struct ProviderRegistry {
    providers: HashMap<ProviderId, Box<dyn LLMProvider>>,
    configs: HashMap<ProviderId, ProviderConfig>,
    default_provider: ProviderId,
    default_model: ModelId,
}

/// 单个 Provider 的用户自定义配置
#[derive(serde::Serialize, serde::Deserialize)]
struct ProviderConfig {
    id: ProviderId,

    // === 连接配置 ===
    api_base: Option<String>,        // 自定义 API 端点（企业代理、私有部署）
    api_key_vault_key: Option<String>,// Keychain 中的 Key 标识
    api_key_env: Option<String>,     // 环境变量名（备选）

    // === 模型映射 ===
    model_overrides: HashMap<String, String>, // 自定义模型名映射
    // 例: { "gpt-4" → "my-deployed-gpt4" }

    // === 行为配置 ===
    timeout_secs: u64,               // 请求超时
    max_retries: u32,               // 重试次数
    retry_delay_ms: u64,            // 重试间隔
    concurrent_requests: usize,      // 最大并发请求数

    // === 代理与网络 ===
    proxy: Option<String>,          // HTTP 代理
    custom_headers: HashMap<String, String>, // 自定义请求头

    // === 计费与预算 ===
    budget: Option<Budget>,         // 预算限制
    cost_alert_threshold: Option<f64>, // 费用告警阈值 (USD)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Budget {
    daily: Option<f64>,             // 每日预算 (USD)
    monthly: Option<f64>,           // 每月预算 (USD)
    per_task: Option<f64>,         // 单任务预算 (USD)
}
```

**配置持久化**：`ProviderConfig` 以 TOML 文件存储在项目根目录 `.xgent/providers.toml` 或用户全局目录 `~/.xgent/providers.toml`。项目级配置覆盖全局配置。

#### 智能路由 (Model Router)

路由策略决定 Agent 的每次请求发送到哪个 Provider/Model：

```rust
#[derive(Resource)]
struct ModelRouter {
    strategy: RoutingStrategy,
    fallback_chain: Vec<(ProviderId, ModelId)>,
    cost_tracker: CostTracker,
}

enum RoutingStrategy {
    /// 固定模型：始终使用指定 Provider + Model
    Fixed { provider: ProviderId, model: ModelId },

    /// 按任务类型路由：不同任务用不同模型
    TaskBased {
        rules: Vec<(TaskType, ProviderId, ModelId)>,
        default: (ProviderId, ModelId),
    },

    /// 智能路由：综合延迟、成本、能力自动选择
    Smart {
        preferences: SmartPreferences,
    },

    /// 乒乓模式：Split-Testing 时两个 Agent 用不同模型
    PingPong {
        agent_a: (ProviderId, ModelId),
        agent_b: (ProviderId, ModelId),
    },
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SmartPreferences {
    prioritize_cost: bool,        // 优先省钱 → 选最便宜的
    prioritize_speed: bool,       // 优先速度 → 选最快响应的
    prioritize_quality: bool,     // 优先质量 → 选最强模型
    prefer_local: bool,           // 优先本地模型 → Ollama 等
    min_context_window: usize,    // 最低上下文窗口要求
    required_capabilities: HashSet<ModelCapability>,
}
```

**降级链 (Fallback Chain)**：

```
请求 → OpenAI GPT-4o
        │ 失败/超时/限流
        ▼
      Anthropic Claude Sonnet
        │ 失败
        ▼
      DeepSeek V3
        │ 失败
        ▼
      Ollama (本地 Qwen2.5-Coder)
        │ 失败
        ▼
      返回错误，UI 显示降级通知
```

每次降级都触发 `ProviderFallbackEvent`，UI 显示"已从 GPT-4o 降级到 Claude Sonnet"的提示。

#### 计费追踪 (Cost Tracker)

```rust
#[derive(Resource)]
struct CostTracker {
    sessions: HashMap<AgentId, SessionCost>,
    daily_total: f64,
    monthly_total: f64,
}

struct SessionCost {
    agent_id: AgentId,
    input_tokens: u64,
    output_tokens: u64,
    total_cost_usd: f64,
    by_model: HashMap<ModelId, ModelCost>,
}

#[derive(Event)]
struct CostAlertEvent {
    level: AlertLevel,            // Warning / Critical
    consumed: f64,
    budget: f64,
    agent_id: Option<AgentId>,
}
```

- 3D 宇宙中每个 AgentAvatar 旁显示实时费用计数器
- 超预算时自动降级到更便宜的模型
- 工具区面板显示当日/当月费用汇总和趋势图

---

### 📡 AI 协议支持

XGent 不只对接 HTTP API，还要支持主流 AI Agent 协议，实现工具和上下文的标准化互操作。

#### 协议全景

| 协议 | 全称 | 角色 | 状态 |
|:---|:---|:---|:---|
| **MCP** | Model Context Protocol | XGent 作为 MCP Client 调用外部工具服务器 | 核心，MVP-1 |
| **ACP** | Agent Communication Protocol | XGent 内部多 Agent 间通信 | 核心，Alpha-1 |
| **OpenAI Compatible** | OpenAI Chat Completions API | 模型调用的事实标准 | 核心，MVP-1 |
| **A2A** | Agent-to-Agent Protocol (Google) | XGent 与外部 Agent 互操作 | 实验，Beta |
| **LSP** | Language Server Protocol | 代码智能（已有，见代码智能管线） | 核心，Alpha-1 |

#### MCP (Model Context Protocol) 支持

MCP 是 Anthropic 提出的标准协议，用于 LLM 与外部工具/数据源的互操作。XGent 作为 **MCP Client**，可以连接任意 MCP Server 来扩展 Agent 的工具能力。

```
┌─────────────────────────────────────┐
│           XGent (MCP Client)         │
│  ┌─────────────────────────────┐    │
│  │ MCP Connection Manager      │    │
│  │  - Server 生命周期管理       │    │
│  │  - Capability 发现与缓存    │    │
│  │  - 请求/响应路由            │    │
│  └──────────┬──────────────────┘    │
│             │ stdio / SSE            │
└─────────────┼───────────────────────┘
              │
    ┌─────────┼─────────┬──────────────┐
    ▼         ▼         ▼              ▼
┌────────┐┌────────┐┌────────┐  ┌──────────┐
│File    ││GitHub  ││Browser │  │ Custom   │
│System  ││        ││        │  │ MCP      │
│Server  ││Server  ││Server  │  │ Server   │
└────────┘└────────┘└────────┘  └──────────┘
```

**MCP 在 XGent 中的集成方式**：

1. **工具桥接**：MCP Server 暴露的 `tools` 自动注册为 XGent Agent 的可用工具，出现在工具调用面板中
2. **资源映射**：MCP Server 的 `resources` 映射为 XGent 的 `CodeNode`，在 3D 宇宙中可见
3. **Prompt 模板**：MCP Server 的 `prompts` 作为快捷命令出现在径向菜单中
4. **3D 可视化**：每个活跃的 MCP Server 在 3D 宇宙中渲染为一个"空间站"，Agent 飞船飞向空间站表示正在调用该 Server 的工具

```rust
/// MCP 连接管理器
#[derive(Resource)]
struct McpConnectionManager {
    connections: HashMap<McpServerId, McpConnection>,
    tool_registry: HashMap<String, McpToolDef>,  // MCP 工具 → 全局工具注册表
}

struct McpConnection {
    id: McpServerId,
    transport: McpTransport,     // Stdio | Sse(String)
    config: McpServerConfig,
    capabilities: McpServerCapabilities,
    status: McpConnectionStatus,  // Connecting | Ready | Error | Stopped
}

/// MCP Server 用户配置（在 providers.toml 或独立 mcp.toml 中）
#[derive(serde::Serialize, serde::Deserialize)]
struct McpServerConfig {
    id: String,
    command: Option<String>,              // stdio 模式：启动命令
    args: Option<Vec<String>>,            // 启动参数
    url: Option<String>,                  // SSE 模式：服务器 URL
    env: Option<HashMap<String, String>>, // 环境变量
    enabled: bool,                        // 是否启用
    auto_start: bool,                     // XGent 启动时自动连接
    trust_level: McpTrustLevel,           // 信任等级（影响安全策略）
}

#[derive(serde::Serialize, serde::Deserialize)]
enum McpTrustLevel {
    /// 完全信任：工具调用无需确认
    Trusted,
    /// 需确认：每次工具调用需用户确认
    ConfirmEach,
    /// 只读：只允许读取操作
    ReadOnly,
}
```

**MCP 配置示例** (`~/.xgent/mcp.toml`)：

```toml
[[servers]]
id = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/project"]
trust_level = "ReadOnly"
auto_start = true

[[servers]]
id = "github"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_TOKEN = "${GITHUB_TOKEN}" }
trust_level = "ConfirmEach"
auto_start = true

[[servers]]
id = "custom-browser"
url = "http://localhost:3001/sse"
trust_level = "Trusted"
auto_start = false
```

#### ACP (Agent Communication Protocol) 支持

ACP 用于 XGent 内部多 Agent 之间的结构化通信，以及 XGent 与外部 Agent 系统的互操作。

**内部 ACP（核心）**：Agent 间的协作协议，用于 Swarm 模式和 Adversarial 模式。

```rust
/// Agent 间消息（缓冲式，支持批量处理）
#[derive(Message)]
struct AgentMessage {
    from: AgentId,
    to: AgentId,                 // 可选广播
    msg_type: AgentMessageType,
    payload: serde_json::Value,
    timestamp: chrono::DateTime<chrono::Utc>,
    correlation_id: Option<String>, // 请求-响应关联
}

enum AgentMessageType {
    /// 任务委派：Orchestrator 分配子任务给 Worker
    TaskAssign,
    /// 任务完成：Worker 报告结果
    TaskComplete,
    /// 请求协助：Worker 遇到困难，请求其他 Agent 帮助
    AssistanceRequest,
    /// 代码审查：Reviewer Agent 发出审查意见
    CodeReview,
    /// 上下文共享：Agent 间传递关键上下文信息
    ContextShare,
    /// 冲突通知：两个 Agent 修改了同一文件
    ConflictNotify,
}
```

**Swarm 模式工作流示例**：

```
用户: "重构整个 auth 模块，添加 OAuth2 支持"

Orchestrator:
  → Explorer Agent: "分析 auth 模块当前结构和依赖"
    ← Explorer: "发现 5 个文件，3 个外部依赖"
  → Planner Agent: "基于探索结果制定重构计划"
    ← Planner: "建议分 3 步：1) 接口抽象 2) OAuth2 实现 3) 测试更新"
  → Coder Agent A: "执行步骤 1：抽象接口"
  → Coder Agent B: "执行步骤 3：更新测试用例"（并行）
    ← Coder A: "接口抽象完成"
    ← Coder B: "测试用例更新完成"
  → Coder Agent: "执行步骤 2：实现 OAuth2"
    ← Coder: "OAuth2 实现完成"
  → Reviewer Agent: "审查所有变更"
    ← Reviewer: "发现 2 个问题，建议修复"
  → Coder Agent: "修复 Reviewer 指出的问题"
```

**外部 ACP（实验性）**：与 Google 的 A2A (Agent-to-Agent) 协议对齐，支持 XGent Agent 与外部 Agent 系统互操作。此功能在 Beta 阶段探索。

#### OpenAI Compatible API

事实上的行业标准，XGent 的基础协议层。绝大多数 Provider 都提供 OpenAI 兼容接口。

```rust
/// OpenAI 兼容协议适配器
struct OpenAICompatibleAdapter {
    config: ProviderConfig,
    client: reqwest::Client,
}

#[async_trait]
impl LLMProvider for OpenAICompatibleAdapter {
    async fn chat(&self, request: ChatRequest) -> Result<ChatStream, ProviderError> {
        // 1. 将 XGent ChatRequest 转换为 OpenAI 格式
        // 2. 发送请求到 config.api_base
        // 3. 处理 SSE 流式响应
        // 4. 转换为 XGent ChatStream
        todo!()
    }
    // ...
}
```

**兼容的 Provider**：

| Provider | api_base | 备注 |
|:---|:---|:---|
| OpenAI | `https://api.openai.com/v1` | 原生 |
| Azure OpenAI | `https://{resource}.openai.azure.com/openai` | 需 `api-key` + `api-version` header |
| Anthropic | 自有协议 | 需专用适配器（非 OpenAI 兼容） |
| DeepSeek | `https://api.deepseek.com/v1` | OpenAI 兼容 |
| 月之暗面 (Moonshot) | `https://api.moonshot.cn/v1` | OpenAI 兼容 |
| 智谱 (GLM) | `https://open.bigmodel.cn/api/paas/v4` | OpenAI 兼容 |
| Ollama (本地) | `http://localhost:11434/v1` | OpenAI 兼容 |
| vLLM (私有部署) | 用户自定义 | OpenAI 兼容 |
| LM Studio (本地) | `http://localhost:1234/v1` | OpenAI 兼容 |

#### 协议与 Provider 的事件集成

所有协议层事件/消息统一汇入 ECS 通信管道：
- 即时通知（降级、状态变更、预算告警）→ `#[derive(Event)]` + Observer
- 缓冲处理（MCP 工具调用、Agent 间消息）→ `#[derive(Message)]` + MessageReader/Writer

```rust
/// MCP 工具调用消息
#[derive(Message)]
struct McpToolCallMessage {
    server_id: McpServerId,
    tool_name: String,
    arguments: serde_json::Value,
    result: Option<serde_json::Value>,
    duration: Duration,
}

/// Provider 状态变更事件（即时通知）
#[derive(Event)]
struct ProviderStatusEvent {
    provider_id: ProviderId,
    status: ProviderStatus,
    latency_ms: Option<u64>,
    error: Option<String>,
}

/// 降级事件（即时通知 UI 层）
#[derive(Event)]
struct ProviderFallbackEvent {
    from_provider: ProviderId,
    from_model: ModelId,
    to_provider: ProviderId,
    to_model: ModelId,
    reason: FallbackReason,
}

/// 预算告警事件（即时通知）
#[derive(Event)]
struct BudgetAlertEvent {
    level: AlertLevel,
    consumed_usd: f64,
    budget_usd: f64,
    scope: BudgetScope,
}
```

---

### 📦 ECS 数据模型与事件契约

以下定义了 XGent 中核心的 ECS Components 和 Events，是所有子系统的通信契约。

#### 核心 Components

```rust
// === 3D 可视化 ===

/// 代码节点（文件/模块/函数）在 3D 空间中的表示
#[derive(Component)]
struct CodeNode {
    path: PathBuf,
    kind: CodeNodeKind,        // File | Module | Function | Struct
    loc: u32,                  // 代码行数，影响星球大小
    complexity: f32,           // 圈复杂度，影响颜色（冷→热）
    change_frequency: f32,     // 变更频率，影响发光脉冲
    target_position: Vec3,     // 目标位置（用于弹性动画）
    velocity: Vec3,             // 当前速度（用于惯性动画）
}

#[derive(Component)]
struct DependencyEdge {
    from: Entity,
    to: Entity,
    weight: f32,               // 依赖强度（引用次数）
    edge_type: EdgeType,       // Import | Call | Inherit | Implement
}

/// AI Agent 在 3D 空间中的化身
#[derive(Component)]
struct AgentAvatar {
    agent_id: AgentId,
    trail: Vec<Vec3>,          // 最近 N 帧的位置轨迹
    state_glow: AgentStateGlow,// 当前状态的视觉反馈
}

/// 相机控制——使用 Bevy 内置 FreeCamera 组件
/// 参见 bevy_camera_controller crate (free_camera feature)
///
/// ```
/// commands.spawn((
///     Camera3d::default(),
///     FreeCamera {
///         walk_speed: 5.0,
///         run_speed: 15.0,
///         friction: 40.0,
///         sensitivity: 0.2,
///         key_forward: KeyCode::KeyW,
///         key_back: KeyCode::KeyS,
///         key_left: KeyCode::KeyA,
///         key_right: KeyCode::KeyD,
///         key_up: KeyCode::KeyE,
///         key_down: KeyCode::KeyQ,
///         ..default()
///     },
/// ));
/// ```

// === 时间线 ===

#[derive(Component)]
struct TimeAnchor {
    id: AnchorId,
    timestamp: chrono::DateTime<chrono::Utc>,
    action: AgentAction,       // 触发此锚点的操作
    git_commit: Option<String>,// 关联的 Git Commit
    diff_snapshot: String,      // 当前状态的 diff
}

// === UI 状态 ===

#[derive(Component)]
struct ChatPanel {
    agent_id: AgentId,
    messages: Vec<ChatMessage>,
    scroll_offset: f32,
}

#[derive(Component)]
struct CodePreviewPanel {
    file_path: PathBuf,
    content: String,
    highlight_range: Option<(usize, usize)>,
    syntax_theme: String,
}
```

#### 核心事件与消息

Bevy 0.19 提供两种通信机制：
- **Event**（`#[derive(Event)]`）：即时触发，通过 Observer 模式响应，适合通知类场景
- **Message**（`#[derive(Message)]`）：缓冲式，通过 `MessageWriter`/`MessageReader` 读写，适合批量处理

```rust
// === Agent 状态（Event — 即时通知 UI 层）===
#[derive(Event)]
struct AgentStatusEvent {
    agent_id: AgentId,
    status: AgentState,
    message: String,
}

// === Agent 动作（Message — 批量处理，需持久化）===
#[derive(Message)]
struct AgentActionMessage {
    agent_id: AgentId,
    action: AgentAction,
    anchor_id: AnchorId,
}

// === 文件系统变更（Message — 可能短时间大量触发）===
#[derive(Message)]
struct FileChangedMessage {
    path: PathBuf,
    change_type: FileChangeType,
}

// === 3D 交互——直接使用 Bevy 内置 Pointer 事件 ===
// 使用 entity.observe() 模式，无需自定义事件
//
// 示例：
// commands.spawn(CodeNode { .. })
//     .observe(|mut event: On<Pointer<Click>>| {
//         event.propagate(false);
//     });

// === 3D 导航通知（Event — 通知 UI 层镜头移动）===
#[derive(Event)]
struct NodeNavigatedEvent {
    from: Option<Entity>,
    to: Entity,
    navigation_method: NavMethod,
}

// === 时间线操作（Event — 即时执行回溯）===
#[derive(Event)]
struct TimeTravelEvent {
    target_anchor: AnchorId,
    travel_mode: TravelMode,
}
```

---

### 🔬 代码智能管线

代码智能管线负责从源代码中提取结构化信息，驱动 3D 可视化和 Agent 上下文。

```
Source Files
    │
    ▼
┌─────────────────┐
│ File Watcher     │── 监听文件变更，触发增量索引
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Parser Layer     │── Tree-sitter 多语言解析
│  - AST           │
│  - Symbols       │
│  - References    │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Graph Builder    │── 构建代码图谱
│  - 依赖图        │
│  - 调用图        │
│  - 继承图        │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Metrics Engine   │── 计算代码度量
│  - 复杂度        │
│  - 变更频率      │
│  - 耦合度        │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Layout Engine    │── 力导向布局 → 3D 坐标
│  - 目录层级      │
│  - 依赖引力      │
│  - LOD 层级      │
└─────────────────┘
```

**Tree-sitter 多语言支持**：通过 `tree-sitter` 的 grammar 集成，支持 Rust、TypeScript、Python、Go 等主流语言。每种语言提供：
- 符号提取（函数、类型、trait/interface、impl）
- 引用解析（import、call、type annotation）
- 诊断信息集成（编译错误、lint 警告）

**力导向布局算法**：

```
目录节点 = 引力中心（按目录层级排列）
文件节点 = 行星（受目录引力和依赖弹力作用）
依赖边   = 弹簧（刚度与依赖强度正相关）

每帧更新：
  F_total = F_directory_gravity + F_dependency_spring + F_repulsion + F_damping
  V_new = V_old * damping + F_total * dt
  P_new = P_old + V_new * dt

LOD 策略：
  近距离：完整几何体 + 文字标签 + 细节
  中距离：简化几何体 + 文件名标签
  远距离：点精灵 + 目录名标签
```

---

### 🚀 特色功能详细设计

#### 1. 3D 代码宇宙 (3D Code Universe)

##### 文件星系 (File Galaxies)

- **星球映射规则**：

  | 代码属性 | 3D 映射 | 示例 |
  |:---|:---|:---|
  | 文件大小 (LOC) | 星球半径 | 1000 行 = 大星球，50 行 = 小行星 |
  | 圈复杂度 | 颜色温度 | 简单 = 蓝色，复杂 = 红色 |
  | 最近变更频率 | 发光脉冲 | 活跃文件持续闪烁 |
  | 代码健康度 | 表面纹理 | 干净 = 光滑，警告/错误 = 裂纹 |
  | 语言类型 | 星球材质 | Rust = 金属质感，JS = 霓虹色 |

- **交互操作**：
  - **WASD 飞行**：在宇宙中自由移动，带惯性物理（Bevy 内置 `FreeCamera` 组件，含摩擦力/加速/减速模型）
  - **鼠标滚轮缩放**：从银河级鸟瞰到函数级近观
  - **双击星球**：打开代码预览面板，同时镜头平滑推进
  - **右键拖拽**：旋转视角，Orbit 模式围绕选中节点旋转
  - **Ctrl+F**：搜索星球名称，镜头自动飞行到目标

##### 依赖虫洞 (Dependency Wormholes)

- **虫洞可视化**：
  - 依赖越强，虫洞越粗、越亮
  - 循环依赖 = 红色虫洞 + 警告图标（实时检测并标记）
  - 过度依赖（上帝模块）= 多条虫洞汇聚成漩涡效果

- **虫洞导航**：点击虫洞入口触发传送动画——镜头沿贝塞尔曲线飞行到目标节点，同时高亮整条依赖链路

- **依赖健康度仪表盘**：
  - 耦合度热力图（2D 叠加层，可切换显示）
  - "上帝模块"自动检测并标记为红色超新星
  - 孤立模块（无依赖）标记为深空暗物质

##### AI 副驾驶视角 (AI Co-pilot POV)

- **飞船轨迹**：Agent 的 3D 飞船在工作时留下发光尾迹，最近 30 秒的轨迹可见
- **思维泡泡**：飞船上方悬浮半透明的"思维泡泡"，显示 Agent 当前的思考摘要
- **操作广播**：Agent 每执行一个工具调用，对应星球闪烁一次，并显示操作类型图标

#### 2. 时空回溯调试器 (Time-Travel Debugger)

##### 时间线组件 (Timeline Component)

- **时间线 UI**：底部横条时间线，每个锚点是一个可点击的节点
  - 缩放级别：从"天"级别到"秒"级别
  - 颜色编码：代码修改 = 橙色，测试运行 = 绿色，错误 = 红色，思考 = 蓝色
  - 迷你 diff 预览：hover 时间锚点，弹出该次变更的 mini diff

- **快照存储策略**：
  - 轻量级：每次 Agent 操作时，仅存储 `git diff` + 元数据
  - 完整快照：用户手动标记或 Agent 完成关键里程碑时，存储完整的文件系统快照
  - Git 集成：每个时间锚点关联一个 Git commit 或 stash

##### 分支探索 (Branch Exploration)

```
main ──── a1 ──── a2 ──── a3 ──── (current)
               │
               └─── b1 ──── b2 ──── (experimental branch)
```

- 在时间线上右键任意锚点 → "从此处分支"
- 新分支在独立的世界实例中运行，不影响主分支
- 分支间可拖拽对比（左右分屏）
- "合并分支"将实验结果应用到当前状态

##### 实用场景

- **"刚才 AI 改了什么？"** → 点击最新时间锚点，查看精确 diff
- **"这个 bug 是什么时候引入的？"** → 二分搜索时间线，逐步缩小范围
- **"我想试试另一种方案"** → 从当前点分叉，并行探索两条路
- **"回滚到 AI 介入前的状态"** → 一键跳转到 Agent 开始前的锚点

#### 3. 感知-DoE-反思 交互 (Sense-DoE-Reflect Workflow)

##### 智能设计实验 (DoE)

当面对非确定性问题时，Agent 主动发起方案探索：

```
用户: "这个模块性能太差，帮我优化"

Agent: "检测到性能问题，我分析了 3 种优化路径："
  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
  │ 方案 A: 缓存层   │  │ 方案 B: 算法优化 │  │ 方案 C: 并行化   │
  │ 预期提速: 3x    │  │ 预期提速: 10x   │  │ 预期提速: 5x    │
  │ 风险: 低        │  │ 风险: 中        │  │ 风险: 高        │
  │ 改动范围: +2文件 │  │ 改动范围: ~5文件 │  │ 改动范围: ~8文件 │
  └─────────────────┘  └─────────────────┘  └─────────────────┘
  [选择 A]  [选择 B]  [选择 C]  [组合 A+B]  [让我先看详细分析]
```

- 方案卡片在 3D 宇宙中以"全息投影"形式呈现
- 点击卡片展开详细的代码 diff 预览
- 选择后 Agent 自动执行，并在时间线上标记决策节点

##### 实时对比 (Split-Testing)

- **触发方式**：在 Agent 面板中点击"分屏对比"按钮，或使用 `Ctrl+Shift+S`
- **实现机制**：
  1. 创建分支 World（Branch World），克隆当前状态
  2. 主世界由 Agent A 操作，分支世界由 Agent B 操作
  3. 两个相机分别渲染到左右两个 Render Target
  4. UI 布局 50/50 分屏，各自独立交互
- **对比结果**：完成后弹出统一对比面板，并排展示 diff，一键选择胜出方案

##### 反思闭环 (Reflect)

Agent 完成任务后，自动生成反思报告：

```markdown
## 反思报告

### 执行摘要
- 耗时: 45s
- 工具调用: 12 次（8 次文件读取, 2 次命令执行, 2 次搜索）
- 修改文件: 3 个
- 新增代码: 47 行 / 删除: 12 行

### 决策回顾
1. 选择算法 B 而非 A → 原因: 复杂度更低，性能更优
2. 放弃方案 C → 原因: 需要引入新依赖，收益不显著

### 风险提示
- 修改了 `parser.rs` 的公共 API，下游模块可能需要适配
- 建议运行完整测试套件确认无破坏性变更
```

反思报告以半透明浮层展示，可展开/折叠，也可以在时间线上回顾。

#### 4. 沉浸式模态面板 (Immersive Modal Panels)

- **半透明 3D 面板**：所有模态面板（设置、Agent 选择、确认对话框）都是半透明的 3D 平面，悬浮在 3D 宇宙前方
- **径向菜单** (`Cmd+K` / `Ctrl+K`)：
  - 环绕光标的 3D 扇形菜单
  - 按功能分组：导航、编辑、Agent、视图
  - 支持模糊搜索，输入即筛选
  - 常用命令显示快捷键提示

#### 5. 代码热力图 (Code Heatmap Overlay)

- 在 3D 宇宙上叠加 2D 热力图层
- 数据源可选：Git blame（谁改的）、test coverage（覆盖度）、complexity（复杂度）、recent changes（最近变更）
- 透明度可调，支持与非热力图视图混合显示
- 热力图数据由 `Code Intelligence Pipeline` 的 `Metrics Engine` 提供

#### 6. 诊断雷达 (Diagnostics Radar)

- 编译错误、lint 警告、test 失败在 3D 宇宙中表现为"信号源"
- 错误数越多，信号越强，星球表面出现裂纹效果
- 点击信号源自动定位到对应文件和行号
- 支持按严重级别过滤（Error / Warning / Info）

---

### 🖥️ UI 布局与交互设计

#### 主界面四区域布局

```
┌───────────────────────────────────────────────────────────┐
│  🔍 搜索栏  │  ⏯ 时间线缩略  │  👤 模型选择  │  ⚙ 设置  │  ← 控制区
├──────────────────────────────────┬────────────────────────┤
│                                  │  Agent 侧边栏         │
│                                  │  ┌──────────────────┐  │
│       3D 代码宇宙                │  │ 💬 对话面板      │  │
│       (主内容区)                 │  │                  │  │
│                                  │  ├──────────────────┤  │
│                                  │  │ 🔧 工具调用面板  │  │
│                                  │  │                  │  │
│                                  │  ├──────────────────┤  │
│                                  │  │ 📋 任务队列      │  │
│                                  │  └──────────────────┘  │
├──────────────────────────────────┴────────────────────────┤
│  📊 性能监控  │  📝 日志  │  ⏱ 时间线详情  │  🌡 热力图  │  ← 工具区
└───────────────────────────────────────────────────────────┘
```

**布局规则**：
- 所有区域边界可拖拽调整大小
- 3D 主内容区支持全屏模式（隐藏侧边栏和工具区）
- 侧边栏支持多 Tab 切换（对话、工具、任务、设置）
- 工具区支持最小化到状态栏

#### 代码预览面板

- 在 3D 宇宙中双击星球后，侧边栏切换到代码预览模式
- 左侧代码（带语法高亮），右侧 AI 解读（函数签名、依赖、复杂度）
- 支持"就地编辑"模式——在预览面板中直接修改代码，Agent 实时感知变更
- 代码变更实时同步到 3D 可视化（星球大小/颜色更新）

---

### ⌨️ 键盘优先工作流

XGent 的核心交互设计原则：**每个操作都有键盘快捷键**，3D 是增强不是必需。

| 快捷键 | 操作 | 描述 |
|:---|:---|:---|
| `Ctrl+K` | 命令面板 | 径向菜单，模糊搜索所有命令 |
| `Ctrl+L` | 聚焦聊天 | 光标跳转到 Agent 对话输入框 |
| `Ctrl+P` | 快速打开 | 按文件名搜索并飞行到目标星球 |
| `Ctrl+Shift+P` | Agent 命令 | 向当前 Agent 发送指令 |
| `Ctrl+Shift+S` | 分屏对比 | 启动 Split-Testing 模式 |
| `Ctrl+Z` | 撤销 Agent | 回退到上一个时间锚点 |
| `Ctrl+Y` | 重做 | 前进到下一个时间锚点 |
| `Ctrl+Shift+Z` | 时间线浏览 | 底部时间线获得焦点 |
| `Ctrl+1/2/3` | 切换视图 | 2D 代码 / 3D 宇宙 / 混合 |
| `W/A/S/D` | 飞行导航 | 在 3D 宇宙中移动（按住时） |
| `Q/E` | 上下移动 | 3D 宇宙中的垂直移动 |
| `Space` | 锁定/解锁 | 锁定到当前选中星球 |
| `Esc` | 取消/返回 | 取消当前操作或返回上级视图 |
| `F2` | 重命名 | 重命名当前选中的文件/符号 |
| `F12` | 跳转定义 | 通过虫洞飞到定义所在星球 |

---

### 🔌 插件与扩展体系

XGent 采用 Bevy Plugin 机制作为扩展点。在 Bevy 0.19 中，Plugin 可以是 struct 或 plain function：

```rust
/// XGent 主 Plugin（struct 方式）
pub struct XGentPlugin;

impl Plugin for XGentPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            CorePlugin,
            AgentPlugin,
            ProviderPlugin,
            ProtocolPlugin,
            TimelinePlugin,
            UniversePlugin,
            UIPlugin,
            CodeIntelPlugin,
            GitPlugin,
            ThemePlugin,
        ))
        .add_message::<AgentActionMessage>()
        .add_message::<FileChangedMessage>()
        .add_observer(on_provider_fallback);
    }
}

/// 简单功能可以用 function 方式
pub fn hello_plugin(app: &mut App) {
    app.add_systems(Startup, setup);
}
```

**内置插件**：

| Plugin | 职责 |
|:---|:---|
| `CorePlugin` | ECS 基础组件注册、事件总线 |
| `AgentPlugin` | Agent 生命周期、LLM 通信、工具调用 |
| `ProviderPlugin` | Provider 注册表、智能路由、降级链、计费追踪 |
| `ProtocolPlugin` | MCP Client、ACP 通信、OpenAI 兼容适配 |
| `TimelinePlugin` | 时间锚点管理、快照存储 |
| `UniversePlugin` | 3D 场景构建、力导向布局、飞行相机 |
| `UIPlugin` | 2D 面板、聊天界面、命令面板 |
| `CodeIntelPlugin` | Tree-sitter 解析、符号索引、诊断 |
| `GitPlugin` | Git 操作桥接、diff 计算 |
| `ThemePlugin` | Feathers 主题引擎、动态换肤（基于 `UiTheme` 设计 Token 系统） |

**扩展点**：
- 自定义 Provider 适配器（接入新的 LLM 服务）
- 自定义 MCP Server 集成
- 自定义 Agent 行为（新的 OrchestrationMode）
- 自定义 3D 渲染效果（新的星球材质、粒子特效）
- 自定义工具（Agent 可调用的新工具）
- 自定义 UI 面板
- 自定义代码分析器（新的语言支持）

---

### 🔐 安全模型

#### Agent 操作分级

| 级别 | 操作 | 默认策略 |
|:---|:---|:---|
| 🟢 安全 | 读取文件、搜索、查看 diff | 自动允许 |
| 🟡 需确认 | 修改文件、运行测试 | 首次确认，同文件后续自动 |
| 🔴 高风险 | 执行任意命令、删除文件、Git push | 每次确认 |
| ⚫ 禁止 | 访问系统目录、网络请求（非 LLM） | 硬编码拒绝 |

#### 实现机制

```rust
#[derive(Resource)]
struct SecurityPolicy {
    auto_approved: HashSet<Tool>,
    confirm_per_file: HashSet<Tool>,
    confirm_always: HashSet<Tool>,
    denied: HashSet<Tool>,
    sandbox_paths: Vec<PathBuf>,     // Agent 只能访问项目目录及子目录
    blocked_commands: Vec<String>,   // 禁止执行的命令黑名单
}

/// 安全检查系统
fn security_check(
    mut tool_requests: MessageReader<ToolCallRequest>,
    mut tool_results: MessageWriter<ToolCallResult>,
    policy: Res<SecurityPolicy>,
) {
    for req in tool_requests.read() {
        match policy.check(&req.tool) {
            Verdict::Approved => { /* 执行 */ },
            Verdict::NeedsConfirmation => { /* 弹出确认面板 */ },
            Verdict::Denied => { /* 拒绝并记录 */ },
        }
    }
}
```

#### API Key 管理

- LLM API Key 存储在操作系统原生 Keychain（Windows Credential Manager / macOS Keychain / Linux Secret Service）
- 内存中仅在需要时解密，不落盘明文
- 支持多 Key 轮换和速率限制

---

### ⚡ 性能工程

XGent 的性能目标是：**10 万个代码节点的项目，3D 宇宙保持 60fps，UI 操作延迟 < 16ms**。

#### 3D 渲染性能

| 策略 | 描述 | 预期收益 |
|:---|:---|:---|
| **LOD (Level of Detail)** | 远距离 → 点精灵，中距离 → 简化几何体，近距离 → 完整几何体 | Draw Call 降低 80%+ |
| **GPU Instancing** | 同类型星球共享 Mesh，实例化差异（位置、大小、颜色） | 批量渲染，单 Draw Call 绘制数百节点 |
| **Frustum Culling** | 只渲染相机视锥内的节点 | 大宇宙场景性能提升显著 |
| **Occlusion Culling** | 被大星球遮挡的小星球不渲染 | 密集区域性能优化 |
| **延迟加载** | 星球材质和文字标签仅在接近时加载 | 减少内存占用和 GPU 纹理压力 |
| **Mesh 合并 (Batching)** | 背景装饰元素合并为单个 Mesh | 减少 Draw Call |

#### 力导向布局性能

```
朴素算法: O(N²) — 每对节点计算引力
优化策略:
  1. Barnes-Hut 近似: O(N log N) — 将远距离节点聚合为质心
  2. 空间哈希: 只计算邻近节点的斥力
  3. 增量更新: 文件变更时只重算受影响节点的力
  4. 异步计算: 布局计算在独立线程，结果通过 channel 传回主线程
  5. 收敛检测: 力的总和低于阈值时停止计算
```

#### UI 性能

| 策略 | 描述 |
|:---|:---|
| **虚拟滚动** | 对话历史、日志、文件列表使用虚拟滚动，只渲染可见区域 |
| **状态脏标记** | UI 组件仅在关联数据变化时重渲染 |
| **实体池化** | 对话消息、日志条目等高频创建的 UI 实体使用对象池，避免反复分配/释放 |
| **文本缓存** | 语法高亮结果缓存，仅在文件变更时重算 |
| **延迟布局** | 侧边栏折叠时跳过子元素布局计算 |

#### 内存管理

- **代码索引**：使用内存映射 (mmap) 读取大文件，避免全量加载
- **快照压缩**：时间锚点的 diff 使用 zstd 压缩存储
- **纹理图集**：所有 UI 图标和星球纹理打包为纹理图集，减少 GPU 纹理切换
- **实体回收**：3D 节点离开视野后回收其 GPU 资源

#### 异步架构

```
Main Thread (Bevy ECS — 60fps)
  │
  ├── Agent Runtime (tokio runtime — async)
  │     ├── LLM API 调用
  │     ├── 工具执行
  │     └── 代码索引更新
  │
  ├── Layout Engine (独立线程)
  │     └── 力导向布局计算
  │
  └── Git Operations (独立线程)
        └── git2 / CLI 调用

通信方式：
  - Agent → Main: mpsc::channel → MessageWriter
  - Layout → Main: mpsc::channel → 组件更新
  - Git → Main: mpsc::channel → MessageWriter
```

---

### 📚 技术栈与依赖

#### 核心框架

```toml
[dependencies]
# 引擎核心 — 使用 Bevy 0.19 (path 依赖指向本地源码)
bevy = { path = "../bevy", features = [
    "3d",           # 3D 渲染 + PBR + glTF + 拾取
    "ui",           # UI + bevy_ui_widgets + bevy_feathers + 拾取
    "free_camera",  # 内置飞行相机控制器
    "bevy_gizmos",  # 调试绘图（虫洞线条、节点高亮等）
    "serialize",    # serde 支持
    "tonemapping_luts",
    "png",
] }

# 异步运行时（Agent 通信、LLM API）
tokio = { version = "1", features = ["full"] }

# HTTP 客户端（LLM API 调用）
reqwest = { version = "0.13", features = ["json", "stream"] }

# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# 用户偏好持久化（Bevy 内置 bevy_settings）
# 无需额外依赖，使用 #[derive(SettingsGroup)] + PreferencesPlugin
```

> **Bevy 0.19 关键变化**：
> - `bevy_ui_widgets` 和 `bevy_feathers` 已成为**官方一级 crate**，通过 `features = ["ui"]` 自动启用
> - `bevy_picking` 是**内置拾取系统**，不再需要第三方 `bevy_mod_picking`
> - `bevy_camera_controller` 提供内置 `FreeCamera`（WASD + 鼠标视角），不再需要自实现
> - `bevy_settings` 提供用户偏好持久化（TOML），可用于 Provider 配置存储
> - 事件系统分为 `Event`（即时 + Observer）和 `Message`（缓冲 + Reader/Writer）两种
> - `bevy_gizmos` 提供丰富的调试绘图 API，适合绘制依赖虫洞等线条

#### UI 层

```toml
# 无需额外依赖！以下均通过 bevy 的 "ui" feature 自动包含：
# - bevy_ui: Node 布局 + Flexbox + CSS Grid + Interaction
# - bevy_ui_widgets: 无头组件 (Button, Checkbox, Slider, Scrollbar, RadioGroup,
#   EditableText/TextInput, Menu, Popover)
# - bevy_feathers: 带主题的样式化控件 (Button, Checkbox, Slider, ToggleSwitch,
#   TextInput, NumberInput, Radio, Menu, ColorPicker, DisclosureToggle)
# - bevy_feathers: 主题系统 (UiTheme, ~150+ 设计 Token, 暗色主题)
#
# bevy_ui_widgets 组件是"无头"的——只有逻辑没有样式，需配合 bevy_feathers 的主题系统
# 或自定义样式使用。bevy_feathers 依赖 bevy_ui_widgets，提供即开即用的暗色主题。
```

#### 3D 可视化与交互

```toml
# 无需额外依赖！以下均通过 bevy 的 "3d" + "ui" features 自动包含：
# - bevy_picking: 内置拾取系统 (Mesh Ray Cast + UI 拾取 + Sprite 拾取)
#   - Pointer 事件: Click, Over, Enter, Move, Leave, Out, Drag, Scroll...
#   - 支持 EntityEvent 冒泡、Pickable 组件控制、RenderLayers
#   - 使用方式: entity.observe(|event: On<Pointer<Click>>| { ... })
# - bevy_gizmos: 调试绘图 (lines, circles, spheres, arrows, arcs, grids, curves)
#   - 适合绘制依赖虫洞线条、节点高亮环、方向指示等
# - bevy_camera_controller (free_camera feature): 内置飞行相机
#   - FreeCamera 组件: WASD + 鼠标视角 + 加速跑 + 滚轮缩放
#   - 含摩擦力/惯性物理模型
# - bevy_pbr: StandardMaterial + ExtendedMaterial + SSAO + SSR + Bloom + ...
#   - ExtendedMaterial 可扩展自定义着色器（星球特效）
```

#### 代码智能

```toml
tree-sitter = "0.24"                    # 多语言解析框架
tree-sitter-rust = "0.23"               # Rust grammar
tree-sitter-typescript = "0.23"         # TypeScript grammar
tree-sitter-python = "0.23"             # Python grammar
```

#### Git 集成

```toml
git2 = "0.20"                 # libgit2 Rust 绑定
```

#### AI Provider 与协议

```toml
# MCP 协议
rmcp = "0.1"                     # Rust MCP SDK（MCP Client 实现）

# SSE (Server-Sent Events)
eventsource-stream = "0.2"     # MCP SSE 传输层

# 异步 trait
async-trait = "0.1"             # LLMProvider trait
```

#### 安全与存储

```toml
keyring = "3"                  # 操作系统原生 Keychain 访问
zstd = "0.13"                  # 快速压缩（快照存储）
```

> **UI 构建选型**：Bevy 0.19 已将 `bevy_ui_widgets`（无头组件）和 `bevy_feathers`（带主题控件）作为官方 crate 内置。`bevy_feathers` 提供完整的暗色主题和 ~150+ 设计 Token，开箱即用，无需第三方 UI 框架。

---

### 🗺️ 实现路线图

| 阶段 | 内容与目标 | 交付物 |
|:---|:---|:---|
| **MVP-1** | 基础 UI 框架 + Provider/协议层基础：主窗口 + Agent 侧边栏聊天 + OpenAI 兼容适配器 + Provider 注册表 + 基础 MCP Client + 文件显示 | 可对话的 AI 编程助手（2D 模式，支持多模型切换） |
| **MVP-2** | 3D 宇宙原型：文件渲染为静态球体 + 目录层级布局 + 点击选中 + 飞行相机 | 可飞行的 3D 代码导航 |
| **MVP-3** | Agent 深度集成：AgentAvatar 可视化 + 工具调用实时反馈 + 智能路由 + 降级链 + 时间线基础 | Agent 操作在 3D 中可见，多模型智能路由 |
| **Alpha-1** | 代码智能管线 + ACP 协议：Tree-sitter 集成 + 依赖虫洞 + 复杂度着色 + 力导向布局 + 内部 ACP 通信 | 依赖关系可视化 + 多 Agent 协作 |
| **Alpha-2** | 时空回溯：完整时间线 UI + 快照存储 + 分支探索 + 时间线导航 | 全功能时间旅行 |
| **Beta-1** | 高级交互 + MCP 深度集成：分屏对比 + 径向菜单 + 沉浸式面板 + MCP 工具桥接 + DoE 方案卡 + Feathers 主题定制 | 差异化交互体验 + MCP 生态打通 |
| **Beta-2** | 体验打磨：诊断雷达 + 热力图 + 计费面板 + 外部 A2A 探索 + 插件 API + 性能优化 + 打包发布 | 可发布的 Beta 版本 |

**执行原则**：

1. **MVP-1 必须先通**：确保 2D 模式下 Agent 对话和文件操作完全可用，这是产品的实用底线
2. **3D 是增强不是必需**：每个 3D 功能都有对应的 2D fallback
3. **渐进式复杂度**：先做静态球体布局 → 再加交互 → 再加动画 → 再加特效
4. **性能前置验证**：每个阶段都需验证 10K 节点场景的帧率，发现瓶颈立即优化
5. **可玩性验证**：每个阶段结束后做 5 分钟用户体验测试，确保"操作感"达标
6. **Provider 先通**：MVP-1 就要支持至少 OpenAI 兼容 + 一个 MCP Server，确保 Agent 调用链端到端可用
