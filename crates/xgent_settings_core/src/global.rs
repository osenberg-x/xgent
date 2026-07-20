//! 全局配置类型。
//!
//! 全局配置存于平台规范路径的 `config.toml`（见 [`crate::paths`]），
//! 包含 provider 列表、默认模型与应用偏好。权威副本在 daemon（step6）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 全局配置根结构。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct GlobalConfig {
    /// 已配置的 provider 集合（key 为 provider id，如 `"openai"`）
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,

    /// 默认 provider id
    #[serde(default)]
    pub default_provider: String,

    /// 默认模型名
    #[serde(default)]
    pub default_model: String,

    /// 应用偏好
    #[serde(default)]
    pub preferences: Preferences,
}

/// 单个 provider 的配置。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProviderConfig {
    /// provider 类型
    pub kind: ProviderKind,
    /// API 基础 URL（如 `"https://api.openai.com/v1"`）
    #[serde(default)]
    pub api_base: String,
    /// API Key（MVP 明文存 TOML，未来考虑 keychain，见 D-02）
    #[serde(default)]
    pub api_key: String,
    /// 模型覆盖（通用名 → 实际模型 id）
    #[serde(default)]
    pub model_overrides: HashMap<String, String>,
    /// 请求超时秒数
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// 最大重试次数。
    ///
    /// `None` 表示无限重试（直到成功或被中断）；`Some(n)` 表示最多重试 n 次。
    /// 仅对可重试错误（`Network`/`StreamParse`）生效，见 agent 侧重试逻辑。
    #[serde(default = "default_max_retries")]
    pub max_retries: Option<u32>,
    /// 重试模式（固定间隔 / 指数退避）
    #[serde(default = "default_retry_mode")]
    pub retry_mode: RetryMode,
    /// 重试初始间隔毫秒。
    ///
    /// 固定模式：每次重试前等待该值；指数模式：作为退避基准值。
    #[serde(default = "default_retry_initial_delay_ms")]
    pub retry_initial_delay_ms: u64,
    /// 重试最大间隔毫秒（指数退避上限，固定模式忽略）。
    #[serde(default = "default_retry_max_delay_ms")]
    pub retry_max_delay_ms: u64,
    /// 指数退避乘数（固定模式忽略）。
    ///
    /// 每次重试间隔 = min(initial * backoff_factor^(n-1), max_delay)。
    #[serde(default = "default_retry_backoff_factor")]
    pub retry_backoff_factor: f64,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            kind: ProviderKind::OpenAiCompat,
            api_base: String::new(),
            api_key: String::new(),
            model_overrides: HashMap::new(),
            timeout_secs: default_timeout_secs(),
            max_retries: default_max_retries(),
            retry_mode: default_retry_mode(),
            retry_initial_delay_ms: default_retry_initial_delay_ms(),
            retry_max_delay_ms: default_retry_max_delay_ms(),
            retry_backoff_factor: default_retry_backoff_factor(),
        }
    }
}

fn default_timeout_secs() -> u64 {
    60
}

fn default_max_retries() -> Option<u32> {
    Some(2)
}

/// 重试模式。
///
/// - `Fixed`：每次重试前等待固定间隔（`retry_initial_delay_ms`）。
/// - `Exponential`：指数退避，间隔 = min(initial * factor^(n-1), max_delay)。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetryMode {
    /// 固定间隔
    #[default]
    Fixed,
    /// 指数退避
    Exponential,
}

fn default_retry_mode() -> RetryMode {
    RetryMode::Fixed
}

fn default_retry_initial_delay_ms() -> u64 {
    500
}

fn default_retry_max_delay_ms() -> u64 {
    30_000
}

fn default_retry_backoff_factor() -> f64 {
    2.0
}

/// provider 类型。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// OpenAI 兼容接口
    #[default]
    OpenAiCompat,
    /// Response API 风格
    ResponseApi,
    /// Anthropic 原生
    Anthropic,
    /// 本地 Ollama（兼容模式）
    Ollama,
    /// 用户自定义
    Custom,
}

/// 应用偏好。
///
/// 手写 `Default` 以提供合理的初始语言/主题。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Preferences {
    /// 界面语言，如 `"zh-CN"` / `"en-US"`
    #[serde(default = "default_language")]
    pub language: String,
    /// 主题（MVP 仅 `"dark"`）
    #[serde(default = "default_theme")]
    pub theme: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: default_language(),
            theme: default_theme(),
        }
    }
}

fn default_language() -> String {
    "zh-CN".to_string()
}

fn default_theme() -> String {
    "dark".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_config_default_empty() {
        let cfg = GlobalConfig::default();
        assert!(cfg.providers.is_empty());
        assert!(cfg.default_provider.is_empty());
        assert_eq!(cfg.preferences.language, "zh-CN");
        assert_eq!(cfg.preferences.theme, "dark");
    }

    #[test]
    fn provider_config_default_values() {
        let p = ProviderConfig::default();
        assert_eq!(p.kind, ProviderKind::OpenAiCompat);
        assert_eq!(p.timeout_secs, 60);
        assert_eq!(p.max_retries, Some(2));
        assert_eq!(p.retry_mode, RetryMode::Fixed);
        assert_eq!(p.retry_initial_delay_ms, 500);
        assert_eq!(p.retry_max_delay_ms, 30_000);
        assert_eq!(p.retry_backoff_factor, 2.0);
    }

    #[test]
    fn provider_config_retries_serde() {
        // 省略字段 → default Some(2)
        let j = r#"{"kind":"open_ai_compat"}"#;
        let p: ProviderConfig = serde_json::from_str(j).unwrap();
        assert_eq!(p.max_retries, Some(2));
        // 显式 null → None（无限重试）
        let j = r#"{"kind":"open_ai_compat","max_retries":null}"#;
        let p: ProviderConfig = serde_json::from_str(j).unwrap();
        assert_eq!(p.max_retries, None);
        // 数字 → Some(n)
        let j = r#"{"kind":"open_ai_compat","max_retries":5}"#;
        let p: ProviderConfig = serde_json::from_str(j).unwrap();
        assert_eq!(p.max_retries, Some(5));
        // None 往返保持 None
        let p = ProviderConfig {
            max_retries: None,
            ..Default::default()
        };
        let s = serde_json::to_string(&p).unwrap();
        let p2: ProviderConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(p2.max_retries, None);
    }

    #[test]
    fn retry_mode_serde_snake_case() {
        let j = serde_json::to_string(&RetryMode::Exponential).unwrap();
        assert_eq!(j, r#""exponential""#);
        let m: RetryMode = serde_json::from_str(r#""fixed""#).unwrap();
        assert_eq!(m, RetryMode::Fixed);
    }

    #[test]
    fn provider_kind_serde_snake_case() {
        let j = serde_json::to_string(&ProviderKind::Anthropic).unwrap();
        assert_eq!(j, r#""anthropic""#);
        let k: ProviderKind = serde_json::from_str(r#""ollama""#).unwrap();
        assert_eq!(k, ProviderKind::Ollama);
    }

    #[test]
    fn global_config_roundtrip() {
        let cfg = GlobalConfig {
            default_provider: "openai".to_string(),
            default_model: "gpt-4".to_string(),
            providers: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "openai".to_string(),
                    ProviderConfig {
                        kind: ProviderKind::OpenAiCompat,
                        api_base: "https://api.openai.com/v1".into(),
                        api_key: "sk-xxx".into(),
                        ..Default::default()
                    },
                );
                m
            },
            preferences: Preferences::default(),
        };
        let j = serde_json::to_string(&cfg).unwrap();
        let cfg2: GlobalConfig = serde_json::from_str(&j).unwrap();
        assert_eq!(cfg2.default_provider, "openai");
        assert_eq!(cfg2.providers.len(), 1);
        let p = cfg2.providers.get("openai").unwrap();
        assert_eq!(p.kind, ProviderKind::OpenAiCompat);
        assert_eq!(p.api_key, "sk-xxx");
        assert_eq!(p.timeout_secs, 60);
    }
}
