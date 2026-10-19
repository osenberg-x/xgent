# 0001-provider-就绪闸门权威源定-daemon-侧

## 背景

F-01（多轮对话）链路在代码结构上完整（UserInput → agent_loop → IPC → daemon ProviderPool → LlmProvider → SSE 回流 → UI），但有两个可用性断点导致"不能对话"：

1. **保存路径整段缺失**：`settings_panel::handle_save_button` 发出 `SaveProviderConfigMessage`，但全代码库无任何 `MessageReader` 消费它——daemon 的 `config.write` handler 存在但无 UI 侧调用方。用户点保存后配置永远停在 UI 内存，daemon 全局配置永不变。
2. **无就绪闸门**：`agent_poll_system` 在 `UserInputMessage` 到达时只判 `ConversationStatus::Idle`，不判 provider 是否就绪。未配 provider 时 `derive_provider_model` 返回空串，`ProviderPool::get("")` 返回 `Err("配置中无 provider: ")`，经 IPC 回流成 `PROVIDER_ERROR`，UI 显示"未知错误"——比禁发更糟，假装成功并污染会话历史。

补断点时浮现一个架构选择：**provider 就绪闸门的权威源定哪侧？** UI 侧 `ProviderInfo` Resource（启动时一次性注入后无人更新），还是 daemon 侧 `ConfigCoordinator`（全局配置权威副本）。

决定性事实：`CONFIG_CHANGED` 通知的 UI 侧接收管道已接通（`fs_event_bridge::pump_notifications` 每帧拉取 daemon 广播，转成 `ConfigChangedMessage`），但全库无 `MessageReader<ConfigChangedMessage>`（该 Message 标了 `#[allow(dead_code)]`）——接收通了，消费端缺。

## 决策

**闸门权威源定 daemon 侧。** UI 侧 `ProviderInfo` Resource 视为 daemon 全局配置的缓存投影，所有刷新经 daemon 广播的 `CONFIG_CHANGED` 触发。

具体链路：
1. 补 `SaveProviderConfigMessage` handler：经 IPC `config.write` 写 daemon 全局配置。
2. daemon `config.write` 成功后广播 `CONFIG_CHANGED`（已有逻辑，`session.rs:211-217`）。
3. 已有的 `pump_notifications` 接收 `CONFIG_CHANGED` 转 `ConfigChangedMessage`。
4. 补一个 reader 系统消费 `ConfigChangedMessage`：重读 daemon `config.read("default_provider")` + `config.read("providers.<id>")`，按就绪判据刷新 UI 侧 `ProviderInfo` Resource。
5. `agent_poll_system` 在 `UserInputMessage` 到达时增加闸门：判 `ProviderInfo` 是否就绪（就绪判据见 CONTEXT.md「Provider 就绪」）。未就绪时不构造请求、不 `push_user`、不进入 `Thinking`，发引导消息提示用户配置 provider。

## 备选方案

### 路径 B：UI 本地写（乐观刷新）

`SaveProviderConfigMessage` handler 直接在 UI 侧更新 `ProviderInfo` Resource（乐观刷新），同时异步 `config.write` 写 daemon 做持久化。权威在 UI，daemon 只是落地。

否决理由：违反 NF-02 多开一致性。窗口 A 乐观刷新自己的 `ProviderInfo` 后，窗口 B 要等窗口 A 的 `config.write` 广播 `CONFIG_CHANGED` 才能刷新——但广播到达前窗口 B 的 `ProviderInfo` 仍是陈旧态，若窗口 B 在此窗口内发对话，会走到 `ProviderPool::get(陈旧 id)`，行为不确定。路径 A 下所有窗口统一靠 daemon 广播刷新，无乐观假设，多开行为一致。

## 结论与后果

- **多开一致**：所有窗口的 `ProviderInfo` 刷新同源（daemon 广播），无乐观窗口。
- **体验代价**：保存配置后到 daemon 广播回写 UI 有数十毫秒延迟，期间 `ProviderInfo` 仍是旧值，闸门会拦住发送——用户看到"保存后输入框仍禁用半秒再解禁"。可接受，因这是正确性换取的必要代价。
- **就绪判据按 ProviderKind 分**（见 CONTEXT.md）：`Ollama` 下 `api_base` 非空即就绪（本地无 key）；其余 kind 需 `api_base` 与 `api_key` 均非空。"配置存在"（map 有 key）不等于"就绪"。
- **遗留**：`model` 字段的二义性（写入 `model_overrides` 还是 `default_model`）未在此 ADR 钉死，留 grilling 第 2 轮。
