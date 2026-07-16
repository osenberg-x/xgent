# XGent

面向个人开发者日常编码的桌面端 AI Code Agent。本 glossary 钉死对话与 provider 配置链路中容易混淆的领域术语，仅记语言，不记实现。

## Language

**Provider**:
配置层指 `GlobalConfig.providers` map 中的一条 `ProviderConfig`（按 id 索引）；运行时层指 daemon `ProviderPool` 中按 id 缓存的 `LlmProvider` trait 实例。同一 id 在两层指同一概念，但"配置存在"与"实例就绪"是不同状态。
_Avoid_: 模型、API、服务

**Provider 就绪**:
某 ProviderConfig 满足发起对话所需的最小字段集。判据按 `ProviderKind` 分：`Ollama` 下 `api_base` 非空即就绪（本地部署通常无 key）；`OpenAiCompat`/`ResponseApi`/`Anthropic`/`Custom` 下 `api_base` 与 `api_key` 均非空才就绪。"配置存在"（map 有该 key）不等于"就绪"。
_Avoid_: 可用、配好了

**default provider**:
`GlobalConfig.default_provider` 指向的全局默认 Provider id。区别于"当前选中 provider"——MVP 无运行时切换 UI，当前选中 = default。命令行 `--provider` 与项目配置 `provider_override` 可覆盖，但覆盖值不写回 default。
_Avoid_: 默认模型、主 provider

**model**:
settings_panel `ModelInput` 填入的模型名，落点为 `ProviderConfig.model_overrides["default"]`（插入式单字段写：设该 provider 的 `model_overrides` map 中键 `"default"` 的值，不覆盖 map 其他条目）。语义是"该 provider 默认用这个模型"，绑定到具体 provider，非全局。区别于 `GlobalConfig.default_model`——后者是"未指定 provider 时的全局兜底模型"，不由 settings_panel 保存路径写入。`ChatRequest.model` 派生时从当前选中 provider 的 `model_overrides["default"]` 取，缺则回退到 `default_model`。
_Avoid_: 默认模型、model id、模型名

**ErrorKind**:
错误分类 enum，变体按"用户可采取的行动"划分，不按底层错误源或 HTTP 状态码划分。MVP 起始变体：`NotConfigured`（闸门拦截：provider 未就绪，引导开 settings_panel）、`AuthFailed`（provider 鉴权失败：API key 错/失效，引导检查 key）、`Network`（连接/超时，可重试）、`StreamParse`（SSE/JSON 解析失败，可重试）、`ProviderError`（provider 返回非鉴权类错误，含原始 message 供排查）。约束：UI 不感知 HTTP 状态码——`ProviderError::Api{status,body}` 的 status 不透给 UI，由 daemon 侧映射到 `AuthFailed`（401/403）或 `ProviderError`（其余）。
_Avoid_: 错误码、错误类型、exception

**最小可用字段集（Provider 配置）**:
settings_panel 收集的 5 个字段：`provider_id`、`kind`、`api_base`、`api_key`、`model`。区别于 `ProviderConfig` 全量 7 字段——`timeout_secs`/`max_retries` 用默认值，UI 不暴露。与「Provider 就绪」判据对齐：这 5 字段足够判定就绪 + 构造 provider 实例。
_Avoid_: provider 表单、配置表单

**Provider 配置（MVP）**:
F-07 在 MVP 阶段的"Provider 切换"边界：用户在 settings_panel 填最小可用字段集（含选 `kind` 下拉），保存后经 daemon 全局配置生效。`kind` 下拉 MVP 暴露 4 变体（`OpenAiCompat`/`ResponseApi`/`Anthropic`/`Ollama`），隐藏 `Custom`——因 `Custom` 需配套的请求模板/header 映射 UI，MVP 未实现，暴露只产半成品配置。MVP 的"自定义 API"由 `OpenAiCompat` + 用户填任意 `api_base` 覆盖（兼容大量第三方接口），真正的 `Custom` provider 留 F-14 或后续。运行时切换 UI（不重启换 provider）MVP 不做，当前选中 = `default_provider`。
_Avoid_: provider 管理、provider 切换面板
