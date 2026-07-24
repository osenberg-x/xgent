# XGent 开发指南（已实现功能总览）

> 本文档梳理 XGent 截至 2026-07-22 已实现的功能、对应的代码与文档、以及开发注意点。
> **维护规则**：后续实现新功能或功能有变化，都需要更新这个文档（见 `AGENTS.md` 第 6 节）。
>
> 代码现状：13 个 crate 已实现（含 `xgent_terminal`），`cargo check --workspace` 通过，约 22k 行 Rust。
> MVP（step1~step12）+ optimization 方案 O1~O10 + F-11 内置编辑器（P1）+ F-19 内置终端（P1）已全部落地。

---

## 1. 功能总览

按需求 `doc/design/requirements.md` 的 F/NF 编号对照实现状态：

### 1.1 核心功能（MVP + 已完成的 P1）

| 编号 | 功能 | 状态 | 实现位置 | 关键文档 |
|:---|:---|:---|:---|:---|
| F-01 | 多轮对话 | ✅ | `xgent_agent/src/bridge.rs`（双层 `run_agent_loop`：外层 follow-up、内层 tool-call+steering；`stream_with_retry` 自动重试可重试错误；流式期 steering 即时中断 race abort、停止边界重新轮询防丢失）+ `compaction.rs`（token 估算 + should_compact 触发 + find_cut_point 切点 + LlmCompactor 摘要） | `plans/step9`、`plans/optimization-from-omp.md` O4/O9 |
| F-02 | 流式输出 | ✅ | `xgent_provider/src/openai_compat.rs`（SSE → ChatEvent 细粒度事件）→ `xgent_ui/src/chat_panel.rs` | `plans/step5`、ADR-0006 |
| F-03 | 工具调用 | ✅ | `xgent_tools/src/builtins/`（ReadFile/WriteFile/SearchFiles/RunCommand）+ `executor.rs`；**工具 schema 经 `AgentBridge.tool_schemas` 注入 LLM 请求**（2026-07-20 修复，见诊断文档）；`conv.messages` 记录完整 tool_call/tool_result 配对（带 call_id） | `plans/step7`、ADR-0007、`conversation-flow-fixes-2026-07-20.md` |
| F-04 | 操作确认 | ✅ | `xgent_tools/src/security.rs`（`resolve_policy`）+ `executor.rs`（ConfirmRequest 流程）+ `xgent_ui/src/confirm_dialog.rs` | `plans/step7`、ADR-0007 |
| F-05 | 项目上下文 | ✅ MVP（方案 A OnDemand）；B/C/D/E 仅 trait 占位 | `xgent_context/src/on_demand.rs`（445 行实现）；**bridge 异步侧 StartLoop 时调 `context.retrieve` 注入项目目录树 + 相关文件**（2026-07-20 修复）；`repo_map.rs`/`vector.rs`/`lsp.rs`/`hybrid.rs` 均为 25 行占位 | `plans/step8`、ADR-0010、`conversation-flow-fixes-2026-07-20.md` |
| F-06 | 会话管理 | ✅ | `xgent_agent/src/session_store.rs`（JSONL append-only）+ `conversation.rs`；**错误持久化为 `SessionEntry::Error`**（2026-07-20 新增）；**新建会话功能**（`NewSessionMessage`/`SessionClearedMessage` + 命令面板 `session.new`，2026-07-20 新增） | `plans/step9`、ADR-0008、`conversation-flow-fixes-2026-07-20.md` |
| F-07 | Provider 切换 | ✅ | `xgent_provider/src/openai_compat.rs`（完整实现）；`response_api.rs`/`anthropic.rs`/`custom.rs` 仅 trait 占位；`xgent_ui/src/settings_panel.rs` | `plans/step5` |
| F-08 | 命令面板 | ✅ | `xui/src/command_palette.rs`（通用组件）+ `xgent_ui/src/command_palette.rs`（业务命令注册） | `plans/step10`/`step11`、`design/ui-design.md` §8 |
| F-09 | 快捷键体系 | ✅ | `xui/src/hotkeys.rs` + `xui/src/shortcuts.rs` + `xgent_ui/src/shortcuts.rs` | `design/ui-design.md`、`plans/step10` |
| F-11 | 内置编辑器（P1） | ✅ | `xui/src/text_editor/`（buffer/find/highlight/render/undo/virtual_render）+ `xgent_ui/src/editor/`（buffer/command/conflict/io/state/tabs/at_syntax） | `design/editor-design.md`、ADR-0009/0010 |
| F-19 | 内置终端（P1） | ✅ | `xgent_terminal/`（`TerminalBackend` trait + `LocalPtyBackend`，portable-pty + vte，不依赖 Bevy）+ `xgent_ui/src/terminal/`（mod/io/tabs/input/output：PTY 桥接 + 多 tab + 行编辑 + vte 渲染 + SideView 集成）+ `xgent_app` 注入 `TerminalIoRuntime` | `design/terminal-design.md`、ADR-0011/0012 |

### 1.2 非功能需求

| 编号 | 需求 | 状态 | 实现位置 |
|:---|:---|:---|:---|
| NF-01 | 跨平台 | ✅ | Unix socket（macOS/Linux）+ named pipe（Windows）见 `xgent_settings_core/src/paths.rs` 与 `xgent_daemon/src/server.rs` |
| NF-02 | 轻量多开 | ✅ | 多进程模型：UI 每项目一个，daemon 全局唯一；`xgent_daemon/src/lifecycle.rs` 随用随启 |
| NF-03 | 性能 | ✅ | 数据驱动 UI；虚拟列表 `xui/src/virtual_list.rs`；流式 channel 非阻塞 |
| NF-04 | 可维护性 | ✅ | ECS Events/Messages 通信；daemon 纯 tokio 可 headless |
| NF-05 | 国际化 | ✅ | `xui_i18n::StringSource` trait；`xgent_settings/src/localizer.rs`（fluent）；资源 `crates/xgent_settings/locales/{zh-CN,en-US}/main.ftl` |
| NF-06 | 可扩展 | ✅ | TUI/Web/3D/自定义工具/MCP 均有 trait 预留（见 §3） |

### 1.3 未实现（P1/P2 留白）

- **F-10 Git 集成**（P1）：未实现。
- **F-12 成本统计**（P1）：未实现（`TokenUsage` 类型已定义，无汇总 UI）。
- **F-13 MCP 支持**（P1）：仅 trait 预留 `xgent_tools/src/mcp.rs::McpTransport`，无实现。
- **F-14 自定义工具**（P2）：未实现。
- **F-15 虚拟宠物**（P1）：未实现（`xgent_pet` crate 未建）。
- **F-16 3D 可视化** / **F-17 TUI** / **F-18 Web**（P2）：未实现，架构留口。
- **插件系统**（架构设计）：参考 Zed 的 WASM 组件模型，设计动态安装/卸载的插件系统。插件经 WIT 接口注册 Agent 工具、命令面板命令、ContextProvider 等扩展点。F-10 Git 集成、F-14 自定义工具等后续功能将以插件形式实现。详见 `doc/design/plugin-system-design.md`。
- **Compaction**（optimization O9）：`xgent_agent/src/compaction.rs` 已落地——token 估算（`tokenizer.rs` 启发式）+ `should_compact`（reserve=max(15% window, 16384)）+ `find_cut_point`（保留最近 token 段，user/assistant 边界切）+ `LlmCompactor`（调 provider 生成摘要）+ `apply_compaction`（summary 前置 + kept）。`AgentEvent::Compacted` 通知 UI，`SessionEntry::Compaction` 持久化。

---

## 2. Crate 拓扑与职责

依赖自底向上（无环）。详见 `doc/design/architecture.md` §4。

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
xgent_agent         ── agent loop + ECS 桥接 + SessionStore
xui                 ── 通用 Bevy UI 组件库，纯依赖 bevy + xui_i18n，可独立发布
xgent_ui            ── XGent 业务 UI（对话/工具/文件/编辑器/设置面板等）
xgent_app           ── UI 进程入口 bin：组装插件 + daemon 拉起 + IPC 客户端
```

### 2.1 各 crate 关键 API

| crate | 关键导出 |
|:---|:---|
| `xgent_core` | `ChatEvent`(12 变体)、`StopReason`、`AgentMessage`/`ContentBlock`/`convert_to_llm`、`ErrorKind`、`SessionEntry`、`EditorState` trait、`FileChanged`、JSON-RPC `Request/Response/Notification`、`methods`/`notifications` 常量 |
| `xui_i18n` | `StringSource` trait |
| `xgent_settings_core` | `GlobalConfig`/`ProviderConfig`(含 `max_retries: Option<u32>`/`retry_mode`/`retry_initial_delay_ms`/`retry_max_delay_ms`/`retry_backoff_factor`)/`ProviderKind`/`RetryMode`(Fixed/Exponential)/`ProjectConfig`/`ContextStrategy`(OnDemand/RepoMap/Vector/Hybrid)/`ToolPolicyConfig`、`GlobalConfigStore`/`ProjectConfigStore`、`paths` |
| `xgent_settings` | `Localizer`（impl StringSource）、`GlobalConfigRes`/`ProjectConfigRes`、`XgentSettingsPlugin` |
| `xgent_provider` | `LlmProvider` trait、`OpenAiCompatProvider`（完整）、`ResponseApiProvider`/`AnthropicProvider`/`CustomApiProvider`（占位）、`build_provider(id, cfg)`、`ChatStream` |
| `xgent_daemon` | `Server`（JSON-RPC over Unix socket/named pipe）、`ProviderPool`、`FsWatcher`、`ConfigStore`、`registry`（多客户端订阅广播）、`lifecycle` |
| `xgent_tools` | `Tool` trait（tier/approval_for/concurrency/execute(signal,on_update)）、`ToolTier`(Read/Write/Exec/UiOnly)、`Concurrency`(Shared/Exclusive)、`ToolError`、`ToolExecutor`、`ConfirmCallback`、`resolve_policy`、4 内置工具、`EditorTool`（UiOnly）、`McpTransport`（占位） |
| `xgent_terminal` | `TerminalBackend` trait（spawn/write/resize/kill/subscribe）、`LocalPtyBackend`（portable-pty）、`TerminalEvent`(Output/Exited)、`SpawnRequest`、`ShellSpec`(Powershell/FromEnv)、`TerminalId`、`TerminalError` |
| `xgent_context` | `ContextProvider` trait、`OnDemandContextProvider`（完整）、`RepoMap`/`Vector`/`Lsp`/`Hybrid`（占位）、`build_context_provider` |
| `xgent_agent` | `XgentAgentPlugin`、`AgentBridge`/`AgentCommand`(StartLoop/Abort/ConfirmDecision/Steering/FollowUp)/`AgentEvent`(含 `RetryAttempt`/`Compacted`)、`AgentBridgeConfig`(含 `compaction`/`context_window`/`compaction_settings`)、`RetryConfig`/`stream_with_retry`、`run_agent_loop`、`StreamOutcome`(tool_calls/usage/stop_reason/pending_steering)、`maybe_compact`、`Conversation`/`ConversationStatus`/`persist_compaction`、`SessionStore`、`CompactionProvider` trait + `LlmCompactor` 实现 + `CompactionSettings`/`should_compact`/`find_cut_point`/`apply_compaction`/`compaction_context_tokens`、`tokenizer`(estimate_message_tokens/estimate_messages_tokens)、`build_request`、events.rs（UserInput/Abort/Steering/FollowUp/Delta/ToolCall/ToolResult/ConfirmRequest/Done/Error/Retry/**Compacted** Message） |
| `xui` | `TextEditor`/`Rope`/`HighlightCache`、`ScrollArea`/`StickToBottom`、`Scrollbar`、`CommandPalette`/`CommandRegistry`、`HotkeyRegistry`、`ChatInput`、`ShortcutsPlugin`、`VirtualList`、`i18n_bridge`（`tr`/`tr_with`/`Strings`） |
| `xgent_ui` | `XgentUiPlugin`、`chat_panel`/`file_panel`/`top_bar`/`status_bar`/`tool_panel`/`command_palette`/`confirm_dialog`/`settings_panel`/`shortcuts`/`theme`/`layout`/`i18n`、`editor/`（buffer/command/conflict/io/state/tabs/at_syntax）、`terminal/`（mod/io/tabs/input/output：PTY 桥接 + 多 tab + 行编辑 + vte 渲染 + SideView 集成） |
| `xgent_app` | `Args`（命令行）、组装 `XuiPlugin` + `XgentSettingsPlugin` + `XgentAgentPlugin` + `XgentUiPlugin` + `ConfigBridgePlugin` + `FsEventBridgePlugin`、`connect_or_spawn_daemon`、`IpcProviderClient` |

---

## 3. 已落地的关键设计决策（ADR）

对应 `doc/decisions/` 下 12 条 ADR，全部已定案并落地：

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
| 0011 | 用户终端 PTY 选 portable-pty | `xgent_terminal`（`LocalPtyBackend`，portable-pty + spawn_blocking 桥接 ECS）+ `xgent_ui::terminal::io`（crossbeam 桥 tokio→ECS）；Win powershell/Unix $SHELL；保持 shell cooked 模式（portable-pty 无跨平台 raw API），UI 侧**透传模式**——按键直接转原始字节发 PTY，shell 回显为唯一显示源（避免双显），获 shell 原生 readline 历史/补全。读循环直接 `tokio::mpsc::Sender::blocking_send` 转发输出（不经 std mpsc 中转 + async 桥接 task，避免阻塞 std `recv` 冻结 runtime）；读循环检测 DSR 光标查询 `\x1b[6n` 并回复 `\x1b[1;1R`（PowerShell/PSReadLine 启动时探测终端，不回复则卡死等待，输入无响应）；writer 经 `Arc<Mutex>` 共享给读循环（DSR 回写）与命令循环（用户输入）。 |
| 0012 | 终端独立 crate + TerminalBackend trait | `xgent_terminal`（`TerminalBackend` trait + `LocalPtyBackend`）+ `xgent_ui::terminal`（UI 层只依赖 trait，经 `TerminalIoRuntime` 注入实现）；对齐 xgent_tools/xgent_context 纯逻辑层模式 |

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
 → 若有 ToolCall → conv.push_tool_call 记录到历史 → ToolExecutor（含确认）→ ToolResult 回灌
   → conv.push_tool_result 记录到历史（与 tool_call 配对，符合 OpenAI 协议）→ 继续内层循环
 → 若 usage.prompt 超 should_compact 阈值 → maybe_compact 调 LlmCompactor 摘要 → req.messages 重建为 summary+kept → 发 Compacted 事件
 → Done{usage, model} → finalize_assistant 写入 usage/model → 退出内层；SessionStore append Message entry
 → UI 据 DoneMessage.usage 累加真实 token（prompt + completion）
```

**关键修复（2026-07-20，见 `doc/conversation-flow-fixes-2026-07-20.md`）**：
- tool_schemas 从 AgentBridge.tool_schemas 注入（启动时从 ToolExecutor 一次性提取），
  修复 LLM 无法发起工具调用的致命 bug。
- 上下文检索在 bridge 异步侧 StartLoop 时执行（ECS 系统同步无法 await retrieve），
  修复项目结构从未注入的 bug。
- tool_call/tool_result 都带 call_id，conv.messages 记录完整配对，修复多轮工具调用后
  OpenAI 协议配对断裂的 bug。
- AgentEvent::Done 携带 usage/model，AssistantMessage.usage 不再为 None，UI token 统计
  用真实 prompt+completion 累加。

### 4.3 agent loop 双层循环（ADR-0007 / optimization O4）

- 外层：follow-up 驱动；停止边界先 `try_recv` steering（防止 steer 在 yield 点丢失，对齐 omp `runLoopBody`）。
- 内层：tool-call + steering——LLM → tool → continue，直到 `tool_calls.is_empty()`。
- Abort：`CancellationToken::cancel()`，`stream_llm_response` 与 `executor.execute` 都 `tokio::select!` 监听。
- Steering：**流式期即时中断**当前流（`stream_llm_response` 的 `select!` race steering_rx，对齐 omp `streamAssistantResponse` 的 abort race），返回 `pending_steering` + `partial_text`，由 `run_agent_loop` 发 `SteeringInterrupted` 事件——ECS 把半截文本 `finalize_assistant` 为被中断的 assistant 消息并清空当前节点（避免与新回复拼接），注入 steering 文本到 `req.messages` 后重新流式。停止边界也重新轮询，避免 steer 丢失。
- Length 截断：`stop_reason == Length` 时不执行 tool_calls（参数可能不完整），为每个补占位 skipped result（对齐 omp `createAbortedToolResult`），回灌 assistant tool_call + tool result 到 req.messages 维持配对，让 LLM 重新生成完整调用。
- Compaction（optimization O9）：每次 stream 拿到 `usage` 后，`maybe_compact` 用 `should_compact(max(provider_prompt, 本地估算), window, settings)` 判断；触发则 `LlmCompactor.compact` 生成摘要，`apply_compaction` 重建 `req.messages`（summary 前置 + kept），发 `AgentEvent::Compacted`。

### 4.4 自动重试（F-01）

- `stream_with_retry` 包装 `stream_llm_response`：失败时按 `RetryConfig` 重试。
- **仅可重试错误重试**：`ErrorKind::Network`（连接/超时）、`StreamParse`（SSE/JSON 解析）；`NotConfigured`/`AuthFailed`/`ProviderError` 立即失败（重试无意义）。
- **次数**：`ProviderConfig.max_retries: Option<u32>`——`None` = 无限重试（直到成功或被中断），`Some(n)` = 最多 n 次。
- **模式**：`RetryMode::Fixed`（固定间隔）/`Exponential`（指数退避 `min(initial * factor^(n-1), max_delay)`）。
- 重试前发 `AgentEvent::RetryAttempt` → ECS 清空半截助手文本并发 `RetryMessage`，UI 据此展示"重试中(第 N 次)"。
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
- **项目级目录** `<project_root>/.xgent/`：项目特定配置（`config.toml` 含 provider 覆盖、tool 策略）。
  - 路径函数：`paths::project_config_dir(root)` / `project_config_file(root)`。
- **daemon socket**：默认平台缓存目录（`dirs::cache_dir()/xgent/daemon.sock`）；设置 `XGENT_AGENT_DIR` 时改用其下 `daemon.sock`（开发隔离）。

**迁移注意**：会话历史从项目级 `<project>/.xgent/sessions/` 迁到全局 `~/.xgent/agent/sessions/`，`session_file_path(id)` 去除 `project_root` 参数。旧项目级会话文件不自动迁移（MVP 会话为临时性，跨项目共享更符合 D-04 演进）。

---

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

> 注：Bevy 0.19 中 `Event` 与 `Message` 是**两个并存的 trait**，语义不同：
> - `Message`（`#[derive(Message)]` + `add_message::<T>()` + `MessageReader`/`MessageWriter`）：缓冲队列，双缓冲（`Messages<M>`），配 `message_update_system` 每帧 `update` 切缓冲。读一次 per update 不丢，两次 update 后保证丢。每个 `MessageReader` 独立 cursor，多 reader 各读全量（广播语义）。
> - `Event`（`#[derive(Event)]` + `Observer` + `On<E>` + `world.trigger(E)`）：即时观察者，trigger 即同步派发给所有 observer，无缓冲、无队列。
>
> **本项目代码统一用 Message**（`crates/` 下无 `add_event`/`EventReader`/`EventWriter`）——这是项目选择，不是 Bevy 强制。`doc/plans/step9/step10/step12` 与 `editor-design.md` 残留 `#[derive(Event)]` 是历史文档遗留，实现时改用了 Message，以代码为准。
>
> **何时该用 Event 而非 Message**（Bevy 语义层面）：纯即时通知、无需跨帧、无需多消费者排队、希望同步触发 observer 处理——如"provider 状态变更"这类"发生即响应"。本项目目前未用 Event（所有通知都走 Message 也工作），但新增"强即时性"通知时可考虑 Event，不要因本文档统称 Message 而排斥它。判据：**需要跨帧/缓冲/多消费者排队 → Message；需要即时同步触发 observer → Event**。

#### 5.1.1.1 动作信号 → Message（默认）或 Event（即时）

**判据**：满足以下任一即为动作信号。

- 有方向（A 发、B 收，或广播给多个订阅者）。
- 有时序语义（"先 abort 再 done" 或 "steering 在流式期即时中断"），顺序必须保证。
- 可能并发多条（流式 delta、多个 tool_call），需要缓冲队列。
- 接收方可能多个（UI 渲染 + 状态栏 + 持久化都要听 Done）。
- 调用方不需要同步等结果（"发完即走"）。

**Message vs Event 选择**（见上方注释的 Bevy 0.19 源码语义）：
- **默认 Message**：跨帧、需缓冲、多消费者各读全量。本项目所有动作信号都是 Message。
- **改用 Event 的判据**：信号严格单帧内被 observer 消费、且无需跨帧保留、希望 trigger 即同步派发。典型场景：provider 状态变更、配置热重载完成、预算告警——这些"发生即响应"的通知若用 Message 也能工作（本项目即如此），但 Event 的即时性更贴合语义。
- **必须用 Message 的场景**：流式 delta（跨帧累积）、用户输入、abort/steering（不能丢、可能跨帧）——这些用 Event 会丢消息。

**Message 用法要点**（Bevy 0.19）：
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
| `GlobalConfigRes` / `ProjectConfigRes` / `Localizer` | 配置与 i18n | `xgent_app` 启动注入 / `config_bridge` 刷新 |

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

### 5.9 编辑器（F-11）边界

- **中等能力**：多行 + 行号 + undo/redo + 查找替换 + tree-sitter 语法高亮（仅 Rust grammar，随二进制编译入，不做按需下载——D-06 已决策）。
- **不含**：LSP、编辑器内部 split view（同一编辑器内多窗格）、诊断、跳转。完整能力边界留后续。（对话/编辑器分屏见 §5.11）
- `xui::TextEditor` 是通用裸件（纯依赖 bevy + xui_i18n + tree-sitter），多标签/文件 IO/冲突协调在业务层 `xgent_ui::editor`。
- 外部文件变更冲突：未脏静默重载 / 脏弹窗三选（丢弃本地 / 保留本地 / 对比合并）。

### 5.10 检索升级路径（ADR-0010）

- MVP 用方案 A（OnDemand：目录树 + ripgrep + 按需读文件）。
- 编辑器上线只触发到 C（向量 RAG）；D（LSP/AST）延后到 LSP 真正接入；E（混合检索）跟随 D。
- 新增检索实现时实现 `ContextProvider` trait，`build_context_provider` 据配置切换，调用方无感。

### 5.11 对话/编辑器分屏（右侧 SideView）

- **布局**：`MainAreaMarker`（横向 row）下五子节点——`FilePanelMarker`（显式宽，由 `PanelWidths` 驱动）+ 左手柄 + `ChatPanelMarker`（flex:1 填充剩余）+ 右手柄 + `SideViewMarker`（显式宽，默认 `display:none`）。手柄由 `resize::handle_bundle` spawn。
- **分屏内容**：编辑器视图（`EditorViewMarker`，代码文件）与文件预览（`FilePreviewMarker`，非代码文件）二者互斥挂于 `SideViewMarker` 下，由 `handle_file_click` 据文件类型切换显隐。
- **展开/收起**：`SideViewCollapsed` Resource 驱动 `toggle_panel_visibility`（合并系统，避免跨系统 B0001）切换 `SideViewMarker` + 右手柄的 `display`。展开触发：点击文件节点（代码/非代码均展开）；收起触发：编辑器返回按钮、关闭最后一个 tab、`Ctrl+\`（`sideview.toggle`）。
- **快捷键**：`Ctrl+\` = `sideview.toggle`（切换分屏）。
- **设计图**：`doc/design/ui-prototype.html` §2.1 P1。

### 5.12 面板拖拽调整大小（resize）

- **模块**：`xgent_ui/src/resize.rs`（`ResizePlugin`）。两条竖向手柄插在主区行布局的面板之间，鼠标拖拽改变相邻面板宽度。
- **数据驱动**：`PanelWidths` Resource（`file_panel`/`side_view` 显式像素宽度），`ActiveResize` Resource（当前激活边界）。`apply_panel_widths` 据资源写面板宽度（折叠态下文件面板置 0）；`handle_resize_drag` 处理拖拽逻辑。
- **拖拽状态机**：手柄 `Interaction::Pressed` + `ButtonInput<MouseButton>::pressed(Left)` 触发启动 → 持续期间据 `AccumulatedMouseMotion.delta.x` 增量更新宽度 → 鼠标释放清除。不引入 `bevy_picking`（默认未启用，会拉重依赖），用 `Interaction` + `ButtonInput` 手搓状态机。
- **钳制**：据 `MainAreaMarker` 的 `ComputedNode.size` × `inverse_scale_factor`（物理→逻辑像素）得主区宽，按 `FILE_PANEL_MIN`(160)/`SIDE_VIEW_MIN`(200)/`CHAT_MIN`(240) 钳制，保证对话主区总有最小空间。
- **视觉**：手柄默认透明（`Color::NONE`），hover/拖拽时变色高亮（`HANDLE_ACTIVE_COLOR`）。右手柄初始 `display:none`（分屏默认收起）。
- **B0001 规避**：`toggle_panel_visibility` 与 `apply_panel_widths` 都写 `&mut Node` 于面板，用 `.after()` 排序；同系统内 `q_file`/`q_side`/`q_handles` 加交叉 `Without` 过滤器证明不相交（`With<A>` 不隐含 `Without<B>`）。

### 5.13 UI 原型对齐（A-H 已落地，D 待实现）

对照 `doc/design/ui-prototype.html` 原型图的差距分阶段实现，详见 `doc/design/ui-gap-plan.md`：
- **A-主题（已落地）**：`Theme` 加状态色 5 色 + 语法高亮色 7 色（`theme.rs`）；`FILE_PANEL_W` 改 240。
- **B-文件树（已落地）**：`file_panel.rs` 加 `fp-head` 标题头 + 折叠按钮；`spawn_entry` 重写为箭头/图标/名称分离的 row；选中/悬停态（`FileSelectedMarker` + `update_file_entry_style`）；`handle_dir_click` 用 `ParamSet` 避免双 `&mut Text` query 冲突（B0001）。
- **C-对话视觉（已落地）**：`chat_panel.rs` 加 `viewtabs`（对话标签 + 会话信息）+ 消息气泡 role 行（头像 + 角色名）+ `input-meta` 快捷键提示栏。
- **E-状态栏分段（已落地）**：`status_bar.rs` 改 row 容器（`StatusDotMarker` + provider + 状态 + token + 编码），状态点忙时脉冲。
- **F-顶栏品牌+caret（已落地）**：`top_bar.rs` 品牌 + provider Button（点击打开设置面板）+ 新建会话 + ⚙。
- **G-预览头+高亮（已落地）**：`file_panel.rs` `spawn_file_preview` 加 `fv-head`（路径 + `FilePreviewMetaMarker` 字节数 + ✕）+ `fv-body`；`.rs` 文件用 `xui::highlight` 高亮。
- **H-确认弹窗 diff（已落地）**：`confirm_dialog.rs` 重写为 modal（head/body/foot + 行级 diff）；`ConfirmRequest` 加 `old_content`/`new_content`，`Tool::preview_diff` 提供 diff 数据。
- **本轮修补（已落地，2026-07-20）**：流式光标迁到助手气泡正文末尾；工具卡片 pending/deny 态（`ToolCardMarker.tool_call_id` + `update_tool_pending` + `ToolResult.denied`）；`update_token_hint` 系统含 Aborting 态。
- **D-markdown/代码块（待实现）**：助手消息纯文本，原型含代码块+高亮。详见 `ui-gap-plan.md`。


---

## 6. 开发流程（与 AGENTS.md 第 6 节对齐）

1. **阅读背景**：开始任务前读 `AGENTS.md`、`doc/design/`、`doc/plans/` 中相关 step 文件；编码时按需查 `../bevy` 源码确认 API（bevy 仍在演进，有 breaking change）。
2. **先通后优**：每模块先最小可用版本，确保编译通过与基本功能，再迭代。
3. **接口先行**：先定义 trait 与协议类型，再实现具体逻辑。
4. **测试驱动**：每 crate 完成后写最小集成测试验证。
5. **不提前引入 3D/TUI/Web/宠物**：MVP 仅 2D GUI，P1 已引入编辑器。
6. **产出文档放 `doc/`**：设计/计划类文档放 `doc/` 对应分类目录，不放项目根。
7. **遗留清理**：根目录 `src/main.rs` 是遗留 Hello world，按计划应删除（`xgent_app` 已接管入口）。
8. **同步本指南**：**后续实现新功能或功能有变化，都需要更新 `doc/dev-tutorial.md`**（见 `AGENTS.md` 第 6 节新增条目）。

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
```

构建依赖本地 `../bevy` 源码（0.19.0），确保该目录存在。
