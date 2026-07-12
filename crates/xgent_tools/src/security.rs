//! 安全策略判定。
//!
//! 综合配置覆盖与工具建议默认值，得出最终执行策略。
//! 配置优先：显式 denied > 显式 approved > 工具默认。

use crate::tool::SecurityPolicy;
use xgent_settings_core::project::ToolPolicyConfig;

/// 综合判定工具的最终安全策略。
///
/// - 配置中 `denied` 列表命中 → [`SecurityPolicy::Denied`]
/// - 配置中 `approved` 列表命中 → [`SecurityPolicy::Approved`]
/// - 未配置 → 用工具建议默认值（内置工具均为 `NeedsConfirmation`）
pub fn resolve_policy(
    tool_id: &str,
    tool_default: SecurityPolicy,
    policy: &ToolPolicyConfig,
) -> SecurityPolicy {
    if policy.denied.iter().any(|t| t == tool_id) {
        return SecurityPolicy::Denied;
    }
    if policy.approved.iter().any(|t| t == tool_id) {
        return SecurityPolicy::Approved;
    }
    tool_default
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(approved: &[&str], denied: &[&str]) -> ToolPolicyConfig {
        ToolPolicyConfig {
            approved: approved.iter().map(|s| s.to_string()).collect(),
            denied: denied.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn unconfigured_uses_tool_default() {
        let p = ToolPolicyConfig::default();
        assert_eq!(
            resolve_policy("read_file", SecurityPolicy::NeedsConfirmation, &p),
            SecurityPolicy::NeedsConfirmation
        );
        assert_eq!(
            resolve_policy("x", SecurityPolicy::Approved, &p),
            SecurityPolicy::Approved
        );
    }

    #[test]
    fn approved_overrides_default() {
        let p = policy(&["read_file"], &[]);
        assert_eq!(
            resolve_policy("read_file", SecurityPolicy::NeedsConfirmation, &p),
            SecurityPolicy::Approved
        );
    }

    #[test]
    fn denied_overrides_everything() {
        // denied 优先于 approved 与工具默认
        let p = policy(&["read_file"], &["read_file"]);
        assert_eq!(
            resolve_policy("read_file", SecurityPolicy::Approved, &p),
            SecurityPolicy::Denied
        );
    }

    #[test]
    fn denied_overrides_approved_only() {
        let p = policy(&[], &["write_file"]);
        assert_eq!(
            resolve_policy("write_file", SecurityPolicy::Approved, &p),
            SecurityPolicy::Denied
        );
    }

    #[test]
    fn unrelated_tool_unaffected() {
        let p = policy(&["read_file"], &["write_file"]);
        assert_eq!(
            resolve_policy("search_files", SecurityPolicy::NeedsConfirmation, &p),
            SecurityPolicy::NeedsConfirmation
        );
    }
}
