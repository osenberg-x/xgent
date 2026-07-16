# 0002-model-落点为-provider-级-model-overrides

## 背景

F-01 可用化补 `SaveProviderConfigMessage` handler 时，`model` 字段（来自 `settings_panel::ModelInput`）有两个潜在落点：

- `GlobalConfig.default_model`（全局默认模型名，字符串）
- `ProviderConfig.model_overrides`（该 provider 专用模型映射，`HashMap<String,String>`）

代码现状约束：`ConfigCoordinator::write` 的 `write_provider_field`（`config_store.rs:147-174`）字段表只认 `kind/api_base/api_key/timeout_secs/max_retries`，**不支持 `model_overrides`**。即现状下 `model` 只能落 `default_model`，除非扩写路径。

场景压边界：用户配两个 provider（openai 用 gpt-4o，anthropic 用 claude-sonnet-4）。若 `model` 落 `default_model`（全局），最后保存的 provider 的 model 会污染所有 provider——切回 openai 发对话时，openai provider 收到 anthropic 的模型名，API 报错。

## 决策

**`model` 落 `ProviderConfig.model_overrides`，不落 `GlobalConfig.default_model`。**

具体契约：
1. 扩 `ConfigCoordinator::write` 支持 key `providers.<id>.model_overrides`，写语义为**插入式**：设该 provider 的 `model_overrides` map 中键 `"default"` 的值为传入字符串，不覆盖 map 其他条目。
2. `SaveProviderConfigMessage` handler 写 `model` 时用上述 key + 键名 `"default"`。
3. `ChatRequest.model` 派生逻辑（`xgent_app::derive_provider_model` 及 `format::build_request`）改为：从当前选中 provider 的 `model_overrides["default"]` 取，缺则回退到 `GlobalConfig.default_model`。

## 备选方案

### 落 `default_model`（全局）

现状唯一可通路径（无需扩 `write_provider_field`）。

否决理由：`default_model` 语义是"未指定 provider 时的全局兜底模型"，不该被"配某个 provider 时填的 model"覆盖。多 provider 各用不同模型是常态（本地 Ollama 用 qwen，云端 OpenAI 用 gpt-4o），落全局会让最后一次保存的 model 污染所有 provider，导致跨 provider 发对话时模型名错配、API 报错。

### 整体替换 `providers.<id>`（整 ProviderConfig）

不单字段写 `model_overrides`，而是构造完整 `ProviderConfig`（含 kind 等）整体替换。

否决理由：`settings_panel` 只收 4 字段（provider_id/api_base/api_key/model），缺 `kind`/`timeout_secs`/`max_retries`。整体替换会丢失未在 UI 暴露的字段，或需 UI 补全所有字段。单字段插入式改动面更小，且保留 `kind` 等字段的既有值。

## 结论与后果

- **语义正确**：model 绑定到具体 provider，多 provider 各自的 model 互不污染。
- **实现代价**：需扩 `ConfigCoordinator::write` 的 `write_provider_field` 认 `model_overrides` 字段（map 插入式语义，非简单 set string）。改动局限在 `config_store.rs`，不波及其他子系统。
- **键名约定**：`model_overrides` map 用 `"default"` 作通用键名。这是 MVP 约定——`model_overrides` 原设计是"通用名→实际模型 id"映射（支持多个别名），MVP 只用 `"default"` 一个键，等 F-07 运行时切换 UI 上线后再考虑暴露多别名。
- **派生回退**：`ChatRequest.model` 取值优先 `model_overrides["default"]`，缺则回退 `default_model`。这保证旧配置（仅写 `default_model` 未写 `model_overrides`）仍可用。
