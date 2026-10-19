# 0006-ChatEvent 细粒度流式事件与 StopReason

## 背景

当前 `xgent_core/src/chat.rs` 的 `ChatEvent` 只有 4 变体：`Delta{text}` / `ToolCall{id,name,args}` / `Done{usage}` / `Error{kind,message}`。差距：无 StopReason（agent loop 无法判断为何结束）、无细粒度事件（UI 无法区分 text_start/end、toolcall 流式增量）、ToolCall 是一次性全量（长参数无流式反馈）、无 Thinking 事件（不支持推理模型）。

优化文档 O1 给出细化方案（Start / TextStart/Delta/End / ThinkingStart/Delta/End / ToolCallStart/Delta/End / Done{reason,usage} / Error），但未明说旧变体（Delta/ToolCall/Done{usage}）是删除还是保留。

## 决策

**按优化文档 O1 方案细化 ChatEvent，旧变体（Delta/ToolCall/Done{usage}）clean cutover 删除，不保留兼容别名。新增 StopReason 枚举作为 Done 的字段。**

具体契约：

1. `ChatEvent` 变体集：`Start{model}` / `TextStart` / `TextDelta{text}` / `TextEnd` / `ThinkingStart` / `ThinkingDelta{text}` / `ThinkingEnd` / `ToolCallStart{index,id,name}` / `ToolCallDelta{index,partial_json}` / `ToolCallEnd{index,args}` / `Done{reason: StopReason, usage: TokenUsage}` / `Error{kind,message}`。
2. `StopReason` 枚举：`Stop` / `ToolUse` / `Length` / `Aborted` / `Error`。
3. **旧变体不保留**：`Delta`/`ToolCall`/`Done{usage}` 删除。同版本演进，UI 与 daemon 同步发版，不存在"旧客户端"。
4. `#[serde(tag = "type", rename_all = "camelCase")]` 保留——JSON-RPC notification 按 `type` 字段分发，新变体即新 type 值。
5. **StopReason 的消费者**：仅 UI 展示与错误恢复参考。agent loop **不依赖** reason 决定是否继续——`tool_calls.is_empty()` 才决定（对齐 omp §2.1）。

## 备选方案

### 方案 B：保留旧变体作兼容别名

`Delta`/`ToolCall`/`Done{usage}` 保留，新变体并行添加，provider 同时发新旧事件。

否决：同项目同版本演进，无旧客户端需要兼容；保留旧变体导致 provider 适配器双发事件、agent loop 双路径消费，维护负担与 bug 面增大，无收益。

### 方案 C：StopReason 放 UI 层而非 core 协议

StopReason 仅 UI 消费，不放 core 的 ChatEvent，而由 UI 侧从 finish_reason 自行映射。

否决：StopReason 是 provider 流式输出的固有语义，跨进程协议层携带合理；放 UI 侧则每个 UI 客户端重复映射逻辑；daemon 侧也需 StopReason 做错误恢复决策（如 Length 后是否重试）。

## 结论与后果

- **clean cutover**：`xgent_provider/src/openai_compat.rs` 的 SSE 解析重写为发射细粒度事件；`xgent_daemon/src/session.rs` 的 IPC notification 透传新事件类型（daemon 不解析 ChatEvent 内部，只透传 JSON）；`xgent_agent/src/bridge.rs` 的 `AgentEvent` 对齐新事件；`xgent_ui/src/chat_panel.rs` 渲染新事件。
- **MVP 不发射 Thinking 事件**：OpenAiCompat 不解析 reasoning_content，Thinking 变体留 P1 给 Anthropic 适配器。但 ChatEvent 枚举中 Thinking 变体已定义，provider 不发即 UI 不渲染。
- **ToolCallDelta 的 partial_json**：MVP 不做 throttled 解析（omp 的 parseStreamingJsonThrottled 留 P1），仅发原始 partial_json 字符串，UI 可选展示；ToolCallEnd 发全量 args。
- **OpenAiCompat 的 finish_reason → StopReason 映射**：`stop`→Stop / `tool_calls`→ToolUse / `length`→Length / abort→Aborted / error→Error。
- **此 ADR 与 ADR-0005 互依**：ChatEvent::ToolCallEnd 的 args 字段类型与 ContentBlock::ToolCall 的 args 一致（serde_json::Value）。
