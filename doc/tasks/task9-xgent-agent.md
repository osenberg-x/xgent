# Task 9: xgent_agent

> 对应实现指导：`doc/plans/step9-xgent-agent.md`
> 前置：step1 xgent_core、step3 xgent_settings_core、step4 xgent_settings、step5 xgent_provider、step7 xgent_tools、step8 xgent_context 已完成

## 任务清单

### 阶段一：脚手架

- [ ] T-9.1 创建 crate 目录与 Cargo.toml
  - 依赖：无
  - 验收：`crates/xgent_agent/Cargo.toml` 存在；依赖为 bevy、xgent_core、xgent_provider、xgent_tools、xgent_context、xgent_settings、xgent_settings_core、serde、serde_json、tokio、async-trait、thiserror；`cargo check -p xgent_agent` 通过。

- [ ] T-9.2 注册到 workspace
  - 依赖：T-9.1
  - 验收：`cargo metadata` 识别。

### 阶段二：ECS 事件契约

- [ ] T-9.3 实现 `events.rs`
  - 依赖：T-9.1
  - 验收：定义所有 Event：UserInputEvent、AbortEvent、DeltaEvent、ToolCallEvent、ToolResultEvent、ConfirmRequestEvent、ConfirmDecisionEvent、DoneEvent、ErrorEvent；编译通过。

### 阶段三：会话状态

- [ ] T-9.4 实现 `conversation.rs`
  - 依赖：T-9.3
  - 验收：定义 `Conversation` Resource（id/messages/status/current_assistant_text）与 `ConversationStatus`（Idle/Thinking/Streaming/ToolRunning/Confirming/Aborting/Error）；derive Resource；编译通过。

### 阶段四：异步桥接

- [ ] T-9.5 实现 `bridge.rs` 的 AgentBridge 与命令/事件类型
  - 依赖：T-9.3
  - 验收：定义 `AgentCommand`（StartLoop/Abort/ConfirmDecision）、`AgentEvent`（Delta/ToolCall/ToolResult/ConfirmRequest/Done/Error）、`AgentBridge` Resource（持有 tokio runtime + cmd_tx + event_rx）；编译通过。

- [ ] T-9.6 实现 ProviderClient（IPC 调 daemon 的封装）
  - 依赖：T-9.5
  - 验收：`ProviderClient` 封装 IPC 调用，`chat(req)` 经 daemon 返回 stream，把 ChatEvent 转 AgentEvent 喂 channel；编译通过（具体 IPC 连接可放 step12，此处用 trait 抽象便于 mock）。

### 阶段五：对话循环

- [ ] T-9.7 实现 `format.rs` 的 build_request
  - 依赖：T-8.3
  - 验收：`build_request(messages, context, provider, model, tools)` 组装 system message（角色 + 上下文注入）+ 历史 + tools；返回 ChatRequest；编译通过。

- [ ] T-9.8 实现 `agent_loop.rs` 的 agent_poll_system
  - 依赖：T-9.4, T-9.5, T-9.7
  - 验收：系统每帧处理 UserInput（构造 ChatRequest 发 StartLoop）、Abort、ConfirmDecision；非阻塞轮询 event_rx 分发 Delta/ToolCall/ToolResult/ConfirmRequest/Done/Error 到对应 EventWriter；更新 Conversation 状态；编译通过。

- [ ] T-9.9 实现工具调用桥接
  - 依赖：T-9.6, T-7.6
  - 验收：agent 异步 task 收到 ToolCall → 调 ToolExecutor（confirm 经 ConfirmRequest 回 ECS、决策经 channel 回 task）→ ToolResult 发回；编译通过。

- [ ] T-9.10 实现上下文检索注入
  - 依赖：T-9.7, T-8.3
  - 验收：新对话轮前异步调 `ContextProvider::retrieve`，结果注入 system message；不阻塞 ECS 帧；编译通过。

- [ ] T-9.11 实现中断
  - 依赖：T-9.8
  - 验收：Abort 命令经 channel 通知 task，task 取消 provider 流（drop receiver），状态回 Idle；编译通过。

### 阶段六：Plugin

- [ ] T-9.12 实现 `lib.rs` 的 XgentAgentPlugin
  - 依赖：T-9.3, T-9.4, T-9.5, T-9.8
  - 验收：Plugin 注册所有 Event、init Conversation/AgentBridge、add agent_poll_system 到 Update；编译通过。

### 阶段七：测试

- [ ] T-9.13 桥接测试（mock provider）
  - 依赖：T-9.12
  - 验收：mock ProviderClient 假流式输出，驱动 agent loop，断言 DeltaEvent 序列与 DoneEvent。

- [ ] T-9.14 工具调用测试（mock）
  - 依赖：T-9.9
  - 验收：mock provider 返回 ToolCall，断言 ToolCallEvent + ConfirmRequestEvent（需确认时）+ ToolResultEvent。

- [ ] T-9.15 中断测试
  - 依赖：T-9.11
  - 验收：对话中发 AbortEvent，agent 停止且状态回 Idle。

- [ ] T-9.16 上下文注入测试
  - 依赖：T-9.10
  - 验收：mock ContextProvider 返回固定 chunks，断言 ChatRequest 的 messages 含上下文。

## 完成标志

- `cargo check -p xgent_agent` 通过
- `cargo test -p xgent_agent` 全绿
- agent loop 经 ECS 桥接驱动，对话/工具调用/确认/中断可用
- 所有跨子系统通信走 Events，无直接方法调用
