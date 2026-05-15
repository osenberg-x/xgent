use bevy::prelude::*;
use bevy::settings::{
    PreferencesPlugin, ReflectSettingsGroup, SavePreferencesDeferred, SavePreferencesSync,
    SettingsGroup,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Resource, SettingsGroup, Reflect, Serialize, Deserialize, Debug, Clone)]
#[reflect(Resource, SettingsGroup, Default)]
#[settings_group(file = "providers")]
pub struct ProviderSettings {
    pub default_provider: String,
    pub default_model: String,
}

impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            default_provider: "deepseek".to_string(),
            default_model: "deepseek-v4-pro".to_string(),
        }
    }
}

/// 单个 Provider 的连接设置
#[derive(Resource, SettingsGroup, Reflect, Serialize, Deserialize, Debug, Clone)]
#[reflect(Resource, SettingsGroup, Default)]
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

impl Default for ProviderEntrySettings {
    fn default() -> Self {
        Self {
            id: "".to_string(),
            api_base: "".to_string(),
            api_key_env: "".to_string(),
            model_overrides: HashMap::new(),
            timeout_secs: 0,
            max_retries: 0,
        }
    }
}

/// 所有 Provider 条目
#[derive(Resource, SettingsGroup, Reflect, Default, Serialize, Deserialize, Debug, Clone)]
#[reflect(Resource, SettingsGroup, Default)]
#[settings_group(file = "providers")]
pub struct ProviderEntriesSettings {
    /// key = provider id, value = 配置
    pub entries: HashMap<String, ProviderEntrySettings>,
}

impl ProviderEntriesSettings {
    /// 获取指定 provider 的配置
    pub fn get(&self, id: &str) -> Option<&ProviderEntrySettings> {
        self.entries.get(id)
    }

    /// 从环境变量中读取 API Key
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

#[derive(Resource, SettingsGroup, Reflect, Default, Serialize, Deserialize, Debug, Clone)]
#[reflect(Resource, SettingsGroup, Default)]
#[settings_group(file = "providers")]
pub struct McpServersSettings {
    pub servers: HashMap<String, McpServerSettings>,
}

impl McpServersSettings {
    /// 获取所有已启用的 MCP Server
    pub fn enabled_servers(&self) -> Vec<(&String, &McpServerSettings)> {
        self.servers
            .iter()
            .filter(|(_, setting)| setting.enabled)
            .collect()
    }
}

#[derive(Reflect, Serialize, Deserialize, Debug, Clone)]
pub struct McpServerSettings {
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub url: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub enabled: bool,
    pub auto_start: bool,
    pub trust_level: String,
}

impl Default for McpServerSettings {
    fn default() -> Self {
        Self {
            command: None,
            args: None,
            url: None,
            env: None,
            enabled: false,
            auto_start: false,
            trust_level: "".to_string(),
        }
    }
}

impl McpServerSettings {
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
    ReadOnly,
    ConfirmEach,
}

#[derive(Resource, SettingsGroup, Reflect, Default, Serialize, Deserialize, Debug, Clone)]
#[reflect(Resource, SettingsGroup, Default)]
pub struct AppSettings {
    /// default dark
    pub theme: String,
}

pub struct XgentSettingsPlugin;

impl Plugin for XgentSettingsPlugin {
    fn build(&self, app: &mut App) {
        // PreferencesPlugin 必须先于其他使用 settings 的 Plugin 添加
        app.add_plugins(PreferencesPlugin::new("dev.xgent"))
            .init_resource::<ProviderSettings>()
            .init_resource::<ProviderEntriesSettings>()
            .init_resource::<McpServersSettings>()
            .init_resource::<AppSettings>();

        // SettingsGroup 资源会在 PreferencesPlugin::build 中自动初始化
        // (从 TOML 文件加载，如果文件不存在则使用 Default)
    }
}

// pub use bevy::settings::{
//     PreferencesPlugin, SavePreferencesDeferred, SavePreferencesSync, SettingsGroup,
// };

// pub use {
//     AppSettings, McpServerSettings, McpServersSettings, ProviderEntriesSettings,
//     ProviderEntrySettings, ProviderSettings,
// };
//
