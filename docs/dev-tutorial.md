# XGent 开发指南（已实现功能总览）

> 本文档梳理 XGent 截至 2026-07-22 已实现的功能、对应的代码与文档、以及开发注意点。
> **维护规则**：后续实现新功能或功能有变化，都需要更新这个文档（见 `AGENTS.md` 第 6 节）。
>
> 代码现状：19 个 crate 已实现（含 `xgent_terminal` / `xgent_spatial`），`cargo check --workspace` 通过，约 22k 行 Rust。
> MVP（step1~step12）+ optimization 方案 O1~O10 + F-11 内置编辑器（P1）+ F-19 内置终端（P1）+ UI v8 M1 分层与模态已落地。

---

## 1. 功能总览

按需求 `docs/design/requirements.md` 的 F/NF 编号对照实现状态：

### 1.1 核心功能（MVP + 已完成的 P1）

| 编号 | 功能 | 状态 | 实现位置 | 关键文档 |
|:---|:---|:---|:---|:---|
| F-01 | 多轮对话 | ✅ | `xgent_agent/src/bridge.rs`（双层 `run_agent_loop`：外层 follow-up、内层 tool-call+steering；`stream_with_retry` 自动重试可重试错误；流式期 steering 即时中断 race abort、停止边界重新轮询防丢失）+ `compaction.rs`（token 估算 + should_compact 触发 + find_cut_point 切点 + LlmCompactor 摘要） | `plans/step9`、`plans/optimization-from-omp.md` O4/O9 |
| F-02 | 流式输出 | ✅ | `xgent_provider/src/openai_compat.rs`（SSE → ChatEvent 细粒度事件）→ `xgent_ui/src/chat_panel.rs` | `plans/step5`、ADR-0006 |
| F-03 | 工具调用 | ✅ | `xgent_tools/src/builtins/`（ReadFile/WriteFile/SearchFiles/RunCommand）+ `executor.rs`；**工具 schema 经 `AgentBridge.tool_schemas` 注入 LLM 请求**（2026-07-20 修复，见诊断文档）；`conv.messages` 记录完整 tool_call/tool_result 配对（带 call_id）；**`search_files` 支持 `regex`（默认关，字面量子串）/`case_insensitive`/`glob`/`file_type`，坏参数在遍历前报 `is_error` 而不是伪装成「未找到匹配」**（2026-10-14，见 §5 注意点 28）；**`write_file`/`edit_file` 写前记检查点，用户可撤销整轮 agent 文件改动**（2026-10-15，见 §5.14）；**同一轮的连续只读工具调用并发执行**（`Tool::concurrency` 从"只声明不消费"变成真判据，见 §5.16） | `plans/step7`、ADR-0007、`conversation-flow-fixes-2026-07-20.md`、`docs/comet/specs/code-search/spec.md`、`docs/comet/specs/file-checkpoint-undo/spec.md`、`docs/comet/specs/tool-concurrency/spec.md` |
| F-04 | 操作确认 | ✅ | `xgent_tools/src/security.rs`（`resolve_policy`）+ `executor.rs`（ConfirmRequest 流程）+ `xgent_ui/src/confirm_dialog.rs` | `plans/step7`、ADR-0007 |
| F-05 | 项目上下文 | ✅ MVP（方案 A OnDemand）；B/C/D/E 仅 trait 占位 | `xgent_context/src/on_demand.rs`；**bridge 异步侧 StartLoop 时调 `context.retrieve` 注入项目目录树 + 相关文件**（2026-07-20 修复）；**中文问句按相邻二字组合切词、rg 走 `--fixed-strings` 字面量、`@file:` 线索直接读文件、目录遍历经 spawn_blocking、Hub 合并后复核预算**（2026-10-14，约束见 §5.10）；`repo_map.rs`/`vector.rs`/`lsp.rs`/`hybrid.rs` 均为 25 行占位 | `plans/step8`、ADR-0010、`conversation-flow-fixes-2026-07-20.md`、`docs/comet/specs/context-retrieval/spec.md` |
| F-06 | 会话管理 | ✅ | `xgent_agent/src/session_store.rs`（JSONL append-only + `list_sessions` + `restore_session`）+ `conversation.rs`（`restore` 方法）+ `xgent_ui/src/session_history.rs`（历史面板 UI）+ `events.rs`（`ListSessionsMessage`/`SessionListMessage`/`RestoreSessionMessage`/`SessionRestoredMessage`）；**新建、列出、恢复** 均完整可用 | `plans/step9`、ADR-0008 |
| F-07 | Provider 切换 | ✅ | `xgent_provider/src/openai_compat.rs`（完整实现）+ `anthropic.rs`（完整实现：Anthropic Messages API 流式 + 工具调用）；`response_api.rs`/`custom.rs` 仅 trait 占位；`xgent_ui/src/settings_panel.rs`（含**模型列表拉取**：`FetchModelsMessage`/`ModelListResultMessage` + ↻ 按钮触发 IPC `provider.listModels`，点击模型项填充输入框）；**设置面板从「provider 四项 + 只写不读」扩到六组配置**（provider 限额 / 工具三态策略 / agent / tools / context / 语言），打开即回填当前值、只写脏键、逐键报告失败原因（2026-10-16，缺口 R3-13，见 §4.5.2） | `plans/step5`、`docs/comet/specs/settings-panel/spec.md` |
| F-08 | 命令面板 | ✅ | `xui/src/command_palette.rs`（通用组件）+ `xgent_ui/src/command_palette.rs`（业务命令注册） | `plans/step10`/`step11`、`design/ui-design.md` §8 |
| F-09 | 快捷键体系 | ✅ | `xui/src/hotkeys.rs` + `xui/src/shortcuts.rs` + `xgent_ui/src/shortcuts.rs` | `design/ui-design.md`、`plans/step10` |
| F-11 | 内置编辑器（P1） | ✅ | `xui/src/text_editor/`（buffer/find/highlight/render/undo/virtual_render）+ `xgent_ui/src/editor/`（buffer/command/conflict/io/state/tabs/at_syntax） | `design/editor-design.md`、ADR-0009/0010 |
| F-19 | 内置终端（P1） | ✅ | `xgent_terminal/`（`TerminalBackend` trait + `LocalPtyBackend`，portable-pty + vte，不依赖 Bevy）+ `xgent_ui/src/terminal/`（mod/io/tabs/input/output：PTY 桥接 + 多 tab + 行编辑 + vte 渲染 + SideView 集成）+ `xgent_app` 注入 `TerminalIoRuntime` | `design/terminal-design.md`、ADR-0011/0012 |
| UI v8 M1 | 分层与模态 | ✅ | `xgent_ui/src/{layout,modal,overlay,picking,kit}.rs`：六静态层根 + `ModalStack` + `require_markers` 拾取白名单 + 抽屉统一裁决 + tooltip 顶层化；`xgent_app` 装配 `UiPickingSettings` | `design/ui-v8-layout-architecture.md`、`docs/comet/specs/ui-m1-layering/spec.md`、§5.17 |
| UI v8 M3 | 滚动契约 / 填满契约 / 省略原语 / 虚拟列表 | ✅ | `xui/src/{scroll_frame,layout,ellipsis,virtual_list}.rs`：`ScrollFrame` 唯一可滚动入口（overflow 运行时回写）+ `region_fill()` 填满契约 + `Ellipsis`/`MidEllipsis` 省略原语（测量走 bevy 排版管线）+ `VirtualList` 不等高支持；`xgent_ui`：9 处假滚动归零、分屏三页改 `region_fill`、对话输入区自适应滚动、6 处省略截断点、文件树与会话历史虚拟化 | `plans/ui-v8-tasks.md` §M3、§5.20 |
| 插件系统 | ✅ | WASM Component + wasmtime 29，3 宿主 crate（`xgent_plugin_api`/`xgent_plugin`/`xgent_plugin_host`）+ 2 插件 crate（`xgent_plugin_hello`/`xgent_plugin_git`）；插件经 WIT 注册 Agent 工具/命令/ContextProvider，动态安装卸载，cancel 穿透 | `design/plugin-system-design.md`、§5.13 |

### 1.2 非功能需求

| 编号 | 需求 | 状态 | 实现位置 |
|:---|:---|:---|:---|
| NF-01 | 跨平台 | ✅ | Unix socket（macOS/Linux）+ named pipe（Windows）见 `xgent_settings_core/src/paths.rs` 与 `xgent_daemon/src/server.rs` |
| NF-02 | 轻量多开 | ✅ | 多进程模型：UI 每项目一个，daemon 全局唯一；`xgent_daemon/src/lifecycle.rs` 随用随启 |
| NF-03 | 性能 | ✅ | 数据驱动 UI；虚拟列表 `xui/src/virtual_list.rs`；流式 channel 非阻塞 |
| NF-04 | 可维护性 | ✅ | ECS Events/Messages 通信；daemon 纯 tokio 可 headless |
| NF-06 | 可扩展 | ✅ | TUI/Web/3D/MCP 均有 trait 预留；**插件系统**（WASM Component）已落地，支持动态注册工具/命令/ContextProvider（见 §5.13） |

### 1.3 未实现（P1/P2 留白）

- **F-10 Git 集成**（P1）：✅ 已实现（`xgent_plugin_git` 插件，git_diff/git_log/git_status 工具 + 命令，经 `host.run_command("git", ...)`）。
- **F-12 成本统计**（P1）：未实现（`TokenUsage` 类型已定义，无汇总 UI）。
- **F-13 MCP 支持**（P1）：仅 trait 预留 `xgent_tools/src/mcp.rs::McpTransport`，无实现。
- **F-14 自定义工具**（P2）：✅ 部分实现（插件系统即自定义工具的实现方式——用户编写 WASM 插件注册工具）。
- **F-15 虚拟宠物**（P1）：未实现（`xgent_pet` crate 未建）。
- **F-16 3D 可视化** / **F-17 TUI** / **F-18 Web**（P2）：未实现，架构留口。
- **插件系统**（架构设计）：✅ 已落地（WASM Component + wasmtime 29）。3 宿主 crate（`xgent_plugin_api`/`xgent_plugin`/`xgent_plugin_host`）+ 2 插件 crate（`xgent_plugin_hello` 测试 / `xgent_plugin_git` 参考）。插件经 WIT 注册 Agent 工具/命令面板命令/ContextProvider，动态安装卸载。详见 `docs/design/plugin-system-design.md` + 本指南 §5.13。
- **Compaction**（optimization O9）：`xgent_agent/src/compaction.rs` 已落地——token 估算（`tokenizer.rs` 启发式）+ `should_compact`（reserve=max(15% window, 16384)）+ `find_cut_point`（保留最近 token 段，user/assistant 边界切）+ `LlmCompactor`（调 provider 生成摘要）+ `apply_compaction`（summary 前置 + kept）。`AgentEvent::Compacted` 通知 UI，`SessionEntry::Compaction` 持久化。

---

## 2. Crate 拓扑与职责

依赖自底向上（无环）。详见 `docs/design/architecture.md` §4。

```
xgent_core          ── 共享类型层（chat/session/editor/proto/fs/...），无 Bevy
xui_i18n            ── StringSource trait，纯无依赖
xgent_settings_core ── 配置纯类型 + TOML 读写 + 平台路径，无 Bevy
xgent_settings      ── Bevy Resource 包装 + fluent Localizer
xgent_provider      ── LlmProvider trait + OpenAiCompat 实现，无 Bevy
xgent_daemon        ── 独立 bin，纯 tokio：provider 池 + 配置协调 + 文件监听 + 多客户端同步
xgent_tools         ── Tool trait + 安全策略 + 执行器 + 4 内置工具，无 Bevy
xgent_context       ── ContextProvider trait + OnDemand 实现，无 Bevy
xgent_terminal      ── PTY 抽象（TerminalBackend trait + LocalPtyBackend，portable-pty）+ ANSI 解析（vte），无 Bevy，UI 侧进程
xgent_spatial       ── 3D 空间图 S0：Cargo.toml 依赖投影 + 确定性/增量布局，无 Bevy（F-16，与 UI 解耦）
xgent_agent         ── agent loop + ECS 桥接 + SessionStore
xui                 ── 通用 Bevy UI 组件库，纯依赖 bevy + xui_i18n，可独立发布
xgent_ui            ── XGent 业务 UI（对话/工具/文件/编辑器/设置面板等；设置面板绑 GlobalConfigRes/ProjectConfigRes 作读模型）
xgent_app           ── UI 进程入口 bin：组装插件 + daemon 拉起 + IPC 客户端
```

### 2.1 各 crate 关键 API

| crate | 关键导出 |
|:---|:---|
| `xgent_core` | `ChatEvent`(12 变体)、`StopReason`、`AgentMessage`/`ContentBlock`/`convert_to_llm`、`ErrorKind`、`SessionEntry`、`EditorState` trait、`FileChanged`、JSON-RPC `Request/Response/Notification`、`methods`/`notifications` 常量 |
| `xui_i18n` | `StringSource` trait |
| `xgent_settings_core` | `GlobalConfig`/`ProviderConfig`(含 `max_retries: Option<u32>`/`retry_mode`/`retry_initial_delay_ms`/`retry_max_delay_ms`/`retry_backoff_factor`，R3-17 新增 `context_window`/`model_context_windows`/`max_output_tokens`/`first_event_timeout_secs` + `resolve_context_window`/`effective_*` 访问器)/`ProviderKind`/`RetryMode`(Fixed/Exponential)/`ProjectConfig`(新增 `AgentConfig`/`ToolConfig`/`ContextLimits` 三节，均带 `effective_*()`)/`ContextStrategy`(OnDemand/RepoMap/Vector/Hybrid)/`ToolPolicyConfig`、`global::defaults`/`project::defaults`（转配置前那批常量的唯一来源）、`GlobalConfigStore`/`ProjectConfigStore`、`paths` |
| `xgent_settings` | `Localizer`（impl StringSource）、`GlobalConfigRes`/`ProjectConfigRes`、`XgentSettingsPlugin` |
| `xgent_provider` | `LlmProvider` trait、`OpenAiCompatProvider`（完整）、`ResponseApiProvider`/`AnthropicProvider`/`CustomApiProvider`（占位）、`build_provider(id, cfg)`、`ChatStream`；R3-17 后适配器经 `with_first_event_timeout_secs` / `with_max_output_tokens` 承接 `ProviderConfig`（`build_provider` 与 `CustomApiProvider::with_config` 负责取值），缺省仍等于原 `FIRST_EVENT_TIMEOUT_SECS`/`8192` 常量 |
| `xgent_daemon` | `Server`（JSON-RPC over Unix socket/named pipe）、`ProviderPool`（`chat` 返回 `(ErrorKind, String)`，ErrorKind 经 `ProviderError::to_error_kind` 映射，透传给 UI 避免误判 Network 重试）、`FsWatcher`、`ConfigStore`、`registry`（多客户端订阅广播）、`lifecycle` |
| `xgent_tools` | `Tool` trait（tier/approval_for/concurrency/execute(signal,on_update)）、`ToolTier`(Read/Write/Exec/UiOnly)、`Concurrency`(Shared/Exclusive)、`ToolError`、`ToolCtx.tool_policy` 为 `Arc<RwLock<ToolPolicyConfig>>`（改完下一次调用生效）、`ToolExecutor`（`execute` 的 `NeedsConfirmation` 路径 `rx.await` 监听 `signal.cancelled()`，Abort 可中断确认等待；确认时长来自共享句柄 `with_shared_params(Arc<RwLock<ToolParams>>)`，`with_params(&ToolParams)` 取快照）；`policy_targets()` 列出已注册工具的 id+tier、`resolved_policy(id, policy)` 给设置面板显示解析后的三态、`ToolExecutorResource`（Arc 共享包装）、`ConfirmCallback`、`resolve_policy`、4 内置工具（`search_files` 支持 `regex`/`case_insensitive`/`glob`/`file_type`，非法参数在遍历前报 `is_error`，见 §5 注意点 28）、`ToolParams`（`[tools]` 派生的命令超时/输出上限/确认超时）、`default_tools(&ToolParams)`、`truncate_output`/`truncate_output_to`、`glob`（crate 内 glob→正则）、`EditorTool`（UiOnly）、`McpTransport`（占位） |
| `xgent_terminal` | `TerminalBackend` trait（spawn/write/resize/kill/subscribe）、`LocalPtyBackend`（portable-pty）、`TerminalEvent`(Output/Exited)、`SpawnRequest`、`ShellSpec`(Powershell/FromEnv)、`TerminalId`、`TerminalError` |
| `xgent_spatial` | `parse_manifest`/`ManifestInfo`、`scan(root)`（crate 图 + `DependsOn` 边 + cluster）、`layout`/`layout_incremental`、`LayoutConfig`/`LayoutReport`；类型在 `xgent_core::spatial`（`SpatialGraph`/`SpatialNode`/`SpatialEdge`/`SpatialCluster`） |
| `xgent_context` | `ContextProvider` trait、`OnDemandContextProvider`（完整：CJK bigram 分词 + rg 字面量检索 + `@file:` 线索 + spawn_blocking 遍历，见 §5.10；五个遍历/检索限额改为 `RetrievalLimits` 字段，`with_limits` 注入快照、`with_shared_limits(Arc<RwLock<RetrievalLimits>>)` 供热应用，缺省等于原常量；每次检索开头取一份限额快照）、`RepoMap`/`Vector`/`Lsp`/`Hybrid`（占位）、`build_context_provider(strategy, root, limits)`/`build_context_provider_shared(strategy, root, Arc<RwLock<limits>>)`、`ContextHub`（Bevy Resource，聚合内置+插件 provider，合并后按预算复核，impl ContextProvider）、`XgentContextPlugin` |
| `xgent_agent` | `XgentAgentPlugin`、`AgentBridge`/`AgentCommand`(StartLoop/Abort/ConfirmDecision/Steering/FollowUp)/`AgentEvent`(含 `RetryAttempt`/`Compacted`)、`AgentBridgeConfig`(含 `compaction`/`context_window`/`compaction_settings`/`retrieval_max_tokens`/`max_events_per_frame`)、`RetryConfig`/`stream_with_retry`、`run_agent_loop`、`StreamOutcome`(tool_calls/usage/stop_reason/pending_steering)、`maybe_compact`、`Conversation`(含 `pending_tool_calls` 批次 + `flush_pending_tool_calls`)/`ConversationStatus`/`persist_compaction`、`SessionStore`、`CompactionProvider` trait + `LlmCompactor` 实现 + `CompactionSettings`/`should_compact`/`find_cut_point`/`apply_compaction`/`compaction_context_tokens`、`tokenizer`(estimate_message_tokens/estimate_messages_tokens)、`build_request`、events.rs（含插件 Message: `CommandResultMessage`/`PluginUnregisterMessage`/`PluginCommandTriggered`） |
| `xui` | `TextEditor`/`Rope`/`HighlightCache`、**`ScrollFrame`（唯一可滚动入口：`vertical()`/`horizontal()`/`stick_bottom()` + `ScrollFrameMarker` + `scroll_to_child`）**、`StickToBottom`、`Scrollbar`、`region_fill()`/`fill_region()`（唯一填满契约）、**`Ellipsis`/`EllipsisMode`(End/Mid)/`EllipsisCache`/`compute_ellipsis`（单行省略原语）**、`CommandPalette`/`CommandRegistry`（含 `remove_by_prefix`/`try_register`）、`HotkeyRegistry`、`ChatInput`、`ShortcutsPlugin`、**`VirtualList`（含 `VirtualListItem`/不等高行高表 `build_measured_offsets`）**、`i18n_bridge`（`tr`/`tr_with`/`Strings`） |
| `xgent_ui` | `XgentUiPlugin`、`chat_panel`/`file_panel`/`top_bar`/`status_bar`/`tool_panel`/`command_palette`/`confirm_dialog`/`settings_panel`/`shortcuts`/`theme`/`layout`/`i18n`、`editor/`（buffer/command/conflict/io/state/tabs/at_syntax）、`terminal/`（mod/io/tabs/input/output：PTY 桥接 + 多 tab + 行编辑 + vte 渲染 + SideView 集成）、`handle_palette_triggers` 路由 `plugin.` 前缀到 `PluginCommandTriggered` |
| `xgent_plugin_api` | 插件作者面向的 API：`Extension` trait（new/register_tools/register_commands/register_context_providers/execute/run_command/retrieve/on_file_changed）、`register_plugin!` 宏（生成 `init-extension` 导出）、WIT 绑定（`wit/plugin.wit`：host/tool/command/context-provider 四 interface + world plugin）、`host` 模块（read_file/write_file/log/get_config/run_command/http_get import）。编译为 `wasm32-wasip2`。 |
| `xgent_plugin` | 插件宿主核心：`PluginHost`（加载/卸载/索引/reload/load_builtin_plugins）、`WasmHost`（wasmtime Engine+Linker+Store，host import impl：read_file/write_file/run_command 带 cancel 穿透）、`WasmPlugin`（专有 tokio Task 串行调用，cancel via tokio::select! 丢弃 oneshot）、`PluginHostProxy`（反转依赖枢纽，三 proxy trait）、`PluginManifest`/`PluginIndex`、`WasmCallError`(Aborted/Failed)。cancel 用 Zed 模式（非 cancel_handle，偏差修正 3）。 |
| `xgent_plugin_host` | ECS 桥接 + 适配器：`PluginTool`（→ Tool trait，完整 id `plugin.<id>.<short>`，schema.name 一致性硬约束）、`PluginCommand`（→ CommandRegistry + PluginCommandRegistry）、`PluginContextProvider`（→ ContextHub）、`PluginHostPlugin`（Bevy Plugin，注册 Message + `plugin_poll_system`）、`plugin_poll_system`（drain PluginEvent + 执行 PluginOp + 处理 PluginCommandTriggered）、`register_proxy_impls`（创建 channel + 三 proxy impl）。 |
| `xgent_plugin_hello` | 测试插件：hello 工具（read tier），验证最小链路。cdylib → wasm32-wasip2。 |
| `xgent_plugin_git` | Git 集成参考插件：git_diff/git_log/git_status 工具 + diff/log/status 命令，经 `host::run_command("git", ...)` 执行。cdylib → wasm32-wasip2，打包到 `xgent_app/assets/plugins/git/`。 |
| `xgent_app` | `Args`（命令行）、组装 `XuiPlugin` + `XgentSettingsPlugin` + `XgentAgentPlugin` + `XgentContextPlugin` + `PluginHostPlugin` + `XgentUiPlugin` + `ConfigBridgePlugin` + `SettingsIoPlugin`（配置快照读取 / 逐键保存 / 热应用句柄 / 对端变更同步）+ `FsEventBridgePlugin`、`connect_or_spawn_daemon`、`IpcProviderClient`、`ToolExecutorResource`（Arc 共享 ToolExecutor）、`PluginHostResource`/`PluginEventRx`、启动时 `load_builtin_plugins` spawn 到 tokio runtime；**配置注入点**：`resolve_context_window`（窗口三级次序 + 回落 warn）+ 由 `ProjectConfig` 派生 `ToolParams`/`RetrievalLimits`/`RunSettings`（见 §4.5.1、§4.5.2）；`LiveSettings` 持四组共享句柄，`apply_snapshot` 是唯一一条把权威配置落进运行时的通路（本地写入与对端广播共用）。 |

---

## 3. 已落地的关键设计决策（ADR）

对应 `docs/decisions/` 下 12 条 ADR，全部已定案并落地：

| ADR | 主题 | 落地点 |
|:---|:---|:---|
| 0001 | provider 就绪闸门由 daemon 权威判定 | `xgent_daemon/src/provider_pool.rs` + `xgent_agent/src/provider_state.rs` |
| 0002 | model 作为 provider 级 model_overrides | `xgent_settings_core/src/global.rs::ProviderConfig` |
| 0003 | ErrorKind 错误细分 | `xgent_core/src/chat.rs::ErrorKind`（NotConfigured/AuthFailed/Network/StreamParse/ProviderError） |
| 0004 | model kind 下拉 MVP 隐藏 Custom | `xgent_ui/src/settings_panel.rs` |
| 0005 | ChatMessage 结构化 + AgentMessage 双层 | `xgent_core/src/chat.rs`（ChatMessage.content: Vec<ContentBlock>、AgentMessage 4 变体、convert_to_llm） |
| 0006 | ChatEvent 12 变体 + StopReason clean cutover | `xgent_core/src/chat.rs`（旧 4 变体已删）；`xgent_provider` SSE 发射；daemon 透传 JSON |
| 0007 | Tool trait 重构（tier/approval_for/concurrency/ToolError/signal） | `xgent_tools/src/tool.rs`、`executor.rs`、4 个 builtins、`security.rs::resolve_policy` |
| 0008 | 会话存储 JSONL append-only | `xgent_core/src/session.rs` + `xgent_agent/src/session_store.rs`（`<agent_dir>/sessions/<session_id>.jsonl`，全局，对齐 pi 布局） |
| 0009 | 编辑器保存绕过 WriteFile + UiOnly tier | `xgent_ui/src/editor/io.rs`（Cmd+S 直接 fs::write）+ `xgent_tools/src/editor_tool.rs`（ToolTier::UiOnly） |
| 0010 | OQ-08 检索升级路径分段（编辑器→C，D 延后到 LSP） | `xgent_context` 仅 OnDemand 实现，其余 trait 占位 |
| 0011 | 用户终端 PTY 选 portable-pty | `xgent_terminal`（`LocalPtyBackend`，portable-pty + spawn_blocking 桥接 ECS）+ `xgent_ui::terminal::io`（crossbeam 桥 tokio→ECS）；Win powershell/Unix $SHELL；保持 shell cooked 模式（portable-pty 无跨平台 raw API），UI 侧**透传模式**——按键直接转原始字节发 PTY，shell 回显为唯一显示源（避免双显），获 shell 原生 readline 历史/补全。读循环直接 `tokio::mpsc::Sender::blocking_send` 转发输出（不经 std mpsc 中转 + async 桥接 task，避免阻塞 std `recv` 冻结 runtime）；读循环检测 DSR 光标查询 `\x1b[6n` 并回复 `\x1b[1;1R`（PowerShell/PSReadLine 启动时探测终端，不回复则卡死等待，输入无响应）；writer 经 `Arc<Mutex>` 共享给读循环（DSR 回写）与命令循环（用户输入）。`LocalPtyBackend` impl `Drop`：drop 时 `take` 所有 session 的 killer slot 调 `kill()`，防止 backend 被 drop 时子进程变孤儿 + 读循环/命令循环线程泄漏（killer 经 `Arc<Mutex<Option<...>>>` 共享给命令循环 `Kill` 路径与 `Drop`，`take` 保证幂等）。vte `execute` 的 BS（`\x08`）处理：`pending_text` 为空时退到 `current` 末 span 的末尾字符（`pop` + 空 span 移除），而非弹出整个 span——修复跨样式边界 BS 丢多字符（如 `ab\x1b[31m\x08c` 应得 `ac` 而非 `c`）。vte `execute` 的 CR（`\r`）用延迟清除（`cr_pending` 标志）：`\r` 设标志不立即清空，下一个 `\n` 时保留内容入 finished（CRLF 换行），下一个普通字符/CSI/Tab 时才清空旧行（行重绘覆盖）——修复 `\r\n` 产生空行的 bug（旧行为 `\r` 直接清空 current，`\n` 推空行）。vte `apply_sgr` 支持 ITU-T T.416 冒号分隔扩展色（`38:5:n` / `38:2:r:g:b`）——vte 将冒号分隔子参数归入同一 sub-param slice，旧代码仅按分号分隔 `iter.next()` 取值导致冒号格式颜色静默丢弃。`xgent_ui::terminal::io::handle_pty_events` 的 `Spawned` 分支：tab 实体已销毁（用户在 PTY spawn 完成前关闭 tab）时 kill 孤儿 PTY 会话，防止 backend sessions map 残留 + 进程泄漏。 |
| 0012 | 终端独立 crate + TerminalBackend trait | `xgent_terminal`（`TerminalBackend` trait + `LocalPtyBackend`）+ `xgent_ui::terminal`（UI 层只依赖 trait，经 `TerminalIoRuntime` 注入实现）；对齐 xgent_tools/xgent_context 纯逻辑层模式 |
| 0014 | v7 视觉基准（Linear 色板 + 字号阶梯 + 密度） | `xgent_ui::theme`（`theme::color`/`type_scale`/`space`/`radius`/`size`/`z`）、`xgent_ui::fonts`（`ui_text`/`mono_text`/`ellipsis_text`）、`xgent_ui::density` |
| 0015 | v8 交互架构层重建（保留 Linear 视觉基准，重建交互层） | M1：`xgent_ui::layout`（六层根）/`modal`/`overlay`/`picking`；M2：`theme::ThemeHandle` + `density`；M3：`xui::scroll_frame`（唯一滚动入口）/`xui::layout::region_fill`（唯一填满契约）/`xui::ellipsis`（单行省略原语）/`xui::virtual_list`（含不等高支持）；设计见 `docs/design/ui-v8-layout-architecture.md`，进度见 `docs/plans/ui-v8-tasks.md` |

---

## 4. 进程模型与数据流

### 4.1 双进程

- **xgent-ui**（每项目/窗口一个）：Bevy App，承担 UI 渲染、agent loop、工具执行（MVP）、上下文构建。
- **xgent-daemon**（全局唯一）：纯 tokio，承担 provider 连接池、全局配置协调、文件监听、多客户端文件状态同步。`lifecycle.rs` 随用随启（首个 UI 拉起，末个退出后延迟退出）。
- **IPC**：JSON-RPC 2.0 over Unix socket（macOS/Linux）/ named pipe（Windows）。方法见 `xgent_core/src/methods.rs`，通知见 `notifications.rs`。

### 4.2 流式对话数据流（F-01/F-02）

```
用户输入
 → xgent_ui::chat_panel 发 UserInputMessage
 → xgent_agent::agent_loop 构造 ChatRequest（build_request：convert_to_llm + 注入 system + tool_schemas）
 → AgentBridge.cmd_tx 发 StartLoop { req }
 → agent_loop_task（tokio task）接到 StartLoop：调 context.retrieve 用最近 user 消息检索，
   refresh_system_message 覆盖 req 首条 system（注入项目目录树 + 相关文件片段）
 → run_agent_loop（双层循环）调 ProviderClient（IPC → daemon → LLM）
 → SSE chunk → ChatEvent::TextDelta 等 → event_tx → ECS → chat_panel 流式渲染
 → 若有 ToolCall → 先发所有 ToolCall 事件（conv 侧 `push_tool_call` 累积到 `pending_tool_calls` 批次）→ 逐个 ToolExecutor（含确认）→ ToolResult 回灌
   → 首个 ToolResult 触发 `flush_pending_tool_calls`：把累积的 ToolCall 块作为**一条** AssistantMessage 固化（符合 OpenAI 协议「一条 assistant 含多 tool_calls」），再逐个 `push_tool_result` → 继续内层循环
 → 若 usage.prompt 超 should_compact 阈值 → maybe_compact 调 LlmCompactor 摘要 → req.messages 重建为 summary+kept → 发 Compacted 事件
 → Done{usage, model} → finalize_assistant 写入 usage/model → 退出内层；SessionStore append Message entry
 → UI 据 DoneMessage.usage 累加真实 token（prompt + completion）
```

**关键修复（2026-07-20，见 `docs/conversation-flow-fixes-2026-07-20.md`）**：
- tool_schemas 从 AgentBridge.tool_schemas 注入（启动时从 ToolExecutor 一次性提取），
  修复 LLM 无法发起工具调用的致命 bug。
- 上下文检索在 bridge 异步侧 StartLoop 时执行（ECS 系统同步无法 await retrieve），
  修复项目结构从未注入的 bug。
- tool_call/tool_result 都带 call_id，conv.messages 记录完整配对，修复多轮工具调用后
  OpenAI 协议配对断裂的 bug。
- AgentEvent::Done 携带 usage/model，AssistantMessage.usage 不再为 None，UI token 统计
  用真实 prompt+completion 累加。

- **多 tool_calls 批量回灌**（2026-07-31 修复）：一轮多个 tool_calls 时，bridge 先发所有 ToolCall 事件再逐个执行+发 ToolResult；req 侧一次性回灌**一条**含所有 ToolCall 块（+首个文本块）的 assistant ChatMessage + N 条 tool result；conv 侧 `push_tool_call` 累积到 `pending_tool_calls`，首个 `push_tool_result` 触发 `flush_pending_tool_calls` 固化为一条 AssistantMessage。修复之前每个 tool_call 拆成独立 assistant 消息破坏 OpenAI 协议（严格 endpoint 会 400）。
- **steering/重试半截文本 req 侧回灌**（2026-07-31 修复）：steering 中断与重试时，conv 侧 `finalize_assistant` 固化半截 assistant 文本，但 bridge 侧 req 之前漏 push（`stream_with_retry` 用 `&ChatRequest` 不可变），导致 LLM 下一轮看不到自己半截话。修复：bridge steering 中断分支 push 被中断 assistant 文本；`stream_with_retry` 改 `&mut ChatRequest`，`stream_llm_response` Err 携带 `partial_text`（三元组），重试前回灌到 req。
- **tool_panel 卡片 call_id 匹配**（2026-07-31 修复）：`ToolCardMarker` 加 `tool_call_id` 字段，`update_tool_result` 按 `tool_call_id` 精确匹配卡片。修复之前用 `tool_id`（工具名）匹配，多次同名工具调用结果错配到第一个卡片。
- **IPC 路径 ErrorKind 透传**（2026-07-31 修复）：`provider_pool.chat` 建流阶段错误（如 401/缺配置）之前 `map_err(|e| e.to_string())` 扁平化为 String，session 包装为 RPC INTERNAL_ERROR，UI `IpcProviderClient` 误判为 `Network` 触发无意义重试（鉴权失败/未配置不可重试却重试）。修复：`provider_pool.chat` 返回 `(ErrorKind, String)`（用 `ProviderError::to_error_kind`），session 编码进 `RpcError.data`，UI 用 `call` 而非 `call_ok` 解析 `data` 恢复 ErrorKind。
- **IPC subscribe race**（2026-07-31 修复）：`IpcProviderClient.chat` 之前先 `call_ok` 拿 stream_id 后才 `subscribe`，但 daemon 推送 task 在 `call_ok` 返回前已启动，`broadcast` 无历史缓存，subscribe 前的首批事件（TextDelta/ToolCallStart/Done）丢失。修复：`subscribe` 提前到 `call` 之前，消费 task 按 `stream_id` 过滤无关通知。
- **正常完成回灌 assistant 文本到 req**（2026-07-31 review 修复）：`run_agent_loop` 的 `tool_calls.is_empty()` 分支（正常停止，无工具调用）之前漏把 `outcome.partial_text` 回灌到 `req.messages`，仅 tool 执行分支回灌。导致对话内 FollowUp/steering 时 `req.messages` 缺最后一轮 assistant 回复——conv 侧 `finalize_assistant` 已固化，但 req/conv 不同步，LLM 下一轮看不到自己刚说的话。修复：该分支把 `partial_text` 作为 assistant 消息 push 到 req（与 conv 侧对称，与 tool 执行分支一致）。
- **RetryMessage/CompactedMessage UI 消费**（2026-07-31 review 修复）：`events.rs` 定义了 `RetryMessage`（重试进度）与 `CompactedMessage`（压缩提示），agent loop 也正确发射，但 `chat_panel.rs` 之前**完全未订阅**——重试时用户只看到半截文本被固化后空白，不知正在重试；压缩时无任何视觉提示。修复：新增 `show_retry_status`（在当前助手节点显示"⟳ 重试中（第 N 次）…"+上次失败原因）与 `show_compacted_notice`（在消息列表插入 dim 提示气泡"✦ 前序对话已摘要压缩"）两个系统。
- **show_retry_status 系统排序**（2026-07-31 review 第二轮修复）：`RetryAttempt` 事件同帧发 `DoneMessage` + `RetryMessage`。`finalize_on_done`（处理 DoneMessage）把半截回复固化为历史气泡并清空当前节点，`show_retry_status`（处理 RetryMessage）写入"重试中"提示。两者必须在同一 `Update` set 内有序：`show_retry_status` 用 `.after(finalize_on_done)` 单独注册。若顺序不定，`show_retry_status` 可能先执行写入"重试中"，`finalize_on_done` 后执行会把"重试中"固化进历史气泡，半截回复丢失。
- **SteeringInterrupted 清空 pending_tool_calls**（2026-07-31 review 第二轮修复）：`handle_agent_event` 的 `SteeringInterrupted` 分支之前不清 `conv.pending_tool_calls`。虽然 steering 中断时 `stream_llm_response` 未发 `AgentEvent::ToolCall`（tool_call 事件在 `run_agent_loop` 工具执行分支才发），`pending_tool_calls` 理论上为空，但防御性清空避免未来逻辑变化导致中断轮的孤儿 tool_call 残留到新一轮 `flush_pending_tool_calls`，破坏 OpenAI tool_call/tool_result 配对。
- **IPC broadcast Lagged 静默断流**（2026-07-31 review 第三轮修复）：`IpcProviderClient::chat` 的消费 task 用 `while let Ok(notif) = rx.recv().await` 消费 broadcast 通知。`tokio::sync::broadcast::Receiver::recv()` 在订阅者消费慢于生产者时返回 `Err(Lagged(n))`，旧逻辑 `while let Ok` 在 `Err` 时退出循环，导致 `tx` drop → agent 侧 `stream.recv()` 返回 `None` → 触发 `StreamParse` 重试 → 重试后可能再次 Lagged 陷入死循环。修复：改为 `match`，`Lagged` 时 `continue`（跳过溢出的旧通知，继续接收新通知），仅 `Closed` 时 `break`。注意 `NotifPump`（fs_event_bridge）的 `try_recv` 已正确处理 `Lagged`（`continue`）。

### 4.3 agent loop 双层循环（ADR-0007 / optimization O4）

- 外层：follow-up 驱动；停止边界先 `try_recv` steering（防止 steer 在 yield 点丢失，对齐 omp `runLoopBody`）。
- 内层：tool-call + steering——LLM → tool → continue，直到 `tool_calls.is_empty()`。
- Abort：`CancellationToken::cancel()`，`stream_llm_response` 与 `executor.execute` 都 `tokio::select!` 监听。
- Steering：**流式期即时中断**当前流（`stream_llm_response` 的 `select!` race steering_rx，对齐 omp `streamAssistantResponse` 的 abort race），返回 `pending_steering` + `partial_text`，由 `run_agent_loop` 发 `SteeringInterrupted` 事件——ECS 把半截文本 `finalize_assistant` 为被中断的 assistant 消息并清空当前节点（避免与新回复拼接），同时 bridge 侧把被中断的 assistant 文本 + steering 文本注入 `req.messages`（2026-07-31 修复：与 conv 侧对称，避免 LLM 下一轮看不到自己半截话）后重新流式。停止边界也重新轮询，避免 steer 丢失。
- Length 截断：`stop_reason == Length` 时不执行 tool_calls（参数可能不完整），为每个补占位 skipped result（对齐 omp `createAbortedToolResult`），回灌 assistant tool_call + tool result 到 req.messages 维持配对，让 LLM 重新生成完整调用。
- **stream 提前断开**（`stream.recv() == None`，provider 未发 Done）：视为 `StreamParse` 可重试错误，返回 `Err` 让 `stream_with_retry` 重试（修复前返回 `Ok(Stop)` 导致上层误执行可能不完整的 tool_calls）。
- **Aborted 直接退出**：`outcome.stop_reason == Aborted`（stream 被 cancel_token 或 steering Abort 中断）时 `run_agent_loop` 直接 return，不走到外层 `steering_rx.recv().await`——否则外层会消费并丢弃下一次 `StartLoop` 命令，导致对话无法重启。
- **Confirming 态 Abort 中断**（2026-07-31 修复）：`executor.execute` 的 `NeedsConfirmation` 路径全程（`confirm.confirm` + `rx.await` 等待决策）用 `tokio::select!` 监听 `signal.cancelled()`，Abort 返回 `ToolError::Aborted`，agent loop 走 abort 回灌路径。关键：`cancel_token` 提升到 `AgentBridge.current_cancel`（`Arc<Mutex<Option<CancellationToken>>>`，task 与 ECS 共享），ECS Abort handler **直接调 `token.cancel()`**——绕过 `steering_rx`（run_agent_loop park 在 `executor.execute` 时无法轮询 steering_rx 消费 Abort 命令）。StartLoop 时 task 设置 current_cancel，对话结束清 None。
- Compaction（optimization O9）：每次 stream 拿到 `usage` 后，`maybe_compact` 用 `should_compact(max(provider_prompt, 本地估算), window, settings)` 判断；触发则 `LlmCompactor.compact` 生成摘要，`apply_compaction` 重建 `req.messages`（summary 前置 + kept），发 `AgentEvent::Compacted`。

### 4.4 自动重试（F-01）

- `stream_with_retry` 包装 `stream_llm_response`：失败时按 `RetryConfig` 重试。
- **仅可重试错误重试**：`ErrorKind::Network`（连接/超时）、`StreamParse`（SSE/JSON 解析）；`NotConfigured`/`AuthFailed`/`ProviderError` 立即失败（重试无意义）。
- **次数**：`ProviderConfig.max_retries: Option<u32>`——`None` = 无限重试（直到成功或被中断），`Some(n)` = 最多 n 次。
- **模式**：`RetryMode::Fixed`（固定间隔）/`Exponential`（指数退避 `min(initial * factor^(n-1), max_delay)`）。
- 重试前发 `AgentEvent::RetryAttempt` → ECS 清空半截助手文本（`finalize_assistant` 固化为被中断消息）并发 `RetryMessage`，UI 据此展示"重试中(第 N 次)"。同时 bridge 侧把半截 assistant 文本回灌到 `req.messages`（2026-07-31 修复：`stream_with_retry` 改 `&mut ChatRequest`，与 conv 侧 `RetryAttempt` 的 `finalize_assistant` 对称，避免重试后 LLM 看不到重试前半截话）。`stream_llm_response` 的 Err 携带 `partial_text`（三元组）供此回灌。
- 退避 sleep 期间 `tokio::select!` 监听 `cancel_token`，abort 可中断重试。
- 配置派生：`RetryConfig: From<&ProviderConfig>`，`main` 启动时注入，`config_bridge` 在 `CONFIG_CHANGED` 刷新时调 `AgentBridge::update_retry_config` 更新（下次对话生效，对话中固定）。

### 4.5 配置目录布局（对齐 pi）

借鉴 pi 的 `~/.pi/agent/` + `<project>/.pi/` 两层布局（见 `pi/packages/coding-agent/src/config.ts`），xgent 配置目录分两层：

- **全局用户目录** `~/.xgent/agent/`（可经 `XGENT_AGENT_DIR` 环境变量覆盖，便于开发隔离与多实例测试）：
  - `config.toml`：全局配置（provider 列表、默认模型、偏好）
  - `sessions/`：会话历史 JSONL（ADR-0008，跨项目共享）
  - `sessions.db`：会话 SQLite（D-04 预留，未启用）
  - `auth.json` / `models.json`：预留
  - 路径函数：`paths::agent_dir()` / `global_config_file()` / `sessions_dir()` / `session_file_path(id)` / `sessions_db_path()`
  - 用 `dirs::home_dir()` 而非 `dirs::config_dir()`，跨平台一致、用户易找（macOS 不埋在 `~/Library/Application Support`）。
- **项目级目录** `<project_root>/.xgent/`：项目特定配置（`config.toml` 含 provider 覆盖、tool 策略、`[agent]`/`[tools]`/`[context]` 运行限额，见 §4.5.1）。
  - 路径函数：`paths::project_config_dir(root)` / `project_config_file(root)`。
- **daemon socket**：默认平台缓存目录（`dirs::cache_dir()/xgent/daemon.sock`）；设置 `XGENT_AGENT_DIR` 时改用其下 `daemon.sock`（开发隔离）。

### 4.5.1 运行参数配置（R3-17，2026-10-14）

台账 R3-17 点名的 13 个限额（落成 14 个配置键，其中上下文窗口另有按模型的映射表）原本是编译期常量，用户无法调整；现在全部可配，且**缺省值逐项等于它替换掉的常量**——不写配置的工程行为与改动前逐字等价（由 `xgent_settings_core` 的 serde 测试锁死）。

约定：数值键 **`0` = 未设置，回落内置缺省**；因此"完全不限"不可表达（这些闸的存在意义就是止住失控成本与卡帧）。取值一律经 `effective_*()` 访问器读取，`0` 的解析只发生在这一处。

全局 `~/.xgent/agent/config.toml` 的 `[providers.<id>]`（端点属性，daemon 与 UI 共用同一 `build_provider`）：

| 键 | 缺省 | 作用点 |
|:---|:---|:---|
| `context_window` | `128_000` | compaction 触发量尺（`xgent_app::resolve_context_window`） |
| `model_context_windows` | 无条目 | 按模型 id 覆盖上面的值 |
| `max_output_tokens` | `8192` | Anthropic Messages 请求体 `max_tokens`（OpenAI 兼容/Response API 不发送该字段，此键对它们无效） |
| `first_event_timeout_secs` | `30` | openai_compat / anthropic 等首个 SSE 事件的上限 |

窗口取值次序：`model_context_windows[当前模型]` > `context_window` > `128_000`，**走内置回落时启动打一条 warn**（窗口猜错的表现是「小模型永不压缩」或「大模型过早压缩」，不告警就无从归因）。不在启动时调 `list_models` 探测——那会把一次网络往返与其失败模式塞进启动路径。

项目级 `<project>/.xgent/config.toml`（按仓库的工作条件，与既有 `tool_policy`/`context_strategy` 同层）：

| 键 | 缺省 | 作用点 |
|:---|:---|:---|
| `[agent] retrieval_max_tokens` | `8_000` | bridge 每轮构造 `ContextQuery.max_tokens` |
| `[agent] max_events_per_frame` | `64` | `agent_poll_system` 单帧 drain 事件数（到界即停，剩余留在 channel，**不丢事件**） |
| `[tools] confirm_timeout_secs` | `300` | `ToolExecutor` 等待确认决策的时长 |
| `[tools] command_timeout_secs` | `60` | `run_command` 执行上限，超时杀整个进程组 |
| `[tools] command_max_output_bytes` | `65_536` | `run_command` 捕获输出上限（仍受共享天花板 `MAX_TOOL_OUTPUT_BYTES` 再约束） |
| `[context] tree_max_depth` | `4` | 目录树摘要深度 |
| `[context] tree_max_entries` | `200` | 目录树条目数（超限以 `...` 收尾） |
| `[context] rg_timeout_secs` | `10` | rg 调用超时，超时降级内置遍历 |
| `[context] max_search_files` | `20` | 检索读取的文件数 |
| `[context] max_keywords` | `16` | 关键词上限（CJK bigram 会成倍放大，8 会截掉中文问句后半段） |

**接线方式**（新增参数时照此路径，不要引全局单例）：`xgent_app` 读配置 → `ToolParams::from_tool_config` / `RetrievalLimits::from_context_limits` / `AgentBridgeConfig` → 构造函数注入。工具与检索 crate **不直接依赖配置节类型**，转换在 app 完成，`Tool`/`ContextProvider` 契约因此与配置层解耦。

> 2026-10-16 起这条路上多了一层共享句柄：三类值与 `RunSettings` 都包在 `Arc<RwLock<…>>` 里，由 `xgent_app::settings_io::LiveSettings` 持有，好让设置面板写完就生效（见 §4.5.2）。构造注入的形式没变——`ToolExecutor::with_shared_params` / `build_context_provider_shared` / `AgentBridgeConfig{tool_policy, run_settings}` 仍收句柄；`with_defaults()` 与 `with_params(&ToolParams)` 这类「取快照」的构造照旧可用（测试与不需要热更新的场合）。

> `xgent_agent` 的每帧上限不再是 `AgentBridge` 上的普通字段，而在共享的 `RunSettings` 里（drain 每帧读一次）。

**迁移注意**：会话历史从项目级 `<project>/.xgent/sessions/` 迁到全局 `~/.xgent/agent/sessions/`，`session_file_path(id)` 去除 `project_root` 参数。旧项目级会话文件不自动迁移（MVP 会话为临时性，跨项目共享更符合 D-04 演进）。

---

### 4.5.2 配置可见性与热应用（R3-13，2026-10-16）

§4.5.1 把 14 个限额从常量搬进了配置文件，但**搬进去≠够得着**：直到本轮之前，设置面板只有 provider 四项，`[tool_policy]`（`AGENTS.md` §5.4 的核心安全旋钮）、`[agent]`/`[tools]`/`[context]` 与 provider 的窗口/输出/首事件限额都只能手改 TOML；而项目作用域根本写不进去——daemon 对 `config scope=project` 直接回「未实现」（`session.rs`），全局白名单也不含那三个限额字段。本轮补的是这条通路，不改任何键的语义。

**四处变化合起来才叫"可达"**：

| 层 | 落地 |
|:---|:---|
| 协议 | `ConfigReadRequest`/`ConfigWriteRequest`/`ConfigChanged` 增加 `project_root: Option<String>`（`#[serde(default)]`，旧客户端发不带该字段的请求行为不变） |
| daemon | `ConfigCoordinator` 按项目根持有权威副本（`projects: HashMap<PathBuf, ProjectConfig>`，首次访问读盘、写时"改副本 → 落盘 → 换入内存"），`scope=project` 从此真实现；全局白名单补 `context_window`/`max_output_tokens`/`first_event_timeout_secs`（`model_context_windows` **只读**——按单字段写一个数字没有确定语义） |
| 面板 | 打开即回填（provider 四项也不例外，此前重开是四个空框）；六个分组：provider / 工具策略 / agent / tools / context / 语言；保存只发**脏键**；部分失败时状态行**逐个点名**已写入的键（`settings-written-keys`，不是"已保存 N 项"那种计数）与失败的键 + daemon 给的原因 |
| 运行时 | 四组共享句柄（下详），改完在下一次消费点生效，不需要重启 |

**项目作用域键语法**（与全局同形，点分；`project` 一次取回整份，面板打开用这一条而不是十几次往返）：

| 键 | 写 | 备注 |
|:---|:---|:---|
| `project` | ✗ 只读 | 整份 `ProjectConfig` |
| `tool_policy` / `agent` / `tools` / `context` | ✗ 只读 | 整段读；整段写会一次清掉该段所有设置，故拒绝 |
| `tool_policy.approved` / `.denied` | ✓ 字符串数组 | 整表替换；面板按三态折算后写两张互斥列表 |
| `agent.retrieval_max_tokens` / `.max_events_per_frame` / `.max_parallel_tools` | ✓ 非负整数 | |
| `tools.confirm_timeout_secs` / `.command_timeout_secs` / `.command_max_output_bytes` | ✓ 非负整数 | |
| `context.tree_max_depth` / `.tree_max_entries` / `.rg_timeout_secs` / `.max_search_files` / `.max_keywords` | ✓ 非负整数 | |

缺 `project_root` 或该根不是已存在的目录都是**显式错误**（`INVALID_PARAMS`），绝不静默按 global 处理——两侧字段语义不同，降级会让用户以为项目配置被改了。变更通知按项目订阅者路由（`broadcast_to_project`），别的项目的窗口不会被叫醒。不要求客户端先 `fs.watch` 过该根：启动订阅是异步的，把保存压在订阅成功上会让"先保存后订阅完成"的窗口收到一条无从解释的失败。

**四组句柄与生效边界**（`xgent_app::settings_io::LiveSettings`，形状沿用既有的 `retry_config`/`loop_bounds`）：

| 句柄 | 消费方 | 读取点 = 生效边界 |
|:---|:---|:---|
| `Arc<RwLock<ToolPolicyConfig>>` | `ToolExecutor::execute` → `resolve_policy`（经 `ToolCtx`） | 下一次工具调用 |
| `Arc<RwLock<ToolParams>>` | 执行器确认等待 + `RunCommand`（共享同一 `Arc`） | 下一次确认 / 下一次命令调用 |
| `Arc<RwLock<RetrievalLimits>>` | `OnDemandContextProvider`（每次检索开头取一次快照） | 下一次检索 |
| `Arc<RwLock<RunSettings>>` | bridge：检索预算/窗口/并发上限在每次 StartLoop 取快照；`max_events_per_frame` 由 drain 每帧读 | 下一条用户消息 / 下一帧 |

**apply 只认整份权威快照**（`apply_snapshot(project, global, provider_id, model)`）：逐键增量应用最容易漏的就是"改了 `context_window` 却没重算窗口"这类半套更新，配置才十几项，整体重算的代价可以忽略。同一条 apply 服务"本窗口写入"与"对端广播"两种来源，因此另一个窗口改配置时本窗口的显示与运行时判据一起跟上。写失败的键**不进句柄**——句柄只按 daemon 真正接受的值更新，运行时用的永远是磁盘真值。

**面板重渲染只认 `SettingsSnapshotReady.tick` 的*增量*，两边各留一道**。`xgent_ui::settings_panel::toggle_panel` 比较"上次渲染时的 tick"（`Local<u32>`），**不看 `is_changed()`**——`ResMut` 的 `deref_mut` 无条件标脏，挂在脏标记上等于"任何碰到这份资源的帧都重建一次"。`xgent_app::settings_io::drain_save` 则只在 `report.failed` 为空时才 `ready.tick += 1`，失败那一帧连这份资源都不碰。两条合起来才是 A9 要的：有键被 daemon 拒掉时，被拒的那一格连同原因留在屏幕上；下一次合法推进（对端改配置、或一次全成功的保存）照旧把显示拉回磁盘值。资源与句柄不受影响，永远跟磁盘一致。

这条判据为什么必须由写 `tick` 的一方定，而不是等面板"收到失败报告后再挡"：`drain_save` 在同一帧里推进 `tick` 并写报告，bevy 的消息要到下一帧才可读，而面板的 `toggle_panel` 排在 `apply_save_report` 之前——等面板知道失败时重建早已发生（第二轮在面板侧加 `hold_rerender` 就是这么死的）。第三轮的教训则是**"两半各自单测、合起来的帧序没人验"**：`settings_io` 里现在有 `a_failed_save_leaves_the_rejected_value_in_the_real_panel_frame`，把真实的 `drain_snapshot`/`drain_save` 与真实的 `SettingsPanelPlugin` 装进同一个 `App`，投一份带失败键的 `SaveTaskResult` 进通道，让 Bevy 自己的调度跑完再去读输入框。为此 `drain_save` 拆成两半：落值与推进 `tick`（不依赖 IPC 与 bridge，因此能被这样装进测试），与 `refresh_provider_after_save`（读报告、经 IPC 重取 `ProviderInfo`，识别判据是纯函数 `touches_provider`，另有单测）。

`RunSettings` 存**生效值**而不是原始值：`0` 哨兵的解析只在 `effective_*()` 那一处，loop 侧读到的是能直接用的数（`max_parallel_tools = 0` 意味着 4，绝不是"不限"）。
## 5. 开发注意点

### 5.1 ECS 通信硬约束（架构 §5.2）

**所有子系统只通过 ECS Events（即时观察者）与 Messages（缓冲消息）通信，禁止直接方法调用。**

- 这条约束只约束 **跨 Plugin 边界** 的调用（即"子系统之间"）。Plugin 内部——同一 crate 内的系统之间、同一系统内的函数调用——按普通 Rust 代码处理，不强制走 Message。
- 违反约束会让 Plugin 无法独立测试、无法 headless 录制/回放。
- 新增跨 agent/UI 交互时，在 `xgent_agent/src/events.rs` 定义 Message，UI 侧用 `MessageReader`/`MessageWriter`。
- 异步逻辑（provider/tools/context）经 tokio task → mpsc channel → ECS 系统每帧非阻塞 `try_recv`（见 `bridge.rs::agent_loop_task`）。

### 5.1.1 何时用 Message / Resource / 直接调用（判据总纲）

约束本身是"跨 Plugin 边界禁止直接方法调用"，但落地时要区分三种交互语义：**动作信号**、**状态投影**、**同步查询**。三者各有合适载体，错配是常见的架构债来源。下表是总纲判据，后三节给出每一类的详细规则与现有正例。

| 交互语义 | 特征 | 载体 | 现有正例 |
|:---|:---|:---|:---|
| 动作信号（action） | "发生了 X"或"请做 X"，有时序、有方向、可能并发多条、可能多消费者 | **Message**（默认）或 **Event**（即时 observer） | `UserInputMessage`、`DeltaMessage`、`SteeringMessage`、`EditorCommand`、`PaletteTriggered`、`HotkeyTriggered` |
| 状态投影（state） | "当前是 X"，可随时读、可被多系统读、读的是某时刻快照 | **Resource** | `Conversation`、`EditorTabs`、`TokenUsage`、`PanelWidths`、`GlobalConfigRes` |
| 同步查询（query） | "我要拿 Y"，请求-响应、有返回值、调用方等结果 | **直接 trait 调用** | `ContextProvider::retrieve`、`EditorState` trait、`Tool::execute` |

**一句话判据**：问自己"这条交互有没有返回值"——
- 调用方需要拿结果才能继续 → 直接 trait 调用（同步查询）；异步侧用 channel 双向化。
- 调用方不需要结果，只要"通知发生了" → Message。
- 多个系统都要读"当前状态" → Resource；状态变更要广播 → 改 Resource + 发一个通知 Message。

> 注：Bevy 0.20 中 `Event` 与 `Message` 是**两个并存的 trait**，语义不同：
> - `Message`（`#[derive(Message)]` + `add_message::<T>()` + `MessageReader`/`MessageWriter`）：缓冲队列，双缓冲（`Messages<M>`），配 `message_update_system` 每帧 `update` 切缓冲。读一次 per update 不丢，两次 update 后保证丢。每个 `MessageReader` 独立 cursor，多 reader 各读全量（广播语义）。
> - `Event`（`#[derive(Event)]` + `Observer` + `On<E>` + `world.trigger(E)`）：即时观察者，trigger 即同步派发给所有 observer，无缓冲、无队列。
>
> **本项目代码统一用 Message**（`crates/` 下无 `add_event`/`EventReader`/`EventWriter`）——这是项目选择，不是 Bevy 强制。`docs/plans/step9/step10/step12` 与 `editor-design.md` 残留 `#[derive(Event)]` 是历史文档遗留，实现时改用了 Message，以代码为准。
>
> **何时该用 Event 而非 Message**（Bevy 语义层面）：纯即时通知、无需跨帧、无需多消费者排队、希望同步触发 observer 处理——如"provider 状态变更"这类"发生即响应"。本项目目前未用 Event（所有通知都走 Message 也工作），但新增"强即时性"通知时可考虑 Event，不要因本文档统称 Message 而排斥它。判据：**需要跨帧/缓冲/多消费者排队 → Message；需要即时同步触发 observer → Event**。

#### 5.1.1.1 动作信号 → Message（默认）或 Event（即时）

**判据**：满足以下任一即为动作信号。

- 有方向（A 发、B 收，或广播给多个订阅者）。
- 有时序语义（"先 abort 再 done" 或 "steering 在流式期即时中断"），顺序必须保证。
- 可能并发多条（流式 delta、多个 tool_call），需要缓冲队列。
- 接收方可能多个（UI 渲染 + 状态栏 + 持久化都要听 Done）。
- 调用方不需要同步等结果（"发完即走"）。

**Message vs Event 选择**（见上方注释的 Bevy 0.20 源码语义）：
- **默认 Message**：跨帧、需缓冲、多消费者各读全量。本项目所有动作信号都是 Message。
- **改用 Event 的判据**：信号严格单帧内被 observer 消费、且无需跨帧保留、希望 trigger 即同步派发。典型场景：provider 状态变更、配置热重载完成、预算告警——这些"发生即响应"的通知若用 Message 也能工作（本项目即如此），但 Event 的即时性更贴合语义。
- **必须用 Message 的场景**：流式 delta（跨帧累积）、用户输入、abort/steering（不能丢、可能跨帧）——这些用 Event 会丢消息。

**Message 用法要点**（Bevy 0.20）：
- 派生 `#[derive(Message)]`，插件 `build` 里 `app.add_message::<T>()` 注册（自动配 `message_update_system`）。
- 写：`MessageWriter<T>::write(payload)`；读：`MessageReader<T>::read()` 迭代（循环消费，单帧内可能多条）。
- 跨帧保留：`Messages<M>` 双缓冲，读一次 per update 不丢，两次 update 后保证丢。`AbortMessage`、`SteeringMessage` 这类必须抵达的信号靠此保证（前提是消费系统至少每帧 read 一次）。
- 多消费者：每个 `MessageReader` 独立 cursor，各读全量（广播语义）。
- 命名约定：`XxxMessage` 表双向、`XxxRequest`/`XxxResult` 表请求-响应配对——后者通常意味着该用 trait 调用（见 §5.1.1.3 反例）。

**Event 用法要点**（本项目暂未用，备查）：
- 派生 `#[derive(Event)]`，`world.trigger(E)` 或 `commands.trigger(E)` 派发。
- 订阅：`world.add_observer(|trigger: On<E>| { ... })`，observer 同步执行。
- 无缓冲、无队列、无 cursor——trigger 即派发，派发完即结束。若 observer 当帧未注册则错过。
- `EntityEvent` + `#[entity_event]` 可做实体级 observer（如 `RemovedComponents`）。

**现状正例**（`crates/xgent_agent/src/events.rs`，16 种 Message）：

| Message | 方向 | 为何用 Message |
|:---|:---|:---|
| `UserInputMessage` | UI → agent | 调用方不等结果；agent 异步消费；需缓冲 |
| `DeltaMessage` | agent → UI | 流式多 chunk、需累积、UI 帧消费 |
| `SteeringMessage` | UI → agent | 时序关键（流式期 race abort）；不能丢 |
| `AbortMessage` | UI → agent | 必须抵达（一帧内若 agent 系统未跑，Message 留到下帧） |
| `DoneMessage` | agent → UI | 多消费者（chat_panel 收尾 + status_bar 累 token + 持久化） |
| `ConfirmDecisionMessage` | UI → agent | 异步确认握手，`SharedConfirm` 经 oneshot 回填 |

**反例（禁止）**：UI 系统直接 `agent_bridge.start_loop(req)` —— 跨 Plugin 边界直接调用，破坏可测性（`bridge_tests.rs` 的"灌消息即测试"模式失效）。

#### 5.1.1.2 状态投影 → 用 Resource

**判据**：满足以下任一即为状态，应放 Resource。

- 多个系统可能随时读"当前值"（不关心谁改的，只看当前快照）。
- 改变后会触发多处 UI 刷新（数据驱动）。
- 持续存在、非一次性（不是"发生了一次"，而是"当前是某值"）。
- 单一权威写入点（避免多系统并发写）。

**规则**：
- **写入点收敛**：每个 Resource 应有唯一"所有者系统"负责写，其他系统只读。`Conversation` 由 `agent_loop` 独占写；`PanelWidths` 由 `handle_resize_drag` 独占写，`apply_panel_widths` 只读。违反则触发 Bevy 的 B0001 或需 `ParamSet`/`.after()` 排序。
- **状态变更通知用 Message**：若其他系统需要"当 X 改变时反应"，改 Resource 后再发一个通知 Message（如配置刷新后 `config_bridge` 发 `CONFIG_CHANGED`）。不要让消费方轮询 Resource 判断是否变了（低效且易漏帧）。
- **异步产物的 Resource 化**：tokio task 回 ECS 的结果是"状态增量"，既可以是 Message（流式 delta）也可以是 Resource（持有 channel receiver 让 ECS 每帧 drain）。**高频流式用 Message**（`DeltaMessage`，每帧一条以上）；**低频控制流用"持有 receiver 的 Resource"**（`PendingRefresh`、`NotifPump`、`EditorIoRuntime` —— 它们持有 `mpsc::Receiver`，ECS 系统 `try_recv` drain）。

**现状正例**：

| Resource | 性质 | 所有者系统 |
|:---|:---|:---|
| `Conversation` | 对话状态（消息历史、当前助手文本、状态机） | `agent_loop` 独占写 |
| `ProviderInfo` / `ContextState` | provider 就绪与上下文状态 | `config_bridge` / `agent_loop` |
| `TokenUsage` | 累计 token | `status_bar` 写、`chat_panel` 订阅 `DoneMessage` 累加 |
| `EditorTabs` / `EditorView` / `SideViewContent` | 编辑器标签与视图状态 | 编辑器各自系统 |
| `PanelWidths` / `ActiveResize` | 面板宽度与拖拽态 | `handle_resize_drag` 写 / `apply_panel_widths` 读 |
| `EditorStateSnapshot` | 编辑器状态只读快照 | `ContextProvider` 查询（见 §5.1.1.3） |
| `AgentBridge` / `PendingRefresh` / `NotifPump` / `EditorIoRuntime` | 持有 tokio channel 端的桥接 Resource | 各自 ECS 系统每帧 drain |
| `GlobalConfigRes` / `ProjectConfigRes` / `Localizer` | 配置与 i18n（两份配置资源是设置面板的读模型，快照与对端变更时刷新） | `xgent_app` 启动注入 / `config_bridge` 与 `settings_io` 刷新 |

**反例（禁止）**：把"当前对话是否进行中"做成 `IsConversationActiveMessage` —— 这是状态不是动作，UI 应读 `Conversation.status` Resource。

#### 5.1.1.3 同步查询 → 用直接 trait 调用

**判据**：满足以下任一即为同步查询，允许（且应）直接 trait 调用，不强制 Message 化。

- 调用方需要返回值才能继续（请求-响应语义）。
- 查询的是"当前状态"而非"触发动作"。
- 在异步上下文（tokio task）内执行，不在 Bevy 主线程同步路径。
- 查询方与被查询方跨 crate，直接调用会成环——此时用 **trait 反转依赖**（§5.2）。

**规则**：
- **trait 定义放底层 crate**（`xgent_core` 或 `xui_i18n`），实现放上层，查询方经 trait 调用。这是"允许直接调用"与"分层不破"的平衡点。
- **查询结果不写回 Resource 就别发 Message**：`ContextProvider::retrieve` 返回 `ContextResult`，调用方（bridge 异步侧）用它构造 `ChatRequest`，这是纯函数式调用，无需事件化。
- **跨进程查询走 IPC**（JSON-RPC request/response，见 `methods.rs`），不强行经 ECS Message——ECS Message 是进程内语义，跨进程本身就有 RPC 协议，硬塞 Message 会加一层无意义翻译。
- **EditorState 模式**：编辑器状态供 context 查询（`@cursor`/`@selection`），定义为 `xgent_core::EditorState` trait，`xgent_ui` 实现，`xgent_context` 经 trait 调用。`editor-design.md` §3.2 的 `EditorStatePolled` Resource 注释"只读视图，不事件化"即此模式。

**现状正例**：

| trait | 定义层 | 实现层 | 调用方 | 为何不事件化 |
|:---|:---|:---|:---|:---|
| `ContextProvider::retrieve` | `xgent_context` | `OnDemandContextProvider` | `agent_loop_task`（tokio 侧） | 请求-响应、有返回值、在 async 上下文 |
| `EditorState`（`@cursor` 查询） | `xgent_core` | `xgent_ui::editor` | `xgent_context::at_syntax` | 跨 crate 查询状态，trait 反转避免成环 |
| `LlmProvider::chat` | `xgent_provider` | `OpenAiCompatProvider` | `ProviderClient`（IPC 适配） | 异步流式，返回 channel 而非 Message |
| `Tool::execute` | `xgent_tools` | 4 builtins + `EditorTool` | `agent_loop_task`（经 `ToolExecutor`） | 请求-响应、有 `ToolResult` 返回 |
| `StringSource::get` | `xui_i18n` | `xgent_settings::Localizer` | `xui::i18n_bridge` | 纯查询、trait 反转 |

**反例（禁止）**：为 `ContextProvider::retrieve` 定义 `ContextQueryMessage` + `ContextResultMessage` 双向 Message + 状态机协调 —— 纯属把 RPC 拍扁成消息，丢失了"返回值"这个最强契约，且 ECS 同步系统无法 `await` retrieve（这正是 2026-07-20 修复把 context 检索挪到 bridge 异步侧的原因）。

#### 5.1.1.4 异步侧的特殊处理

约束的真正复杂度在"异步↔同步"边界。规则：

- **tokio task ↔ ECS 必经 channel**：`AgentCommand`（ECS → tokio）、`AgentEvent`（tokio → ECS）双向 mpsc（`bridge.rs`）。**ECS 主线程永不 `await`**——约束落到这点：任何会 `await` 的调用都必须在 tokio task 内，结果经 channel 回 ECS。
- **ECS 同步系统内禁止 `await`**：`context.retrieve`、`provider.chat`、`tool.execute` 都是 async，**不能在 ECS 系统直接调**。要么挪到 tokio task（bridge 模式），要么用"持有 receiver 的 Resource"每帧 drain。
- **确认握手用 oneshot，不用 Mutex**：`SharedConfirm` 持 `oneshot::Sender<ConfirmDecision>`，ECS 写 `ConfirmDecisionMessage` → `agent_loop` 读 → 调 `shared_confirm.set_sender(tx)` 回填 → tokio task 的 `oneshot::Receiver::await` 唤醒。**这是约束下处理"请求-响应跨同步/异步"的标准范式**，不要替换为 `Mutex<ConfirmDecision>` + 轮询。
- **ECS 同步路径的纯查询允许直接调**：若 trait 方法是同步的（如 `xgent_core::EditorState::cursor`/`active_path`/`selection` 同步返回，见 `state.rs::impl EditorState for EditorStateSnapshot`），ECS 系统可直接 `Res<EditorStateSnapshot>` 读或经 trait 调用，无需 channel。判据是"是否 `async`"，不是"是否跨 crate"。

#### 5.1.1.5 新增交互的决策流程

新增任何跨 Plugin 交互时，按此顺序自问：

1. **调用方需要返回值吗？**
   - 是 → 同步查询（§5.1.1.3）。看是否跨 crate：是则 trait 定义放下层；否则直接函数调用。
   - 否 → 进入 2。
2. **是"当前状态"还是"发生了一次"？**
   - 状态 → Resource（§5.1.1.2）。问：谁独占写？哪些系统读？状态变更要通知谁（发 Message）？
   - 事件 → Message（默认）或 Event（§5.1.1.1）。问：需跨帧/缓冲吗（是→Message）？强即时同步触发吗（是→Event）？多消费者吗？有返回值吗（有则不该用 Message/Event）？
3. **在异步上下文吗？**
   - ECS 同步 + 需 await → 违规。挪到 tokio task，结果经 channel 回 ECS（§5.1.1.4）。
   - ECS 同步 + 纯同步 trait → 直接调，无需 channel。
   - tokio task 内 → 直接调 async trait，结果经 `AgentEvent` channel 回 ECS。
4. **跨进程吗？**
   - 是 → JSON-RPC（request/response 同步语义、notification 单向语义），不强行经 ECS Message。跨进程通知回 ECS 后可转 Message（`FileChanged` → `FileChangedEvent` 桥接）。

**反模式速查**：
- ❌ 把状态做成 Message（`IsXxxActiveMessage`）—— 应 Resource。
- ❌ 把查询做成双向 Message + 状态机 —— 应直接 trait 调用。
- ❌ ECS 系统内 `await` async trait —— 挪 tokio task。
- ❌ 用 `Mutex<T>` + 轮询做异步握手 —— 用 `oneshot` channel。
- ❌ 跨 Plugin 边界直接方法调用（如 UI 调 `agent.start_loop`）—— 走 Message。

### 5.2 反转依赖模式（避免成环）

- `xui` 不依赖任何 `xgent_*`，可独立发布——业务字符串经 `xui_i18n::StringSource` trait 注入。
- `xgent_context` 不依赖 `xgent_ui`——编辑器状态经 `xgent_core::EditorState` trait，`xgent_ui` 实现该 trait，`xgent_context` 经 trait 查询（处理 `@file`/`@cursor`/`@selection` 引用，见 `xgent_ui/src/editor/at_syntax.rs`）。
- 新增需要跨层查询的能力时，trait 定义放底层（`xgent_core`），实现放上层，查询方经 trait 调用。

### 5.3 安全模型（架构 §11）

**默认所有工具调用（含只读）均为 `NeedsConfirmation`**。用户可在配置按 tool_id 提升为 `Approved` 或降为 `Denied`。

- `resolve_policy` 4 步顺序：配置 denied → 配置 approved → `tool.approval_for(input)` 动态 tier → 兜底 `NeedsConfirmation`。
- **例外**：`ToolTier::UiOnly`（编辑器 agent 工具）默认 `Approved`，不走确认（ADR-0009）。
- **例外**：编辑器用户保存（Cmd+S）直接 `fs::write`，不经 WriteFile 工具、不经确认（ADR-0009）。
- 新增工具时：只读工具 `tier()=Read, concurrency()=Shared`；写工具 `Write, Exclusive`；执行工具 `Exec, Exclusive`；危险命令检测 override `approval_for` 返回更严 tier。

### 5.4 ChatEvent clean cutover（ADR-0006）

- 旧 4 变体（`Delta/ToolCall/Done/Error`）已删除，**不留兼容别名**。
- 新增 provider 适配器时，必须发射细粒度事件：`Start{model}` → `TextStart/TextDelta/TextEnd` → `ToolCallStart/Delta/End`（按 index 聚合）→ `Done{reason: StopReason, usage}`。
- agent loop 不依赖 `StopReason` 决定是否继续——`tool_calls.is_empty()` 才决定。

### 5.5 会话持久化（ADR-0008）

- JSONL append-only，存全局 `<agent_dir>/sessions/<session_id>.jsonl`（即 `~/.xgent/agent/sessions/`，可经 `XGENT_AGENT_DIR` 覆盖；对齐 pi 的 `~/.pi/agent/sessions/` 布局，跨项目共享）。
- Compaction 触发时 append `SessionEntry::Compaction`（记录摘要文本、first_kept_id、tokens_before）；JSONL append-only 不重写历史，恢复会话时读到 CompactionEntry 即知前文已摘要。
- 会话开始 append `SessionEntry::Header`，每次 AssistantMessage 完成（Done）append `Message` entry。
- `Conversation.session_store` 在首次对话时经 `ensure_session_store` 初始化。
- 元数据索引/prompt 历史/模型使用统计保留 SQLite（P1，未实现）。

### 5.6 Provider 流式协议正确性（ADR-0005）

- `message_to_json` 按 role 展开为 OpenAI 协议形态：
  - assistant 的 `ContentBlock::ToolCall` 提到顶层 `tool_calls` 字段；
  - tool role 消息必须带 `tool_call_id`（修复旧版缺此字段被 OpenAI 400 拒绝的 bug）。
- Stream 双层超时：首事件 `first_timeout` 防慢响应挂死，后续每事件 `idle_timeout` 防流卡死；超时发 `ChatEvent::Error{kind: Network}`。

### 5.7 daemon 不依赖 Bevy

- daemon 是纯 tokio 服务，**不要在 daemon 引入 bevy 依赖**。
- 配置类型经 `xgent_settings_core`（纯类型，无 Bevy），daemon/provider 用这层；Bevy Resource 包装在 `xgent_settings`，agent/ui 用。
- 新增需跨进程共享的类型放 `xgent_core`，不要放 `xgent_settings`。

### 5.8 i18n 从一开始内置（NF-05）

- 所有用户可见字符串走 fluent（`crates/xgent_settings/locales/{zh-CN,en-US}/main.ftl`）。
- 经 `xui::tr`/`tr_with`/`Strings` Resource（由 `xgent_app` 注入 `Localizer::default()`）。
- 新增 UI 文案时，在两份 `.ftl` 都加 entry，代码用 `tr("key")`，不硬编码字符串。
- **语言切换持久化**：命令面板切换语言时经 IPC `config.write` 写 `preferences.language` 到全局配置（`SaveLanguageMessage` → `config_bridge::save_language`），重启后保持。

### 5.9 编辑器（F-11）边界

- **中等能力**：多行 + 行号 + undo/redo + 查找替换 + tree-sitter 语法高亮（仅 Rust grammar，随二进制编译入，不做按需下载——D-06 已决策）。
- **不含**：LSP、编辑器内部 split view（同一编辑器内多窗格）、诊断、跳转。完整能力边界留后续。（对话/编辑器分屏见 §5.11）
- `xui::TextEditor` 是通用裸件（纯依赖 bevy + xui_i18n + tree-sitter），多标签/文件 IO/冲突协调在业务层 `xgent_ui::editor`。
- **字号跟随主题**：`EditorTheme`（xui）的 `font_size`/`text`/`text_dim`/`line_height_ratio` 由 `xgent_ui::editor::sync_editor_theme` 系统（跑在 `xui::TextEditorUpdateSet` 前）从 `Theme` 同步——编辑器正文字号跟随 `Theme::font_size` 单一可配源（未来接入 settings 即可「跟随系统/用户偏好」调整），行高比默认 1.5（对齐 zed `buffer_line_height = comfortable`）。
- **系统字体**：`xgent_app::startup::load_system_font` 在 Startup 加载 macOS 系统 Menlo 字体（`/System/Library/Fonts/Menlo.ttc`）覆盖 `Assets<Font>` 的 `AssetId::default()`，替代 Bevy 内置 FiraMono——Menlo 度量紧凑、抗锯齿清晰，与 zed/VSCode 视觉一致；非 macOS 静默回退 FiraMono。
- **行高一致性（硬约束）**：编辑器虚拟化渲染（`xui::text_editor::virtual_render`）的定位数学——占位高度 `行数×line_height`、可见区间、Text 节点 `top` 偏移——全部基于 `TextEditor.line_height`（= `font_size × line_height_ratio` 取整）。Bevy 的 `LineHeight` 组件**不从 `Text` 根级联到 `TextSpan` 子节点**（每个 span 自带 required 组件，默认 `RelativeToFont(1.2)`），故虚拟内容 span 与行号列都必须**显式**写 `LineHeight::Px(line_height)`；新 spawn 的 `TextEditor` 初始 `line_height` 也要用同源派生值（见 `xgent_ui::editor::tabs`），否则滚动越深行号与内容错位越大。
- 外部文件变更冲突：未脏静默重载 / 脏弹窗三选（丢弃本地 / 保留本地 / 对比合并）。

### 5.10 检索升级路径（ADR-0010）

- MVP 用方案 A（OnDemand：目录树 + ripgrep + 按需读文件）。
- 编辑器上线只触发到 C（向量 RAG）；D（LSP/AST）延后到 LSP 真正接入；E（混合检索）跟随 D。
- 新增检索实现时实现 `ContextProvider` trait，`build_context_provider` 据配置切换，调用方无感。

**方案 A 的检索约束**（2026-10-14 落地，见 `docs/comet/specs/context-retrieval/spec.md`）：

- **中文分词**：`extract_keywords` 把 CJK 连续串切成**相邻二字组合（bigram）**，单字丢弃。整句作为一个关键词时任何源码都不会包含它，中文提问必然零命中；二字片段正是子串匹配与 rg 字面量能命中的单位。英文按字符数（非字节）≥3 过滤，另有一小张中英虚词表（`CN_FUNC_CHARS` / `EN_STOPWORDS`）作精度过滤。关键词上限 `MAX_KEYWORDS = 16`——bigram 会成倍放大数量，8 会截掉中文问句后半段。
- **rg 用字面量**：`rg_args` 产出 `--fixed-strings` + 每个关键词一个 `-e`，不拼 `a|b` 正则。关键词是用户输入的文本：拼正则时 `.` 变通配、`(`/`+` 让模式非法，rg 失败会触发全量遍历回退。
- **`@file:` 线索**：bridge 把编辑器 `@` 引用作为 hint 传入（形如 `@file:src/main.rs`）。这类 hint 由 `file_refs` 解析后**直接读该文件**，不再当检索词（旧实现拿整串当关键词，任何文件内容都不含 `@file:...`，引用文件等于没引用）。
- **遍历不进 async 线程**：`collect_tree_lines` / `collect_matching_files` 是非 async 纯同步函数，全部 `std::fs` 调用只存在于它们内部，由 `tree_summary` / `fallback_search` 经 `tokio::task::spawn_blocking` 调用（NF-03）。
- **relevance 说真话**：chunk 的 `relevance` 由 `relevance_for` 生成，标明来源角色（当前打开文件 / `@file` 引用 / 关键词检索命中）并列出**内容里实际命中**的关键词，不再是固定文案。
- **Hub 合并后复核预算**：`clamp_to_budget` 按 provider 顺序贪心保留装得进的 chunk，装不下的**整块丢弃**（hub 不截断别的 provider 交来的文件片段），`total_tokens` 由保留 chunk 重算；`tree_summary` 不占 chunk 预算。

### 5.11 上下文面板与文件抽屉（v7，ADR-0014）

- **布局（v7 四列终态）**：`MainAreaMarker`（横向 row）下四子节点——`ActivityBarMarker`（图标轨 52px）+ `ChatPanelMarker`（flex:1 填充剩余）+ 右手柄 + `SideViewMarker`（上下文面板，默认展开 720px）。文件面板已抽屉化（M5-T6）：`FileDrawerOpen` Resource 驱动左侧 overlay（`DRAWER_W=320`、`surface` 底 + 右边框 + 遮罩点击关闭），rail 文件钮 / `filepanel.toggle`(Cmd+B) 切换；点文件条目发 `OpenFileRequest` 走上下文面板预览页（内嵌预览区已删除）。
- **上下文面板三页签**（M5-T1）：`SideViewContent::{None, Editor, Preview, Diff, Terminal}`，`PageTabMarker` 页签条 38px（预览/差异/终端）；Editor/Preview 归一预览页（M5-T2）。差异页（M5-T4）`editor/diff_view.rs`：buffer `TextEditor.rope` vs `EditorBuffer.disk_content` 经共享 `diff.rs::line_diff` 渲染（add=st_ok/del=st_fail 行 tint）。
- **展开/收起**：`SideViewCollapsed` Resource 驱动 `toggle_panel_visibility`；窗口 <1100px 自动收起（`responsive_collapse`）。
- **快捷键**：`Ctrl+\` = `sideview.toggle`；`Cmd+Shift+E` = `editor.view`（切预览页签，M7-T3 重映射）；`Ctrl+`` = 终端页。

### 5.12 面板拖拽调整大小（resize，v7）

- **模块**：`xgent_ui/src/resize.rs`（`ResizePlugin`）。右手柄拖拽改变上下文面板宽度；左手柄已随文件面板抽屉化移除（`ResizeEdge::Left` 删除）。
- **数据驱动**：`PanelWidths`（仅 `side_view`）、`ActiveResize`。`apply_panel_widths` 据资源写面板宽度。
- **钳制**：纯函数 `clamp_side_view(w, available)`（单测覆盖）：下限 `SIDE_VIEW_MIN=380`、上限 = 主区宽 − `CHAT_MIN=520`；启动/缩窗统一钳制（`clamp_on_window_resize`），双击右手柄复位 720（`CONTEXT_W_DEFAULT`）。
- **视觉**：手柄透明，hover/拖拽 → `accent_glow` 底 + 2px `accent_interactive` 竖线。

### 5.12.1 v7 视觉体系（kit/fonts/theme）

- **`kit.rs`（业务层小件库）**：`IconAssets`（`assets/icons/*.png` 白描边 2x PNG，ImageNode 乘法染色）+ `icon()`；`HoverTint` 数据驱动 hover 三态（单一 `hover_tint_system` 全局处理，禁止逐组件写 hover 系统）；`Tooltip`（悬停 500ms 延迟浮现）；构造器 `ghost_button/primary_button/icon_button/pill/kbd/section_label`；`ToastMessage` + `show_toast`（底部居中浮层，`tooltip_bg` 恒暗底，2.2s TTL 自动消隐，全局唯一节点替换式刷新）。
- **`fonts.rs`**：`UiFonts{ui,mono}`（Inter Variable 直读模式 + Menlo，由 `xgent_app/startup.rs` 加载注入）；文本构造器 `ui_text(text,size,weight,color,line_height)` / `mono_text(_weighted)(...)`——**全部业务文本经构造器**（M6-T6 清点后仅 2 处例外：kit section_label 需 LetterSpacing、editor 行号列 editor_theme 动态字号）。
- **`theme.rs`**：v3 字段全量（Linear 令牌：表面明度阶梯 bg/surface/elevated/code_bg、四级文字、靛紫 accent 组、Radix 状态色、warm 陪伴例外、tooltip 反色）；`size` 常量（TOP_BAR_H 52/STATUS_BAR_H 32/RAIL_W 52/CONTEXT_W_* /DRAWER_W 320）；`type_scale` 排版阶梯 8 档；`radius` 圆角阶梯。
- **图标管线**：`docs/design/icons/export_png.py`（cairosvg，`currentColor`→`#FFFFFF` 强制）→ `crates/xgent_app/assets/icons/{name}@2x.png`。

### 5.12.2 上下文条与 welcome（v7）

- **`context_scope.rs`**：会话区顶部只读 chips（数据源=编辑器 tabs 的 `EditorBuffer.path`），「+ 添加」打开文件抽屉；标签/添加钮跨重建保留（签名比对）。
- **`welcome.rs`**：会话空态欢迎页（品牌块 + 标题/副标题 + 最近会话，进空态发 `ListSessionsMessage`，点击 `RestoreSessionMessage` 恢复，首条输入即隐藏）。**不再摆 3 张"快捷卡"**：它们与输入框上方的 qa chips 行同义（都只把提示词填入输入框，对会话的呈现与处理毫无差别），双份入口既冗余又会被误读成"选会话类型"；提示词入口全局只留 chips 行一处。

### 5.12.3 截图工具（ui-snapshot）

`xgent_app/startup.rs`：`XGENT_SHOT=<路径>` 环境变量启动 3 秒后自动截图一次；F12 随时截到 `target/snapshots/`（期验收对照原型用）。


---

### 5.12.4 Bevy 0.20 UI 交互模型（0.19→0.20 适配，详见 `docs/notes/bevy-0.20-migration.md`）

- **可交互节点三件套**：`bevy::ui_widgets::Button`（行为控件）+ `Hovered::default()`（opt-in 悬停跟踪）+（业务 marker）。0.20 的 `bevy::ui::Button` 是弃用 type alias，**不能作值使用**；`bevy::ui::Interaction` 也已弃用，禁止再用。
- **点击判定**：`Query<..., Added<Pressed>>`——`ui::Pressed` 由 `ButtonPlugin`（在 DefaultPlugins）监听指针事件（含子节点冒泡）自动插拔；按下帧即 `Added`，等价旧 `Changed<Interaction> + == Pressed` 的"按下即触发"边沿语义。事件 handler 内部 `propagate(false)`，嵌套按钮（tab 里的 ×）在最内层消费，父按钮不会误触发。
- **悬停判定**：`Query<..., &Hovered>`（`.get()` 取 bool）或 `Has<Pressed>` 轮询。`Hovered` 由 picking 按 CSS `:hover` 后代语义维护——悬停按钮内文本/图标子节点时根节点也为 true（旧 `ui_focus_system` 靠 FocusPolicy 默认 Pass 穿透子节点实现同等效果；**`picking::hover::PickingInteraction` 无后代回溯，不能用于带子节点的交互根**，这是本次迁移踩过的坑）。
- **穿透/阻断**：UI picking 后端忽略 `FocusPolicy`；无 `Pickable` 组件默认阻断下层（抽屉面板压遮罩防误触即依赖此默认），需要放行用 `bevy_picking::Pickable { should_block_lower: false, ..}`。
- **文本输入**：可编辑实体 = `EditableText`（状态载体）+ `TextInput`（行为控件，`TextInputPlugin` 在 DefaultPlugins）成对挂载；编辑器（F-11）为虚拟化只读态，不挂 `EditableText`，文本在 `TextEditor.rope`。
- **文本输入必须挂 `TabIndex`**（`bevy::input_focus::tab_navigation::TabIndex(0)`，硬性）：0.20 的点击聚焦链路是 `PointerPress` → 冒泡 `AcquireFocus` → `PointerFocusPlugin` 的 `acquire_focus_tab_index` 解析，**只对带 `TabIndex` 的实体生效**；缺它时点击会一路冒泡到窗口并被清空（等价"点空白处失焦"），键盘事件经 `InputDispatchPlugin` 投递时无处可去 → 输入框敲字全无反应（设置面板 provider 输入即栽在此）。`AutoFocus` 只在 spawn 那一帧生效，不能替代点击聚焦。回归网：`tests/interaction_model.rs`（机制，含无 `TabIndex` 的反例）+ `tests/settings_input_focus.rs`（开面板→点击→敲字端到端）。
- **对话区空态覆盖层挂消息流，不挂对话主区**：`welcome::spawn_welcome` 的父节点是 `MessageListMarker`（绝对定位 inset 0 铺满 + 不透明底）。挂 `ChatPanelMarker` 会连下方输入卡一起盖住，空态时输入框看不见（原型 `.welcome` 亦是 `.conversation` 的子节点）。覆盖层内**顶对齐而非垂直居中**：下限窗口（1024x640）下消息流盒高仅约 340px，字号放大或最近会话满 3 条时内容会高于盒高，居中会让品牌块溢出顶部被裁，顶对齐只损失底部。
- **后续优化路线**（rem 全局缩放 / 多开焦点 / BSN 与 feathers 采纳评估）见 `docs/notes/bevy-0.20-migration.md` §4。

### 5.13 插件系统（WASM 组件模型）

插件系统已落地：WASM Component + wasmtime 29，插件经 WIT 注册 Agent 工具/命令面板命令/ContextProvider。设计文档 `docs/design/plugin-system-design.md` v3 + 偏差修正（见 `local://plugin-system-impl-plan.md`）。

**crate 拓扑**（3 宿主 + 2 插件）：
- `xgent_plugin_api` — 插件作者面向：`Extension` trait + `register_plugin!` 宏 + WIT 绑定（`wit/plugin.wit`）。rlib，被插件 crate 依赖。
- `xgent_plugin` — 宿主核心：`PluginHost`/`WasmHost`/`WasmPlugin`/`PluginHostProxy` + 清单解析 + `host_state`（HostState + Host trait impl，从 wasm_host.rs 拆出控行数）。不依赖业务 crate（经 proxy 反转依赖）。
- `xgent_plugin_host` — ECS 桥接：`PluginTool`/`PluginCommand`/`PluginContextProvider` 适配器 + `PluginHostPlugin`（`plugin_poll_system`）。
- `xgent_plugin_hello` / `xgent_plugin_git` — 测试/参考插件（cdylib → wasm32-wasip2）。

**关键约束（开发注意）**：

1. **WASM 目标用 `wasm32-wasip2`**（原生 component model，无需 adapter）。偏差修正 2：设计文档 §14 D-P1 "不在此草案定论"，实测 wasip2 + wasmtime 29 + wit-bindgen 0.22 + async + trappable_imports 组合通过。构建：`cargo build -p xgent_plugin_git --target wasm32-wasip2`。

2. **id ↔ schema.name 一致性硬约束**（阻断级）：插件工具 `Tool::id()` 与 `ToolSchema.name` 必须返回完整 id `plugin.<plugin_id>.<short_id>`。`PluginTool::new` 显式覆盖 `schema.name`，不信任插件作者在 schema JSON 里写的 name。否则 LLM 回传的 `tool_call.id` 命中不到 `ToolExecutor`。

3. **cancel 用 Zed 模式，非 `Store::cancel_handle()`**（偏差修正 3）：
   - `WasmPlugin` 持 `tx: UnboundedSender<PluginCall>`，专有 tokio Task 串行处理（独占 `Store::&mut`）。
   - `call_tool_execute` 的 cancel = `tokio::select!` on `cancel_token.cancelled()` vs oneshot；cancel 时**丢弃 oneshot**（返回 `Err(Aborted)`），不主动中断 WASM。
   - `host.run_command` 宿主侧 impl 内部 `tokio::select!` on `cancel_token.cancelled()` → `child.kill().await` + 返回 `command-error::cancelled`。

4. **ECS Message 放 `xgent_agent/src/events.rs`**（偏差修正 4，非 xgent_core）：`CommandResultMessage`/`PluginUnregisterMessage`/`PluginCommandTriggered`/`PluginLoadedMessage`（P5 新增，加载完成通知）用 `#[derive(Message)]` + `add_message::<E>()`。`plugin_poll_system` drain `PluginEvent`（含 `CommandResult`/`Unregister`/`Loaded` 三变体）转对应 Message。

5. **proxy 取用机制（硬性，§13 Step P4）**：proxy impl 不在 `PluginHostPlugin::build` 时抓 `ResMut`（跨 tokio task 持有破坏 Send+Sync）。改为持 `Sender<PluginOp>` 发指令到 `PluginOpQueue`，`plugin_poll_system`（主线程）内 `world.resource_mut::<T>()` 执行。

6. **`ToolExecutor` 改为 interior-mutable + `Resource`**：`tools: RwLock<HashMap>`，`register`/`remove_by_prefix`/`schemas` 改 `&self`。`ToolExecutorResource(Arc<ToolExecutor>)` 包装让 World 与 `AgentBridge` 共享同一实例（bridge 持 Arc，World 持 Resource(Arc clone)）。

7. **`ContextHub` impl `ContextProvider`**：`xgent_app` 把 `Arc<ContextHub>` 作为 `Arc<dyn ContextProvider>` 注入 `AgentBridgeConfig.context`，agent 侧无感于内置 vs 插件 provider 区分。`ContextHub` 用 `RwLock<Vec<Arc<dyn ContextProvider>>>`，`retrieve_all` 先 clone Arc 出锁再 await（避免持有 guard 跨 await 破坏 Send）。

8. **`handle_palette_triggers` 路由 `plugin.` 前缀**（偏差修正 5）：`id if id.starts_with("plugin.")` → 发 `PluginCommandTriggered` Message，`plugin_poll_system` 消费后 spawn async `PluginCommand::run`，结果经 `PluginHost::emit_command_result` → `PluginEvent::CommandResult` → `CommandResultMessage`。

9. **启动时序（硬性）**：`App::new` → 业务 Plugin（XgentTools/CommandPalette/XgentContext）→ `PluginHostPlugin`（build 时 `register_proxy_impls` 已 set proxy）→ `load_builtin_plugins`（spawn 到 tokio runtime）→ run。proxy 注册在 `PluginHostPlugin::build`，此时 ToolExecutor/CommandRegistry/ContextHub 已就绪。

10. **构建插件**：`./build_plugins.sh`（debug）或 `./build_plugins.sh release`。打包到 `crates/xgent_app/assets/plugins/<id>/`（plugin.toml + extension.wasm）。首次启动 `load_builtin_plugins` 检测 `installed/` 为空时从 `assets/plugins/` 预装。

11. **wit-bindgen 0.22 约束**：guest 侧 `wit_bindgen::generate!` 必须在 crate 根（不在 `mod wit` 内），否则 `export!` 宏无法在 crate 根解析。host 侧 `wasmtime::component::bindgen!` 生成 `Plugin` world struct，`add_to_linker` + `instantiate_async` + `call_*_async`。WIT `result<_, E>` 用下划线表示 unit ok（非 `()`）。

12. **WASM 引擎错误传播**（review S1）：`wasm_engine()` 返回 `Result<&'static Engine, WasmCallError>`，用 `LazyLock<Result<Engine, WasmCallError>>` 持存（非 `OnceLock + expect`，AGENTS §5.7 库代码避免 expect；稳定 Rust 无 `get_or_try_init` 故用 LazyLock）。

13. **`InFlightGuard` RAII 计数**（review S4）：`WasmPlugin::dispatch` 用 `InFlightGuard`（构造 +1，Drop -1）管理 in-flight 计数，避免错误路径手写 `fetch_sub` 遗漏。`map_result_string` 统一双层 `Result<Result<T,String>, wasm::Error>` 映射。

14. **`SchemaVersion` newtype + tier 校验**（review S7S8/P3）：`PluginManifest.schema_version: SchemaVersion(i32)` newtype + `SUPPORTED: RangeInclusive` 常量。`validate` 校验清单 tier ∈ {read,write,exec}，`ui_only` 加载拒绝（§5.3.2 插件不支持 UiOnly）。`validate_ids` 批量校验去重。

15. **`push-update` + `on_update` 桥接**（review P1）：WIT host 接口加 `push-update(tool-id, update: string)`，`HostState.push_update` 持回调，`call_tool_execute` 接收 `Option<Arc<dyn Fn(String)>>`，execute 前 set、后 clear。`PluginTool::execute` 桥接受限（`ToolUpdateCallback` 是 `Box` 非 `'static`，且 ToolExecutor MVP 总传 None）——基础设施就绪，待 `ToolUpdateCallback` 改 `Arc` 后接通。

16. **`summarize` WIT 接口**（review P2/S11）：WIT tool 接口加 `summarize(tool-id, input) -> string`，`Extension` trait 加默认实现，`WasmPlugin::call_tool_summarize` 暴露。`PluginTool::summarize` 因 `Tool::summarize` 是同步 trait 方法而 WIT 是 async，MVP 保留本地默认（接口已留待 summarize 改 async 后接通）。

17. **enable/disable/install_dev + reload 过滤**（review P4/P5）：`PluginHost` 持 `enabled: Mutex<BTreeMap<String,bool>>`，`reload_all` 跳过 disabled。`enable_extension`/`disable_extension`（设 enabled + reload/unregister）、`install_dev_extension(src, id)`（复制源码目录到 installed/，跨平台稳定替代 symlink）。`stop_file_watcher` 显式停止 watcher 线程。

18. **Git 插件补 git_commit/git_history**（review P6）：`xgent_plugin_git` 现提供 4 工具（git_diff/git_log/git_status read + **git_commit write**）+ 4 命令 + `git_history` ContextProvider（`retrieve` 调 `git log -n20`）。

19. **YAGNI 移除 http-get 占位**（review S9）：WIT `host.http-get` 与 `HostState::http_get` 永远返回未实现的占位已移除（需时再加，避免 Speculative Generality）。

20. **沙箱路径穿越修复**（深度 review D1/D3）：`HostState::resolve_and_check` 三重防护——拒绝含 `..` 组件的输入路径（防 `../` 穿越沙箱边界）、绝对路径须在 project_root 内、`canonicalize` 解析后路径（跟随 symlink）再 `starts_with` 校验。`Path::starts_with` 是字面组件前缀匹配不规范化 `..`，故必须显式拒绝 + canonicalize。

21. **run_command cwd 权限校验**（深度 review D2）：cwd 经 `resolve_and_check` + `fs-read` 权限校验（此前 cwd 无校验，可逃逸项目根）。`command` program 仅字符串相等校验，`args` 无白名单（argv 注入面，设计 §9.2 已知限制）。

22. **install_dev 用 symlink**（深度 review P5）：`install_dev_extension` 优先 symlink 指向源码目录（跨平台 `symlink_dir`，Windows 需开发者模式），失败回退复制 + warn。源码改动自动反映到 `installed/`，dev 热重载生效。

23. **已标注的 MVP 限制**（深度 review，设计文档 §10.4/§14 D-P7~D-P9）：配置不热更新（`HostState.config` 快照，需 reload）、WASM 纯计算死循环致 task 永驻（`drain_pending_drop` 不 kill task）、on_update 流式回灌未接通（`ToolUpdateCallback` 签名限制）、PluginOp channel 无背压（proxy trait 同步约束）。

24. **全面评审修复（2026-09-27，10 轮）**——契约级变更须知：
   - **`provider.listModels` 支持可选 override 参数**（`kind`/`api_base`/`api_key`）：设置面板用编辑中的草稿凭据探测模型列表，daemon 临时构造 provider 直连、不写配置不进池（此前经 `config.write` 永久落盘草稿）；向后兼容（无 override 走池缓存）。
   - **`peer.fileChanged` 通知参数为完整 `FileChanged`**（`project_root`/`path`/`kind`）：UI 统一按 `FileChanged` 反序列化，此前 `{"path"}` 会被静默丢弃、多开同步失效。
   - **`xui::EditorActive` 组件约定**：多 tab 宿主须在激活 buffer 实体上增删该标记，编辑器快捷键（Cmd+S/Z/F/H）只作用于带标记者（`xgent_ui::editor::rebuild_editor_tabs` 维护）。
   - **UI 进程 Startup 订阅 `fs.watch`**（`xgent_app::startup::open_project`，经 bridge runtime spawn）：daemon 只向订阅者推 `fs.changed`，漏订阅则编辑器冲突检测收不到任何通知。
   - **agent loop 停等期迟到 StartLoop 上返回顶层循环**（`bridge.rs::run_agent_loop` 返回 `Option<(ChatRequest, editor_queries)>`）：ECS Done 置 Idle 后的新输入走 StartLoop，旧实现在外层 `recv()` 把它吞掉、会话永久卡 Thinking。
   - **Anthropic 并行 tool_use 按 index 聚合**（`HashMap<u32, ToolUseAccum>`）：单累积器会覆盖第一个工具调用的 args、其 `ToolCallEnd` 永不发射。
   - **OpenAI 流 Done 延迟到流末尾发射**（`StreamState::finish`）：`include_usage` 的 usage chunk 在 finish_reason 之后到达，旧的 finish 即发 Done 使 usage 恒 0。
   - **PTY `Exited` 由独立 wait 线程权威上报**（`local_pty.rs`）：shell 退出但 grandchild 持 slave 时 reader 永不 EOF，仅靠 reader EOF 检测会漏报 Exited；且多线程 wait 同一 child 会 ECHILD 丢退出码——reader/Kill 路径均不再 wait。
   - **配置/编辑器写盘均原子化**（同目录临时文件 + rename）：`GlobalConfigStore`/`ProjectConfigStore`/`xgent_ui::editor::io`/`write_file` 工具；崩溃断电不再产生截断 TOML（曾会致 daemon 拒绝启动）。
   - **确认弹窗键盘守卫**（`confirm_dialog.rs`）：焦点在 `EditableText` 或终端视图独占键盘时，Enter/Esc 不触发弹窗决策（否则聊天输入回车会静默放行工具调用）。
   - **安全策略动态升级生效**（`security.rs::resolve_policy`）：配置 approved 后仍执行 `approval_for(input)`，危险命令（`rm -rf`/`sudo`）动态 tier 升级会退回 `NeedsConfirmation`（`ToolTier::severity()` 定义危险度序）。

25. **评审回归修复（2026-09-28，第 2 批 3 轮）**——对第 24 条修复的回归审查 + 低覆盖模块补审：
   - **run_command 截断下溢**：截断判据是字节数而头尾保留按字符数，CJK 大输出触发 `total - keep` usize 下溢（debug panic 会打死 agent 任务）——加字符数双重判据；非 Unix 击杀退化为 `start_kill`（原 no-op 会让取消/超时分支 `wait()` 永久挂起）。
   - **确认弹窗键盘语义定稿**：spawn 时 `InputFocus` 移到弹窗、决策经 `close_dialog` 统一还原到 chat 输入框；**Esc 恒放行**（Deny 是安全方向，误拒无害误放行有害），Enter 保留 EditableText/终端守卫。
   - **IPC 健壮性**：daemon 对无法解析的行回 `PARSE_ERROR/-32600` 错误响应（原静默丢弃，调用方永久挂死无痕迹）；UI `call()` 等响应限时 120s（> provider 最长请求超时）。
   - **杂项**：registry 新增 `unsubscribe_project`（fs.watch 回滚只删本项目，不 clear 全部订阅）；lifecycle 计时器落地前锁内复查 `client_count==0`（防活跃会话被 30s 退出计时杀掉）；anthropic message_delta 兜底 drain 按 index 排序（HashMap 迭代序随机）；fs_watcher 注册成功后复查订阅集（补偿与 unwatch_client 的并发窗口）；provider_client 消费 task 加 180s 空闲兜底 + 事件反序列化失败 warn（版本偏差丢 Done 时不再滞留）；UI 全局配置损坏时打 error 日志（原静默回退默认，与 daemon 拒启行为不一致）；`preferences.language` 的 config.changed 跨窗口同步到本窗口 Localizer。

26. **Bevy 0.20.0-rc.2 适配（2026-10-05，经三轮自审修订）**——本地 `../bevy` 升 0.20.0-rc.2，破坏面 100% 集中在 UI 交互模型（其余 crate 零改动零警告，xui 隔离层设计得到验证）：
   - `bevy::ui::Button`（0.20 变弃用 type alias，48 处编译错误）→ `bevy::ui_widgets::Button`（20 文件显式导入压过 prelude 弃用导出；附带 a11y `Role::Button`）。
   - `bevy::ui::Interaction`（弃用，145 处警告）→ 官方 **`Hovered` + `Pressed`** 双组件：48 个 Button spawn 点补挂 `Hovered::default()`；点击查询改 `Added<Pressed>`（按下即触发，等价旧边沿语义）；悬停查询改 `&Hovered`/`Has<Pressed>` 轮询。`Pressed` 由 `ButtonPlugin` 经冒泡指针事件自动插拔（嵌套按钮在最内层 `propagate(false)` 消费，父按钮不误触发）。
   - **踩坑记录**：初版曾迁到 `picking::hover::PickingInteraction`（API 形状与旧 Interaction 同构），但该组件只写 hover map 的**最顶层命中实体**、无后代回溯——xgent 按钮几乎都带文字/图标子节点，悬停子节点时根节点状态不更新（旧 `ui_focus_system` 靠 Node required 的 `FocusPolicy` 默认 **Pass** 穿透子节点）。官方 `Hovered` 按 CSS `:hover` 后代语义维护（命中实体+祖先集合），才是带子节点交互根的正确状态源。
   - 删除 `FocusPolicy::Block`（0.20 UI picking 后端忽略 FocusPolicy，无 `Pickable` 默认阻断即等价）；3 个文本输入框（chat/palette/settings）`EditableText` 旁补挂 `TextInput`（0.20 起输入行为挂在 TextInputPlugin）；tests/file_preview 模拟点击同步换新模型。
   - 策略与后续优化路线（rem 缩放、多开焦点、BSN/feathers 采纳评估）沉淀于 `docs/notes/bevy-0.20-migration.md`，交互模型速查见 §5.12.4。
   - **功能性 review 补充（2026-10-07）**：① 发现 0.20 指针事件从非 Button 子实体**冒泡到 Button 祖先**（全局 observer 逐级运行）——抽屉面板点击会误触遮罩关闭，修复为 `kit.rs::block_press_bubbling()`（`ui_widgets::observe` 实体作用域 observer 消费 `PointerPress`，等价旧 FocusPolicy::Block；**裸 `Observer` 组件是全局观察者，严禁直接插**）；② resize 手柄拖拽启动加 `hovered` 门控（窄手柄移出后 `Pressed` 残留防御）；③ 冒烟测试抓出 `handle_confirm_keyboard` 的 `Res<InputFocus>` + `ResMut<InputFocus>` B0002 重复访问（既存缺陷，0.20 起启动即 panic），合并为单一 `ResMut`；④ 新增 `tests/interaction_model.rs` headless 交互契约测试（6 用例：后代悬停/按下冒泡/嵌套消费/面板压遮罩 ×2/同帧边界观测）——0.20 交互模型回归金丝雀。

27. **Bevy 0.20.0 正式版切换（2026-10-11）**——`../bevy` 从 rc.2 检出切到上游 `v0.20.0` tag（`b3bdd75be Release 0.20.0`），path 依赖形态与 9 个 feature 名（keyboard/system_font_discovery/system_clipboard/track_location/ui/bevy_ui_widgets/default_font/bevy_dev_tools/serialize）全部沿用不变：
   - **零代码适配**：rc.2→0.20.0 仅 13 个 commit，UI 侧唯一改动是弃用别名 `Interaction` 由 `pub(crate)` 转 `pub`；xgent 不碰 `replace_related`/bevy_image saver/BSN 宏/自定义 shader，`cargo check --workspace --all-targets` 直接 0 error 0 warning。命中 `docs/notes/bevy-0.20-migration.md` §4.5 的预判（正式版破坏面远小于 rc 轮）。
   - **唯一阻塞是工具链**：bevy 0.20.0 `rust-version = 1.97.1`，rustc 1.97.0 会被 cargo 以 `rustc X is not supported by the following packages` 拒编（依赖图解析阶段就失败，不是编译错误）。本机已升到 stable 1.99.0。
   - **邻仓库操作注意**：`../bevy` 是共享工作区，本次发现其中残留一个未完成的 interactive rebase（19 个 0.19.0 release 提交变基到某 Solari 提交）。动它之前先 `git status` + 读 `.git/rebase-merge/{orig-head,onto}` 判断有无独有工作，再 `rebase --abort`（可经 reflog 找回）→ `fetch upstream --tags` → `checkout --detach v0.20.0`。
   - **验证结果（rustc 1.99.0）**：`cargo check --workspace --all-targets` 0 error 0 warning；`cargo test --workspace` 46 个测试二进制 **537 passed / 0 failed / 1 ignored**；`cargo clippy --workspace --all-targets` 通过，残留 **8 处同类警告**——`#[must_use]` 无 message 且返回类型本身已是 must_use（xgent_agent `bridge.rs`/`compaction.rs`、xgent_context `provider.rs`、xgent_provider `provider.rs`、xgent_terminal `backend.rs`、xgent_tools `executor.rs`/`mcp.rs`/`tool.rs`），属 clippy 1.99 新增 lint 面、与 bevy 0.20 无关，本轮未处理。
   - **新发现的操作陷阱**：`cargo fmt --all` 会连带格式化 path 依赖 `../bevy` 的源码（实测报出 `bevy_pbr/src/render/mesh.rs` 2 处 diff），不带 `--check` 直接跑会改写那个上游检出；`cargo fmt` 与 `cargo fmt -p <crate>` 实测 0 diff。以后统一用 `cargo fmt -p <crate>`（已写进 `AGENTS.md` 第 3 节）。
   - **冷重建成本**：换 rustc 版本会让整个依赖图缓存失效——本轮 `cargo check` 13m39s、`cargo test`（含真码生成与链接）约 70 分钟。评估升级代价时按这个量级算。

28. **搜索能力与运行参数（2026-10-14，缺口 R3-3 / R3-17）**：
   - **坏参数是错误，不是零结果**（`xgent_tools::search_files`）。改前只支持子串，模型写 `"(foo"` 之类的模式会拿到「未找到匹配」，于是断定代码不存在并给出错答。现在 `regex` / `glob` / `file_type` 三者任一非法，都在**遍历开始之前**返回 `is_error` 且错因点名是哪个参数、为什么非法。判据写进工具描述里，因为选参数的是模型。
   - `regex` 是**逐行**匹配、默认关闭（`regex: false` 保持字面量子串，既有调用与测试行为不变）。用 `regex` crate（1.13.0 已在 `Cargo.lock` 里，作直接依赖不增加编译量）而非自写引擎；lookaround/反向引用不属于该引擎语言，报同一类非法错。
   - `glob` 自建翻译（`*` 不跨 `/`、`**` 跨、`?` 单字符、`{a,b}` 分支，其余字面量转义），**不含 `/` 的模式只匹配文件名**——`"*.rs"` 在任意深度命中，这是 ripgrep 的 `--glob` 约定，也是用户写下它时的预期。不引 `globset`。
   - `file_type` 用 ripgrep `--type` 的名称词表（`rust`/`python`/`cpp`…，一个名称覆盖多个扩展名）。未知名称报错并列出支持集合；合法名称但工程内无该类型文件是**正常空结果**，不是错误。
   - 被筛掉的文件不读、也不占用 `MAX_RESULTS`；忽略目录、单文件 1MB 跳过、深度上限、取消边界全部不变。
   - **台账点名的 13 个运行限额全部转配置**（14 个键）：见 §4.5.1。核心不变量是「缺省值逐项等于它替换掉的常量」，因此不写配置的工程行为等价——这条由 serde 测试当作数据断言，而不是靠人读 `default` 函数。
   - 加新参数请沿 `xgent_app → ToolParams / RetrievalLimits / AgentBridgeConfig → 构造函数注入` 这条路；工具与检索 crate **不 import 配置节类型**，`Tool` / `ContextProvider` 契约与配置层保持解耦（`ToolExecutor::with_defaults()` 仍可用，它取 `ToolParams::default()`）。
   - **本轮查实的一处台账低估**：R3-21 记「编辑器查找替换仅字面匹配」，实际 `xui::FindState` 在 `xgent_ui` 里**零消费者**——查找替换不是「只支持字面量」，而是产品里根本不可达。下一条变更应按"接 UI + 正则"估工作量，别按"加个开关"估。

29. **设置面板与配置热应用（2026-10-16，缺口 R3-13）**——本轮踩到的都是"看起来对、其实错"的形状，逐条记：
   - **显示的生效值不能当作用户填的值写回去**（真缺陷，由 `untouched_fields_produce_no_writes` 抓出）：未设置的数值字段要显示内置缺省（4、8000…），于是"打开面板 → 点保存"会把显示值写成配置。判据：`stored == 0 && parsed == effective` 视为未改动。脏检查的基准必须是**存储值**，不是显示值。
   - **只写脏键顺带保住凭据**：api_key 框回填已存值，用户没碰就不发写请求——此前"面板重开是空框 + 保存发全部字段"的组合会把已存 key 清空（`empty_api_key_field_does_not_wipe_the_stored_key` 钉住）。
   - **读锁守卫不能跨 `await`**（`ToolExecutor::execute` / `parallel_safe`）：`match resolve_policy(.., &ctx.tool_policy.read())` 的临时守卫活到整个 match 结束，里面还 `await` 确认，future 直接不 `Send`。写成 `let resolved = …;` 让守卫在语句末尾释放。`parking_lot` 的守卫不是 `Send`，这条编译期就会炸，但写法上是很容易顺手犯的错误。
   - **任何写全局配置的 daemon 测试都必须先隔离落盘路径**（这条是本轮独立验证者抓出来的高危缺陷）。`ConfigCoordinator::write` 是真落盘到 `~/.xgent/agent/config.toml` 的：`config_store.rs` 那批既有测试用 `with_config` 构造却没有隔离，本轮新增的 `session.rs` provider 池两条测试同样漏了——于是 `cargo test -p xgent_daemon`（含 Runtime 的形式化检查）把开发机的配置改写成了测试夹具（`providers.myai / old.example.test / sk-old`），且不可恢复。现在的做法是 `src/test_util.rs::isolate_agent_dir()`（进程级临时目录 + 串行锁，幂等、不随 Drop 复原），全局写入类测试一律先调它，并由 `global_write_lands_inside_the_isolated_dir` 钉住「隔离确实生效」。教训比「记得加隔离」更硬：**测试的副作用越界到用户目录时，全绿不代表安全**；而「跑前跑后 md5 一致」这类自检必须对**最终候选**再跑一遍——我只在加完 `config_store` 隔离后验过一次，之后新加的测试又把它打破了。
   - **headless 装配里 Bevy UI 不产出子节点坐标**：没有 `UiRoot` 时面板根量不出尺寸（实测 `ComputedNode` 全是 `0×0`，所有子节点 `border_box().min` 相同），picking 命中谁取决于 spawn 顺序。因此显示类断言一律读组件（`EditableText` / `SettingsNumInput`），键盘派发测试直接把 `InputFocus` 设到目标输入框。为免「删掉 `TabIndex(0)` 仍全绿」这种空档，新增 `every_settings_input_carries_tab_index` 锁组件契约（删 `TabIndex` 必失败，变异反证 M29 已证），点击机制本身由 `tests/interaction_model.rs` 覆盖；真机那条完整的「点一下就能打字」只能人肉验，**别把它写成已被自动测试保护**。旧的 `settings_input_focus.rs` 原本就是靠几何塌陷的巧合通过的，面板一变形它就炸——**别把 headless 布局当可用前提**。
   - 滚动容器用 `xui::scroll_area::ScrollArea`（与对话区/终端同一套）。`ScrollArea` 自身是 `Bundle` 且含 `Node`，再补一个 `Node` 会 panic（bundle 组件重复）。
   - `ConfigScope::Project` 现在真实现，`project_root` 走 `#[serde(default)]` 的 `Option`：**新增协议字段一律带 default**；`Option` 字段缺省时 serde 本就填 `None`（default 是写明而非前提），但加**非** `Option` 字段时不带 default 会让旧客户端整条请求反序列化失败。
   - apply 走"整份快照重算"而不是逐键增量（见 §4.5.2）：漏算 `context_window` 这种半套更新是增量写法的固有风险，配置只有十几项，整体重算更便宜也更不容易错。
   - **Bevy 的 `is_changed()` 不能当"数据真的变了"用**：`ResMut` 的 `deref_mut` 无条件标脏，只要某个系统拿到 `ResMut` 并写了一次，下一帧所有看 `is_changed()` 的系统都会被叫醒。面板重渲染因此改成比较 `tick` **增量**（`Local<u32>` 记住上次渲染到哪个计数），app 侧在有键被拒时连那份资源都不碰。判据：**跨系统的"该不该重画"信号要是一个可比较的单调计数，而不是变更标记**——变更标记会随着任何一次触碰误报，而且误报得完全合法。
   - **"生产者单测 + 消费者单测"覆盖不了一次跨系统行为**：A9 连输两轮，两轮的原因都是两边各自绿、合起来的帧序没人验。最终修法是把 `drain_save` 拆到不依赖 IPC/bridge，再与真实的 `SettingsPanelPlugin` 装进同一个 `App` 端到端跑。配套的反证也必须成对：本轮 M40（面板闸门退回 `is_changed()`）只有面板侧用例抓得到，M41（app 不看 `failed` 就推进 `tick`）由集成用例与计数用例抓，少一边就漏一边。

### 5.14 文件改动检查点与撤销（R3-11）

agent 写过的文件此前**无法退回**：编辑器内的 `UndoStack` 只管缓冲区键入，`write_file`/`edit_file` 留在目标旁边的 `.xgent-bak` 没有任何读取方。现在由 `xgent_tools::checkpoint::CheckpointStore` 承担：

- **落点**：`<project>/.xgent/checkpoints/`，`turns/<会话号>/<轮号>.json` 是清单（撤销的单位），`blobs/<会话号>/<轮号>-<序号>-<路径哈希>.bin` 是改前内容。`.xgent` 本就被检索层忽略，检查点不会回灌进上下文。
- **打点时机**：`ToolExecutor::run_tool` 是唯一咽喉——三条执行路径（自动批准 / 本次允许 / 会话允许）都经它，**写前**调 `snapshot_before_write`、写入成功后调 `finalize_after_write` 回填「agent 写进去的那份」哈希。新增写文件工具时必须实现 `Tool::write_targets`，否则它的改动天然不可撤销（read/search/run_command 都返回空，属预期）。
- **轮次归属**：`run_agent_loop` 外层每进一次调 `begin_turn(session)`，会话号取 `SessionId` 的**数值**（`conv.id.0`）经 `AgentCommand::StartLoop` 传入——不能用 `session:N` 字符串反解，既有 `restore()` 会把它解析成 0。
- **只撤「真正记到东西的一轮」**：清单在首次记到条目时才创建，纯问答或只读工具轮次不留痕，撤销指针不会停在空轮上（否则命令面板点了没反应）。
- **不覆盖别人的工作**：撤销前比对当前内容与记录的写入后哈希；对不上（用户改过、被 git checkout 过）就**跳过并报名**，其余文件照常恢复；用户删掉的文件不重建。跳过原因分四类：已被外部修改 / 已被删除 / 改前内容丢失 / 无法确认归属。
- **降级方向**：快照记不下时**不阻塞写入**（撤销机制不能反过来弄坏主功能），但该轮被记入 uncovered，撤销汇报如实说明「有 N 个文件无检查点」，绝不谎报「没有可撤销的改动」。
- **保留**：每会话 20 轮，超出在**新清单第一次落盘后**淘汰最旧（挂在 begin_turn 上会差一轮——刚要写的那轮不该算进窗口）。blob 名必须带条目序号，否则同轮同文件两次写入互相覆盖，撤销只能退到倒数第二个状态。
- **入口**：命令面板 `agent.undo-changes`（`try_register`，非 `plugin.` 前缀）→ `undo::request_undo` 只在 `Conversation.status == Idle` 时经 `AgentBridge` 自带的 tokio runtime 执行（**不要**改用 `EditorIoRuntime.handle`：它在 `xgent_app` 里从未注入，依赖它撤销会永远报「不可用」）；汇总走 toast，有跳过/未覆盖/写盘失败时再落一条常驻提示（toast 只活 2.2s）。撤销**只改磁盘**，打开中的 buffer 沿用既有 fs-watch：干净重载、脏的进冲突对话框，不再开第二条重载通路。
- **边界（必须如实告诉用户）**：`run_command` 的磁盘副作用（`sed -i`、`git apply`、`cargo fmt`）与插件 `host.write_file` 的写入**不在覆盖范围内**——只要产出常驻提示就随文案带上这句（`undo-coverage-note`），不让用户以为「agent 做过的事都能退回」；不做 redo，不给 agent 自助撤销的工具（那会覆盖用户同期编辑）。列表/括号/冒号一律走语料（`undo-list-sep`、`undo-skip-item`、`undo-part-with-list`），**别在 Rust 里硬编码全角标点**，否则英文渲染成 `c.rs（changed externally）`。

`.xgent-bak` 的产生已随本机制移除（`atomic::backup_existing` 删除）：单槽位会被下次写入覆盖、无人读取、还污染用户项目的 `git status`。崩溃安全性反而更好——改前内容先于目标写入落盘。用户项目里历史遗留的 `*.xgent-bak` 不做自动清理（删用户的文件不是本机制的决定）。

### 5.16 工具并行执行（R3-15）

`Tool::concurrency()` 自 ADR-0007 起就有 `Shared`/`Exclusive` 两值，但**全仓没有任何读取方**：agent 内层 tool-call 循环把 `outcome.tool_calls` 一个个 `await` 完。现在它成了真判据，落在三处：

- **资格判定 `ToolExecutor::parallel_safe(tool_id, input, ctx)`（xgent_tools）**：三条**互相独立**的信号全过才允许重叠——① 解析出的策略不需要用户确认（`Approved`，或 `NeedsConfirmation` 但该工具已进会话 AllowAll 集合）；② 工具自报 `Concurrency::Shared`；③ `tier()` **恰好**是 `Read`。**③ 承担两件事**：其一，`Tool::concurrency()` 的 trait 默认值就是 `Shared`，只信②等于让忘了声明 `Exclusive` 的写工具自动进并发组；其二，`UiOnly` 被刻意排除——`resolve_policy` 对 `UiOnly` 是**无条件放行**的（不弹窗），收进来就等于让已发货的 `editor.*` 在用户什么都没配的默认配置下就成组并发，而 `open_file → goto` 是有顺序语义的。① 的原因是确认通道只有一个槽位（`bridge.rs` 的 `SharedConfirm` 持 `Mutex<Option<Sender>>`），两个并发请求会互相覆盖 sender，被覆盖那次只能报「确认被取消」，用户决策就此丢失。判定只是**建议**——策略、弹窗与检查点仍只在 `execute` 里发生，并行是调度层，不是第二套执行路径。
- **分组调度（`bridge.rs::run_agent_loop`）**：从当前下标起取**连续**可并行的区间（长度钳到 `max_parallel_tools`），取不到就说明这里是屏障——单独跑完再继续。**不重排调用去凑并行**：模型先要的写必须先在前面执行。同组用 `futures::join_all` 在**当前任务里**轮询而不是 `spawn`，这样 loop 被取消或提前返回时不会留下后台任务继续写文件。
- **顺序不变是硬要求**：`ToolResult` 事件与回灌进 `req.messages` 的结果都按模型给出的**原始下标**发，先跑完的不抢先——组完成才是结果可见的粒度。每个 `call_id` 恰好一个 result：同组已启动的成员各有真实结果，因中断而根本没启动的分组补占位（对齐 max_tokens 截断路径）。

配置：`[agent] max_parallel_tools` 缺省 `4`，`0` 表示未设置 → 落回 4（**不是不限**：这个闸是拿去挡子进程/socket/fd 数量的）。经 `xgent_app` → `AgentBridgeConfig` → `run_agent_loop`，工具 crate 不 import 配置类型，与 §5 注意点 28 的解耦方式一致。

**默认配置下这条能力是惰性的**：多数工具默认 `NeedsConfirmation`（`AGENTS.md` §5.4），待确认即不够资格；`UiOnly` 是策略自动放行的例外，靠的正是上面③把它排除。于是不改配置就与串行时代逐字相同，用户把只读工具提进 `approved`（或点了 AllowAll）之后并发才出现。

测试判据（照做别退回耗时断言）：并发用**在跑数**（`AtomicUsize` 进入 +1、退出 −1、记历史最大）+ **有界等待**观测——探针要求"至少看到 N 个同时在跑"，串行调度下必然超时、观测值停在 1，于是"调度器退回串行"会被直接抓到。总耗时判据既 flake 又能被"串行但很快"骗过，别用。`bridge_tests.rs` 的 `ProbeTool` 是这套观测的实现。

### 5.17 UI 分层与模态（v8 M1）

v8 M1 把「浮层放在哪、谁吃点击、Esc 关谁」从散落实现收敛为四个原语：

- **六静态层根**（`layout.rs`）：`BASE 0 / DOCK 100 / DRAWER 200 / DROPDOWN 300 / TOOLTIP 500 / TOAST 600`，常量在 `theme::z`。业务浮层只往层根挂，`MainArea`/`ChatPanel` 的 `Overflow::clip()` 不再裁到 tooltip。`GlobalZIndex(` 只允许出现在 `layout.rs`（BASE）、`kit.rs`（`layer_root`/`drawer_z`/toast）与 `modal.rs`（栈深派生）。
- **拾取白名单**（`picking.rs` + `xgent_app`）：`UiPickingSettings { require_markers: true }` 后，没有 `Pickable` 的节点对拾取完全惰性——层根与布局容器不再吞点击。可交互叶子（`Button`/`EditableText`）由 `attach_pickable_to_interactive` 自动补 `Pickable`；**新增自定义可点组件时必须自己带 `Pickable`，否则点不动**。
- **模态栈**（`modal.rs`）：`ModalPush`/`ModalPop` + `Dismiss{EscOnly,EscAndScrim,NotDismissable}`；z = `400 + depth*8`；出栈还原 `InputFocus`。`shortcuts.rs` 在栈非空（或本帧 Esc 被消费）时整体让位，`chat.abort` 不再穿透。设置面板监听 `ModalDismissRequested{id:"settings"}` 自行关；确认弹窗是 `NotDismissable`，Esc=拒绝由它自己处理。
- **抽屉统一裁决**（`overlay.rs`）：rail 文件钮、「+ 添加」、`filepanel.toggle` 都只发 `DrawerToggle`，由 `OverlayRegistry` 单一系统决定开谁关谁，`FileDrawerOpen`/`SessionHistoryState` 降为派生视图。

测试注意：模态相关 Message 用 `Option<MessageWriter<...>>` 取用——不装 `ModalPlugin` 的 headless 测试不会 panic，也不会重复注册消息。Esc 不能直接 `ButtonInput::press`（PreUpdate 会清掉 `just_pressed`），测试应写 `KeyboardInput` 消息。

### 5.18 主题权威与令牌收敛（v8 M2a）

- **ThemeHandle 是唯一权威**（`theme.rs`）：`ThemeHandle { theme, generation }`；`Themed { role, generation }` 挂在 BASE 六个表面节点上；`repaint_on_theme_change` 在 generation 变化时按角色重绘并同步 `Theme` 快照（旧 `Res<Theme>` 调用点不改签名）。
- **单一构造入口**：`Theme::dark()` 为 `pub(crate)`，生产代码不得现场构造；`px()` 只在 `theme.rs` 定义。
- **尺寸/圆角令牌**：区域尺寸走 `theme::size`（含 `MSG_MAX_W/OVERLAY_*/MODAL_MIN_W/HANDLE_W` 等），圆角走 `theme::radius`；`Val::Px(<数字>)` 只允许 0/1/2 这类装饰值。
- **最小宽度单源**：`resize.rs` 引用 `size::CONTEXT_W_MIN/CHAT_MIN/HANDLE_W`。
- **色板**：星形与 `Color::WHITE` 已入 `theme.star/on_accent`；ANSI 表是显式白名单（终端协议色，不参与主题）。
- **窗口**：主窗口 `WindowResolution::new(1280, 800)`（逻辑像素）。
- M2b 已闭合（密度档位与 hover 量化见 §5.19）。

### 5.19 密度档位与 hover 量化（v8 M2b）

**密度档位**（`preferences.density` = `comfortable` 缺省 / `compact`）：

- **权威链**：`GlobalConfigRes.preferences.density` → `sync_theme_from_config` → `ThemeHandle.density`（与主题共用一个 `generation`）。偏好一变，下一帧全界面按新档重排，不重建节点、不重启。
- **实现是「间距属性写回」而非改调用点**（`density.rs`）：`DensityBase` 记下每个 `Node` 的基线 `padding/margin/row_gap/column_gap`；`capture_density_bases` 对新节点记基线并立即按当前档写回，`apply_density_on_change` 在 `ThemeHandle` 变化时按「基线 × 系数」重写。写入始终以基线为源，所以**重复应用不累乘**，切回舒适即逐像素还原。
- **只缩间距**：`Val::Px` 参与换算，`Percent`/`Auto`/0 原样；字号、图标、`Node.width/height`、边框宽度都不动。系数 0.75 是刻意挑的——4px 网格 4→3、8→6、12→9、16→12、20→15、24→18、32→24 全部落在整数像素，不出半像素模糊。
- **入口**：设置面板「语言」下方新增「密度」分段按钮（与语言同一枚 `segment_button`），点击即生效并发 `SaveDensityMessage`，**不进「保存」通路**；`xgent_app::settings_io::save_density` 经 `CONFIG_WRITE` 写 `preferences.density` 后重读快照。daemon 侧 `config_store` 的键白名单各加一条，写只接受 `comfortable`/`compact`（非法值报错，不静默回落）。
- **启动即生效**：`request_preferences_on_startup` 在 Startup 读一次全局配置，只回填 `preferences`（`SnapshotResult.preferences_only`）。**别改成走面板快照**——面板快照会连带回填项目配置并重写 `LiveSettings`，而开机 `ProviderInfo` 还没就绪（provider id 为空），`resolve_context_window` 会回落内置缺省，把用户配的上下文窗口冲掉。

**hover 量化**（`kit.rs`）：

- `hover_tint_system` 只处理 `Or<(Changed<Hovered>, Changed<Pressed>)>` 的节点——`Hovered` 是 immutable 组件，只在指针进出时被重写，天生适合变更检测；空闲帧查询为空，不做全量扫描。
- **松手要单独接**：`Pressed` 是「插入/移除」的标记，移除这一帧 `Changed` 看不到（`hover_tint_release_system` + `RemovedComponents<Pressed>`）。漏了它，按钮松手后停在按下色，直到指针下次进出才恢复。
- `HoverTintPollStats { visits, releases }` 是这条验收的可观测证据（不是面向用户的功能）：空闲帧 `visits == 0`。

**测试注意**：密度集成测试（`tests/density.rs`）要用真实插件产出节点——对话面板需要 `UiFonts`、`MeshPlugin` 与 agent 的一批消息类型，否则 system 参数校验会以 "Resource does not exist" 直接 panic（看不到系统名，因为 bevy 的 `debug` feature 未开）。

### 5.20 滚动契约 / 填满契约 / 省略原语 / 虚拟列表（v8 M3）

M3 把「哪里能滚、哪里填满、超长了怎么办、列表大了怎么办」四条从散落实现收敛成原语。

**滚动：`xui::ScrollFrame` 是唯一入口**（`xui/src/scroll_frame.rs`）

- `ScrollFrame::vertical() / horizontal() / stick_bottom()` 返回 bundle：`Node + ScrollPosition + ScrollFrameMarker + Scrollbar`。**`overflow` 由构造器决定**，`enforce_scroll_frame_overflow` 系统在布局前把 `Node.overflow` 回写成构造器的值——调用方改了也会被改回来（类型 + 运行时双保险）。
- **不要手搓滚动容器**：`Overflow::clip_y()` 配 `ScrollPosition` 是「假滚动」——bevy 只在 `OverflowAxis::Scroll` 轴消费偏移（`bevy_ui/src/layout/mod.rs:476-491`），`clip_y` 轴的偏移被直接丢弃 → 看着能滚、实际滚不动。业务代码里 `Overflow::clip_y()` 已归零（由 `tests/scroll_contract.rs` 的源码级契约测试守住）。
- **文本必须放在滚动容器的子节点里**：挂在容器自身的文本按内容盒原点定位（渲染用节点的 `content_box().min`），滚动偏移只施加给子节点 → 文本不跟着滚。
- `scroll_to_child`（消息 `ScrollToChild` + 纯函数 `compute_scroll_to_child`）把指定子项滚进视口；`StickToBottom` 贴底跟随随 `stick_bottom()` 一起给。
- `xui::Scrollbar` 的轨道是容器的子节点，**必须带 `IgnoreScroll`**（否则随内容滚走）；轨道节点由插件 spawn，容器已有轨道子节点时不再重复 spawn（否则子节点每帧增殖）。

**填满：`xui::region_fill()`**（`xui/src/layout.rs`）

- `region_fill()` = `flex_grow:1 + min_width/min_height:ZERO`；`fill_region(&mut node)` 改写既有节点并清掉百分比高度。
- **有兄弟节点的列里禁止 `height: Val::Percent(100.0)`**：百分比高度相对父容器内容盒算，同列还有固定高度兄弟（分屏的 38px 页签条）时会把底部顶出视口（审计 D2：终端状态条永久掉出 38px）。分屏三页（编辑器/差异/终端）已改 `region_fill`。

**省略：`xui::Ellipsis`**（`xui/src/ellipsis.rs`）

- bevy 0.20 **没有** `TextTruncate`，`TextBounds` 官方自陈硬裁不可靠 → 省略号必须自建。
- 测量走 bevy 自己的排版管线（`TextPipeline::create_text_measure`），与渲染同一套度量；决策是纯函数 `compute_ellipsis(source, tail, min_keep, mode, avail, measure)`：装得下就原样，装不下就在「可变前缀」上二分。
- 两种模式：`EllipsisMode::End`（`abc…`）与 `EllipsisMode::Mid`（路径：`a/…/x/y.rs`，保留首段与末两段）。中段模式的「完整文本」是**原文**，不是 `head+suffix`。
- `EllipsisCache` 按 `(源文本, 可用宽, 字号, 省略号, 模式)` 记忆结果（命中/未命中计数可观测），宽度不变时不重复测量。
- **前提**：节点宽度必须**确定**（固定宽 / 百分比 / flex 收缩后的宽度），否则文本把节点撑开，没有「装不下」这回事；文本节点要能收缩（`flex_shrink:1 + min_width:ZERO`）。
- 业务侧用 `fonts::ellipsis_text()` / `fonts::ellipsis_path_text()` 构造（自带 `TextLayout{NoWrap}` + `Ellipsis`）。

**对话输入区**（`xgent_ui/src/chat_panel.rs`）

- 三段式：外框（边框/焦点环/忙碌红边）+ 内层滚动区（`ScrollFrame`）+ 文本节点。文本节点**不挂 `Text`**——`Text` 与 `EditableText` 会争同一个 `ContentSize`（一个按 `Text` 量、一个按 `visible_lines` 量），高度会时对时错；但缺 `Text` 时 `require_markers` 下节点必须自带 `Pickable` 才收得到点击。
- **高度自适应靠 `EditableText::visible_lines`**：bevy 0.20 的可编辑文本节点高度只看 `visible_lines`（默认 `Some(1.0)`），不由文本内容决定 → `update_input_visible_lines` 按排版结果回写可视行数。
- **光标行跟随要自己写**：bevy 的 `TextViewport::reveal_caret` 只在焦点变化时触发，继续打字不会跟随光标（长输入会把当前行藏起来）→ `update_input_scroll_to_caret` 按 `TextLayoutInfo.cursor` 的光标矩形自己算滚动目标。

**虚拟列表**（`xui/src/virtual_list.rs` + `xgent_ui` 的文件树/会话历史）

- `VirtualList` 给出可见区间（`first_visible`/`visible_count`）；`VirtualListItem` 标记的行会把**实测高度**写回列表（不等高：估算 + 实测回写的累积偏移表，`build_measured_offsets` / `compute_visible_range_measured`）。
- 业务侧三件套：数据层拍平成一维（文件树 `flatten_tree`）、只 spawn 可见行（绝对定位到行位）、**占位块撑出列表总高度**（否则滚动条只反映已渲染的行）。
- **数据换代要带代次**（`FileTreeRow.generation`）：数据重建后落在可见区间内的旧行内容会过期（同一文件出现两行）。
- **虚拟列表需要视口高度才有内容**：headless 装配没有渲染目标时视口高 0 → 可见行数为 0 → 列表看起来是空的（文件树因此加最小高度 240px）。

**headless 装配清单**（这轮踩得最多，M3 顺带修好 12 个长期红测）：UI 布局要有尺寸，需要 `bevy::ui::UiPlugin` + `bevy::window::WindowPlugin` + 相机 + `bevy::picking::PickingPlugin`（否则 `bevy_ui::widget::viewport` 缺 `HoverMap`）+ `bevy::image::ImagePlugin` 与 `init_asset::<TextureAtlasLayout>()`；**`xgent_ui::overlay::OverlayPlugin` 必装**（`DrawerToggle` 消息由它注册，缺它时文件面板整体报错、文件树从不重建）。排查手段：临时给 bevy 加 `debug` feature，系统名会直接印在报错里。

### 5.21 跨平台字体候选栈（v8 M4-T1）

`mono`（等宽）曾经是「macOS 独占特权路径」：非 macOS 走 `Handle::default()`，而默认槽位已被 Inter 占据——**Windows/Linux 上的「等宽」其实是比例字体**（审计 A1）。v8 改成**按槽位装配**：

- **四个槽位**（`xgent_ui::fonts::FontSlot`）：`ui`（界面拉丁，内嵌 Inter Variable 作默认字体资产）、`ui_cjk`（界面中文）、`mono`（代码/终端/diff/行号）、`mono_cjk`（终端内中文）。
- **候选栈**（`FontSpec` + `FontCandidate`）：每槽一份**有序**候选，平台差异只存在于这份**数据**里（`font_specs()` 按 `std::env::consts::OS` 选表），代码**不分叉**（禁止 `cfg!(target_os = …)`，`tests/font_slots.rs` 有源码级断言守住）。
- **解析规则**（`resolve_slot`，纯函数）：① 按栈序取第一个系统里**真实存在**的家族（用 parley 字体集合的家族名快照判定）→ `FontOrigin::System`；② 全缺则用该槽位的**内嵌兜底资产** → `FontOrigin::Embedded`；③ 再全缺 → `FontOrigin::Failed` + 该槽位声明的通用族兜底，**调用方记 `warn!`**（绝不静默）。
- **硬约束**：`mono` **永不**回退到 `ui` 槽位或任何比例字体（设计系统 C-FONT-1）；它的兜底通用族是 `Monospace`，内嵌兜底是 **JetBrains Mono**（OFL 1.1，随包分发，`assets/fonts/`）。
- **可核对**：`xgent_app::startup::load_fonts` 启动时解析四个槽位并 `tracing::info!` 打出「槽位 → 家族名 → 来源」（C-FONT-2），三平台各启动一次即可核对；`assets/fonts/README.md` 记了两个字体的来源与许可。

**坑**：

- **`ui` 槽位不能带字体句柄**：`ui_text` 不接收 `UiFonts`（约 50 处调用点），所以 `ui` 只能继续用默认字体资产；CJK 靠 parley 的系统回退承担（`ui_cjk` 本轮只做解析与日志，专用 CJK 表面留给 M4-T4）。
- **内嵌资产的 `alias` 是空串**：`Font::from_bytes` 不解析家族名 → 内嵌字体只能按 `FontSource::Handle` 引用，**不能**写进 `FontSource::Families` 的 fallback 链。
- **查家族存在性要先取快照**：`collection.family_id(name)` / `family_names()` 都要 `&mut` → 先 `family_names().map(String::from).collect()` 再用闭包查。
- `UiFonts.mono` 的类型是 `FontSource`（候选栈解析结果），不是 `Handle<Font>`；headless 装配用 `UiFonts::fallback()`。

## 6. 开发流程（与 AGENTS.md 第 6 节对齐）



1. **阅读背景**：开始任务前读 `AGENTS.md`、`docs/design/`、`docs/plans/` 中相关 step 文件；编码时按需查 `../bevy` 源码确认 API（bevy 仍在演进，有 breaking change）。
2. **先通后优**：每模块先最小可用版本，确保编译通过与基本功能，再迭代。
3. **接口先行**：先定义 trait 与协议类型，再实现具体逻辑。
4. **测试驱动**：每 crate 完成后写最小集成测试验证。
5. **不提前引入 3D/TUI/Web/宠物**：MVP 仅 2D GUI，P1 已引入编辑器。
6. **产出文档放 `docs/`**：设计/计划类文档放 `docs/` 对应分类目录，不放项目根。
7. **遗留清理**：根目录 `src/main.rs` 是遗留 Hello world，按计划应删除（`xgent_app` 已接管入口）。
8. **同步本指南**：**后续实现新功能或功能有变化，都需要更新 `docs/dev-tutorial.md`**（见 `AGENTS.md` 第 6 节新增条目）。

---

## 7. 常用命令

```bash
cargo check                        # 全量编译检查
cargo check -p <crate>             # 单 crate 编译检查
cargo run -p xgent_app             # 运行 UI 进程（自动拉起 daemon）
cargo run -p xgent_daemon          # 单独运行守护进程
cargo test                         # 全量测试
cargo test -p <crate>              # 单 crate 测试
cargo tree -p <crate>              # 依赖树（验证 crate 独立性，如 xui 不含 xgent_*）
cargo fmt                          # 格式化
cargo clippy --workspace           # lint
# 插件系统（需 wasm32-wasip2 target：rustup target add wasm32-wasip2）
./build_plugins.sh                 # 构建内建插件（git）并打包到 xgent_app/assets/plugins/
./build_plugins.sh release         # release profile（更小 wasm）
cargo test -p xgent_plugin -p xgent_plugin_host  # 插件集成测试（需先 build_plugins.sh）

构建依赖本地 `../bevy` 源码（检出上游 `v0.20.0` tag，detached HEAD），确保该目录存在且指向正式版；工具链需满足 bevy MSRV（rustc ≥ 1.97.1），否则 cargo 在依赖解析阶段即拒绝编译。
