//! 配置 TOML 读写。
//!
//! [`GlobalConfigStore`] 读写全局配置，[`ProjectConfigStore`] 读写项目配置。
//! 文件不存在时返回 `Default`；写入时确保父目录存在。

use crate::{global::GlobalConfig, paths, project::ProjectConfig};
use std::path::Path;
use xgent_core::{XgentError, XgentResult};

/// 全局配置存储（TOML 文件读写）。
pub struct GlobalConfigStore;

impl GlobalConfigStore {
    /// 读取全局配置。
    ///
    /// 文件不存在时返回 `Default`；存在但解析失败则返回错误。
    pub fn load() -> XgentResult<GlobalConfig> {
        Self::load_from(&paths::global_config_file())
    }

    /// 保存全局配置（确保目录存在）。
    pub fn save(cfg: &GlobalConfig) -> XgentResult<()> {
        Self::save_to(cfg, &paths::global_config_file())
    }

    /// 从指定路径读取配置（测试与自定义路径用）。
    pub fn load_from(path: &Path) -> XgentResult<GlobalConfig> {
        match std::fs::read_to_string(path) {
            Ok(s) => {
                let cfg: GlobalConfig = toml::from_str(&s)
                    .map_err(|e| XgentError::Config(format!("parse {}: {e}", path.display())))?;
                Ok(cfg)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(GlobalConfig::default()),
            Err(e) => Err(XgentError::Io(e)),
        }
    }

    /// 保存配置到指定路径（确保父目录存在）。
    pub fn save_to(cfg: &GlobalConfig, path: &Path) -> XgentResult<()> {
        ensure_parent_dir(path)?;
        let s = toml::to_string_pretty(cfg)
            .map_err(|e| XgentError::Config(format!("serialize: {e}")))?;
        std::fs::write(path, s)?;
        Ok(())
    }
}

/// 项目配置存储（TOML 文件读写）。
pub struct ProjectConfigStore;

impl ProjectConfigStore {
    /// 读取项目配置（`<project_root>/.xgent/config.toml`）。
    pub fn load(project_root: &Path) -> XgentResult<ProjectConfig> {
        ProjectConfigStore::load_from(&paths::project_config_file(project_root))
    }

    /// 保存项目配置。
    pub fn save(cfg: &ProjectConfig) -> XgentResult<()> {
        let root = Path::new(&cfg.project_root);
        ProjectConfigStore::save_to(cfg, &paths::project_config_file(root))
    }

    /// 从指定路径读取项目配置。
    pub fn load_from(path: &Path) -> XgentResult<ProjectConfig> {
        match std::fs::read_to_string(path) {
            Ok(s) => {
                let cfg: ProjectConfig = toml::from_str(&s)
                    .map_err(|e| XgentError::Config(format!("parse {}: {e}", path.display())))?;
                Ok(cfg)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProjectConfig::default()),
            Err(e) => Err(XgentError::Io(e)),
        }
    }

    /// 保存项目配置到指定路径（确保父目录存在）。
    pub fn save_to(cfg: &ProjectConfig, path: &Path) -> XgentResult<()> {
        ensure_parent_dir(path)?;
        let s = toml::to_string_pretty(cfg)
            .map_err(|e| XgentError::Config(format!("serialize: {e}")))?;
        std::fs::write(path, s)?;
        Ok(())
    }
}

/// 确保路径的父目录存在。
fn ensure_parent_dir(path: &Path) -> XgentResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::global::{Preferences, ProviderConfig};
    use tempfile::tempdir;

    #[test]
    fn global_store_load_missing_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.toml");
        let cfg = GlobalConfigStore::load_from(&path).unwrap();
        assert!(cfg.providers.is_empty());
    }

    #[test]
    fn global_store_save_then_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");

        let cfg = GlobalConfig {
            default_provider: "openai".to_string(),
            default_model: "gpt-4".to_string(),
            preferences: Preferences {
                theme: "dark".to_string(),
                ..Default::default()
            },
            providers: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "openai".to_string(),
                    ProviderConfig {
                        kind: crate::global::ProviderKind::OpenAiCompat,
                        api_base: "https://api.openai.com/v1".into(),
                        api_key: "sk-xxx".into(),
                        ..Default::default()
                    },
                );
                m
            },
        };

        GlobalConfigStore::save_to(&cfg, &path).unwrap();
        assert!(path.exists(), "config file should be created");

        let cfg2 = GlobalConfigStore::load_from(&path).unwrap();
        assert_eq!(cfg2.default_provider, "openai");
        assert_eq!(cfg2.default_model, "gpt-4");
        assert_eq!(cfg2.providers.len(), 1);
        let p = cfg2.providers.get("openai").unwrap();
        assert_eq!(p.api_key, "sk-xxx");
        assert_eq!(p.api_base, "https://api.openai.com/v1");
    }

    #[test]
    fn project_store_load_missing_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("x.toml");
        let cfg = ProjectConfigStore::load_from(&path).unwrap();
        assert_eq!(
            cfg.context_strategy,
            crate::project::ContextStrategy::OnDemand
        );
    }

    #[test]
    fn project_store_save_then_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".xgent").join("config.toml");

        let cfg = ProjectConfig {
            project_root: dir.path().to_string_lossy().to_string(),
            provider_override: Some("anthropic".into()),
            model_override: None,
            context_strategy: crate::project::ContextStrategy::RepoMap,
            tool_policy: crate::project::ToolPolicyConfig {
                approved: vec!["read_file".into()],
                denied: vec![],
            },
        };

        ProjectConfigStore::save_to(&cfg, &path).unwrap();
        let cfg2 = ProjectConfigStore::load_from(&path).unwrap();
        assert_eq!(cfg2.provider_override.as_deref(), Some("anthropic"));
        assert_eq!(
            cfg2.context_strategy,
            crate::project::ContextStrategy::RepoMap
        );
        assert_eq!(cfg2.tool_policy.approved, vec!["read_file".to_string()]);
    }

    #[test]
    fn save_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("c").join("config.toml");
        let cfg = GlobalConfig::default();
        GlobalConfigStore::save_to(&cfg, &path).unwrap();
        assert!(path.exists());
    }
}
