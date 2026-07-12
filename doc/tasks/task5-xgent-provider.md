# Task 5: xgent_provider

> 对应实现指导：`doc/plans/step5-xgent-provider.md`
> 前置：step1 xgent_core、step3 xgent_settings_core 已完成

## 任务清单

### 阶段一：脚手架

- [ ] T-5.1 创建 crate 目录与 Cargo.toml
  - 依赖：无
  - 验收：`crates/xgent_provider/Cargo.toml` 存在；依赖为 xgent_core、xgent_settings_core、serde、serde_json、tokio、reqwest、async-trait、eventsource-stream、thiserror、futures-core；**不依赖 bevy**；`cargo check -p xgent_provider` 通过（空 lib.rs）。

- [ ] T-5.2 注册到 workspace
  - 依赖：T-5.1
  - 验收：`cargo metadata` 识别该 crate。

### 阶段二：抽象 trait

- [ ] T-5.3 实现 `provider.rs` 的 LlmProvider trait
  - 依赖：T-5.1
  - 验收：定义 `LlmProvider`（`#[async_trait]`，`id()`、`list_models()`、`chat(req) -> Result<(StreamId, ChatStream)>`、`health_check()`）、`ChatStream = mpsc::Receiver<ChatEvent>`、`ModelInfo`、`ProviderError`（Network/Api{status,body}/Stream/Config）；编译通过。

### 阶段三：SSE 解析辅助

- [ ] T-5.4 实现 `sse.rs`
  - 依赖：T-5.1
  - 验收：实现 `parse_sse_stream(body)` 把 reqwest 字节流转成 SSE 事件流再 parse 为 `serde_json::Value`；编译通过。

- [ ] T-5.5 验证 SSE 解析（mock）
  - 依赖：T-5.4
  - 验收：用 `futures::stream::iter` 构造 OpenAI 风格 SSE 文本（多个 `data: {...}\n\n`），喂给 parse 函数，断言解析出正确的 JSON Value 序列。

### 阶段四：OpenAI compatible 适配器

- [ ] T-5.6 实现 `openai_compat.rs` 结构与 list_models
  - 依赖：T-5.3
  - 验收：定义 `OpenAiCompatProvider`（id/api_base/api_key/client: reqwest::Client）；impl `LlmProvider` 的 `id()`、`list_models()`（GET {api_base}/models，Bearer 认证，解析 data[].id）、`health_check()`；编译通过。

- [ ] T-5.7 实现 chat 流式
  - 依赖：T-5.4, T-5.6
  - 验收：impl `chat()`：POST {api_base}/chat/completions（stream=true），spawn task 消费 SSE，把 `choices[0].delta.content` 转 `ChatEvent::Delta`、`finish_reason==stop` 转 `Done`，经 mpsc 发送；编译通过。

- [ ] T-5.8 实现工具调用解析
  - 依赖：T-5.7
  - 验收：解析 `choices[0].delta.tool_calls`，按 `index` 聚合分块，完整后发 `ChatEvent::ToolCall`；编译通过。

### 阶段五：占位适配器与构造函数

- [ ] T-5.9 实现 response_api/anthropic/custom 占位
  - 依赖：T-5.3
  - 验收：三个占位 struct，impl `LlmProvider` 各方法返回 `ProviderError::Config("not implemented yet")`；编译通过。

- [ ] T-5.10 实现 `lib.rs` 的 build_provider
  - 依赖：T-5.6, T-5.9
  - 验收：`build_provider(cfg: &ProviderConfig) -> Box<dyn LlmProvider>`，按 `ProviderKind` 分发到对应适配器；OpenAiCompat/Ollama 走 OpenAiCompatProvider；编译通过。

### 阶段六：测试

- [ ] T-5.11 验证不依赖 Bevy
  - 依赖：T-5.10
  - 验收：`cargo tree -p xgent_provider` 不含 bevy。

- [ ] T-5.12 真实 provider 测试（手动/可选）
  - 依赖：T-5.7
  - 验收：配 OpenAI 或本地 Ollama，`list_models` 返回非空、`chat` 流式输出 Delta 序列与 Done；无 key 时跳过（标 `#[ignore]`）。

- [ ] T-5.13 构造函数分发测试
  - 依赖：T-5.10
  - 验收：从各 `ProviderKind` 的 `ProviderConfig` 构造，断言返回的 provider `id()` 正确。

## 完成标志

- `cargo check -p xgent_provider` 通过
- `cargo test -p xgent_provider` 全绿（含 SSE mock 测试；真实 provider 测试 ignored）
- `cargo tree -p xgent_provider` 不含 bevy
- `LlmProvider` trait 与 OpenAiCompat 适配器可用，ResponseApi/Anthropic/Custom 占位不阻塞
