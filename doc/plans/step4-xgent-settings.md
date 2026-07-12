# Step 4: xgent_settings

## 模块职责

在 `xgent_settings_core` 纯类型之上做 Bevy 集成与 i18n：

1. **Bevy Resource 包装**：把 core 的 `GlobalConfig`/`ProjectConfig` 包装为 Bevy Resource（derive `Resource`/`Reflect`），供 agent/ui 使用。
2. **fluent i18n**：封装 `Localizer`，加载 `.ftl` 资源、运行时切换语言、impl `xui_i18n::StringSource` 供 `xui` 调用。
3. **Plugin**：注册 Resource、初始化 Localizer。

**关键**：本 crate 依赖 Bevy（UI 侧使用）。daemon/provider 不依赖本 crate，只依赖 core，保持轻量。

## 前置依赖

- xgent_settings_core（配置类型、TOML 读写、路径）
- xui_i18n（StringSource trait）
- xgent_core（错误类型）

## 目标文件结构

```
crates/xgent_settings/
├── Cargo.toml
├── locales/
│   ├── zh-CN/
│   │   └── main.ftl
│   └── en-US/
│       └── main.ftl
└── src/
    ├── lib.rs              # Plugin + 模块导出
    ├── resources.rs        # Bevy Resource 包装（newtype 包 core 类型）
    └── localizer.rs        # fluent Localizer + impl StringSource
```

## Cargo.toml

```toml
[package]
name = "xgent_settings"
version = "0.1.0"
edition = "2024"

[dependencies]
bevy = { workspace = true, features = ["serialize"] }
xgent_core = { path = "../xgent_core" }
xgent_settings_core = { path = "../xgent_settings_core" }
xui_i18n = { path = "../xui_i18n" }
fluent = "0.17"
unic-langid = "0.9"
fluent-bundle = "0.13"
once_cell = "1"
```

说明：
- 引入 Bevy 是因配置作为 Bevy Resource，需 `Reflect`/`Resource` 派生。
- fluent 系列用于 i18n。
- 依赖 `xui_i18n` 以 impl `StringSource` trait（反转依赖：trait 在 xui_i18n，实现在此）。

## 关键类型与接口

### 1. resources.rs — Bevy Resource 包装

```rust
use bevy::prelude::*;
use xgent_settings_core::{GlobalConfig, ProjectConfig};

/// 全局配置 Bevy Resource（newtype 包装 core 类型，加 Resource + Reflect）
#[derive(Resource, Reflect, Deref, DerefMut, Debug, Clone, Default)]
#[reflect(Resource, Default)]
pub struct GlobalConfigRes(pub GlobalConfig);

#[derive(Resource, Reflect, Deref, DerefMut, Debug, Clone, Default)]
#[reflect(Resource, Default)]
pub struct ProjectConfigRes(pub ProjectConfig);
```

说明：用 newtype 包装而非直接给 core 类型加 `Resource` 派生，避免 core 依赖 Bevy。`Deref`/`DerefMut` 使调用方可像用 core 类型一样访问字段。

### 2. localizer.rs — fluent Localizer + impl StringSource

```rust
use bevy::prelude::*;
use fluent::{bundle::FluentBundle, FluentResource, FluentArgs};
use unic_langid::LanguageIdentifier;
use std::sync::Arc;
use xui_i18n::StringSource;

#[derive(Resource)]
pub struct Localizer {
    bundle: Arc<FluentBundle<FluentResource>>,
    lang: String,
}

impl Localizer {
    pub fn load(lang: &str) -> Self { /* include_str! 内嵌 .ftl 加载 */ }
    pub fn switch(&mut self, lang: &str) { /* reload */ }
    pub fn current_lang(&self) -> &str { &self.lang }
}

/// impl xui_i18n::StringSource，使 xui 可经 trait 调用取字符串
/// （反转依赖：trait 在 xui_i18n，实现在此，xui 不依赖 xgent_settings）
impl StringSource for Localizer {
    fn get(&self, key: &str, args: &[(&str, String)]) -> String {
        // bundle.get_message(key) + format with args
    }
    fn current_lang(&self) -> &str { &self.lang }
}
```

**.ftl 资源示例**（`locales/zh-CN/main.ftl`）：
```ftl
app-title = XGent
welcome = 欢迎
chat-placeholder = 输入消息，按 Enter 发送
confirm-write-file = 确认写入文件 { $path }？
confirm-run-command = 确认运行命令：{ $cmd }
```

**内嵌资源**：`include_str!` 把 .ftl 编译进二进制。

### 3. lib.rs — Plugin

```rust
use bevy::prelude::*;
use xgent_settings_core::GlobalConfigStore;

pub struct XgentSettingsPlugin;

impl Plugin for XgentSettingsPlugin {
    fn build(&self, app: &mut App) {
        let global = GlobalConfigStore::load().unwrap_or_default();
        let lang = global.preferences.language.clone();
        app
            .insert_resource(GlobalConfigRes(global))
            .insert_resource(Localizer::load(if lang.is_empty() { "zh-CN" } else { &lang }))
            .register_type::<GlobalConfigRes>()
            .register_type::<ProjectConfigRes>();
        // ProjectConfigRes 由 xgent_app 在打开项目时 insert_resource
    }
}
```

## 实现要点

1. **newtype 包装**：core 类型不加 Bevy 派生，用 newtype + `Deref` 包装，保持 core 纯净。
2. **impl StringSource**：`Localizer` impl `xui_i18n::StringSource`，使 `xui` 经 trait 取字符串，`xui` 不依赖 `xgent_settings`（反转依赖）。
3. **fluent 资源内嵌**：`include_str!` 编译进二进制，多开不依赖外部文件。
4. **语言切换**：`Localizer::switch` reload bundle，发 Bevy 事件触发 UI 刷新。
5. **Resource 初始化从 TOML**：用 `insert_resource` 而非 `init_resource`，从 `GlobalConfigStore::load()` 加载。
6. **不暴露 core 写操作**：UI 侧全局配置写经 daemon（IPC config.write），不直接调 `GlobalConfigStore::save`，避免多客户端写冲突。

## 验证方法

1. **编译检查**：
   ```bash
   cargo check -p xgent_settings
   ```
2. **Resource 测试**：最小 Bevy App 加 Plugin，断言 `GlobalConfigRes` 与 `Localizer` 资源存在。
3. **StringSource 测试**：取 `Localizer`，调 `StringSource::get("welcome", &[])` 返回"欢迎"。
4. **语言切换测试**：switch 到 en-US，`get("welcome")` 返回 "Welcome"。
5. **Deref 测试**：`GlobalConfigRes` 可像 `GlobalConfig` 一样访问字段。

## 完成后下一步

xgent_settings 完成后 → 实现 **xgent_provider**（LLM Provider 抽象 + OpenAI compatible 适配器），它依赖 core 与 settings_core 的 ProviderConfig。
