# xgent_settings — 详细编码指导

## 下一步要实现的模块

**xgent_settings** — 它是所有其他模块的基石，Provider 配置、MCP Server 配置、应用偏好都依赖它。没有它，后续模块无法读取配置。

---

## 模块职责

基于 Bevy 内置 `bevy_settings` crate 的 `PreferencesPlugin` + `#[derive(SettingsGroup)]` 实现配置持久化。自动读写平台特定目录下的 TOML 文件。

---

## 目标文件结构

```
crates/xgent_settings/
├── Cargo.toml
└── src/
    └── lib.rs           # 所有类型 + Plugin，MVP-1 单文件足够
```

MVP-1 阶段代码量很小（~150 行），不需要拆分模块。

---

## Cargo.toml

```toml
[package]
name = "xgent_settings"
version = "0.1.0"
edition = "2024"

[dependencies]
bevy = { workspace = true, features = ["bevy_settings", "serialize"] }
serde = { workspace = true }
serde_json = { workspace = true }
```

**注意**：`bevy_settings` 需要 `serialize` feature 才能正常工作（它依赖 serde 来序列化/反序列化 TOML）。

---

## lib.rs — 完整实现指导

### 1. 导入

```rust
use bevy::prelude::*;
use bevy::settings::{PreferencesPlugin, SavePreferencesDeferred, SettingsGroup};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
```

### 2. 核心配置类型

**原则**：所有 SettingsGroup 必须同时 derive `Resource`, `SettingsGroup`, `Reflect`, `Default`, `Serialize`, `Deserialize`，并且 `#[reflect(Resource, SettingsGroup, Default)]`。

#### ProviderSettings — Provider 全局配置

存储在 `providers.toml` 中。

```rust
/// Provider 全局默认配置
#[derive(Resource, SettingsGroup, Reflect, Default, Serialize, Deserialize, Debug, Clone)]
#[reflect(Resource, SettingsGroup, Default)]
#[settings_group(file = "providers")]
pub struct ProviderSettings {
    /// 默认 Provider ID，如 "openai"
    pub default_provider: String,
    /// 默认模型，如 "gpt-4o"
    pub default_model: String,
}
```

Default 实现应该给出合理默认值。由于 `Default` derive 要求字段也实现 Default，String 的默认值是 `""`，所以需要在 Plugin build 中设置初始值，或者用自定义 Default：

```rust
impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            default_provider: "openai".to_string(),
            default_model: "gpt-4o".to_string(),
        }
    }
}
```

#### ProviderEntrySettings — 单个 Provider 配置

也存储在 `providers.toml` 中，与 ProviderSettings 共享同一个文件（同 group 名）。

```rust
/// 单个 Provider 的连接配置
#[derive(Resource, SettingsGroup, Reflect, Serialize, Deserialize, Debug, Clone)]
#[reflect(Resource, SettingsGroup)]
#[settings_group(group = "provider_entry", file = "providers")]
pub struct ProviderEntrySettings {
    pub id: String,
    /// API 端点，如 "https://api.openai.com/v1"
    pub api_base: String,
    /// API Key 的环境变量名，如 "OPENAI_API_KEY"
    pub api_key_env: String,
    /// 自定义模型名映射，如 {"gpt-4": "my-deployed-gpt4"}
    pub model_overrides: HashMap<String, String>,
    /// 请求超时（秒）
    pub timeout_secs: u64,
    /// 最大重试次数
    pub max_retries: u32,
}
```

注意 `ProviderEntrySettings` 不 derive `Default`（因为 id/api_base 没有合理默认值）。这意味着它不会被自动初始化——需要通过代码手动插入。

**关键点**：TOML 文件中 `ProviderEntrySettings` 的 section 名是 `provider_entry`（由 `group = "provider_entry"` 决定）。如果有多个 Provider，每个 Provider 是一个独立的 Resource 实例，但 SettingsGroup 目前是 **一个类型 = 一个 Resource**。对于 MVP-1，我们可以先只支持一个 Provider 配置，后续再用 `HashMap<String, ProviderEntry>` 方式扩展。

**MVP-1 简化方案**：将所有 Provider 条目合并到一个 Resource 中：

```rust
/// 所有 Provider 条目（MVP-1 简化版）
#[derive(Resource, SettingsGroup, Reflect, Default, Serialize, Deserialize, Debug, Clone)]
#[reflect(Resource, SettingsGroup, Default)]
#[settings_group(file = "providers")]
pub struct ProviderEntriesSettings {
    /// key = provider id, value = 配置
    pub entries: HashMap<String, ProviderEntryConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Reflect, Default)]
pub struct ProviderEntryConfig {
    pub api_base: String,
    pub api_key_env: String,
    pub model_overrides: HashMap<String, String>,
    pub timeout_secs: u64,
    pub max_retries: u32,
}

impl Default for ProviderEntryConfig {
    fn default() -> Self {
        Self {
            api_base: String::new(),
            api_key_env: String::new(),
            model_overrides: HashMap::new(),
            timeout_secs: 60,
            max_retries: 3,
        }
    }
}
```

这样 TOML 文件长这样：

```toml
# providers.toml
[provider_settings]
default_provider = "openai"
default_model = "gpt-4o"

[provider_entries_settings]
[provider_entries_settings.entries.openai]
api_base = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
timeout_secs = 60
max_retries = 3

[provider_entries_settings.entries.ollama]
api_base = "http://localhost:11434/v1"
api_key_env = ""
timeout_secs = 120
max_retries = 1
```

#### McpServersSettings — MCP Server 配置

存储在 `mcp.toml` 中。

```rust
#[derive(Resource, SettingsGroup, Reflect, Default, Serialize, Deserialize, Debug, Clone)]
#[reflect(Resource, SettingsGroup, Default)]
#[settings_group(file = "mcp")]
pub struct McpServersSettings {
    pub servers: HashMap<String, McpServerConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Reflect, Default)]
pub struct McpServerConfig {
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub url: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub enabled: bool,
    pub auto_start: bool,
    pub trust_level: String,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            command: None,
            args: None,
            url: None,
            env: None,
            enabled: true,
            auto_start: false,
            trust_level: "ConfirmEach".to_string(),
        }
    }
}
```

#### AppSettings — 应用偏好

存储在默认 `settings.toml` 中。

```rust
#[derive(Resource, SettingsGroup, Reflect, Default, Serialize, Deserialize, Debug, Clone)]
#[reflect(Resource, SettingsGroup, Default)]
pub struct AppSettings {
    pub theme: String,  // "dark" (MVP-1 only dark)
}
```

### 3. Plugin

```rust
pub struct XgentSettingsPlugin;

impl Plugin for XgentSettingsPlugin {
    fn build(&self, app: &mut App) {
        // PreferencesPlugin 必须先于其他使用 settings 的 Plugin 添加
        app.add_plugins(PreferencesPlugin::new("dev.xgent"));

        // SettingsGroup 资源会在 PreferencesPlugin::build 中自动初始化
        // （从 TOML 文件加载，如果文件不存在则使用 Default）
    }
}
```

**重要**：`PreferencesPlugin` 会自动注册所有已添加到 App 中的 `SettingsGroup` 资源。你不需要手动 `init_resource`——Plugin 在 build 时会检查已注册的 Resource 类型，如果它们实现了 `SettingsGroup`，就自动从 TOML 加载值。

但实际上你仍需要确保这些 Resource 在 App 中注册。最安全的做法是在 `XgentSettingsPlugin::build` 中显式初始化：

```rust
impl Plugin for XgentSettingsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PreferencesPlugin::new("dev.xgent"))
            .init_resource::<ProviderSettings>()
            .init_resource::<ProviderEntriesSettings>()
            .init_resource::<McpServersSettings>()
            .init_resource::<AppSettings>();
    }
}
```

`PreferencesPlugin` 的 build 会拦截这些 init_resource，用 TOML 中读取的值覆盖默认值。

### 4. 公开 API

```rust
pub use bevy::settings::{PreferencesPlugin, SavePreferencesDeferred, SavePreferencesSync, SettingsGroup};

// 重新导出所有配置类型
pub use {
    AppSettings, McpServerConfig, McpServersSettings, ProviderEntriesSettings, ProviderEntryConfig,
    ProviderSettings,
};
```

### 5. 辅助方法（可选但推荐）

```rust
impl ProviderEntriesSettings {
    /// 获取指定 provider 的配置
    pub fn get(&self, id: &str) -> Option<&ProviderEntryConfig> {
        self.entries.get(id)
    }

    /// 从环境变量读取 API Key
    pub fn resolve_api_key(&self, id: &str) -> Option<String> {
        self.entries.get(id).and_then(|config| {
            if config.api_key_env.is_empty() {
                None
            } else {
                std::env::var(&config.api_key_env).ok()
            }
        })
    }
}

impl McpServersSettings {
    /// 获取所有已启用的 MCP Server
    pub fn enabled_servers(&self) -> Vec<(&String, &McpServerConfig)> {
        self.servers
            .iter()
            .filter(|(_, config)| config.enabled)
            .collect()
    }
}

impl McpServerConfig {
    pub fn trust_level(&self) -> McpTrustLevel {
        match self.trust_level.as_str() {
            "Trusted" => McpTrustLevel::Trusted,
            "ReadOnly" => McpTrustLevel::ReadOnly,
            _ => McpTrustLevel::ConfirmEach,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTrustLevel {
    Trusted,
    ConfirmEach,
    ReadOnly,
}
```

---

## 验证方法

完成实现后，按以下步骤验证：

1. **编译检查**：在 workspace 根目录运行 `cargo check -p xgent_settings`
2. **单元测试**：创建一个最小 Bevy App，添加 `XgentSettingsPlugin`，检查 Resource 被正确初始化
3. **TOML 生成**：运行应用后，检查平台偏好目录下是否生成了 `providers.toml`、`mcp.toml`、`settings.toml`
   - Windows: `%APPDATA%\dev.xgent\`
   - macOS: `~/Library/Application Support/dev.xgent/`
   - Linux: `~/.config/dev.xgent/`
4. **TOML 读取**：手动编辑 TOML 文件，重启应用，确认值被正确加载

### 最小验证 App

```rust
// 在任意 main 函数中
App::new()
    .add_plugins(MinimalPlugins)
    .add_plugins(XgentSettingsPlugin)
    .add_systems(Update, |settings: Res<ProviderSettings>| {
        info!("default_provider: {}", settings.default_provider);
    })
    .run();
```

---

## 完成后下一步

xgent_settings 完成后 → 实现 **xgent_provider**（LLM Provider 抽象 + OpenAI 兼容适配器），因为它是 Agent 引擎的前置依赖。
