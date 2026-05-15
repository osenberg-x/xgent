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

#[derive(Resource, SettingsGroup, Reflect, Default, Serialize, Deserialize, Debug, Clone)]
#[reflect(Resource, SettingsGroup, Default)]
#[settings_group(file = "providers")]
pub struct McpServersSettings {
    pub servers: HashMap<String, McpServerSettings>,
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
