//! 全局配置权威副本与读写协调。
//!
//! daemon 持有全局配置的权威副本。`config.read` 返回对应字段的值，
//! `config.write` 更新内存副本、持久化到 TOML，并返回 [`ConfigChanged`]
//! 供调用方广播给所有客户端。
//!
//! MVP 阶段 key 限定为已知顶层字段（见 [`KnownKey`]），点分路径访问
//! 复杂嵌套结构（如 `providers.openai.api_key`）后续迭代支持。

use serde_json::{Value, json};
use xgent_core::config::{ConfigChanged, ConfigScope};
use xgent_core::error::{XgentError, XgentResult};
use xgent_settings_core::{GlobalConfig, GlobalConfigStore};

/// 已知的全局配置 key（点分路径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownKey {
    /// `default_provider`
    DefaultProvider,
    /// `default_model`
    DefaultModel,
    /// `preferences.language`
    Language,
    /// `preferences.theme`
    Theme,
}

impl KnownKey {
    /// 从点分 key 字符串解析。
    pub fn parse(key: &str) -> Option<Self> {
        match key {
            "default_provider" => Some(Self::DefaultProvider),
            "default_model" => Some(Self::DefaultModel),
            "preferences.language" => Some(Self::Language),
            "preferences.theme" => Some(Self::Theme),
            _ => None,
        }
    }

    /// 对应的点分 key 字符串。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DefaultProvider => "default_provider",
            Self::DefaultModel => "default_model",
            Self::Language => "preferences.language",
            Self::Theme => "preferences.theme",
        }
    }
}

/// 全局配置权威副本。
pub struct ConfigCoordinator {
    config: GlobalConfig,
}

impl ConfigCoordinator {
    /// 从 TOML 加载（不存在则默认）。
    pub fn load() -> XgentResult<Self> {
        let config = GlobalConfigStore::load()?;
        Ok(Self { config })
    }

    /// 用指定配置构造（主要用于测试）。
    #[allow(dead_code)]
    pub fn with_config(config: GlobalConfig) -> Self {
        Self { config }
    }

    /// 当前配置的只读引用。
    pub fn config(&self) -> &GlobalConfig {
        &self.config
    }

    /// 读取指定 key 的值。未知 key 返回 `Value::Null`。
    pub fn read(&self, key: &str) -> Value {
        match KnownKey::parse(key) {
            Some(KnownKey::DefaultProvider) => {
                json!(self.config.default_provider)
            }
            Some(KnownKey::DefaultModel) => json!(self.config.default_model),
            Some(KnownKey::Language) => json!(self.config.preferences.language),
            Some(KnownKey::Theme) => json!(self.config.preferences.theme),
            None => Value::Null,
        }
    }

    /// 写入指定 key 的值。
    ///
    /// 更新内存副本并持久化到 TOML，返回 [`ConfigChanged`] 供广播。
    /// 类型不匹配（如期望字符串但收到数字）返回 [`XgentError::Config`]。
    pub fn write(&mut self, key: &str, value: Value) -> XgentResult<ConfigChanged> {
        let known = KnownKey::parse(key)
            .ok_or_else(|| XgentError::Config(format!("未知配置 key: {key}")))?;
        match known {
            KnownKey::DefaultProvider => {
                self.config.default_provider = parse_string(key, &value)?;
            }
            KnownKey::DefaultModel => {
                self.config.default_model = parse_string(key, &value)?;
            }
            KnownKey::Language => {
                self.config.preferences.language = parse_string(key, &value)?;
            }
            KnownKey::Theme => {
                self.config.preferences.theme = parse_string(key, &value)?;
            }
        }
        // 持久化
        GlobalConfigStore::save(&self.config)?;
        Ok(ConfigChanged {
            scope: ConfigScope::Global,
            key: known.as_str().to_string(),
            value,
        })
    }
}

/// 把 [`Value`] 解析为字符串，类型不符则报错。
fn parse_string(key: &str, value: &Value) -> XgentResult<String> {
    value
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| XgentError::Config(format!("配置 {key} 期望字符串值，收到: {value}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_config() -> GlobalConfig {
        GlobalConfig::default()
    }

    #[test]
    fn read_returns_known_keys() {
        let mut cfg = empty_config();
        cfg.default_provider = "openai".into();
        cfg.default_model = "gpt-4".into();
        cfg.preferences.language = "zh-CN".into();
        cfg.preferences.theme = "dark".into();
        let c = ConfigCoordinator::with_config(cfg);
        assert_eq!(c.read("default_provider"), json!("openai"));
        assert_eq!(c.read("default_model"), json!("gpt-4"));
        assert_eq!(c.read("preferences.language"), json!("zh-CN"));
        assert_eq!(c.read("preferences.theme"), json!("dark"));
        // 未知 key
        assert_eq!(c.read("unknown.key"), Value::Null);
    }

    #[test]
    fn write_updates_memory_and_returns_changed() {
        let mut c = ConfigCoordinator::with_config(empty_config());
        let changed = c.write("default_model", json!("claude-3")).unwrap();
        assert_eq!(changed.key, "default_model");
        assert_eq!(changed.value, json!("claude-3"));
        assert_eq!(c.read("default_model"), json!("claude-3"));
    }

    #[test]
    fn write_unknown_key_errors() {
        let mut c = ConfigCoordinator::with_config(empty_config());
        let err = c.write("no.such.key", json!("x")).unwrap_err();
        assert!(matches!(err, XgentError::Config(_)));
    }

    #[test]
    fn write_wrong_type_errors() {
        let mut c = ConfigCoordinator::with_config(empty_config());
        let err = c.write("default_model", json!(42)).unwrap_err();
        assert!(matches!(err, XgentError::Config(_)));
    }

    #[test]
    fn write_persists_to_memory_and_reflects_in_config() {
        // 持久化到 TOML 的链路在 settings_core 已测过往返；
        // 此处验证 write 后 config() 副本同步更新。
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write("default_model", json!("gpt-5")).unwrap();
        assert_eq!(c.config().default_model, "gpt-5");
        c.write("preferences.language", json!("en-US")).unwrap();
        assert_eq!(c.config().preferences.language, "en-US");
    }

    #[test]
    fn known_key_parse_roundtrip() {
        for key in [
            KnownKey::DefaultProvider,
            KnownKey::DefaultModel,
            KnownKey::Language,
            KnownKey::Theme,
        ] {
            let s = key.as_str();
            assert_eq!(KnownKey::parse(s), Some(key));
        }
        assert_eq!(KnownKey::parse("nope"), None);
    }
}
