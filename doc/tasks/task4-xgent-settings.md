# Task 4: xgent_settings

> 对应实现指导：`doc/plans/step4-xgent-settings.md`
> 前置：step1 xgent_core、step2 xui_i18n、step3 xgent_settings_core 已完成

## 任务清单

### 阶段一：脚手架

- [ ] T-4.1 创建 crate 目录与 Cargo.toml
  - 依赖：无
  - 验收：`crates/xgent_settings/Cargo.toml` 存在；依赖为 bevy(serialize feature)、xgent_core、xgent_settings_core、xui_i18n、fluent、unic-langid、fluent-bundle、once_cell；`cargo check -p xgent_settings` 通过（空 lib.rs）。

- [ ] T-4.2 注册到 workspace
  - 依赖：T-4.1
  - 验收：`cargo metadata` 识别该 crate；依赖解析正确。

### 阶段二：Bevy Resource 包装

- [ ] T-4.3 实现 `resources.rs` 的 GlobalConfigRes
  - 依赖：T-4.1
  - 验收：定义 `GlobalConfigRes(GlobalConfig)` newtype，derive `Resource/Reflect/Deref/DerefMut/Debug/Clone/Default`，`#[reflect(Resource, Default)]`；包装 `xgent_settings_core::GlobalConfig`；编译通过。

- [ ] T-4.4 实现 `resources.rs` 的 ProjectConfigRes
  - 依赖：T-4.3
  - 验收：同上结构包装 `ProjectConfig`；编译通过。

- [ ] T-4.5 验证 Deref 透传
  - 依赖：T-4.3, T-4.4
  - 验收：测试 `GlobalConfigRes` 可像 `GlobalConfig` 一样访问字段（如 `res.default_provider`）；编译通过。

### 阶段三：fluent Localizer

- [ ] T-4.6 创建 locales 资源
  - 依赖：T-4.1
  - 验收：`crates/xgent_settings/locales/zh-CN/main.ftl` 与 `en-US/main.ftl` 存在，含基础 key（app-title/welcome/chat-placeholder/confirm-write-file/confirm-run-command）。

- [ ] T-4.7 实现 `localizer.rs` 的 Localizer 基础
  - 依赖：T-4.6
  - 验收：定义 `Localizer`（持有 `Arc<FluentBundle<FluentResource>>` 与 `lang: String`）derive `Resource`；`load(lang)` 用 `include_str!` 内嵌 .ftl 加载；`switch(lang)` reload；`current_lang()`；编译通过。

- [ ] T-4.8 impl `xui_i18n::StringSource` for Localizer
  - 依赖：T-4.7
  - 验收：实现 `get(key, args)`（bundle.get_message + 格式化）与 `current_lang()`；编译通过。

- [ ] T-4.9 验证 i18n 取值与切换
  - 依赖：T-4.8
  - 验收：测试 `Localizer::load("zh-CN")` 的 `get("welcome", &[])` 返回"欢迎"；`switch("en-US")` 后返回 "Welcome"。

### 阶段四：Plugin 与集成

- [ ] T-4.10 实现 `lib.rs` 的 XgentSettingsPlugin
  - 依赖：T-4.3, T-4.4, T-4.7
  - 验收：Plugin build 中 `insert_resource(GlobalConfigRes(GlobalConfigStore::load().unwrap_or_default()))`、`insert_resource(Localizer::load(...))`、`register_type`；编译通过。

- [ ] T-4.11 验证 Resource 初始化
  - 依赖：T-4.10
  - 验收：最小 Bevy App（`MinimalPlugins + XgentSettingsPlugin`）运行后，`GlobalConfigRes` 与 `Localizer` 资源存在且可读取。

- [ ] T-4.12 验证 StringSource 注入路径
  - 依赖：T-4.8, T-4.10
  - 验收：测试从 App 取 `Localizer`，作为 `&dyn StringSource` 调 `get` 返回正确串（确认 trait object 可用，为 xgent_app 注入 xui::Strings 铺路）。

## 完成标志

- `cargo check -p xgent_settings` 通过
- `cargo test -p xgent_settings` 全绿
- `Localizer` impl `StringSource`，可被 xui 经 trait 调用
- 配置类型经 newtype 包装为 Bevy Resource，core 类型未引入 Bevy 派生
- fluent 资源内嵌，语言切换可用
