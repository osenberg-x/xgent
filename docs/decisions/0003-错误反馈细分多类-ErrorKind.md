# 0003-错误反馈细分多类-ErrorKind

## 背景

F-01 错误反馈现状两个问题：

1. **错误无分类**：`ErrorMessage(pub String)` 扁平。`run_conversation`（`bridge.rs:183-224`）两条错误路径都丢类型——行 185 `provider.chat()` 返 `Err(String)`，行 221 `ChatEvent::Error{message:String}`。`ProviderError` 的 4 变体（Network/Api/Stream/Config）在 `ProviderClient` trait 边界被压扁成 String。闸门拦截（ADR 0001）是第三条纯本地错误路径。三类错误源全压成一个字符串，UI 无法区分"该配 provider"还是"该重试"。

2. **错误污染历史**：`AgentEvent::Error` 只设 `status=Error` + 发 `ErrorMessage`，不调 `finalize_assistant`。但 `on_error`（`chat_panel.rs:281-295`）把错误文本写进 `CurrentAssistantText` 节点。若 provider 出错后仍发 `Done`，`finalize_on_done` 会把错误文本固化成助手历史消息；即使不发 done，错误气泡也悬在消息列被当历史。**错误不是历史。**

## 决策

**`ErrorMessage` 升级为带 `ErrorKind` 分类的 enum，细分多类。变体按"用户可采取的行动"划分，不按底层错误源或 HTTP 状态码划分。**

具体契约：

1. `ErrorMessage` 改为 `ErrorMessage { kind: ErrorKind, message: String }`。`message` 始终带可读原文供排查。

2. `ErrorKind` 起始变体（见 CONTEXT.md「ErrorKind」）：
   - `NotConfigured`：闸门本地拦截（provider 未就绪）。
   - `AuthFailed`：provider 鉴权失败（API key 错/失效）。
   - `Network`：连接/超时，可重试。
   - `StreamParse`：SSE/JSON 解析失败，可重试。
   - `ProviderError`：provider 返回非鉴权类错误，含原始 message。

3. **UI 不感知 HTTP 状态码**：`ProviderError::Api{status, body}` 的 `status` 不透给 UI。daemon 侧（`ProviderPool` 或 `IpcProviderClient`）负责映射：`status` 为 401/403 → `AuthFailed`；其余 → `ProviderError`。`ProviderError::Network` → `ErrorKind::Network`；`Stream` → `StreamParse`；`Config` → `NotConfigured`。

4. **`ProviderClient::chat` 返类型从 `Result<_, String>` 改为 `Result<_, ProviderError>`**（或带 `ErrorKind` 的新错误类型），让类型信息跨 IPC 边界保留。daemon `provider.chat` JSON-RPC 响应需序列化 `ErrorKind` 字段。

5. **错误不进历史**：`on_error` 把错误渲染到独立 transient 节点（非 `CurrentAssistantText`），不调用 `finalize_assistant`，不被 `finalize_on_done` 固化。下一个 `UserInputMessage` 到达时清空错误节点。`Conversation::messages` 只收用户与助手的真实往返，错误是旁路提示。

## 备选方案

### 不分类（保持 `ErrorMessage(String)`）

实现零改动。否决：用户分不清"该配 provider"还是"该重试"，F-01 体验不可用。

### 粗分两类（Config | Transient）

UI 只需回答"配 provider 还是重试"。否决：用户选了细分多类。细分能区分"鉴权失败（改 key）"与"网络超时（直接重试）"，粗分把这两类压一起，鉴权失败时提示"重试"会反复 401。

## 结论与后果

- **改动面**：`ErrorMessage` 类型变（波及 `events.rs` 定义 + `agent_loop.rs` 构造 + `chat_panel.rs` reader）；`ProviderClient` trait 返类型变（波及 `bridge.rs` + `provider_client.rs` + daemon `provider_pool.rs`）；新增 daemon 侧 `ProviderError→ErrorKind` 映射。属跨 crate 的类型签名变更，但集中在对话链路，不波及 tools/context。
- **错误 UI 契约**：`NotConfigured`/`AuthFailed` → 渲染错误 + 引导按钮（开 settings_panel）；`Network`/`StreamParse` → 渲染错误 + 重试按钮（重发上一条 `UserInputMessage`）；`ProviderError` → 渲染错误原文 + 无行动按钮。
- **错误不进历史**：修复为独立 transient 节点，`Conversation::messages` 不收错误。`Conversation::status` 在 `Error` 态时允许重发（`forward_input_submission` 行 304 已认 `Error` 可重发，机制在）。
- **IPC 序列化**：daemon `provider.chat` 的 `PROVIDER_ERROR` notification 需加 `kind` 字段（序列化 `ErrorKind`），`IpcProviderClient` 反序列化时构造对应 `ErrorMessage`。
