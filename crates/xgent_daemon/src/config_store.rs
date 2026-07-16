//! 全局配置权威副本与读写协调。
//!
//! daemon 持有全局配置的权威副本。`config.read` 返回对应字段的值，
//! `config.write` 更新内存副本、持久化到 TOML，并返回 [`ConfigChanged`]
//! 供调用方广播给所有客户端。
//!
//! 支持的 key：
//! - `default_provider` / `default_model` — 字符串
//! - `preferences.language` / `preferences.theme` — 字符串
//! - `providers` — 整个 providers map（读：JSON 对象；写：替换整个 map）
//! - `providers.<id>` — 单个 provider 配置（读：JSON 对象；写：插入/替换）
//! - `providers.<id>.api_base` / `api_key` / `kind` / `model_overrides` — 单字段（`model_overrides` 为插入式：设 map 键 `"default"`）

use serde_json::{Value, json};
use xgent_core::config::{ConfigChanged, ConfigScope};
use xgent_core::error::{XgentError, XgentResult};
use xgent_settings_core::global::ProviderConfig;
use xgent_settings_core::{GlobalConfig, GlobalConfigStore};

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
        match key {
            "default_provider" => json!(self.config.default_provider),
            "default_model" => json!(self.config.default_model),
            "preferences.language" => json!(self.config.preferences.language),
            "preferences.theme" => json!(self.config.preferences.theme),
            "providers" => json!(self.config.providers),
            other if other.starts_with("providers.") => {
                let parts: Vec<&str> = other.split('.').collect();
                // parts[0]="providers", parts[1]=id, parts[2..]=可选字段
                if parts.len() < 2 || parts[1].is_empty() {
                    return Value::Null;
                }
                let id = parts[1];
                let Some(pc) = self.config.providers.get(id) else {
                    return Value::Null;
                };
                if parts.len() == 2 {
                    json!(pc)
                } else {
                    read_provider_field(pc, parts[2])
                }
            }
            _ => Value::Null,
        }
    }

    /// 写入指定 key 的值。
    ///
    /// 更新内存副本并持久化到 TOML，返回 [`ConfigChanged`] 供广播。
    pub fn write(&mut self, key: &str, value: Value) -> XgentResult<ConfigChanged> {
        match key {
            "default_provider" => {
                self.config.default_provider = parse_string(key, &value)?;
            }
            "default_model" => {
                self.config.default_model = parse_string(key, &value)?;
            }
            "preferences.language" => {
                self.config.preferences.language = parse_string(key, &value)?;
            }
            "preferences.theme" => {
                self.config.preferences.theme = parse_string(key, &value)?;
            }
            "providers" => {
                // 整体替换 providers map
                let map: std::collections::HashMap<String, ProviderConfig> =
                    serde_json::from_value(value.clone()).map_err(|e| {
                        XgentError::Config(format!("providers 值反序列化失败: {e}"))
                    })?;
                self.config.providers = map;
            }
            other if other.starts_with("providers.") => {
                let parts: Vec<&str> = other.split('.').collect();
                if parts.len() < 2 || parts[1].is_empty() {
                    return Err(XgentError::Config(format!("无效配置 key: {key}")));
                }
                let id = parts[1].to_string();
                if parts.len() == 2 {
                    // 写入/替换整个 ProviderConfig
                    let pc: ProviderConfig = serde_json::from_value(value.clone())
                        .map_err(|e| {
                            XgentError::Config(format!("provider 配置反序列化失败: {e}"))
                        })?;
                    self.config.providers.insert(id, pc);
                } else {
                    // 写入单个字段
                    let field = parts[2];
                    let pc = self
                        .config
                        .providers
                        .entry(id)
                        .or_insert_with(ProviderConfig::default);
                    write_provider_field(pc, field, &value)?;
                }
            }
            _ => {
                return Err(XgentError::Config(format!("未知配置 key: {key}")));
            }
        }
        // 持久化
        GlobalConfigStore::save(&self.config)?;
        Ok(ConfigChanged {
            scope: ConfigScope::Global,
            key: key.to_string(),
            value,
        })
    }
}

/// 读取 provider 单字段。
fn read_provider_field(pc: &ProviderConfig, field: &str) -> Value {
    match field {
        "kind" => json!(pc.kind),
        "api_base" => json!(pc.api_base),
        "api_key" => json!(pc.api_key),
        "model_overrides" => json!(pc.model_overrides),
        "timeout_secs" => json!(pc.timeout_secs),
        "max_retries" => json!(pc.max_retries),
        _ => Value::Null,
    }
}

/// 写入 provider 单字段。
fn write_provider_field(pc: &mut ProviderConfig, field: &str, value: &Value) -> XgentResult<()> {
    match field {
        "kind" => {
            pc.kind = serde_json::from_value(value.clone()).map_err(|e| {
                XgentError::Config(format!("provider.kind 反序列化失败: {e}"))
            })?;
        }
        "api_base" => pc.api_base = parse_string(field, value)?,
        "api_key" => pc.api_key = parse_string(field, value)?,
        "model_overrides" => {
            // 插入式：设 model_overrides["default"] 的值，不覆盖 map 其他条目。
            // value 为字符串（模型名），写入键 "default"。
            let model_name = parse_string(field, value)?;
            pc.model_overrides.insert("default".to_string(), model_name);
        }
        "timeout_secs" => {
            pc.timeout_secs = value
                .as_u64()
                .ok_or_else(|| XgentError::Config(format!("timeout_secs 期望数字: {value}")))?;
        }
        "max_retries" => {
            pc.max_retries = value
                .as_u64()
                .ok_or_else(|| XgentError::Config(format!("max_retries 期望数字: {value}")))?
                as u32;
        }
        _ => {
            return Err(XgentError::Config(format!(
                "未知 provider 字段: {field}"
            )));
        }
    }
    Ok(())
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
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write("default_model", json!("gpt-5")).unwrap();
        assert_eq!(c.config().default_model, "gpt-5");
        c.write("preferences.language", json!("en-US")).unwrap();
        assert_eq!(c.config().preferences.language, "en-US");
    }

    #[test]
    fn write_provider_field_creates_entry() {
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write("providers.myai.api_base", json!("https://api.example.com/v1"))
            .unwrap();
        c.write("providers.myai.api_key", json!("sk-xxx"))
            .unwrap();
        assert_eq!(
            c.read("providers.myai.api_base"),
            json!("https://api.example.com/v1")
        );
        assert_eq!(c.read("providers.myai.api_key"), json!("sk-xxx"));
        // 默认 kind 是 openai_compat
        assert_eq!(c.read("providers.myai.kind"), json!("open_ai_compat"));
    }

    #[test]
    fn write_provider_model_overrides_inserts_default_key() {
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write("providers.myai.api_base", json!("https://api.example.com/v1"))
            .unwrap();
        // 写 model_overrides：插入式，设 model_overrides["default"]
        c.write("providers.myai.model_overrides", json!("gpt-4o"))
            .unwrap();
        let got = c.read("providers.myai.model_overrides");
        // 读回整个 map，应含 "default" 键
        assert_eq!(got["default"], json!("gpt-4o"));
        // 再写一次，仍是插入式（覆盖 default 值，不影响其他键）
        c.write("providers.myai.model_overrides", json!("gpt-4o-mini"))
            .unwrap();
        let got2 = c.read("providers.myai.model_overrides");
        assert_eq!(got2["default"], json!("gpt-4o-mini"));
    }

    #[test]
    fn read_nonexistent_provider_returns_null() {
        let c = ConfigCoordinator::with_config(empty_config());
        assert_eq!(c.read("providers.nonexistent.api_base"), Value::Null);
        assert_eq!(c.read("providers.nonexistent"), Value::Null);
    }

    #[test]
    fn write_whole_provider_config() {
        let mut c = ConfigCoordinator::with_config(empty_config());
        let pc = json!({
            "kind": "open_ai_compat",
            "api_base": "https://api.deepseek.com/v1",
            "api_key": "sk-deep"
        });
        c.write("providers.deepseek", pc).unwrap();
        let got = c.read("providers.deepseek");
        assert_eq!(got["api_base"], json!("https://api.deepseek.com/v1"));
        assert_eq!(got["api_key"], json!("sk-deep"));
    }
}
