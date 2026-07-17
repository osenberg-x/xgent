# 优化方案执行任务计划

> 基于 [优化方案](optimization-from-omp.md) 与 grilling 产出的 4 个 ADR（[0005](../decisions/0005-chatmessage-结构化-agentmessage-双层类型.md)~[0008](../decisions/0008-会话存储-jsonl-决策.md)），制定分阶段执行任务清单。
>
> 状态：执行任务计划 · 2026-07-17

---

## 0. 执行原则

1. **依赖顺序优先**：P0 四项有严格依赖（O1→O2→O3→O4），不可并行。
2. **每项有验收**：每项任务明确"完成的可观察证据"，非"编译通过"。
3. **clean cutover**：破坏性变更不留兼容别名、不保留旧 trait/变体（见 ADR-0005/0006/0007）。
4. **每项完成后 cargo check -p <crate>**：跨 crate 改动需全 workspace check。
5. **每项完成后 commit**：原子提交，消息中文，引用 ADR 编号。

---

## 阶段一：P0 核心 agent 能力

目标：agent 能真正多轮对话、工具调用后自动继续、可中断、有流式进度。

### T1: O1 ChatEvent 细化 + StopReason

**依赖**：无（基础协议类型）
**ADR**：[0006](../decisions/0006-chatevent-细粒度流式事件-clean-cutover.md)
**影响 crate**：xgent_core, xgent_provider, xgent_daemon, xgent_agent, xgent_ui

**子任务**：

- T1.1 `xgent_core/src/chat.rs`：ChatEvent 重构为 12 变体（Start/TextStart/TextDelta/TextEnd/ThinkingStart/ThinkingDelta/ThinkingEnd/ToolCallStart/ToolCallDelta/ToolCallEnd/Done{reason,usage}/Error），删除旧 Delta/ToolCall/Done{usage}。新增 StopReason 枚举（Stop/ToolUse/Length/Aborted/Error）。
- T1.2 `xgent_provider/src/openai_compat.rs`：`handle_chunk` 重写，SSE delta.content→TextDelta；delta.tool_calls 按 index 聚合→ToolCallStart/Delta/End；finish_reason→StopReason 映射；流首 emit Start{model}。
- T1.3 `xgent_provider/src/sse.rs`：辅助 tool_calls delta 聚合（按 index 累积 partial JSON）。
- T1.4 `xgent_daemon/src/session.rs`：IPC notification 透传新事件类型（daemon 不解析 ChatEvent 内部，只透传 JSON）。
- T1.5 `xgent_agent/src/bridge.rs`：AgentEvent 对齐新 ChatEvent（Delta(String)→TextDelta、ToolCall→ToolCallEnd 等）。
- T1.6 `xgent_agent/src/events.rs`：新增 ThinkingMessage、ToolCallUpdateMessage 等 Bevy Message。
- T1.7 `xgent_ui/src/chat_panel.rs`：渲染新事件（thinking 块、工具调用流式预览）。

**验收**：
- `cargo check --workspace` 通过。
- 单元测试：OpenAI SSE fixture（含 tool_calls 分块）解析为正确的 ChatEvent 序列。
- StopReason 映射测试：finish_reason=stop/tool_calls/length 分别映射 Stop/ToolUse/Length。
- `cargo run -p xgent_app` 跑一次纯文本对话，UI 显示 TextDelta 流式。

---

### T2: O2 AgentMessage + ChatMessage 结构化

**依赖**：T1（ChatEvent 的新 ToolCallEnd 产出 ContentBlock::ToolCall 兼容的 args 类型）
**ADR**：[0005](../decisions/0005-chatmessage-结构化-agentmessage-双层类型.md)
**影响 crate**：xgent_core, xgent_provider, xgent_agent, xgent_ui

**子任务**：

- T2.1 `xgent_core/src/chat.rs`：ChatMessage 改为 `{ role, content: Vec<ContentBlock> }`。新增 ContentBlock 枚举（Text/ToolCall/ToolResult/Image）。新增 AgentMessage 枚举（User/Assistant/ToolResult/Notification）+ 4 个子 struct。新增 `convert_to_llm(&[AgentMessage]) -> Vec<ChatMessage>`。
- T2.2 `xgent_provider/src/openai_compat.rs`：`message_to_json` 重写，按 role 展开（assistant+ToolCall→content+tool_calls 顶层字段；Tool→role:tool+content+tool_call_id；User/System→content 文本拼接）。修复当前缺 tool_call_id 的协议 bug。
- T2.3 `xgent_agent/src/conversation.rs`：`Conversation.messages` 从 `Vec<ChatMessage>` 改为 `Vec<AgentMessage>`。append_user/append_assistant/append_tool_result 改用 AgentMessage 变体。
- T2.4 `xgent_agent/src/bridge.rs`：`run_conversation` 调用 provider 前经 `convert_to_llm` 转换。流式事件累积为 AssistantMessage.content（Vec<ContentBlock>）。
- T2.5 `xgent_agent/src/format.rs`：`build_request` 接收 `&[AgentMessage]`，内部 convert_to_llm 后构造 ChatRequest。
- T2.6 `xgent_ui/src/chat_panel.rs`：渲染 AgentMessage 体系（Notification 变体显示为系统通知样式，区别于 user/assistant 气泡）。

**验收**：
- `cargo check --workspace` 通过。
- 单元测试：`convert_to_llm` 过滤 Notification、Assistant(Vec<ContentBlock>) 保留结构、ToolResult 转 Role::Tool+ToolResult block。
- 单元测试：`message_to_json` 对 assistant 含 ToolCall 产出带 `tool_calls` 顶层字段的 JSON；对 Tool role 产出带 `tool_call_id` 的 JSON。
- `cargo run -p xgent_app` 跑一次单轮 tool calling（如让模型调 read_file），请求 body 含合法 tool_calls/tool_call_id（可用日志或抓包验证）。

---

### T3: O3 Tool trait 增强

**依赖**：T2（ToolError 的错误回灌路径需要 AgentMessage::ToolResult）
**ADR**：[0007](../decisions/0007-tool-trait-tier-approval-signal-破坏性重构.md)
**影响 crate**：xgent_tools, xgent_agent

**子任务**：

- T3.1 `xgent_tools/Cargo.toml`：加 `tokio-util = { workspace = true }`。
- T3.2 `xgent_tools/src/tool.rs`：新增 `Concurrency`/`ToolTier`/`ToolError` 类型。Tool trait 重构：删 `fn policy()`，加 `fn tier()`/`fn approval_for(&Value)`/`fn concurrency()`，execute 签名改为 `async fn execute(&self, input, ctx, signal: CancellationToken, on_update: Option<&ToolUpdateCallback>) -> Result<ToolResult, ToolError>`。ToolResult 的 `success` 改 `is_error`（语义反转，对齐 omp）。新增 `ToolUpdateCallback` 类型。
- T3.3 `xgent_tools/src/security.rs`：`resolve_policy(tool_id, tier, input, tool, policy) -> SecurityPolicy`，按"配置 denied→配置 approved→tool.approval_for 动态 tier→MVP 默认全 NeedsConfirmation"顺序。
- T3.4 `xgent_tools/src/executor.rs`：`ToolExecutor::execute` 传 CancellationToken + on_update 给 Tool::execute。并发调度：Shared 工具可并行（tokio::join_all），Exclusive 串行。ToolError::Aborted 让 agent loop 走 abort 路径，Failed/Timeout 走错误回灌。
- T3.5 `xgent_tools/src/builtins/read_file.rs`：impl Tool 新签名，tier=Read, concurrency=Shared, execute 传 signal。
- T3.6 `xgent_tools/src/builtins/write_file.rs`：tier=Write, concurrency=Exclusive。
- T3.7 `xgent_tools/src/builtins/search_files.rs`：tier=Read, concurrency=Shared, execute 传 signal + on_update（进度）。
- T3.8 `xgent_tools/src/builtins/run_command.rs`：tier=Exec, concurrency=Exclusive。`approval_for` 检测 `rm -rf`/`sudo`/`mkfs` 始终返回 Exec。signal 传子进程（`tokio::select!` 监听 cancel + kill child）。on_update 推送 stdout 增量。
- T3.9 `xgent_agent/src/bridge.rs`：`run_conversation` 传 CancellationToken 给 executor。

**验收**：
- `cargo check --workspace` 通过。
- 单元测试：resolve_policy 的 4 种路径（denied/approved/动态 tier/默认）。
- 单元测试：ReadFile/SearchFiles 可并行（两个 Shared 工具同时 execute 用 tokio::join 完成）；WriteFile 串行（Exclusive 工具排队）。
- 单元测试：RunCommand 的 approval_for 对 `rm -rf /` 返回 Exec，对普通命令返回 Exec（默认）。
- 单元测试：CancellationToken cancel 后，RunCommand 的子进程被 kill，execute 返回 ToolError::Aborted。

---

### T4: O4 Agent Loop 双层循环 + abort signal

**依赖**：T1（新 ChatEvent）、T2（AgentMessage 用于消息回灌）、T3（ToolExecutor 传 signal）
**影响 crate**：xgent_agent, xgent_ui

**子任务**：

- T4.1 `xgent_agent/src/bridge.rs`：`run_conversation` 改为 `run_agent_loop` 双层循环。外层 follow-up 驱动，内层 tool-call + steering。`stream_llm_response` 用 `tokio::select!` 监听 ChatEvent 与 CancellationToken。工具结果回灌为 AgentMessage::ToolResult（经 convert_to_llm 转换）。
- T4.2 `xgent_agent/src/bridge.rs`：AgentCommand 加 `Steering(ChatMessage)` / `FollowUp(ChatMessage)` 变体。Abort 调用 `cancel_token.cancel()`。
- T4.3 `xgent_agent/src/agent_loop.rs`：`agent_poll_system` 处理新 AgentCommand 变体（Steering/FollowUp 经 channel 发给 task）。
- T4.4 `xgent_agent/src/events.rs`：新增 `SteeringMessage` Bevy Message。
- T4.5 `xgent_ui/src/chat_panel.rs`：用户在 agent 执行中输入框发 SteeringMessage（注入到当前对话，不中断工具）；agent 停止后输入发 FollowUp。

**验收**：
- `cargo check --workspace` 通过。
- 集成测试：模拟 provider 返回 ChatEvent 序列（TextDelta + ToolCallEnd + Done{ToolUse}），验证内层循环自动执行工具并继续下一轮 LLM 调用，直到 Done{Stop}。
- 集成测试：CancellationToken cancel 后，stream_llm_response 立即返回空 tool_calls，循环退出，发 AgentEvent::Done。
- 集成测试：Steering 消息在内层循环工具完成后注入到 req.messages（不中断正在执行的工具）。
- `cargo run -p xgent_app` 跑一次多轮 tool calling（如让模型 read_file 后基于内容回答），无人工干预自动完成。

---

## 阶段二：P1 体验增强

目标：会话持久化、提示词模板化、Approval 动态化、Provider 流式增强。各项独立性强，可并行。

### T5: O5 会话持久化 JSONL

**依赖**：T2（SessionMessage.message 用 AgentMessage 类型）
**ADR**：[0008](../decisions/0008-会话存储-jsonl-决策.md)
**影响 crate**：xgent_core, xgent_agent

**子任务**：

- T5.1 `xgent_core/src/session.rs`：新增 SessionEntry/Header/Message/ModelChange 类型。
- T5.2 `xgent_core/src/lib.rs`：导出 session 模块。
- T5.3 `xgent_agent/src/session_store.rs`：新增 SessionStore（open/append/load_all）。同步 append（writeln 即持久化）。
- T5.4 `xgent_agent/src/conversation.rs`：会话开始 append Header；每次 AssistantMessage 完成（Done）append Message entry。
- T5.5 `xgent_agent/src/bridge.rs`：agent_loop_task 持有 SessionStore。

**验收**：
- `cargo check --workspace` 通过。
- 单元测试：append 3 条 entry 后 load_all 返回相同 3 条（serde round-trip）。
- `cargo run -p xgent_app` 跑一次对话后，`<platform_path>/xgent/sessions/<dir_encoded>/*.jsonl` 文件存在且可 `cat` 看到合法 JSONL。

---

### T6: O6 系统提示词模板化

**依赖**：T2（build_request 用 AgentMessage）
**影响 crate**：xgent_agent

**子任务**：

- T6.1 `crates/xgent_agent/src/prompts/system.md`：新增（include_str! 内联）。
- T6.2 `crates/xgent_agent/src/prompts/project-context.md`：新增。
- T6.3 `xgent_agent/src/format.rs`：`build_request` 用 `include_str!` 加载模板，`format!` 注入项目上下文。系统提示词作为首条 `ChatMessage{role:System, content:[ContentBlock::Text]}`。

**验收**：
- `cargo check --workspace` 通过。
- 单元测试：build_request 产出首条为 Role::System + ContentBlock::Text 含模板内容 + 项目上下文。
- `cargo run -p xgent_app` 跑一次对话，日志或抓包可见系统提示词含模板与项目上下文。

---

### T7: O7 工具 Approval 动态化

**依赖**：T3（resolve_policy 已在 T3.3 实现）
**影响 crate**：xgent_tools, xgent_settings_core

**子任务**：

- T7.1 `xgent_settings_core`：`ToolPolicyConfig` 确认 approved/denied 字段（若已有则跳过）。
- T7.2 `xgent_tools/src/security.rs`：补 resolve_policy 单元测试覆盖 ToolTier×配置矩阵。
- T7.3 文档：`doc/plans/step7-xgent-tools.md` 更新 Approval 决议路径说明。

**验收**：
- `cargo check --workspace` 通过。
- 单元测试：Read/Write/Exec 三 tier 在 approved/denied/默认配置下的 9 种组合决议正确。

---

### T8: O8 Provider 流式增强

**依赖**：T1（细粒度事件已在 T1.2 实现）
**影响 crate**：xgent_provider

**子任务**：

- T8.1 `xgent_provider/src/openai_compat.rs`：加 Stream 超时（首事件 + idle），用 `tokio::time::timeout` 或 `tokio_util::timeout::Timeout`。超时发 ChatEvent::Error{kind:Network, message:"stream timeout"}。
- T8.2 `xgent_provider/src/openai_compat.rs`：Start 事件 emit model 字段。
- T8.3 单元测试：mock SSE 慢流（首事件延迟）触发超时。

**验收**：
- `cargo check --workspace` 通过。
- 单元测试：首事件超时与 idle 超时分别触发 ChatEvent::Error{Network}。
- `cargo run -p xgent_app` 对慢响应 provider 不再 hang，超时后 UI 显示网络错误。

---

## 阶段三：P2 预留接口

目标：仅定义 trait，不实现。不阻塞 MVP。

### T9: O9 CompactionProvider trait 预留

**依赖**：T2（AgentMessage 类型）
**影响 crate**：xgent_agent

**子任务**：

- T9.1 `xgent_agent/src/compaction.rs`：新增 `CompactionProvider` trait + `CompactionResult`/`CompactionError` 类型。
- T9.2 `xgent_agent/src/lib.rs`：导出 compaction 模块。

**验收**：
- `cargo check -p xgent_agent` 通过。
- 无实现，trait 定义编译通过即可。

---

### T10: O10 McpTransport trait 预留

**依赖**：无
**影响 crate**：xgent_tools

**子任务**：

- T10.1 `xgent_tools/src/mcp.rs`：新增 `McpTransport` trait。
- T10.2 `xgent_tools/src/lib.rs`：导出 mcp 模块。

**验收**：
- `cargo check -p xgent_tools` 通过。
- 无实现，trait 定义编译通过即可。

---

## 依赖关系图

```
T1 (O1 ChatEvent) ──┬──→ T2 (O2 AgentMessage) ──┬──→ T4 (O4 Agent Loop)
                    │                            │
                    │                            ├──→ T5 (O5 JSONL)
                    │                            ├──→ T6 (O6 提示词)
                    │                            └──→ T9 (O9 Compaction)
                    │
                    └──→ T8 (O8 Provider 增强)

T3 (O3 Tool trait) ──→ T4 (O4 Agent Loop)
                   └──→ T7 (O7 Approval)

T10 (O10 MCP) 独立
```

**关键路径**：T1 → T2 → T3 → T4（P0 串行，不可并行）。
**可并行**：T5/T6/T7/T8（P1，依赖 P0 完成后可并行）；T9/T10（P2，独立）。

---

## 文档更新清单（所有任务完成后）

| 文档 | 更新内容 |
|:---|:---|
| `doc/design/architecture.md` | §6.4 会话存储改 JSONL（ADR-0008）；§6.1 ChatEvent 细化（ADR-0006）；§6.2 Tool trait 增强（ADR-0007） |
| `doc/plans/step1-xgent-core.md` | ChatEvent 新变体、StopReason、AgentMessage 体系、ContentBlock、convert_to_llm |
| `doc/plans/step5-xgent-provider.md` | 流式细粒度事件、StopReason 映射、Stream 超时、message_to_json 按 role 展开 |
| `doc/plans/step7-xgent-tools.md` | Tool trait 新签名、ToolTier、Concurrency、ToolError、resolve_policy |
| `doc/plans/step9-xgent-agent.md` | 双层循环、abort signal、steering、SessionStore |

---

## 风险与缓解

| 风险 | 缓解 |
|:---|:---|
| T1 ChatEvent 重构跨 5 crate，连锁改动大 | 一次性完成，cargo check --workspace 每子任务后验证；旧变体 clean cutover 不留双路径 |
| T2 ChatMessage 结构化是协议级变更，影响 provider 适配器 | ADR-0005 已定案，message_to_json 按 role 展开修复 tool_call_id bug；单元测试覆盖 OpenAI 协议形态 |
| T3 Tool trait 破坏性变更影响 4 工具+executor+security+bridge | ADR-0007 已定案 clean cutover；影响面 4+3（文档原估漏 security.rs） |
| T4 双层循环复杂度 | MVP 简化：steering 非中断（工具完成后注入），不实现 omp 的消费/非消费队列分离、pause gate、soft tool requirement |
| T2 与 T4 的循环依赖假象 | T2 先于 T4：T4 的消息回灌需 T2 的 AgentMessage::ToolResult，故 T2 必须先完成 |
