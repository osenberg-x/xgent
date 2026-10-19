# 0004-kind-下拉选择-MVP-隐藏-Custom

## 背景

F-01 可用化要求 settings_panel 能正确配置多类 provider。现状 `settings_panel`（`settings_panel.rs:142-184`）只渲染 4 输入框（`provider_id`/`api_base`/`api_key`/`model`），**无 `kind` 选择器**。`ProviderConfig::kind` 默认 `OpenAiCompat`（`global.rs:77-78`），导致用户配 Anthropic 原生接口也被当 OpenAI 兼容协议发请求，必败。F-07"Provider 切换"在 UI 侧断裂，不只 F-01。

`ProviderKind` 有 5 变体：`OpenAiCompat`/`ResponseApi`/`Anthropic`/`Ollama`/`Custom`。`Custom` 变体语义是"用户自定义请求模板/header 映射"，需配套 UI 让用户填这些——MVP 未实现该 UI。

## 决策

**`kind` 用下拉选择控件，MVP 暴露 4 变体（`OpenAiCompat`/`ResponseApi`/`Anthropic`/`Ollama`），隐藏 `Custom`。**

具体契约：
1. settings_panel 加 `kind` 下拉控件（`Dropdown` 或等价），5 字段集见 CONTEXT.md「最小可用字段集（Provider 配置）」。
2. `SaveProviderConfigMessage` 加 `kind: ProviderKind` 字段，handler 写 `providers.<id>.kind`（`write_provider_field` 已支持，`config_store.rs:149-153`）。
3. 下拉只列 4 变体，`Custom` 不出现在选项中。
4. MVP 的"自定义 API"由 `OpenAiCompat` + 用户填任意 `api_base` 覆盖——兼容大量第三方 OpenAI 兼容接口（如中转站、本地 vLLM 等）。

## 备选方案

### 下拉，全 5 变体暴露（含 Custom）

否决：`Custom` 需配套的请求模板/header 映射 UI，MVP 未实现。暴露 `Custom` 选项后用户选了却无法填配套参数，只产半成品配置——该 provider 永远不就绪，阻塞 F-01 体验。隐藏 `Custom` 直到其配套 UI 就绪。

### 文本输入 kind

用户手填 kind 字符串，daemon 侧解析。否决：易错（拼写、大小写），且 enum 变体有限，下拉更安全。

## 结论与后果

- **MVP F-07 边界**：Provider 切换 = 在 settings_panel 选 kind + 填 api_base/api_key/model + 保存。运行时切换（不重启换 provider）MVP 不做，当前选中 = `default_provider`。
- **`Custom` 留口**：`ProviderKind::Custom` 枚举值保留，仅 UI 不暴露。F-14 自定义工具或后续迭代补配套 UI 后再暴露。
- **就绪判据不变**：4 暴露变体的就绪判据已在 CONTEXT.md「Provider 就绪」钉死，按 kind 分。
- **改动面**：`settings_panel` 加下拉控件 + `SaveProviderConfigMessage` 加 `kind` 字段 + handler 写 `kind`。`config_store.rs` 无改动（`write_provider_field` 已支持 kind）。
