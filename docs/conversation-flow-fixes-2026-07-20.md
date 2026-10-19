# 对话流程问题诊断与修复（2026-07-20）

> 本文档记录对 XGent 对话流程（agent loop）的问题诊断、参考决策与修复内容，
> 供后续维护参考。对照源码 `/Users/xdo/ws/oh-my-pi`（omp）的 `agent-loop.ts` 工业级实现。

---

## 1. 问题清单

诊断基于 `crates/xgent_agent/src/{agent_loop.rs,bridge.rs,format.rs,conversation.rs}`、
`crates/xgent_ui/src/{chat_panel.rs,status_bar.rs}`、`crates/xgent_core/src/{chat.rs,session.rs}` 的代码审查。

### 1.1 致命问题（阻断核心功能）

| # | 问题 | 影响 | 根因 |
|:---|:---|:---|:---|
| P1 | **工具 schema 从未注入 LLM 请求** | LLM 无法发起工具调用，F-03 工具调用能力实际不可用 | `agent_loop.rs::agent_poll_system` 调 `build_request` 时传 `None`（注释"后续注入"但从未实现）；`ToolExecutor::schemas()` 存在却无人调用 |
| P2 | **上下文检索从未执行** | 项目目录树、相关文件片段从未注入 system prompt，LLM 对项目结构一无所知 | `ContextState` Resource 永远是 default 空；`OnDemandContextProvider::retrieve` 从未被调用；`AgentBridgeConfig.context` Arc 持有但 `agent_loop_task` 未使用 |
| P3 | **token 统计永远为 0** | UI 状态栏 token 计数始终 0 | `status_bar::track_token_usage` 在 `DoneMessage` 时读 `conv.current_assistant_text`，但 `handle_agent_event` 处理 `Done` 时**先** `finalize_assistant`（take 走文本）**后**发 `DoneMessage`，读到空字符串 |
| P4 | **AssistantMessage.usage 永远为 None** | provider 返回的 token usage 丢失，持久化的会话历史无 usage 数据 | `Conversation::finalize_assistant` 硬编码 `usage: None`，未接收 provider 的 `StreamOutcome.usage` |
| P5 | **conv.messages 缺 assistant tool_call 消息** | 多轮工具调用后，`conv.messages` 只有 tool result 没有 tool_call，下次 StartLoop 时 `convert_to_llm` 生成孤儿 tool result，OpenAI 协议要求 tool_call 与 tool_result 配对，会 400 拒绝 | `handle_agent_event` 的 `ToolCall` 分支只发 UI 消息，不 push 到 `conv.messages`；`AgentEvent::ToolCall` 也不带 `call_id`（只有 tool_id=工具名），无法配对 |
| P6 | **错误消息未持久化** | 出错后 session JSONL 无记录，恢复会话看不到失败点 | `handle_agent_event` 的 `Error` 分支只发 `ErrorMessage`，不调持久化；`SessionEntry` 无 Error 变体 |

### 1.2 鲁棒性问题（边界场景失效）

| # | 问题 | 影响 | 对照 omp |
|:---|:---|:---|:---|
| R1 | **steering 中断后半截文本与新回复拼接** | 用户插话后，已流的半截 assistant 文本与新一轮流式文本混在一起 | omp `runLoopBody` 在 steering 中断时把 partial assistant 消息固化为 aborted 边界，新一轮从空开始 |
| R2 | **tool_call 被中断未补占位 result** | OpenAI 要求每个 tool_call 必须有对应 tool_result，中断时不补会导致下次请求配对断裂 | omp `createAbortedToolResult` / `createSyntheticToolResultMessage` 为未完成调用补 skipped/aborted result |
| R3 | **StopReason::Length 截断时仍执行不完整 tool_calls** | max_tokens 截断时 tool_call 参数可能不完整，执行会失败或产生错误副作用 | omp：Length 时不执行 tool_calls，补占位 skipped result，让 LLM 重新生成完整调用 |

### 1.3 功能缺失

| # | 问题 | 影响 |
|:---|:---|:---|
| F1 | **无新建会话功能** | 会话 id 永远是 `SessionId(1)`，无法清空开始新对话；命令面板 `session.new` 是 TODO |
| F2 | **错误时无 UI 提示** | 出错时只显示错误文本，无"可继续"提示，用户不知能否重试 |

---

## 2. 参考决策（对照 omp 源码）

分析 `/Users/xdo/ws/oh-my-pi/packages/agent/src/agent-loop.ts`（2403 行）与
`/Users/xdo/ws/oh-my-pi/packages/coding-agent/src/prompts/` 的关键设计：

### 2.1 tool_call/tool_result 配对约束（omp `runLoopBody` line 951-1105）

omp 在 assistant 消息 `stopReason == "error" | "aborted"` 时，为每个未执行的 toolCall
创建 placeholder tool result（`createAbortedToolResult`），维持 API 的 tool_use/tool_result
配对不变。Length 截断（`stopReason === "length"`）时同样补 skipped result，不执行。

**决策**：xgent 采纳此约束——`AgentEvent::ToolCall` 与 `ToolResult` 都加 `call_id` 字段，
`Conversation` 新增 `push_tool_call`，`handle_agent_event` 在 ToolCall/ToolResult 分支都 push 到
`conv.messages`，保证下次 StartLoop 时消息序列符合 OpenAI 协议。

### 2.2 steering 中断的 partial assistant 消息（omp `streamAssistantResponse`）

omp 在流式被 steering 中断时，返回已收集的 partial assistant message，由 `runLoopBody`
固化为 aborted assistant 边界（`emitAbortedAssistantMessage`），然后 steering 作为新 user
消息，模型重新生成。这样历史完整，UI 不拼接。

**决策**：xgent 新增 `AgentEvent::SteeringInterrupted { partial_text }`，
`stream_llm_response` 累积 `partial_text` 并在 steering 中断时返回；
`run_agent_loop` 发 `SteeringInterrupted` 事件；`handle_agent_event` 把半截文本
`finalize_assistant` 为 assistant 消息并清空 `current_assistant_text`，复用 `DoneMessage`
让 UI 把半截文本固化为历史气泡。

### 2.3 token usage 传递链（omp `AssistantMessage.usage`）

omp 的 AssistantMessage 直接携带 usage，经 EventStream 传递到 UI 与持久化。

**决策**：xgent 的 `AgentEvent::Done` 改为 struct variant `{ usage, model }`，
`StreamOutcome` 保存 `last_usage`/`last_model`，`handle_agent_event` 传给
`finalize_assistant` 写入 `AssistantMessage.usage`，`DoneMessage` 也带 usage 供
`track_token_usage` 用真实 `prompt + completion` 累加。

### 2.4 上下文检索时机（omp `syncContextBeforeModelCall`）

omp 在每次 model call 前调 `config.syncContextBeforeModelCall` 刷新 context。

**决策**：xgent 的 ECS 系统同步，无法 await `context.retrieve`。改为在 bridge 异步侧
`agent_loop_task` 的 `StartLoop` 分支调 `cfg.context.retrieve()`，用结果调
`format::refresh_system_message` 覆盖 req 首条 system 消息。ECS 侧用空 `ContextResult`
占位构造 req，bridge 侧覆盖。

---

## 3. 修复内容

### 3.1 致命问题修复

#### P1：工具 schema 注入

- `AgentBridge` Resource 新增 `tool_schemas: Arc<Vec<ToolSchema>>` 字段，
  `AgentBridge::new` 启动时从 `cfg.executor.schemas()` 一次性提取（运行期工具集合不变）。
- `agent_loop.rs` 的 `build_request` 调用从 `None` 改为
  `Some(bridge.tool_schemas.as_ref().clone())`（StartLoop 与 FollowUp 两处）。

#### P2：上下文检索执行

- `format.rs` 抽出 `build_system_text(context)` 与 `refresh_system_message(req, context)`、
  `last_user_text(messages)` 三个函数。
- `agent_loop_task` 的 `StartLoop` 分支：用 `last_user_text` 从 req 提取最近 user 消息
  构造 `ContextQuery`，调 `cfg.context.retrieve(query)`，再 `refresh_system_message` 覆盖
  req 首条 system 消息。

#### P3 + P4：token usage 链路

- `AgentEvent::Done` 从 unit 变体改为 struct `{ usage: Option<TokenUsage>, model: Option<String> }`。
- `StreamOutcome` 新增 `partial_text` 字段；`run_agent_loop` 在内层循环记录 `last_usage`/`last_model`，
  正常完成时发带字段的 Done，abort 路径发 `Done { usage: None, model: None }`。
- `Conversation::finalize_assistant` 签名改为 `(usage, model)`，写入 `AssistantMessage.usage/model`。
- `DoneMessage` 加 `usage`/`model` 字段。
- `status_bar::track_token_usage` 改为读 `DoneMessage.usage` 的 `prompt + completion` 累加，
  不再读已清空的 `current_assistant_text`。

#### P5：conv.messages 工具调用配对

- `AgentEvent::ToolCall` 加 `call_id` 字段；`AgentEvent::ToolResult` 加 `call_id` 字段。
- `Conversation` 新增 `push_tool_call(call_id, tool_name, args)`：push 一条含
  `ContentBlock::ToolCall` 的 Assistant 消息。
- `handle_agent_event` 的 `ToolCall` 分支调 `conv.push_tool_call`；
  `ToolResult` 分支调 `conv.push_tool_result`（用 `call_id` 作 `tool_call_id` 配对）。
- bridge 侧发 `ToolCall`/`ToolResult` 事件时传 `call_id`。

#### P6：错误持久化

- `xgent_core::session::SessionEntry` 新增 `Error(ErrorEntry)` 变体；
  `ErrorEntry { id, parent_id, timestamp, kind: ErrorKind, message }`。
- `Conversation` 新增 `persist_error(kind, message)`：append 一条 `ErrorEntry`。
- `handle_agent_event` 的 `Error` 分支调 `conv.persist_error`。

### 3.2 鲁棒性修复

#### R1：steering 中断保留半截文本

- `AgentEvent` 新增 `SteeringInterrupted { partial_text }` 变体。
- `stream_llm_response` 累积 `partial_text`，steering 中断时返回 `Some(partial_text.clone())`。
- `run_agent_loop` 的 `pending_steering` 分支发 `SteeringInterrupted` 事件。
- `handle_agent_event` 的 `SteeringInterrupted` 分支：把 `partial_text` 赋给
  `current_assistant_text`，`finalize_assistant` 固化为 assistant 消息，清空当前节点，
  复用 `DoneMessage` 让 UI 把半截文本固化为历史气泡。

#### R2 + R3：tool_call 中断与 Length 截断补占位

- `run_agent_loop` 的 tool 执行 abort 分支已发 `ToolResult { is_error: true }`（原有）。
- 新增 `StopReason::Length` 分支：不执行 tool_calls，为每个补占位 skipped result
  （发 `ToolResult { is_error: true, output: "工具调用因 max_tokens 截断而未执行" }`，
  并回灌 assistant tool_call + tool result 到 `req.messages` 维持配对）。
- `StreamOutcome.stop_reason` 去掉 `#[allow(dead_code)]`，被 Length 判断使用。

### 3.3 功能补全

#### F1：新建会话

- `events.rs` 新增 `NewSessionMessage`（UI → agent）与 `SessionClearedMessage`（agent → UI）。
- `Conversation` 新增 `reset()`：用时间戳生成新 `SessionId`、清空 messages、
  `session_store = None`（下次首次对话重新打开）。
- `agent_poll_system` 新增 `NewSessionMessage` 处理（仅 Idle/Error 接受），
  调 `conv.reset()` + 发 `SessionClearedMessage`。
- `chat_panel.rs` 新增 `clear_on_new_session` 系统：收到 `SessionClearedMessage` 时
  `despawn_related::<Children>()` 清空消息列表 + 清空当前助手文本节点。
- `command_palette.rs` 的 `handle_palette_triggers` 实现 `session.new`：发 `NewSessionMessage`。

#### F2：错误时 UI 提示

- `chat_panel::on_error` 错误文本后追加 `"\n\n（重新输入可继续对话）"` 提示。
- Error 状态下用户输入已被 `agent_poll_system` 接受（白名单含 Error），功能本就可用，仅加视觉提示。

---

## 4. 变更文件清单

| 文件 | 变更类型 |
|:---|:---|
| `crates/xgent_core/src/session.rs` | 新增 `SessionEntry::Error` 变体与 `ErrorEntry` 结构 |
| `crates/xgent_agent/src/bridge.rs` | `AgentBridge` 加 `tool_schemas`；`AgentEvent::Done` 改 struct 变体带 usage/model；新增 `SteeringInterrupted`；`ToolCall`/`ToolResult` 加 `call_id`；`StreamOutcome` 加 `partial_text`；`agent_loop_task` StartLoop 调 retrieve + refresh_system；`run_agent_loop` 新增 Length 分支、steering 中断发事件；`stream_llm_response` 累积 partial_text |
| `crates/xgent_agent/src/format.rs` | 抽出 `build_system_text`、新增 `refresh_system_message`、`last_user_text` |
| `crates/xgent_agent/src/conversation.rs` | `finalize_assistant` 加 usage/model 参数；新增 `push_tool_call`、`persist_error`、`reset` |
| `crates/xgent_agent/src/events.rs` | `DoneMessage` 加 usage/model；新增 `NewSessionMessage`、`SessionClearedMessage` |
| `crates/xgent_agent/src/agent_loop.rs` | `build_request` 传 tool_schemas；`handle_agent_event` Done 传 usage/model、ToolCall/ToolResult push 到 conv、Error 持久化、SteeringInterrupted 处理；新增 NewSession 处理 |
| `crates/xgent_agent/src/lib.rs` | Plugin 注册 `NewSessionMessage`/`SessionClearedMessage` |
| `crates/xgent_ui/src/status_bar.rs` | `track_token_usage` 用真实 usage |
| `crates/xgent_ui/src/chat_panel.rs` | 新增 `clear_on_new_session`；`on_error` 加可继续提示 |
| `crates/xgent_ui/src/command_palette.rs` | `handle_palette_triggers` 实现 `session.new` |

---

## 5. 验证

- `cargo check --workspace`：通过（仅 4 个预存 warning，与本次无关）。
- `cargo test --workspace`：全部通过（agent crate 38 个测试 + 其余 crate 测试，0 failed），
  既有测试不回归。

---

## 6. 后续待办（未在本轮修复）

- **会话恢复**：`SessionStore::load_all` 已实现但无调用方，恢复会话历史到 `conv.messages` 留 P1。
- **FollowUp 时的上下文刷新**：当前 FollowUp 只追加 text 到 bridge 的 req.messages，
  不重新调 `context.retrieve`（StartLoop 才检索）。FollowUp 检索需把 `cfg.context` 传入
  `run_agent_loop`，改动较大，留后续。
- **多会话切换 UI**：当前只有"新建会话"，无会话列表切换（需 SQLite 索引，D-04）。
- **Abort 超时保护**：若 `AgentEvent::Done` 因 channel 满迟到，`Aborting` 状态可能卡住
  （实际 Done 会到，边界罕见，留观察）。
