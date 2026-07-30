# XGent 插件系统设计文档

> 本文档定义 XGent 插件系统的架构、加载机制、扩展点契约与实现计划。
> 参考 Zed 的 WASM 插件机制，结合 XGent 的 Bevy ECS 架构与多进程模型做适配设计。
>
> 状态：草案 v2 · 已据 review 修订（对齐 Tool trait 全签名、统一 id 命名空间、补 daemon 复用路径与 i18n 待决策点）

---

## 目录

1. [设计目标与约束](#1-设计目标与约束)
2. [Zed 插件机制调研总结](#2-zed-插件机制调研总结)
3. [XGent 插件系统总览](#3-xgent-插件系统总览)
4. [加载机制：WASM 组件模型](#4-加载机制wasm-组件模型)
5. [扩展点契约](#5-扩展点契约)
6. [插件清单](#6-插件清单)
7. [插件宿主与注册表](#7-插件宿主与注册表)
8. [插件生命周期](#8-插件生命周期)
9. [沙箱与安全](#9-沙箱与安全)
10. [配置与设置](#10-配置与设置)
11. [内建插件迁移计划](#11-内建插件迁移计划)
12. [新增 crate 与依赖关系](#12-新增-crate-与依赖关系)
13. [分步实现计划](#13-分步实现计划)
14. [待决策点](#14-待决策点)

---

## 1. 设计目标与约束

### 1.1 设计目标

- **动态安装/卸载**：插件可在运行时安装、卸载、启用/禁用、升级，无需重新编译宿主。
- **安全隔离**：插件运行在沙箱中，无法直接访问宿主内存或文件系统（除非经授权）。
- **多扩展点**：插件可注册 Agent 工具、命令面板命令、Provider 适配器、ContextProvider、UI 面板等。
- **与现有架构兼容**：不破坏现有 ECS 通信契约（Events/Messages）、进程模型、安全模型。
- **离线优先**：插件编译为 WASM，随二进制或本地安装，不依赖远程服务运行（远程仅用于下载分发）。

### 1.2 约束

- **Rust 生态**：插件用 Rust 编写，编译为 `wasm32-wasip2` 目标。
- **Bevy ECS 桥接**：插件不能直接操作 Bevy World，经宿主提供的 API 间接交互。
- **daemon 不依赖 Bevy**：插件宿主在 UI 进程侧（MVP）；daemon 侧的 provider 池未来可扩展支持插件 provider，但 MVP 不涉及。
- **ECS 通信硬约束不变**：插件与宿主之间通过宿主 API（经 WIT 接口）通信，不直接发 ECS Message。

### 1.3 不做（Non-Goals）

- 不做插件市场/远程服务端（MVP 仅本地安装 + 可选的 tar.gz 下载 URL）。
- 不做插件间直接通信（插件只与宿主交互）。
- 不做热重载开发模式（MVP 用 dev symlink + 手动 reload）。
- 不做 TUI/Web 端插件加载（后续形态再说）。

---

## 2. Zed 插件机制调研总结

Zed 的插件系统经过充分验证，核心设计如下：

### 2.1 架构分层

| 层 | crate | 职责 |
|:---|:---|:---|
| 插件 API | `zed_extension_api` | 插件作者面向的 trait + WIT 绑定 + `register_extension!` 宏 |
| 插件宿主 | `extension_host` | `ExtensionStore`（加载/卸载/索引）+ `WasmHost`（wasmtime 引擎）+ `ExtensionBuilder`（编译） |
| 宿主代理 | `extension::ExtensionHostProxy` | 反转依赖枢纽：各子系统注册 proxy impl，插件经 proxy 回调宿主 |
| 插件清单 | `extension::ExtensionManifest` | `extension.toml` 解析为结构化清单（id/name/version/扩展点声明） |

### 2.2 核心机制

1. **WASM 组件模型**：插件编译为 `wasm32-wasip1` + wit-component 适配为 WASM Component。wasmtime 引擎加载执行，WASI preview2 提供文件系统/环境沙箱。
2. **WIT 接口契约**：用 WIT（WebAssembly Interface Type）定义宿主↔插件接口，`wit-bindgen` 生成 Rust 绑定。接口版本化管理（`since_v0_0_1` ~ `since_v0_3_0`），宿主按 `wasm_api_version` 兼容性加载。
3. **ExtensionHostProxy 反转依赖**：宿主各子系统（language/theme/slash_command/context_server）各注册一个 proxy trait impl 到 `ExtensionHostProxy`。插件经 WIT 调用 → `WasmState` → `on_main_thread` → proxy → 宿主子系统。这使 `extension` crate 不依赖任何业务 crate。
4. **清单驱动加载**：`extension.toml` 声明插件提供的能力（grammars/languages/language_servers/slash_commands/context_servers/themes/snippets）。`ExtensionStore` 扫描安装目录，解析清单，构建索引（`ExtensionIndex`），据索引 diff 执行加载/卸载。
5. **文件监听自动重载**：notify 监听安装目录，变化时 debounce 200ms 后重建索引 + reload。
6. **dev 模式**：`install_dev_extension` 用 symlink 指向源码目录，`rebuild_dev_extension` 重新编译 WASM 后 reload。

### 2.3 插件作者体验

```rust
// 插件 Cargo.toml: crate-type = ["cdylib"], 依赖 zed_extension_api
struct MyExtension;

impl zed::Extension for MyExtension {
    fn new() -> Self { MyExtension }

    fn language_server_command(&mut self, id: &LanguageServerId, worktree: &Worktree) -> Result<Command> {
        // 返回启动 LSP 的命令
    }
}

zed::register_extension!(MyExtension);
```

`register_extension!` 宏生成 `init-extension` 导出函数，宿主加载 WASM 后调用它初始化插件实例。

### 2.4 适配 XGent 的关键差异

| 维度 | Zed | XGent |
|:---|:---|:---|
| 宿主框架 | GPUI（自研 UI 框架） | Bevy ECS |
| 通信模型 | Entity + EventEmitter + Actions | ECS Events/Messages + Resource |
| 扩展点 | language_server/grammar/theme/slash_command/context_server/snippet/indexed_docs | agent_tool/command/provider/context_provider/ui_panel |
| 进程模型 | 单进程 | 双进程（UI + daemon） |
| provider | 无（Zed 无 LLM） | LlmProvider trait（插件可注册新 provider 适配器） |

核心适配点：**XGent 的 ECS 通信契约要求插件不直接操作 World**。插件经 WIT 接口调用宿主 API，宿主在 tokio task 或 ECS 系统内桥接为 Message/Resource 变更。

---

## 3. XGent 插件系统总览

```
┌─────────────────────────────────────────────────────────────────┐
│                        UI 进程（xgent_app）                       │
│                                                                  │
│  ┌──────────────┐    WIT 接口    ┌──────────────────────────┐   │
│  │  插件 WASM   │ ←────────────→ │    PluginHost            │   │
│  │ (wasmtime)   │                │  ┌────────────────────┐  │   │
│  │              │                │  │ ExtensionStore     │  │   │
│  │ Git 插件     │                │  │ (加载/卸载/索引)    │  │   │
│  │ Markdown 插件│                │  ├────────────────────┤  │   │
│  │ ...          │                │  │ WasmEngine         │  │   │
│  │              │                │  │ (wasmtime 实例池)   │  │   │
│  └──────────────┘                │  ├────────────────────┤  │   │
│                                  │  │ PluginHostProxy    │  │   │
│                                  │  │ (反转依赖枢纽)      │  │   │
│                                  │  └────────────────────┘  │   │
│                                  └───────────┬──────────────┘   │
│                                              │                   │
│              ┌───────────────┬───────────────┼───────────────┐   │
│              ↓               ↓               ↓               ↓   │
│         ToolExecutor   CommandRegistry  ProviderPool   ContextHub │
│         (agent 工具)    (命令面板)      (provider 池)  (上下文)   │
│              ↓               ↓               ↓               ↓   │
│         ════════════════ Bevy ECS Events/Messages/Resource ═════ │
│                                                                  │
│                          ↕ JSON-RPC                              │
├──────────────────────────────────────────────────────────────────┤
│                    守护进程（xgent_daemon）                        │
│             Provider 连接池 / 全局配置 / 文件监听                   │
└──────────────────────────────────────────────────────────────────┘
```

**核心设计决策**：

1. **WASM 组件模型 + wasmtime**：与 Zed 一致。安全沙箱、跨平台、版本化接口。
2. **PluginHostProxy 反转依赖**：与 Zed 一致。`xgent_plugin` crate 不依赖任何业务 crate，各子系统注册 proxy impl。
3. **扩展点适配层**：每个扩展点有一个 proxy trait + 一个适配系统，把插件注册的能力桥接进现有 ECS 体系。

---

## 4. 加载机制：WASM 组件模型

### 4.1 技术栈

| 组件 | 选型 | 理由 |
|:---|:---|:---|
| WASM 运行时 | `wasmtime`（component model + async） | Zed 验证过、成熟、Rust 原生 |
| 编译目标 | `wasm32-wasip2` | 原生 component model，无需 preview1→preview2 适配器（Zed 用 wasip1 + adapter 是历史原因） |
| 接口定义 | WIT（`wit-bindgen` 生成绑定） | 类型安全、版本化、跨语言预留 |
| WASI | `wasmtime-wasi`（preview2） | 文件系统沙箱、环境变量 |
| 插件 API crate | `xgent_plugin_api`（对标 `zed_extension_api`） | 插件作者面向的 trait + 宏 |

### 4.2 WASM 引擎管理

```rust
/// WASM 引擎单例（全局唯一，所有插件共享）。
///
/// 引擎创建失败是致命错误（配置非法 / 资源不足），经 `PluginHost::new`
/// 在启动时一次性初始化并传播错误（库代码避免 `unwrap`，见 AGENTS §5.7）。
fn wasm_engine() -> Result<wasmtime::Engine, PluginError> {
    static ENGINE: OnceLock<wasmtime::Engine> = OnceLock::new();
    ENGINE.get_or_try_init(|| {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        wasmtime::Engine::new(&config).map_err(PluginError::EngineInit)
    }).cloned()
}
```

- 单 `Engine`，多 `Store`（每插件一个 Store，隔离状态）。
- `Store` 持有 `PluginState`（WASI ctx + ResourceTable + manifest + host proxy 引用）。
- 插件调用经 `mpsc::UnboundedSender<PluginCall>` 序列化（同一插件的调用串行，不同插件并行）——与 Zed 的 `WasmExtension.tx` 模式一致。

### 4.3 线程模型

```
ECS 主线程（Bevy Update）
    │
    │ 1. PluginPollSystem 每帧 try_recv drain（插件生命周期事件：加载/卸载/重载）
    ↓
PluginHost Resource（持有 mpsc::Receiver<PluginLifecycleEvent>）
    │
    │ 2. 工具执行：agent_loop_task 调 ToolExecutor.execute(plugin_tool)
    ↓
tokio task（wasmtime async store，与 agent_loop 同 task）
    │
    │ 3. WASM 函数执行，经 WIT 回调宿主（host.read-file 等）
    ↓
PluginHostProxy → 各子系统（ToolExecutor / CommandRegistry / ...）
    │
    │ 4. 工具结果经 ToolResult 直接返回 agent_loop（与内建工具同一路径，见 §5.3.1）
    ↓
agent_loop 回灌 conv.messages（不另开 PluginEvent 通道）
```

**关键约束**：ECS 主线程永不 `await`。所有 WASM 调用在 tokio task 内执行——与现有 `AgentBridge` 模式一致（见 dev-tutorial §5.1.1.4）。

**两类回流的区分**：
- **工具执行结果**：请求-响应语义，走 `Tool::execute` 同步 trait 返回（tokio task 内 await），**不**经 ECS Message。与内建工具完全一致（见 §5.3.1）。
- **插件生命周期事件**（加载/卸载/启用/禁用完成的通知）：动作信号，经 `mpsc::Receiver<PluginLifecycleEvent>` → `PluginPollSystem` 每帧 drain → ECS Message（如 `PluginLoadedMessage`）。UI 据此刷新插件管理面板。

### 4.4 异步取消（CancellationToken → WASM）

插件工具的 `execute` 接收 `tokio_util::sync::CancellationToken`（ADR-0007），cancel 时需中断 WASM 调用。机制：

- `WasmHost::execute_tool` 在 tokio task 内 `tokio::select!` 监听 `signal.cancelled()` 与 wasmtime async future。
- cancel 触发时 **drop wasmtime future**：wasmtime async store 的 future 被 drop 即停止推进，`Store` 状态保留（不损坏），返回 `ToolError::Aborted`。
- **不使用** `Store::cancel_handle`（那是同步中断 WASM 执行的 API，与 async future drop 语义重叠，选 drop 更简洁）。

**实现风险标注**（Step P2 验证项）：
- 需验证 wasmtime 27 的 async future drop 是否可靠中断 WASM 执行（不残留后台 task、不泄漏 Store）。
- 需验证 WASM 内调宿主回调（`host.read-file` 等）期间 cancel 是否立即生效（若回调阻塞，drop future 可能等回调返回）。
- 若 drop 语义不足，退化为 `Store::cancel_handle` + 轮询超时兜底。此项在 Step P2 单元测试中验证后定稿。

---

## 5. 扩展点契约

### 5.1 扩展点总览

| 扩展点 | WIT 接口 | 宿主适配 | 现有 trait 对接 |
|:---|:---|:---|:---|
| Agent 工具 | `tool` | `PluginTool` 包装为 `Tool` trait | `ToolExecutor.register()` |
| 命令面板命令 | `command` | `PluginCommand` 注册到 `CommandRegistry` | `xui::CommandRegistry` |
| Provider 适配器 | `provider` | `PluginProvider` 包装为 `LlmProvider` trait | `ProviderPool`（daemon 侧，MVP 不接） |
| ContextProvider | `context_provider` | `PluginContextProvider` 包装为 `ContextProvider` trait | `ContextHub` |
| UI 面板 | `ui_panel` | 注册面板元数据，`xgent_ui` 渲染 | 布局系统（P2） |

### 5.2 WIT 接口定义（MVP 范围）

```wit
// xgent-plugin-api/wit/plugin.wit

package xgent:plugin;

interface host {
    // 宿主提供给插件的能力
    read-file: func(path: string) -> result<string, string>;
    write-file: func(path: string, content: string) -> result<_, string>;
    log: func(level: log-level, message: string);
    get-config: func(key: string) -> option<string>;
}

interface tool {
    // 插件提供给宿主的工具能力
    register: func(tools: list<tool-def>);

    // 同步方法：供 PluginTool 适配器实现 Tool trait 的同步方法
    summarize: func(tool-id: string, input: json) -> string;
    approval-for: func(tool-id: string, input: json) -> tool-tier;

    // 异步方法：execute 返回完整 tool-result；
    // preview-diff 返回 option，None 时确认弹窗退化为纯文本。
    execute: func(tool-id: string, input: json) -> result<tool-result, string>;
    preview-diff: func(tool-id: string, input: json) -> option<diff-pair>;
}

// execute 期间插件经 host.push-update 推流式中间结果（对齐 ToolUpdateCallback）。
// host 接口补充：push-update: func(tool-id: string, update: tool-result);

interface command {
    register: func(commands: list<command-def>);
}

interface context-provider {
    register: func(providers: list<provider-def>);
}

// 插件入口
world plugin {
    import host;
    export tool;
    export command;
    export context-provider;
}
```

### 5.3 Agent 工具扩展点（详细）

插件注册的工具经 `PluginTool` 适配器包装为 `xgent_tools::Tool` trait 实现，注入 `ToolExecutor`。适配器必须实现现有 `Tool` trait 的**全部**方法（`id` / `schema` / `tier` / `approval_for` / `concurrency` / `summarize` / `preview_diff` / `execute`），签名与返回类型须与 `crates/xgent_tools/src/tool.rs` 完全一致：

```rust
/// 插件工具适配器：把 WIT tool 调用桥接为 Tool trait。
///
/// 持有宿主侧 `WasmHost` 句柄，各方法经 WIT 回调插件实例。
/// `tool_def` 来自清单 + 插件 `register` 时的 `tool-def`。
pub struct PluginTool {
    manifest: Arc<PluginManifest>,
    tool_def: ToolDef,           // 含 id / schema / tier / concurrency（清单声明）
    host: Arc<WasmHost>,
}

#[async_trait::async_trait]
impl xgent_tools::Tool for PluginTool {
    fn id(&self) -> &str {
        // 统一命名空间：plugin.<plugin_id>.<tool_id>
        // （见 §5.5 id 命名空间约定）
        &self.tool_def.full_id
    }

    fn schema(&self) -> xgent_core::chat::ToolSchema {
        self.tool_def.schema.clone().into()
    }

    /// 静态分层，由清单 `[tools].definitions[].tier` 声明，
    /// 字符串映射到 `ToolTier`（见 §9.3 tier 映射表）。
    fn tier(&self) -> xgent_tools::ToolTier {
        self.tool_def.tier
    }

    /// 按输入动态收紧 tier：经 WIT `tool.approval-for` 让插件
    /// 对危险输入返回更高 tier（如 git 工具检测 force-push 返回 Write）。
    /// 插件未实现该方法时回退到 `tier()`。
    fn approval_for(&self, input: &Value) -> xgent_tools::ToolTier {
        // 同步调用：WIT tool.approval-for 是同步函数，在当前 tokio task 内直调
        self.host.call_approval_for(&self.tool_def.id, input)
            .unwrap_or(self.tier())
    }

    /// 并发声明，由清单声明（read→Shared，write/exec→Exclusive）。
    fn concurrency(&self) -> xgent_tools::Concurrency {
        self.tool_def.concurrency
    }

    fn summarize(&self, input: &Value) -> String {
        // 经 WIT `tool.summarize` 同步调用，返回人类可读摘要
        self.host.call_summarize(&self.tool_def.id, input)
            .unwrap_or_else(|_| self.tool_def.description.clone())
    }

    /// 为确认弹窗提供 diff 数据（ADR-0007 确认弹窗依赖此方法渲染行级 diff）。
    /// 经 WIT `tool.preview-diff` 调用插件；插件未实现时返回 None，
    /// 确认弹窗退化为纯文本 summary（与内建只读工具一致）。
    async fn preview_diff(
        &self,
        input: &Value,
        _ctx: &xgent_tools::ToolCtx,
    ) -> Option<(String, String)> {
        self.host.call_preview_diff(&self.tool_def.id, input).await.ok().flatten()
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &xgent_tools::ToolCtx,
        signal: tokio_util::sync::CancellationToken,
        on_update: Option<&xgent_tools::ToolUpdateCallback>,
    ) -> Result<xgent_tools::ToolResult, xgent_tools::ToolError> {
        // 执行路径见 §5.3.1：在 tokio task 内 await WIT tool.execute，
        // 结果经 AgentEvent channel 回 agent_loop（与内建工具一致），
        // 不另开 PluginEvent 通道。
        self.host.execute_tool(&self.tool_def.id, input, ctx, signal, on_update).await
    }
}
```

#### 5.3.1 执行与结果回流路径

插件工具的 `execute` 与内建工具走**同一桥接路径**，不引入独立的 `PluginEvent` 通道：

```
agent_loop_task（tokio）
  → ToolExecutor.execute(plugin_tool, input)
  → PluginTool::execute → WasmHost 在同 tokio task 内 await WIT tool.execute
      ↓ wasmtime async store（cancel 经 Store::cancel_handle + future drop，见 §4.4）
  → 插件返回 tool-result（output / is_error / denied / side_effect）
  → ToolExecutor 把 ToolResult 回灌 conv（与内建工具完全一致）
  → agent_loop 继续
```

`on_update`（流式中间结果）经 WIT `tool.execute` 内的宿主回调 `host.push-update` 回灌，`WasmHost` 把回调转成 `ToolUpdateCallback` 调用——与 `RunCommand` 的 stdout 增量同构。MVP 阶段若插件不实现流式更新，`on_update` 传 `None`。

#### 5.3.2 安全模型对接

- 插件工具的 `tier()` 由清单 `[tools].definitions[].tier` 声明，字符串映射到 `ToolTier`（映射见 §9.3）。
- `resolve_policy` 对插件工具正常工作，4 步顺序与内建工具一致（配置 denied → 配置 approved → `approval_for(input)` 动态 tier → 兜底 `NeedsConfirmation`）。
- 插件工具**默认 `NeedsConfirmation`**（含只读 tier，与内建工具默认一致）；用户可在配置中按完整 id 提升/降级。
- `ToolTier::UiOnly` 对插件**不支持**：插件不能注册 UI-only 工具（UI-only 工具操作宿主 UI 状态，插件无权直接操作 Bevy World；如需 UI 动作，走 §5.4 命令扩展点或 P2 的 UI 面板扩展点）。清单声明 `tier = "ui_only"` 时加载拒绝。

### 5.4 命令面板扩展点

插件注册的命令注入 `CommandRegistry`，触发时经 WIT 回调插件：

```rust
/// 插件命令适配器。
pub struct PluginCommand {
    manifest: Arc<PluginManifest>,
    def: CommandDef,
    host: Arc<WasmHost>,
}

// 注册到 CommandRegistry
fn register_plugin_commands(registry: &mut CommandRegistry, plugin: &PluginInstance) {
    for def in &plugin.commands {
        registry.register(PaletteCommand {
            id: format!("plugin.{}.{}", plugin.id, def.id),
            label: def.label.clone(),
            kind: CommandKind::Action,
        });
    }
}
```

`handle_palette_triggers` 检测 `plugin.` 前缀的命令 id，经 `PluginHost` 调 WIT `command.run`。

**对现有 crate 的改动**（落地时需修改）：
- `xui::CommandRegistry` 现仅有 `register`（`crates/xui/src/command_palette.rs:35`），**需新增 `unregister(plugin_id)`**——按 id 前缀 `plugin.<plugin_id>.` 批量移除，供 `PluginCommandProxy::unregister_commands` 调用。
- `handle_palette_triggers` 现不存在：需在 `xgent_ui` 新增系统（或扩展现有命令触发系统），检测 `PaletteTriggered.id` 以 `plugin.` 开头时，从 `PluginHost` Resource 取 `WasmHost` 句柄，经 WIT `command.run` 回调插件。非 `plugin.` 前缀的命令走现有内建逻辑。

### 5.5 id 命名空间约定

所有插件注册的能力（工具、命令、context provider）使用统一 id 格式：

| 能力 | id 格式 | 示例 |
|:---|:---|:---|
| 工具 | `plugin.<plugin_id>.<tool_id>` | `plugin.git.git_diff` |
| 命令 | `plugin.<plugin_id>.<command_id>` | `plugin.git.commit` |
| ContextProvider | `plugin.<plugin_id>.<provider_id>` | `plugin.git.git_history` |

**理由**：
- `plugin.` 前缀使宿主可据前缀区分插件能力与内建能力，路由到 `PluginHost`。
- `<plugin_id>.<sub_id>` 二段式使 `unregister(plugin_id)` 可按前缀批量移除。
- `resolve_policy` 按**完整 id** 匹配配置（与内建工具 `read_file` 等 id 同一命名空间，无冲突）。
- 清单 `[tools].definitions[].id` 声明的是 `<tool_id>`（不含 `plugin.<plugin_id>.` 前缀），宿主加载时拼成完整 id。

---

## 6. 插件清单

### 6.1 `plugin.toml` 格式

```toml
id = "git"
name = "Git 集成"
description = "Git diff/commit/log 工具与命令"
version = "0.1.0"
schema_version = 1
authors = ["XGent Team"]
repository = "https://github.com/user/xgent-plugin-git"

[lib]
kind = "rust"           # 编译为 WASM 的语言（MVP 仅 rust）

# 声明本插件提供的扩展点
[tools]
definitions = [
    { id = "git_diff", tier = "read", description = "查看 Git diff" },
    { id = "git_commit", tier = "write", description = "提交更改" },
    { id = "git_log", tier = "read", description = "查看提交历史" },
]

[commands]
definitions = [
    { id = "diff", label = "Git: 查看 Diff" },
    { id = "commit", label = "Git: 提交" },
    { id = "log", label = "Git: 提交历史" },
]

[context_providers]
definitions = [
    { id = "git_history", description = "Git 提交历史上下文" },
]
```

### 6.2 清单结构

```rust
/// 插件清单（从 plugin.toml 解析）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: Arc<str>,
    pub name: String,
    pub version: Arc<str>,
    pub schema_version: SchemaVersion,
    pub description: Option<String>,
    pub repository: Option<String>,
    pub authors: Vec<String>,
    pub lib: LibManifestEntry,
    pub tools: Vec<ToolManifestEntry>,
    pub commands: Vec<CommandManifestEntry>,
    pub context_providers: Vec<ContextProviderManifestEntry>,
}
```

### 6.3 Schema 版本

与 Zed 一致，用 `SchemaVersion(i32)` 管理清单格式版本。宿主声明支持的版本范围，不兼容的清单拒绝加载。

**当前版本**：`schema_version = 1`（v1，本设计文档定义的初始格式）。宿主启动时声明 `SUPPORTED_SCHEMA_RANGE = 1..=1`，未来清单格式演进（新增字段、调整结构）时递增版本号并扩展上界。

```rust
/// 清单 schema 版本。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaVersion(pub i32);

impl SchemaVersion {
    /// 当前宿主支持的版本范围。
    pub const SUPPORTED: std::ops::RangeInclusive<i32> = 1..=1;
}
```

---

## 7. 插件宿主与注册表

### 7.1 crate 划分

| crate | 职责 | 依赖 |
|:---|:---|:---|
| `xgent_plugin_api` | 插件作者面向的 trait + WIT 绑定 + `register_plugin!` 宏 | 无（纯 WASM target） |
| `xgent_plugin` | `PluginHost`（加载/卸载/索引）+ `WasmHost` + `PluginHostProxy` + 清单解析 | `xgent_core`, wasmtime, wasmtime-wasi |
| `xgent_plugin_host` | ECS 桥接 + 各扩展点适配器（PluginTool/PluginCommand/...） | `xgent_plugin`, `xgent_tools`, `xgent_agent`, `xui` |

### 7.2 PluginHostProxy（反转依赖枢纽）

与 Zed 的 `ExtensionHostProxy` 完全同构：

```rust
/// 插件宿主代理：各子系统注册 proxy impl，插件经 proxy 回调宿主。
#[derive(Default)]
pub struct PluginHostProxy {
    tool_proxy: RwLock<Option<Arc<dyn PluginToolProxy>>>,
    command_proxy: RwLock<Option<Arc<dyn PluginCommandProxy>>>,
    context_proxy: RwLock<Option<Arc<dyn PluginContextProxy>>>,
}

pub trait PluginToolProxy: Send + Sync {
    /// 注册插件工具到 ToolExecutor。
    fn register_tools(&self, plugin: Arc<dyn PluginInstance>, tools: Vec<ToolDef>);
    /// 卸载插件工具。
    fn unregister_tools(&self, plugin_id: &str);
}

pub trait PluginCommandProxy: Send + Sync {
    fn register_commands(&self, plugin: Arc<dyn PluginInstance>, commands: Vec<CommandDef>);
    fn unregister_commands(&self, plugin_id: &str);
}
```

**注册时机**：`xgent_app` 启动时，各子系统将自己的 proxy impl 注册到 `PluginHostProxy`。插件加载时，`PluginHost.extensions_updated` 据清单调用各 proxy 的 `register_*`。

### 7.3 ExtensionStore（对标 Zed 的 ExtensionStore）

```rust
/// 插件存储：管理安装/卸载/索引/重载。
pub struct PluginHost {
    proxy: Arc<PluginHostProxy>,
    wasm_host: Arc<WasmHost>,
    plugin_index: PluginIndex,
    installed_dir: PathBuf,      // ~/.xgent/agent/plugins/installed/
    index_path: PathBuf,          // ~/.xgent/agent/plugins/index.json
    wasm_extensions: Vec<(Arc<PluginManifest>, WasmPlugin)>,
    reload_tx: UnboundedSender<Option<Arc<str>>>,
    outstanding_operations: BTreeMap<Arc<str>, PluginOperation>,
}
```

核心方法（与 Zed 对齐）：
- `reload()` — debounce 后重建索引 + diff 加载/卸载
- `install_extension(url)` — 下载 tar.gz + 解压 + reload
- `uninstall_extension(id)` — 删除目录 + reload
- `install_dev_extension(path)` — symlink + 编译 + reload
- `extensions_updated(new_index)` — diff old/new，调 proxy 注册/注销

### 7.4 插件索引

```rust
#[derive(Default, Serialize, Deserialize)]
pub struct PluginIndex {
    pub plugins: BTreeMap<Arc<str>, PluginIndexEntry>,
}

pub struct PluginIndexEntry {
    pub manifest: Arc<PluginManifest>,
    pub dev: bool,
    pub enabled: bool,
}
```

索引文件 `~/.xgent/agent/plugins/index.json` 缓存已安装插件清单，启动时同步加载，按需异步重建（与 Zed 一致）。

---

## 8. 插件生命周期

### 8.1 安装

```
用户指定 tar.gz URL 或本地目录
  → PluginHost.install_extension(id, url)
  → 下载 + 解压到 installed_dir/<id>/
  → reload(Some(id))
  → debounce 200ms
  → rebuild_extension_index()
  → extensions_updated(new_index)
  → diff: 新增 → 加载 WASM → 调 init_extension → 调 proxy.register_*
  → 写 index.json
```

### 8.2 卸载

```
PluginHost.uninstall_extension(id)
  → 删除 installed_dir/<id>/
  → reload(None)
  → diff: 移除 → 调 proxy.unregister_*(id)
  → 丢弃 WasmPlugin 实例（Store drop）
```

### 8.3 启用/禁用

不卸载 WASM，仅从注册表移除/重新注册能力：

```
PluginHost.disable_extension(id)
  → proxy.unregister_*(id)     // 从 ToolExecutor/CommandRegistry 移除
  → 保留 WasmPlugin 实例与索引条目（enabled=false）
```

### 8.4 升级

```
PluginHost.upgrade_extension(id, version)
  → 下载新版本 tar.gz + 覆盖解压
  → reload(Some(id))
  → diff: 修改 → unregister 旧能力 + 加载新 WASM + register 新能力
```

### 8.5 文件监听自动重载

与 Zed 一致，notify 监听 `installed_dir`，变化时 debounce 200ms 后 reload。dev 模式下监听 symlink 目标。

---

## 9. 沙箱与安全

### 9.1 WASM 沙箱

- **文件系统隔离**：WASI preopen 仅限插件工作目录 `~/.xgent/agent/plugins/work/<id>/`。插件无法访问项目文件系统（除非经 `host.read_file` / `host.write_file` 授权 API）。
- **无网络直接访问**：WASM 默认无网络能力。需网络的插件（如下载 LSP binary）经 `host.http_get` 代理。
- **无进程创建**：WASM 无法 spawn 子进程。需运行命令的插件经 `host.run_command`（受安全模型约束，走确认流程）。
- **内存隔离**：各插件独立 `Store`，内存不共享。

### 9.2 宿主 API 权限

插件清单可声明所需权限（`permissions` 字段），加载时校验：

```toml
[permissions]
fs-read = ["**"]           # 可读项目文件
fs-write = ["src/**"]      # 可写 src 目录
network = ["api.github.com"]  # 可访问的域名
command = ["git", "rg"]    # 可运行的命令
```

宿主 API 调用时校验权限，拒绝越权操作。

### 9.3 工具安全模型对接

插件注册的工具经 `ToolExecutor.execute` 时，`resolve_policy` 正常工作：
- 插件工具 id 格式 `plugin.<plugin_id>.<tool_id>`（见 §5.5）。
- 配置可按完整 id 设定 policy（`Approved` / `NeedsConfirmation` / `Denied`）。
- 默认 `NeedsConfirmation`（含只读 tier，与内置工具默认一致）。

**tier 字符串 → `ToolTier` 映射**（清单 `[tools].definitions[].tier` 声明）：

| 清单 tier 字符串 | `ToolTier` | `Concurrency`（清单未声明时的默认） | 说明 |
|:---|:---|:---|:---|
| `"read"` | `Read` | `Shared` | 只读（如 git_diff / git_log） |
| `"write"` | `Write` | `Exclusive` | 写入（如 git_commit） |
| `"exec"` | `Exec` | `Exclusive` | 执行命令（如 run_command 类） |
| `"ui_only"` | — | — | **不支持**，加载拒绝（见 §5.3.2） |

清单可用 `[tools].definitions[].concurrency` 覆盖默认并发声明（如只读工具强制 `Exclusive`）。

---

## 10. 配置与设置

### 10.1 插件配置目录

```
~/.xgent/agent/
├── config.toml          # 全局配置（含 [plugins] 段，见 §10.2；非独立文件）
└── plugins/
    ├── installed/       # 已安装插件目录
    │   ├── git/         # 每插件一目录
    │   │   ├── plugin.toml
    │   │   ├── extension.wasm
    │   │   └── ...（资源文件）
    │   └── markdown/
    ├── work/            # 插件工作目录（WASI preopen 沙箱）
    │   ├── git/
    │   └── markdown/
    └── index.json       # 插件索引（缓存清单）
```

> **注意**：插件全局配置（启用/禁用、自动更新等）并入 `<agent_dir>/config.toml` 的 `[plugins]` 段（见 §10.2），**不是独立文件**。`<agent_dir>` 即 `xgent_settings_core::paths::agent_dir()`（`~/.xgent/agent/`，可经 `XGENT_AGENT_DIR` 覆盖）。

路径函数放 `xgent_settings_core::paths`（新增，对齐现有 `agent_dir()`/`sessions_dir()` 模式）：
- `plugins_dir()` → `agent_dir().join("plugins")`
- `plugin_installed_dir(id)` → `plugins_dir().join("installed").join(id)`
- `plugin_work_dir(id)` → `plugins_dir().join("work").join(id)`
- `plugin_index_path()` → `plugins_dir().join("index.json")`

### 10.2 插件设置

在 `GlobalConfig` 中新增插件配置段：

```rust
/// 插件配置（全局配置的一部分）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginConfig {
    /// 已安装插件 id → 是否启用
    pub enabled: BTreeMap<String, bool>,
    /// 是否自动更新插件
    pub auto_update: bool,
    /// 自动安装的内建插件列表
    pub auto_install: Vec<String>,
}
```

### 10.3 项目级插件配置

项目配置 `<project>/.xgent/config.toml` 可覆盖全局插件配置：
- 禁用特定插件（如项目不需要 Git 集成）
- 覆盖插件工具策略（`tool_policy` 按 `plugin.id.tool_id` 设定）

---

## 11. 内建插件迁移计划

### 11.1 迁移原则

- **渐进式**：先建插件系统骨架，再把现有内建能力逐步迁移为插件。
- **内建插件仍随宿主编译**：MVP 阶段内建插件编译为 WASM 后随二进制发布（预装到 `installed/`），不依赖运行时下载。
- **现有 crate 不删除**：`xgent_tools` 的 4 个内建工具（ReadFile/WriteFile/SearchFiles/RunCommand）保持为"核心工具"，不迁移为插件（它们是 agent 的基础能力，且无外部依赖）。

### 11.2 迁移目标

| 功能 | 当前状态 | 迁移为插件 | 说明 |
|:---|:---|:---|:---|
| F-10 Git 集成 | 未实现 | ✅ `xgent_plugin_git` | 作为首个插件验证系统 |
| Markdown 渲染 | 未实现（D 项） | ✅ `xgent_plugin_markdown` | 助手消息 markdown 渲染 |
| F-13 MCP 支持 | trait 占位 | ✅ `xgent_plugin_mcp` | MCP 协议适配为插件工具 |
| F-14 自定义工具 | 未实现 | ✅ 用户编写的插件 | 插件系统即自定义工具的实现方式 |
| F-15 虚拟宠物 | 未实现 | ✅ `xgent_plugin_pet` | 可选插件，可开关 |
| F-12 成本统计 | 未实现 | ✅ `xgent_plugin_cost` | token 统计 UI 面板 |
| 代码搜索增强 | 内置 SearchFiles | 后续可迁移 | MVP 保持内置 |

### 11.3 Git 插件作为参考实现

`xgent_plugin_git` 作为首个插件，验证完整链路：

```
xgent_plugin_git/
├── Cargo.toml              # crate-type = ["cdylib"], dep: xgent_plugin_api
├── plugin.toml             # 清单
├── src/
│   └── lib.rs              # impl Extension, register_plugin!
└── README.md
```

提供的扩展点：
- **工具**：`git_diff`（read tier）、`git_commit`（write tier）、`git_log`（read tier）、`git_status`（read tier）
- **命令**：`git.diff`、`git.commit`、`git.log`、`git.status`
- **ContextProvider**：`git_history`（提交历史上下文）

---

## 12. 新增 crate 与依赖关系

### 12.1 新增 crate

```
crates/
├── xgent_plugin_api/        # 插件 API（对标 zed_extension_api）
├── xgent_plugin/            # 插件宿主核心（对标 extension + extension_host）
└── xgent_plugin_host/       # ECS 桥接 + 扩展点适配器
```

可选的插件 crate（`extensions/` 目录，对标 Zed 的 `extensions/`）：

```
extensions/
├── git/                     # F-10 Git 集成插件
├── markdown/                # Markdown 渲染插件
└── ...
```

### 12.2 依赖关系（更新后）

```
xgent_core ←──────── 一切共享类型的基础
     ↑
xui_i18n ← xui, xgent_settings
xgent_settings_core ← xgent_daemon, xgent_provider, xgent_settings, xgent_plugin
xgent_provider ← xgent_daemon, xgent_agent
xgent_tools ← xgent_agent, xgent_plugin_host
xgent_context ← xgent_agent
xgent_settings ← xgent_daemon, xgent_agent, xgent_ui
     ↑
xgent_agent ← xgent_ui, xgent_plugin_host
xgent_plugin ← xgent_plugin_host, xgent_daemon   （宿主核心，无 Bevy；UI 与 daemon 都可复用）
xgent_plugin_api ← (插件 crate)                   （纯 WASM target，无宿主依赖）
     ↑
xui ← xgent_ui, xgent_plugin_host                 （plugin_host 用 CommandRegistry）
     ↑
xgent_plugin_host ← xgent_plugin, xgent_tools, xgent_agent, xui   （ECS 桥接 + 各 proxy impl）
     ↑
xgent_app → 组装所有 UI 侧 crate + PluginHost
```

**关键依赖原则**：
- `xgent_plugin` 只依赖 `xgent_core` + wasmtime + wasmtime-wasi，**不依赖任何业务 crate**（Tool/CommandRegistry/Provider/Context）且**不依赖 Bevy**——经 `PluginHostProxy` 反转依赖。这使 **daemon 可直接复用 `xgent_plugin::PluginHost`** 加载 provider 插件（D-P4 演进路径），无需引入 Bevy。
- `xgent_plugin_host` 依赖 `xgent_plugin` + `xgent_tools`（impl `Tool` for `PluginTool`）+ `xui`（注入 `CommandRegistry`）+ `xgent_agent`（接 `AgentBridge`），实现各 proxy trait + 适配器。它是 UI 侧的 Bevy 桥接层，**daemon 不依赖它**。
- `xgent_plugin_api` 是独立 crate，编译目标 `wasm32-wasip2`，不依赖宿主任何 crate。
- `xgent_app` 负责组装：创建 `PluginHost` → 注册各 proxy impl → 添加 `PluginHostPlugin`（Bevy Plugin）。

**daemon 复用路径（D-P4）**：`xgent_daemon` 直接依赖 `xgent_plugin`（纯逻辑，无 Bevy），在 daemon 侧实例化 `PluginHost` 加载 provider 类插件。`PluginHostProxy` 在 daemon 侧注册 daemon 自己的 proxy impl（如 `ProviderPoolProxy`）。UI 侧的 `xgent_plugin_host` 与 daemon 侧的 plugin 加载**互不依赖**，各自注册所需 proxy。MVP 不实现 daemon 侧加载（D-P4 待决策），但 `xgent_plugin` 的无 Bevy 设计已为此留口。

### 12.3 workspace Cargo.toml 新增依赖

```toml
[workspace.dependencies]
wasmtime = { version = "27", features = ["component-model", "async"] }
wasmtime-wasi = "27"
wit-bindgen = "0.40"
wit-component = "0.40"
```

---

## 13. 分步实现计划

### Step P1: xgent_plugin_api — 插件 API crate

**职责**：定义插件作者面向的 `Extension` trait + WIT 绑定 + `register_plugin!` 宏。

**关键内容**：
- `wit/plugin.wit` — WIT 接口定义（host/tool/command/context_provider）
- `Extension` trait — 插件实现（new/register_tools/register_commands/...）
- `register_plugin!` 宏 — 生成 `init-extension` 导出函数
- WIT 绑定经 `wit-bindgen::generate!` 生成

**验证**：编译为 `wasm32-wasip2` 目标通过。

### Step P2: xgent_plugin — 插件宿主核心

**职责**：`PluginHost` + `WasmHost` + `PluginHostProxy` + 清单解析 + 索引管理。

**关键内容**：
- `PluginManifest` — plugin.toml 解析
- `PluginHost` — 加载/卸载/索引/重载（对标 Zed ExtensionStore）
- `WasmHost` — wasmtime 引擎 + Store 管理 + WASI ctx
- `WasmPlugin` — 加载后的插件实例（对标 Zed WasmExtension）
- `PluginHostProxy` — 反转依赖枢纽（tool/command/context proxy trait）
- 文件监听 + debounce 重载

**验证**：单元测试加载一个测试 WASM 插件，验证 init/register 流程。

### Step P3: xgent_plugin_host — ECS 桥接与适配器

**职责**：把插件能力桥接进现有 ECS 体系。

**关键内容**：
- `PluginHostPlugin`（Bevy Plugin）— 注册 `PluginHost` Resource + `PluginPollSystem`
- `PluginTool` — 适配为 `Tool` trait，注入 `ToolExecutor`
- `PluginCommand` — 注册到 `CommandRegistry`
- `PluginContextProvider` — 适配为 `ContextProvider` trait
- 各 proxy trait impl — 注册到 `PluginHostProxy`
- `PluginPollSystem` — 每帧 drain `mpsc::Receiver<PluginLifecycleEvent>`（插件加载/卸载/重载通知），发 ECS Message（见 §4.3 两类回流）

**验证**：集成测试——加载测试插件，验证工具注册到 ToolExecutor、命令注册到 CommandRegistry。

### Step P4: xgent_app 组装 + 配置

**职责**：在 `xgent_app` 中组装插件系统。

**关键内容**：
- `main.rs` 创建 `PluginHost`，注册各 proxy impl
- 添加 `PluginHostPlugin` 到 App
- 插件配置（`PluginConfig`）接入 `GlobalConfig`
- 插件管理 UI 面板（设置面板新增"插件"页：安装/卸载/启用/禁用）
- 命令面板新增 `plugin.install` / `plugin.uninstall` / `plugin.reload` 命令

**验证**：启动应用，安装一个测试插件，验证工具与命令可用。

### Step P5: xgent_plugin_git — 参考实现

**职责**：Git 集成插件，验证完整链路 + 实现 F-10。

**关键内容**：
- `plugin.toml` — 声明 git_diff/git_commit/git_log/git_status 工具 + 命令
- `Extension` impl — 工具执行逻辑（调 `host.run_command("git", ...)`）
- 经 `host.read_file` 读取项目文件
- 经 `host.get_config` 读取插件配置

**验证**：安装 Git 插件后，agent 可调用 git_diff 工具，命令面板可触发 Git 命令。

### Step P6: 内建插件预装机制

**职责**：内建插件（git/markdown/...）随宿主二进制发布，首次启动时预装。

**关键内容**：
- 编译脚本：`extensions/*/` 编译为 WASM，打包到 `xgent_app` 的 `assets/plugins/`（随二进制 `include_bytes!` 或 `assets/` 目录）。
- 首次启动：`PluginHost` 检测 `installed/` 为空（或 `index.json` 不存在），从 `assets/plugins/` 把内建插件复制到 `installed/<id>/`，**然后走与正常安装相同的 `reload()` 路径**（debounce → rebuild_index → extensions_updated → 加载 WASM → register）——不另开预装专用路径，复用 §8.1 生命周期。
- `PluginConfig.auto_install` 控制哪些内建插件自动安装（默认全部内建插件自动安装；用户可禁用）。

**验证**：首次启动应用，Git 插件自动可用。

---

## 14. 待决策点

### D-P1: WASM 编译目标

**选项**：
- A: `wasm32-wasip2`（原生 component model，无需 adapter）—— 推荐
- B: `wasm32-wasip1` + wit-component adapter（与 Zed 一致，兼容性更好但需 adapter）

**倾向**：A。Zed 用 wasip1 + adapter 是历史原因（wasip2 不可用时）。现在 wasip2 已稳定，直接用更简洁。

### D-P2: 插件 API 版本管理

**问题**：WIT 接口演进时如何管理兼容性？

**方案**（参考 Zed）：
- WIT 接口按版本分目录：`wit/since_v0_1_0/`、`wit/since_v0_2_0/`、...
- 插件 WASM 自定义段 `xgent:api-version` 记录编译时的 API 版本
- 宿主声明支持的版本范围，按版本选择 WIT 绑定
- `is_version_compatible()` 校验

### D-P3: 插件 UI 面板扩展点

**问题**：插件如何提供自定义 UI 面板？

**MVP 不实现**。P2 阶段设计：
- 插件经 WIT 声明面板元数据（id/title/icon/默认位置）
- 宿主在布局系统中预留面板槽位
- 插件经 WIT `ui.render` 回调提供渲染指令（声明式 UI 描述，非直接操作 Bevy UI）
- 或：插件只提供数据，宿主用模板渲染（更安全但灵活性低）

### D-P4: Provider 插件接入 daemon

**问题**：插件注册的 LlmProvider 适配器如何接入 daemon 的 ProviderPool？

**MVP 不实现**。后续设计：
- 插件 provider 在 UI 侧实例化，经 IPC 代理到 daemon
- 或：daemon 侧也加载插件 WASM（daemon 纯 tokio，不依赖 Bevy，wasmtime 可用）
- 倾向后者：daemon 侧 `PluginHost`（无 Bevy 依赖），provider 插件在 daemon 加载

### D-P5: 插件市场

**问题**：是否实现插件市场（远程仓库）？

**MVP 不实现**。MVP 仅支持：
- 本地目录安装（dev 模式）
- tar.gz URL 安装（手动指定 URL）

后续可建类似 Zed 的 extensions API 服务端，提供搜索/下载/版本管理。

### D-P6: 插件 i18n

**问题**：插件提供的用户可见字符串（工具 `summarize`、命令 `label`、UI 面板标题）如何走 i18n（NF-05 要求所有用户可见字符串可翻译）？

**MVP 倾向**：插件字符串暂不强制 i18n，允许插件自带中/英文硬编码字符串（插件作者负责）。架构留口：
- 方案 A（推荐）：插件清单声明 `default_locale`（如 `"zh-CN"`），插件自带 `.ftl` 资源（打包进 WASM 或 `installed/<id>/locales/`），宿主经 WIT `host.get-locale` 查询当前语言，插件据当前语言选字符串。宿主不负责翻译插件字符串。
- 方案 B：插件经 WIT `host.tr(key)` 调宿主 `Localizer`，由宿主统一翻译。需插件向宿主注册 `.ftl` 资源，复杂度高。
- 倾向 A：插件自治，宿主不背插件翻译复杂度。MVP 先不实现 WIT `host.get-locale`（插件字符串硬编码），P1 阶段补 WIT 接口 + 方案 A。

---

## 附录 A: Zed 关键源码参考
| 机制 | Zed 源码位置 | XGent 对应 |
|:---|:---|:---|
| Extension trait（API 侧） | `crates/extension_api/src/extension_api.rs` | `xgent_plugin_api::Extension` |
| Extension trait（宿主侧） | `crates/extension/src/extension.rs` | `xgent_plugin::PluginInstance` |
| ExtensionManifest | `crates/extension/src/extension_manifest.rs` | `xgent_plugin::PluginManifest` |
| ExtensionHostProxy | `crates/extension/src/extension_host_proxy.rs` | `xgent_plugin::PluginHostProxy` |
| ExtensionStore | `crates/extension_host/src/extension_host.rs` | `xgent_plugin::PluginHost` |
| WasmHost | `crates/extension_host/src/wasm_host.rs` | `xgent_plugin::WasmHost` |
| ExtensionBuilder | `crates/extension/src/extension_builder.rs` | （MVP 不实现，编译走宿主构建脚本，见 Step P6） |
| register_extension! 宏 | `crates/extension_api/src/extension_api.rs:166` | `xgent_plugin_api::register_plugin!` |
| 插件示例 | `extensions/toml/`、`extensions/slash-commands-example/` | `extensions/git/` |
