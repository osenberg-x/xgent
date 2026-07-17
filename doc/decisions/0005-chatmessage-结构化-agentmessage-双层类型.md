# 0005-ChatMessage 结构化与 AgentMessage 双层类型

## 背景

优化文档 `doc/plans/optimization-from-omp.md` O2 原方案保留 `ChatMessage { role, content: String }` 作为 LLM 层类型，新增 `AgentMessage` 作为 agent 层类型，`convert_to_llm` 把 `AssistantMessage(Vec<ContentBlock>)` 压扁成 `content: String`。

此方案违背所借鉴的 omp 实际做法，且在协议层不合法：

1. **omp 的 LLM 层 `Message` 也是结构化的**：content 是 `ContentBlock[]`（text/tool_use/tool_result/image 等），`convertToLlm` 只过滤 UI-only 类型，保留结构。参见 `doc/notes/oh-my-pi-study.md` §2.2 上下文构建流水线第 2 步。
2. **OpenAI chat completions 协议要求**：assistant 调用工具后，下一条 assistant message 必须带 `tool_calls` 数组字段；`tool` role 消息必须带 `tool_call_id` 字段。当前 `openai_compat.rs:94-100` 的 `message_to_json` 对 `Role::Tool` 只发 `{role, content}`，**缺 `tool_call_id`**——连单轮 tool calling 都会被 OpenAI 拒绝（400 invalid_request_error）。
3. **Anthropic 协议原生就是 content block 数组**，结构化 ChatMessage 与之天然对齐。

## 决策

**`ChatMessage` 改造为结构化：`role + Vec<ContentBlock>`，与 Anthropic 协议原生形态对齐；`AgentMessage` 作为 agent 层类型，含 UI-only 变体。`convert_to_llm` 只过滤 UI-only 类型，保留结构化 content。**

具体契约：

1. `xgent_core/src/chat.rs` 中 `ChatMessage` 改为 `{ role: Role, content: Vec<ContentBlock> }`。
2. `ContentBlock` 枚举：`Text { text }` / `ToolCall { id, name, args }` / `ToolResult { tool_call_id, content, is_error }` / `Image { data, mime_type }`。
3. `AgentMessage` 枚举：`User` / `Assistant` / `ToolResult` / `Notification`（UI-only，不发给 LLM）。
4. `convert_to_llm(&[AgentMessage]) -> Vec<ChatMessage>`：过滤 `Notification`，`Assistant(Vec<ContentBlock>)` 直接作为 `ChatMessage.content`（不压扁），`ToolResult` 转为 `Role::Tool` + `ContentBlock::ToolResult`。
5. `OpenAiCompatProvider::message_to_json` 按 role 展开：
   - `Role::Assistant` + 含 `ToolCall` block → `{role:"assistant", content: <text 部分>, tool_calls: [...]}`（OpenAI 协议要求 tool_calls 是顶层字段，非 content 内）
   - `Role::Tool` + `ToolResult` block → `{role:"tool", content, tool_call_id}`
   - `Role::User`/`System` → `{role, content: <text 拼接>}`

## 备选方案

### 方案 B：最小修补（仅加 `tool_call_id` 字段到 ChatMessage）

保留 `content: String`，assistant 的 tool_calls 序列化为 JSON 字符串塞 content，靠 provider 适配器解析。

否决：不符 omp 实际做法；多模态（image）受限；每个 provider 适配器都要重复解析逻辑；与 Anthropic 原生协议形态不一致，未来加 Anthropic 适配器要重写。

### 方案 C：暂搁 O2，先做 O1+O4

承认 O2 原方案不完整，先把 AgentMessage 引入让 Conversation 持有结构化消息，协议级正确性留到 provider 改造时一并解决。

否决：期间跑不通多轮 tool calling——O4 双层循环的内层循环依赖 tool 结果回灌为合法 LLM message，O2 不正确则 O4 无法验证。O2 必须先于 O4 完成且协议正确。

## 结论与后果

- **协议正确性优先**：ChatMessage 结构化是 multi-turn tool calling 的前置条件，非过度设计。
- **影响范围**：`xgent_core/src/chat.rs`（ChatMessage + ContentBlock + AgentMessage 体系 + convert_to_llm）、`xgent_provider/src/openai_compat.rs`（message_to_json 重写）、`xgent_agent/src/conversation.rs`（messages 改 Vec<AgentMessage>）、`xgent_agent/src/bridge.rs`（调用前 convert_to_llm）、`xgent_agent/src/format.rs`（build_request 用 AgentMessage）。
- **MVP 不实现 Image block 的 UI 上传**：ContentBlock::Image 类型定义保留，但 MVP 无图片输入 UI，OpenAiCompat 的 message_to_json 对 Image block 暂不展开（遇则报 `ProviderError::Config`）。
- **此 ADR 取代优化文档 O2 §3.2 的原方案**：O2 章节需据此修订。
