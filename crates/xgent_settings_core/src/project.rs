//! 项目配置类型（本地隔离，存于 `<project>/.xgent/config.toml`）。

use serde::{Deserialize, Serialize};

/// 项目配置根结构。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ProjectConfig {
    /// 项目根目录（绝对路径）
    #[serde(default)]
    pub project_root: String,

    /// 覆盖默认 provider（None 表示用全局默认）
    #[serde(default)]
    pub provider_override: Option<String>,

    /// 覆盖默认模型
    #[serde(default)]
    pub model_override: Option<String>,

    /// 上下文检索策略
    #[serde(default)]
    pub context_strategy: ContextStrategy,

    /// 工具信任级别覆盖
    #[serde(default)]
    pub tool_policy: ToolPolicyConfig,
}

/// 上下文检索策略。
///
/// MVP 仅用 `OnDemand`（方案 A 无索引·按需读取），其余为后续阶段占位。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContextStrategy {
    /// MVP：方案 A 无索引·按需读取
    #[default]
    OnDemand,
    /// B 阶段：基于仓库结构映射
    RepoMap,
    /// C 阶段：向量检索
    Vector,
    /// E 阶段：混合检索
    Hybrid,
}

/// 工具策略配置：按工具 id 覆盖默认信任级别。
///
/// 默认所有工具为 `NeedsConfirmation`（见架构安全模型 11.1）。
/// `approved` 提升为自动执行，`denied` 降为拒绝。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ToolPolicyConfig {
    /// 提升为自动执行的工具 id 列表
    #[serde(default)]
    pub approved: Vec<String>,

    /// 降为拒绝的工具 id 列表
    #[serde(default)]
    pub denied: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_config_default() {
        let cfg = ProjectConfig::default();
        assert_eq!(cfg.context_strategy, ContextStrategy::OnDemand);
        assert!(cfg.provider_override.is_none());
        assert!(cfg.tool_policy.approved.is_empty());
        assert!(cfg.tool_policy.denied.is_empty());
    }

    #[test]
    fn context_strategy_default_is_ondemand() {
        assert_eq!(ContextStrategy::default(), ContextStrategy::OnDemand);
    }

    #[test]
    fn context_strategy_serde_snake_case() {
        let j = serde_json::to_string(&ContextStrategy::RepoMap).unwrap();
        assert_eq!(j, r#""repo_map""#);
        let c: ContextStrategy = serde_json::from_str(r#""vector""#).unwrap();
        assert_eq!(c, ContextStrategy::Vector);
    }

    #[test]
    fn project_config_roundtrip() {
        let cfg = ProjectConfig {
            project_root: "/abs/proj".into(),
            provider_override: Some("anthropic".into()),
            model_override: None,
            context_strategy: ContextStrategy::RepoMap,
            tool_policy: ToolPolicyConfig {
                approved: vec!["read_file".into()],
                denied: vec!["rm_rf".into()],
            },
        };
        let j = serde_json::to_string(&cfg).unwrap();
        let cfg2: ProjectConfig = serde_json::from_str(&j).unwrap();
        assert_eq!(cfg2.project_root, "/abs/proj");
        assert_eq!(cfg2.provider_override.as_deref(), Some("anthropic"));
        assert!(cfg2.model_override.is_none());
        assert_eq!(cfg2.context_strategy, ContextStrategy::RepoMap);
        assert_eq!(cfg2.tool_policy.approved, vec!["read_file".to_string()]);
        assert_eq!(cfg2.tool_policy.denied, vec!["rm_rf".to_string()]);
    }
}
