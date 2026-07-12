//! 配置读写请求/响应与变更通知类型。
//!
//! 配置分全局（用户级）与项目两级作用域，UI 通过 JSON-RPC 读写，
//! daemon 在配置变更时广播 [`ConfigChanged`] 通知。

use serde::{Deserialize, Serialize};

/// 配置作用域。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigScope {
    /// 用户全局配置
    Global,
    /// 当前项目配置
    Project,
}

/// 配置读请求参数（UI → daemon `config.read`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigReadRequest {
    /// 配置作用域
    pub scope: ConfigScope,
    /// 配置键（点分路径，如 `"providers.openai.api_key"`）
    pub key: String,
}

/// 配置写请求参数（UI → daemon `config.write`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigWriteRequest {
    /// 配置作用域
    pub scope: ConfigScope,
    /// 配置键
    pub key: String,
    /// 配置值
    pub value: serde_json::Value,
}

/// 配置变更通知（daemon → UI `config.changed`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChanged {
    /// 配置作用域
    pub scope: ConfigScope,
    /// 变更的配置键
    pub key: String,
    /// 新值
    pub value: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_scope_serializes_lowercase() {
        let j = serde_json::to_string(&ConfigScope::Global).unwrap();
        assert_eq!(j, r#""global""#);
        let s: ConfigScope = serde_json::from_str(r#""project""#).unwrap();
        assert_eq!(s, ConfigScope::Project);
    }

    #[test]
    fn config_read_request_roundtrip() {
        let r = ConfigReadRequest {
            scope: ConfigScope::Project,
            key: "providers.openai.api_key".into(),
        };
        let j = serde_json::to_string(&r).unwrap();
        let r2: ConfigReadRequest = serde_json::from_str(&j).unwrap();
        assert_eq!(r2.scope, ConfigScope::Project);
        assert_eq!(r2.key, "providers.openai.api_key");
    }

    #[test]
    fn config_write_request_roundtrip() {
        let r = ConfigWriteRequest {
            scope: ConfigScope::Global,
            key: "theme".into(),
            value: serde_json::json!("dark"),
        };
        let j = serde_json::to_string(&r).unwrap();
        let r2: ConfigWriteRequest = serde_json::from_str(&j).unwrap();
        assert_eq!(r2.scope, ConfigScope::Global);
        assert_eq!(r2.value, serde_json::json!("dark"));
    }

    #[test]
    fn config_changed_roundtrip() {
        let c = ConfigChanged {
            scope: ConfigScope::Global,
            key: "theme".into(),
            value: serde_json::json!("light"),
        };
        let j = serde_json::to_string(&c).unwrap();
        let c2: ConfigChanged = serde_json::from_str(&j).unwrap();
        assert_eq!(c2.scope, ConfigScope::Global);
        assert_eq!(c2.key, "theme");
        assert_eq!(c2.value, serde_json::json!("light"));
    }
}
