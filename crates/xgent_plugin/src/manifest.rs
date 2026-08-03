//! 插件清单解析（`plugin.toml`）。
//!
//! 照设计文档 §6.2 定义清单结构与 TOML 反序列化。id 规则校验 `[a-z][a-z0-9_]*`（§6.4）。
//! 对标 Zed `extension/src/extension_manifest.rs`。

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 插件清单错误。
#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("清单解析失败: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("清单字段非法: {0}")]
    Invalid(String),
}

/// 插件清单（`plugin.toml` 反序列化）。
///
/// 照设计文档 §6.2。`id` 必须匹配 `[a-z][a-z0-9_]*`，加载时校验。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub version: String,
    pub schema_version: i32,
    #[serde(default)]
    pub authors: Vec<String>,
    pub repository: Option<String>,
    pub lib: LibManifest,
    #[serde(default)]
    pub tools: Vec<ToolManifestEntry>,
    #[serde(default)]
    pub commands: Vec<CommandManifestEntry>,
    #[serde(default)]
    pub context_providers: Vec<ContextProviderManifestEntry>,
    #[serde(default)]
    pub permissions: PermissionsManifest,
}

/// 库清单（编译语言）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LibManifest {
    /// MVP 仅 "rust"。
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "rust".to_string()
}

/// 权限声明（§9.2）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionsManifest {
    #[serde(default)]
    pub fs_read: Vec<String>,
    #[serde(default)]
    pub fs_write: Vec<String>,
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub command: Vec<String>,
}

/// 工具清单条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifestEntry {
    pub id: String,
    /// "read" / "write" / "exec"
    pub tier: String,
    #[serde(default)]
    pub description: String,
}

/// 命令清单条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandManifestEntry {
    pub id: String,
    pub label: String,
}

/// ContextProvider 清单条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextProviderManifestEntry {
    pub id: String,
    #[serde(default)]
    pub description: String,
}

impl PluginManifest {
    /// 从 TOML 字符串解析，并校验 id 规则（§6.4）。
    pub fn from_toml(s: &str) -> Result<Self, ManifestError> {
        let manifest: PluginManifest = toml::from_str(s)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// 校验 id 与短 id 命名规则：`[a-z][a-z0-9_]*`。
    fn validate(&self) -> Result<(), ManifestError> {
        validate_id(&self.id).map_err(|e| ManifestError::Invalid(format!("id: {e}")))?;
        for t in &self.tools {
            validate_id(&t.id).map_err(|e| ManifestError::Invalid(format!("tool id: {e}")))?;
        }
        for c in &self.commands {
            validate_id(&c.id).map_err(|e| ManifestError::Invalid(format!("command id: {e}")))?;
        }
        for p in &self.context_providers {
            validate_id(&p.id).map_err(|e| ManifestError::Invalid(format!("provider id: {e}")))?;
        }
        Ok(())
    }
}

/// 校验 id 匹配 `[a-z][a-z0-9_]*`。
fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("id 不能为空".into());
    }
    let mut chars = id.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() {
        return Err(format!("id 必须以小写字母开头: {id}"));
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err(format!("id 只能含小写字母/数字/下划线: {id}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_git_manifest() {
        let toml = r#"
id = "git"
name = "Git 集成"
description = "Git diff/commit/log"
version = "0.1.0"
schema_version = 1
authors = ["XGent Team"]

[lib]
kind = "rust"

[[tools]]
id = "git_diff"
tier = "read"
description = "查看 Git diff"

[[commands]]
id = "diff"
label = "Git: Diff"

[[context_providers]]
id = "git_history"

[permissions]
fs-read = ["**"]
command = ["git"]
"#;
        let m = PluginManifest::from_toml(toml).expect("解析成功");
        assert_eq!(m.id, "git");
        assert_eq!(m.tools.len(), 1);
        assert_eq!(m.tools[0].id, "git_diff");
        assert_eq!(m.permissions.command, vec!["git".to_string()]);
    }

    #[test]
    fn reject_invalid_id() {
        let toml = r#"
id = "Git"
name = "x"
version = "0.1.0"
schema_version = 1
[lib]
"#;
        let err = PluginManifest::from_toml(toml).unwrap_err();
        assert!(matches!(err, ManifestError::Invalid(_)));
    }
}
