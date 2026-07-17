# XGent 架构设计文档

> 本文档定义 XGent 的系统架构、进程模型、模块边界、数据流与关键抽象接口。
> 从零设计，不参考旧有设计文档。需求见 `doc/design/requirements.md`。
>
> 状态：草案 v1 · 待评审

---

## 目录

1. [架构总览](#1-架构总览)
2. [进程模型](#2-进程模型)
3. [分层架构](#3-分层架构)
4. [crate 划分](#4-crate-划分)
5. [数据流](#5-数据流)
6. [关键抽象接口](#6-关键抽象接口)
7. [守护进程演进策略](#7-守护进程演进策略)
8. [跨平台与系统窗口](#8-跨平台与系统窗口)
9. [国际化架构](#9-国际化架构)
10. [扩展点：TUI / Web / 3D / 宠物](#10-扩展点tui--web--3d--宠物)
11. [安全模型](#11-安全模型)
12. [技术选型约束](#12-技术选型约束)
13. [待决策点](#13-待决策点)

---

## 1. 架构总览

XGent 采用**多进程 + 分层 + 数据驱动**架构：

- **多进程**：每个项目/窗口一个独立 UI 进程（xgent-ui），共享一个轻量后台守护进程（xgent-daemon）。守护进程负责 provider 连接池、全局配置、文件监听与多客户端文件状态同步。
- **分层**：从下到上分平台层、基础设施层、应用层、交互层、表现层。每层只依赖下层，禁止反向依赖。
- **数据驱动**：UI 是 agent 状态的实时投影；子系统只通过 ECS Events（即时观察者）与 Messages（缓冲消息）通信，禁止直接方法调用。

核心设计原则（来自需求文档）：

- 实用优先、数据驱动、可扩展、轻量、3D 留白、跨平台。
- 所有抽象接口设计为"可上移可下放"（UI 进程 ↔ 守护进程），使守护进程演进（瘦→中→胖）不推翻架构。

```
┌─────────────────────────────────────────────────────────────┐
│                     UI 进程（每项目一个）                      │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  表现层    3D Viewport / 2D Panels / Overlay / 宠物    │  │
│  ├───────────────────────────────────────────────────────┤  │
│  │  交互层    Input Router / Command Palette / Shortcuts  │  │
│  ├───────────────────────────────────────────────────────┤  │
│  │  应用层    Agent Loop / Conversation / Context Builder │  │
│  ├───────────────────────────────────────────────────────┤  │
│  │  基础设施层 Provider Client / Tool Exec / Repo Map     │  │
│  ├───────────────────────────────────────────────────────┤  │
│  │  平台层    Bevy ECS (World/Schedule/Events/Messages)   │  │
│  └───────────────────────────────────────────────────────┘  │
│                          ↕ JSON-RPC                          │
├─────────────────────────────────────────────────────────────┤
│                  守护进程（随用随启，全局唯一）                │
│  Provider 连接池 / 全局配置 / 文件监听 / 多客户端文件状态同步   │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. 进程模型

### 2.1 两个进程角色

| 进程 | 实例数 | 职责 | 生命周期 |
|:---|:---|:---|:---|
| xgent-ui | 每项目/窗口一个 | UI 渲染、交互、agent loop、工具执行（MVP）、上下文构建 | 随窗口开关 |
| xgent-daemon | 全局唯一 | provider 连接池、全局配置协调、文件监听、多客户端文件状态同步 | 随用随启：首个 UI 进程拉起，末个退出时退出 |

### 2.2 启动与发现

- UI 进程启动时，先探测本地 daemon（Unix socket `/tmp/xgent-daemon.sock` on macOS/Linux、named pipe `\\.\pipe\xgent-daemon` on Windows）。
- 若 daemon 未运行，UI 进程 fork 拉起 daemon 并等待就绪；若已运行，直接连接。
- daemon 维护"已连接 UI 客户端"计数，计数归零后延迟退出（避免快速重启抖动）。

### 2.3 IPC 协议

- **JSON-RPC 2.0 over 本地 socket**。
- 请求/响应：UI → daemon（provider 调用、配置读写、文件监听订阅）。
- 通知：daemon → UI（文件变更事件、provider 状态变化、其他客户端文件状态更新）。
- 流式：daemon 的 provider 流式响应通过 JSON-RPC notification 逐 chunk 推送（类似 SSE over JSON-RPC）。
- 选型理由：文本可读、易调试、跨平台、语义可被未来 Web 端复用。

### 2.4 agent loop 归属

- **agent loop 放 UI 侧**（每客户端独立）。daemon 只做资源池，不背编排复杂度。
- 理由：MVP 每客户端独立对话循环，隔离好、崩溃互不影响；未来若要多客户端共享对话再上移。

---

## 3. 分层架构

每层只依赖下层。禁止反向依赖与跨层调用。

### 3.1 平台层（Bevy ECS）

- World、Schedule、Resources、Events、Messages、Systems。
- 所有跨子系统通信只走 Events（即时观察者）或 Messages（缓冲队列）。
- **硬性约束**：子系统之间禁止直接方法调用。目的：每个 Plugin 独立可测、可 headless、可录制/回放消息流。
- **headless 运行**：daemon 本身不依赖 Bevy（纯 tokio）即 headless；UI 侧测试可用 `MinimalPlugins`（无渲染）驱动 agent 逻辑，验证事件流。

### 3.2 基础设施层

- **Provider Client**：通过 IPC 调用 daemon 的 provider 连接池，封装为 Bevy Resource。流式响应经 channel 喂入 ECS。
- **Tool Exec**：工具执行（MVP 在 UI 侧）。分级确认（见安全模型）。
- **Repo Map / Context**：项目上下文构建。MVP 用方案 A（无索引·按需读取 + ripgrep + 目录树），经 `ContextProvider` trait 抽象，支持未来升级 B/C/D/E。
- **Settings**：配置读写。全局配置经 daemon 协调，项目配置本地。

### 3.3 应用层

- **Agent Loop**：对话循环——构建上下文 → 调 provider → 解析响应 → 若有工具调用则执行（经确认）→ 回灌结果 → 循环。
- **Conversation**：会话状态、历史、中断/重试。
- **Context Builder**：根据当前对话决定取哪些上下文喂 LLM（调用基础设施层 ContextProvider）。
- **Session**：会话管理（新建/切换/历史），持久化到本地 SQLite。

### 3.4 交互层

- **Input Router**：键盘/鼠标路由。
- **Command Palette**：F-08，Cmd+P / Cmd+Shift+P 风格全局命令入口。
- **Shortcuts**：F-09，参考 VSCode 快捷键体系。

### 3.5 表现层

界面设计详见 `doc/design/ui-design.md`，包含布局、面板、交互流、视觉规范、快捷键表。

MVP 范围：

- **布局**：顶栏 + 文件面板（左，可折叠）+ 对话主区（右，flex:1）+ 状态栏。对话为 MVP 主交互区。
- **对话面板**：消息列表（流式渲染）+ 输入框（多行，Ctrl+Enter 发送）+ 中断。
- **工具调用卡片**：内联在对话流中，展示工具名/参数/状态/结果，折叠态摘要。
- **确认弹窗**：`NeedsConfirmation` 工具触发 modal overlay，展示 diff + 允许/拒绝。
- **文件面板**：只读文件树 + 内容预览。
- **状态栏**：provider/model + 会话状态 + token 指示。
- **命令面板**：Cmd+P / Cmd+Shift+P 风格全局命令入口。

预留（MVP 不实现）：

- **3D Viewport**：F-16，作为可选插件层，MVP 不实现。
- **宠物层**：F-15，独立可选模块，可开关；桌面置顶透明窗口渲染（见第 8 节）。
- **Overlay/HUD**：通知、token 流式指示（P1）。

---

## 4. crate 划分

采用 workspace 多 crate 结构。crate 边界与分层对应，依赖方向严格自上而下。

```
xgent/                     workspace 根
├── crates/
│   ├── xgent_core/            # 共享类型：事件/消息/错误/协议契约（被 UI 与 daemon 共用）
│   ├── xui_i18n/              # (lib) i18n trait（StringSource），纯无依赖，被 xui 与 xgent_settings 共用
│   ├── xgent_settings_core/   # (lib) 配置纯类型（GlobalConfig/ProjectConfig 等），不依赖 Bevy；daemon/provider 用
│   ├── xgent_settings/        # (lib) 配置的 Bevy Resource 包装 + TOML 读写 + fluent Localizer（impl StringSource）
│   ├── xgent_daemon/          # (bin) 守护进程：provider 池、配置、文件监听、多客户端同步（不依赖 Bevy）
│   ├── xgent_provider/        # provider 抽象 + OpenAI compatible / Response API / 自定义 API 适配（不依赖 Bevy）
│   ├── xgent_tools/           # 工具枚举 + 安全策略 + 执行器（不依赖 Bevy）
│   ├── xgent_context/         # 项目上下文检索（ContextProvider trait + A 方案实现，不依赖 Bevy）
│   ├── xgent_agent/           # agent loop + 对话编排 + ECS 桥接
│   ├── xui/                   # (lib) 通用 Bevy UI 组件库（可独立发布，纯依赖 bevy + xui_i18n）
│   ├── xgent_ui/              # (lib) XGent 业务 UI：对话/工具/文件面板等（依赖 xui）
│   ├── xgent_pet/             # 虚拟宠物（可选模块，可开关）
│   └── xgent_app/             # (bin) UI 进程入口，组装 UI 侧所有插件
└── doc/                       # 文档
```

### 4.1 关于 settings 拆分（core + Bevy 包装）

`xgent_settings` 拆为两层，解决 daemon/provider “不依赖 Bevy”与配置类型需 Bevy 派生的矛盾：

- **`xgent_settings_core`**：纯配置类型（`GlobalConfig`/`ProviderConfig`/`ProjectConfig`/`ContextStrategy` 等）+ TOML 读写（`ConfigStore`）+ 平台路径工具，**不依赖 Bevy**。daemon 与 provider 依赖它，保持轻量。
- **`xgent_settings`**：在 core 类型上做 Bevy Resource 包装（derive `Resource`/`Reflect`）+ fluent `Localizer`（impl `xui_i18n::StringSource`）。agent/ui 依赖它。

### 4.1 关于 xui（通用 UI 组件库）

`xui` 是一个**可脱离 XGent 独立发布、被其他 Bevy 项目复用**的通用 UI 组件库，纯依赖 bevy + xui_i18n，不依赖任何 `xgent_*` crate。

**动机**：bevy_feathers / bevy_ui_widgets 官方明确标注 experimental、会 breaking、建议“copy into your own project”。`xui` 作为薄封装层隔离官方 breaking change，集中升级成本。

**封装策略**：官方已覆盖的（button/checkbox/slider/dialog/menu/popover 等基础 widget，以及 text_input 的 IME 支持）**直接用官方**，不重复造轮子；`xui` 只封装官方未覆盖或需增强的部分。

**封装范围**：

| 编号 | 模块 | 阶段 |
|:---|:---|:---|
| K-02 | 虚拟列表组件（只渲染可见项，大列表性能） | MVP |
| K-03 | 命令面板组件（模糊匹配 + 键盘导航 + 命令注册表） | MVP |
| K-05 | 输入增强封装（多行 + 发送语义，基于官方 text_input 薄封装，不重写核心） | MVP |
| K-06 | 快捷键体系（注册表 + 冲突检测 + 平台修饰键抽象） | MVP |
| K-07 | i18n 桥接（与 fluent Localizer 的 UI 渲染桥接辅助） | MVP |
| K-01 | 主题系统增强（可切换/可定制主题层） | P1 延后 |
| K-04 | 系统窗口管理（多窗口、透明置顶无边框跨平台封装，宠物用） | P1 延后 |

MVP 阶段主题直接用官方 `bevy_feathers::dark_theme`，系统窗口用官方 `bevy_window` 默认能力。

### 依赖关系（无环）

```
xgent_core ←──────── 一切共享类型的基础
     ↑
xui_i18n ← xui, xgent_settings   （纯 trait，无依赖）
xgent_settings_core ← xgent_daemon, xgent_provider, xgent_settings
xgent_provider ← xgent_daemon（provider 池）
xgent_tools ← xgent_agent
xgent_context ← xgent_agent
xgent_settings ← xgent_daemon, xgent_agent, xgent_ui
     ↑
xgent_agent ← xgent_ui
xgent_pet ← xgent_ui（可选）
     ↑
xui（纯依赖 bevy + xui_i18n，可独立发布） ← xgent_ui
     ↑
xgent_app → 组装所有 UI 侧 crate
```

说明：
- `xgent_core` 承载跨进程共享的协议类型（JSON-RPC 请求/响应/通知的 schema、事件类型、错误类型），UI 与 daemon 都依赖它。
- `xgent_settings_core` 是纯配置类型层，不依赖 Bevy，供 daemon/provider 使用，避免拖入 Bevy。
- `xgent_settings` 是 core 的 Bevy Resource 包装 + fluent Localizer，供 agent/ui 使用。
- `xui_i18n` 是极小 crate，只放 `StringSource` trait，纯无依赖。`xui` 与 `xgent_settings` 都依赖它——`xui` 定义 trait 使用点，`xgent_settings::Localizer` impl 该 trait。这样 `xui` 不依赖 `xgent_*`，仍可独立发布。
- `xgent_provider` 同时被 daemon（连接池实现）与 UI（客户端 trait 调用）依赖。
- `xgent_pet` 独立，UI 侧按需加载，关闭时不进编译产物或运行时不初始化。
- `xui` 纯依赖 bevy + xui_i18n，**不依赖任何 xgent_* crate**，保证可独立发布被其他 Bevy 项目复用。`xgent_ui` 依赖 `xui` 获取通用组件，自身负责业务 UI（对话面板、文件面板等）。

---

## 5. 数据流

### 5.1 用户提问 → AI 回复（流式）

```
用户输入
  → Input Router → ConversationSystem（追加 user message）
  → AgentLoopSystem
      → ContextBuilder（取项目上下文）
      → ProviderClient（经 IPC 调 daemon）
          → daemon 连接池 → LLM
      → 流式 chunk 经 channel 回灌 ECS（DeltaEvent）
  → ChatPanelSystem 订阅 DeltaEvent → 实时渲染
  → 若响应含 ToolCall → ToolCallEvent
      → ConfirmSystem（高危需确认）→ ToolExecSystem 执行 → ToolResultMessage
      → 回灌 ConversationSystem → 继续 AgentLoop
  → DoneEvent → 会话落 SQLite
```

### 5.2 文件变更同步

```
项目内文件改动
  → daemon 文件监听（notify crate）
  → 判定所属项目 → 推送 FileChanged 通知给订阅该项目的 UI 客户端
  → UI 侧 ECS FileChangedEvent
      → RepoMap 增量更新（B 阶段）
      → 编辑器刷新（若有打开该文件）
      → 对话上下文失效标记
```

### 5.3 多客户端同项目文件状态同步

```
客户端 A 编辑文件 F
  → ToolExec（写文件）→ 通知 daemon
  → daemon 广播 FileChanged(F) 给同项目其他客户端 B
  → 客户端 B 刷新 F 的视图与上下文
```

---

## 6. 关键抽象接口

所有接口设计为"可上移可下放"：MVP 在 UI 侧实现，未来可上移到 daemon 而不破坏调用方。

### 6.1 Provider 抽象

```rust
trait LlmProvider {
    fn id(&self) -> &str;
    fn list_models(&self) -> Vec<ModelInfo>;
    /// 流式对话，返回 tokio mpsc Receiver of ChatEvent
    fn chat(&self, req: ChatRequest) -> ChatStream;
    fn health_check(&self) -> Result<()>;
}
```

- `ChatEvent`：Delta(text) / ToolCall(...) / Done(usage) / Error。
- 适配器：
  - `OpenAiCompatProvider`：OpenAI compatible 接口（OpenAI、DeepSeek、Ollama 兼容模式等）。
  - `ResponseApiProvider`：Response API 风格接口。
  - `AnthropicProvider`：Anthropic 原生。
  - `CustomApiProvider`：用户自定义 endpoint/headers/body 模板。
- daemon 侧：`ProviderPool` 持有各 provider 的连接（reqwest client 复用、连接池、限流）。
- UI 侧：`ProviderClient` 经 IPC 调用 `ProviderPool`，本地无连接。

### 6.2 工具抽象

```rust
trait Tool {
    fn id(&self) -> &str;
    fn schema(&self) -> ToolSchema;          // 给 LLM 的 JSON schema
    fn policy(&self) -> SecurityPolicy;       // Approved / NeedsConfirmation / Denied
    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolResult;
}
```

- 内置工具：ReadFile、WriteFile、SearchFiles、RunCommand、Git*（P1）。
- 未来：McpTool（F-13）、UserDefinedTool（F-14）经同一 trait 接入。
- 执行流程：ToolCall → 查 policy → 若 NeedsConfirmation 则发 ConfirmRequest 到 UI → 用户决策 → 执行或拒绝 → ToolResult。
- MVP 工具执行在 UI 侧；未来上移 daemon 时，`Tool` trait 不变，实现从本地直调改为 IPC 调 daemon。

### 6.3 上下文检索抽象

```rust
trait ContextProvider {
    /// 给定对话与查询，返回应喂给 LLM 的上下文片段
    async fn retrieve(&self, query: &ContextQuery) -> Vec<ContextChunk>;
    /// 通知文件变更，供索引增量更新
    fn on_file_changed(&self, path: &Path);
}
```

- MVP 实现 `OnDemandContextProvider`（方案 A：目录树 + ripgrep + 按需读文件）。
- B 阶段：`RepoMapContextProvider`（tree-sitter 符号图）。
- C 阶段：`VectorContextProvider`（本地嵌入 + 向量库）。
- D 阶段：`LspContextProvider`（LSP/AST）。
- E 阶段：`HybridContextProvider`（多路召回 + 融合排序）。
- 演进切换：通过配置切换实现，调用方（AgentLoop）无感。

### 6.4 配置抽象

- 全局配置（daemon 协调）：provider 列表、API key、全局偏好。TOML 存平台规范路径。
- 项目配置（本地隔离）：项目级 provider 覆盖、上下文策略、工具白名单。TOML 存 `<project>/.xgent/config.toml`。
- 会话历史：JSONL append-only（主存储），存 `<project>/.xgent/sessions/<session_id>.jsonl`。元数据索引/prompt 历史/模型使用统计保留 SQLite（P1）。见 ADR-0008。
- daemon 侧 `ConfigStore` 持有全局配置并协调多客户端读写（文件锁 + 变更通知）。

### 6.5 IPC 协议契约

- 定义在 `xgent_core`，UI 与 daemon 共用。
- 方法（UI → daemon）：
  - `provider.chat`（流式，返回 stream id，后续 notification 推 chunk）
  - `provider.listModels`
  - `config.read` / `config.write`
  - `fs.watch`（订阅项目路径）
- 通知（daemon → UI）：
  - `provider.delta` / `provider.toolCall` / `provider.done` / `provider.error`
  - `fs.changed`
  - `config.changed`
  - `peer.fileChanged`（其他客户端文件状态更新）

---

## 7. 守护进程演进策略

按 `doc/notes/daemon-scope-research.md` 选项 1 演进。所有上移候选职责用 trait 抽象，可上移可下放。

### 7.1 MVP：瘦后台

daemon 职责：a provider 连接池 + b 全局配置协调 + c 文件监听 + d 多客户端文件状态同步。

UI 侧承担：e 索引（无）、f 会话历史（本地 SQLite）、g 工具执行、h 成本统计。

### 7.2 B 阶段（repo map）：升中后台

- 索引职责 e 上移：daemon 统一监听文件 → 增量更新 repo map → 推送订阅项目。
- 会话历史 f 视一致性需求决定是否上移。
- 切换不破坏 `ContextProvider` trait 与调用方。

### 7.3 C/D 阶段 + Web 端：视需求升胖后台

- 向量库/LSP 索引天然放后台长驻。
- Web 端（F-18）强制上移 g 工具执行（WASM 限制）。
- 上移时 `Tool` trait 不变，实现从本地改为 IPC。

### 7.4 演进保证

- 所有"可上移"职责定义 trait，UI 侧注入"本地实现"或"IPC 实现"。
- 切换由配置驱动，编译期（feature）或运行期（配置）均可。
- 目标：任何一次上移都不破坏上层调用方。

---

## 8. 跨平台与系统窗口

### 8.1 跨平台

- Windows / macOS / Linux 桌面端同步支持。
- 本地 socket：Unix domain socket（macOS/Linux）、named pipe（Windows），封装统一接口。
- 文件监听：`notify` crate（跨平台）。
- 平台规范配置路径：macOS `~/Library/Application Support/xgent/`、Windows `%APPDATA%/xgent/`、Linux `~/.config/xgent/`。

### 8.2 系统窗口能力（宠物需要）

桌面宠物 F-15 需要"置顶透明无边框窗口"，Bevy 原生对此支持有限。处理方式：

- Bevy 主窗口承载正常 UI。
- 宠物窗口单独处理：通过 winit 窗口装饰控制（decorations=false、transparent=true、always_on_top），Bevy 支持通过 `Window` 组件配置这些属性。
- 点击穿透等高级能力若 Bevy 不支持，走平台特定方案（后续 P1 宠物实现时评估，MVP 不涉及）。
- 宠物为可选模块（`xgent_pet`），关闭时不初始化、不占资源。

### 8.3 多窗口

- MVP 单窗口（一个 UI 进程一个主窗口）。
- 宠物窗口是唯一额外的系统窗口（P1）。
- 多开 = 多个 UI 进程，每个进程一个主窗口（不依赖 Bevy MultiWindow）。

---

## 9. 国际化架构

- **从一开始内置 i18n**（NF-05）。
- 所有用户可见字符串经 i18n 层，不硬编码。
- 采用 **fluent**（`fluent-rs` + `.ftl` 资源）。资源按语言分目录：`crates/xgent_settings/locales/zh-CN/`、`en-US/` 等。
- **i18n trait 放在 `xui_i18n` crate**（极小、纯无依赖），定义 `StringSource` trait。`xgent_settings::Localizer` impl 该 trait，`xui` 通过 trait 调用取字符串。这样 `xui` 不依赖 `xgent_*`，仍可独立发布。
- UI 侧通过注入的 `StringSource` 资源获取当前语言字符串，语言切换实时生效。
- 前期以中文为主，但架构保证所有字符串可翻译。

---

## 10. 扩展点：TUI / Web / 3D / 宠物

所有扩展点在架构上预留，MVP 不实现、不阻塞。

### 10.1 TUI（F-17）

- 核心逻辑（agent、provider、tools、context）与 UI 前端解耦。
- 未来 TUI 作为同一套核心的另一套前端：复用 `xgent_core` / `xgent_agent` / `xgent_provider` 等，替换 `xgent_ui` 为 TUI 实现（ratatui）。
- 保证：核心层不依赖任何渲染/UI 类型。

### 10.2 Web（F-18）

- provider/tools/context 走 trait 抽象，Web 端注入不同实现（工具执行经服务端中转，文件访问经服务端）。
- JSON-RPC 协议语义可复用。
- 胖后台是 Web 端的强制前提（见第 7 节）。

### 10.3 3D（F-16）

- 作为可选插件层。
- 数据源是 ECS 的现有状态（项目结构、文件、对话历史），3D 只读投影。
- MVP 不实现，但 ECS 数据契约保证 3D 可读取所需状态。

### 10.4 宠物（F-15）

- 独立 crate `xgent_pet`，可开关。
- 订阅 agent 状态 Event（思考中/执行工具/完成/出错）映射为宠物表情动作。
- 等级随使用时长增长，仅解锁外观，不影响功能。
- 桌面置顶透明窗口渲染（见第 8 节）。

---

## 11. 安全模型

### 11.1 工具执行分级

工具能力信任**可配置**，参考成熟 code agent 的做法：默认所有工具调用（含只读类）均为 `NeedsConfirmation`，用户可在配置中按工具 id 提升为 `Approved`（自动执行）或降为 `Denied`。

| 级别 | 行为 | 默认 |
|:---|:---|:---|
| Approved | 自动执行，无需确认 | 可配置提升 |
| NeedsConfirmation | 弹窗确认后执行 | **默认值** |
| Denied | 拒绝执行 | 可配置降级 |

配置示例（项目或全局）：`ReadFile/SearchFiles` 可提升为 Approved 以减少打扰，`RunCommand` 保持 NeedsConfirmation，危险操作可 Denied。

### 11.2 多客户端权限

- 同项目多客户端的文件写操作经 daemon 广播，避免冲突。
- MVP 工具执行在 UI 侧，权限就近；上移 daemon 后需设计后台执行权限模型（未来）。

### 11.3 API Key 存储

- API key 存全局配置（TOML），daemon 统一持有与使用，UI 侧不直接接触 key。
- 是否引入 OS keychain（macOS Keychain / Windows Credential Manager）待定（见第 13 节）。

---

## 12. 技术选型约束

已确定的选型：

| 项目 | 选型 | 理由 |
|:---|:---|:---|
| UI/渲染/ECS | Bevy 全栈 | 数据驱动、未来 3D 无缝、统一技术栈 |
| 异步运行时 | tokio（Bevy Resource，系统每帧轮询 channel） | 成熟、与 reqwest/SSE 生态一致 |
| HTTP 客户端 | reqwest | 成熟、流式支持 |
| SSE 解析 | eventsource-stream | 流式 provider 必需 |
| 序列化 | serde + serde_json | 标准 |
| IPC | JSON-RPC 2.0 over 本地 socket | 可读、跨平台、可复用语义 |
| 配置存储 | TOML（配置）+ SQLite（会话历史/日志） | 配置可读、历史结构化 |
| 文件监听 | notify | 跨平台 |
| 错误处理 | thiserror | 标准 |
| i18n | fluent（fluent-rs + `.ftl` 资源） | 规范完备、复数/性别支持、运行时切换、翻译协作成熟 |
| 守护进程并发 | tokio 多任务（Arc<RwLock> 共享状态） | IO 密集场景最自然，与 reqwest/SSE 生态一致 |

---

## 13. 待决策点

以下点在架构上已留口子，具体方案待实现时定，不阻塞架构定稿：

- **D-01（已决策）**：i18n 采用 **fluent**（Mozilla ICU MessageFormat，`.ftl` 资源，`fluent-rs` 实现）。选型理由：规范完备、复数/性别/选择全支持、运行时切换语言天然支持、翻译协作工具链成熟、未来多语言扩展强。UI 侧封装 `Localizer` Bevy Resource 调用 fluent bundle，语言切换实时生效。资源按语言分目录：`crates/xgent_settings/locales/<lang>/`。
- **D-02**：API Key 是否引入 OS keychain（macOS Keychain / Windows Credential Manager）增强安全性，还是仅存 TOML。
- **D-03（已决策）**：守护进程采用 **tokio 多任务并发**模型（共享状态用 `Arc<RwLock>`，锁粒度小：连接池表、配置、订阅表）。理由：daemon 是 IO 密集（网络/文件/IPC），reqwest 与 eventsource-stream 都基于 tokio，多任务最自然。
- **D-04**：会话历史 SQLite schema 与全文检索需求（是否需要对历史对话做搜索）。
- **D-05**：命令面板（F-08）的命令注册机制（声明式注册 vs 反射）。
- **D-06**：tree-sitter grammar 的分发方式（随二进制打包 vs 按需下载），B 阶段前需定。
- **D-07**：宠物等级与使用时时的具体度量（按对话轮数、活跃时长、还是其他），P1 阶段定。

### 13.1 需求 OQ 与架构 D 决策编号映射

两套编号体系重叠部分对照（便于交叉查阅）：

| 需求 OQ | 架构 D | 主题 | 状态 |
|:---|:---|:---|:---|
| OQ-01 | — | provider 接口范围 | 已决策（见 F-07） |
| OQ-02 | — | 内置编辑器能力边界 | 已决策（见 F-11） |
| OQ-03 | — | 快捷键体系 | 已决策（见 F-09） |
| OQ-04 | — | 宠物外观来源 | 已决策（见 4.3） |
| OQ-05 | — | 宠物对话陪伴呈现形式 | 待 P1 定 |
| OQ-06 | — | 多开隔离与共享边界 | 已决策（见 NF-02） |
| OQ-07 | D-04 | 会话持久化形态 | 待实现时定 |
| OQ-08 | — | 项目上下文检索策略 | 已决策（见 4.3/正文） |
| OQ-09 | D-01 | i18n 方案 | 已决策（fluent） |
| OQ-10 | — | 成本统计细节 | 待实现时定 |
| — | D-02 | API Key 是否用 OS keychain | 待定 |
| — | D-03 | 守护进程并发模型 | 已决策（tokio 多任务） |
| — | D-05 | 命令注册机制 | 待定 |
| — | D-06 | tree-sitter grammar 分发 | 待定（B 阶段前） |
| — | D-07 | 宠物等级度量 | 待定（P1） |
