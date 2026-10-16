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
    /// 项目根（`scope = project` 时必填，绝对路径）。
    ///
    /// 旧客户端不发这个字段照旧能解析：`Option` 字段缺省时 serde 本就填 `None`
    /// （`#[serde(default)]` 是把这个事实写明，不是它生效的前提）。真正的兼容闸门在
    /// daemon 侧——project 作用域缺 root 是显式错误，不会猜一个订阅过的项目。
    #[serde(default)]
    pub project_root: Option<String>,
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
    /// 项目根（`scope = project` 时必填，绝对路径），语义同 [`ConfigReadRequest`]。
    #[serde(default)]
    pub project_root: Option<String>,
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
    /// 项目根（仅 `scope = project` 携带）。
    ///
    /// 客户端靠它判断这次变更是否与自己有利害关系：同一 daemon 上可能同时挂着
    /// 多个项目的窗口，缺这个字段会让别的项目的窗口以为自己的配置被改了。
    #[serde(default)]
    pub project_root: Option<String>,
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
            project_root: Some("/tmp/proj".into()),
        };
        let j = serde_json::to_string(&r).unwrap();
        let r2: ConfigReadRequest = serde_json::from_str(&j).unwrap();
        assert_eq!(r2.scope, ConfigScope::Project);
        assert_eq!(r2.key, "providers.openai.api_key");
        assert_eq!(r2.project_root.as_deref(), Some("/tmp/proj"));
    }

    #[test]
    fn config_write_request_roundtrip() {
        let r = ConfigWriteRequest {
            scope: ConfigScope::Global,
            key: "theme".into(),
            value: serde_json::json!("dark"),
            project_root: None,
        };
        let j = serde_json::to_string(&r).unwrap();
        let r2: ConfigWriteRequest = serde_json::from_str(&j).unwrap();
        assert_eq!(r2.scope, ConfigScope::Global);
        assert_eq!(r2.value, serde_json::json!("dark"));
        assert_eq!(r2.project_root, None);
    }

    #[test]
    fn config_changed_roundtrip() {
        let c = ConfigChanged {
            scope: ConfigScope::Global,
            key: "theme".into(),
            value: serde_json::json!("light"),
            project_root: None,
        };
        let j = serde_json::to_string(&c).unwrap();
        let c2: ConfigChanged = serde_json::from_str(&j).unwrap();
        assert_eq!(c2.scope, ConfigScope::Global);
        assert_eq!(c2.key, "theme");
        assert_eq!(c2.value, serde_json::json!("light"));
    }

    /// 旧客户端的线格式里没有 `project_root`：`Option` 字段缺省即 `None`，请求照旧解析。
    /// 加**非** `Option` 字段时才真的需要 `#[serde(default)]`，否则缺字段会整体失败。
    #[test]
    fn legacy_wire_without_project_root_still_parses() {
        let r: ConfigReadRequest =
            serde_json::from_str(r#"{"scope":"global","key":"default_provider"}"#).unwrap();
        assert_eq!(r.scope, ConfigScope::Global);
        assert_eq!(r.project_root, None);

        let w: ConfigWriteRequest =
            serde_json::from_str(r#"{"scope":"global","key":"x","value":1}"#).unwrap();
        assert_eq!(w.project_root, None);

        let c: ConfigChanged =
            serde_json::from_str(r#"{"scope":"global","key":"x","value":1}"#).unwrap();
        assert_eq!(c.project_root, None);
    }
}
