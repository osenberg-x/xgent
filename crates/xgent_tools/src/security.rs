//! 安全策略判定。
//!
//! 综合配置覆盖、工具 tier 与动态 `approval_for(input)`，得出最终执行策略。
//! 决议顺序：配置 denied → 配置 approved → 工具 `approval_for(input)` 动态 tier
//! → 按 tier 推导默认策略（`UiOnly`→`Approved`，其余→`NeedsConfirmation`）。

use crate::tool::{SecurityPolicy, Tool, ToolTier};
use serde_json::Value;
use xgent_settings_core::project::ToolPolicyConfig;

/// 综合判定工具的最终安全策略。
///
/// 决议顺序：
/// 1. `policy.denied` 命中 → [`SecurityPolicy::Denied`]
/// 2. `policy.approved` 命中 → [`SecurityPolicy::Approved`]
/// 3. `tool.approval_for(input)` 动态 tier（保留供未来更严格判定）
/// 4. 按 tier 推导默认：
///    - [`ToolTier::UiOnly`] → [`SecurityPolicy::Approved`]（仅 UI 状态变更，无副作用）
///    - `Read`/`Write`/`Exec` → [`SecurityPolicy::NeedsConfirmation`]（MVP 默认全需确认）
/// `tool` 参数用于调用 `approval_for`；`tier` 为工具静态分层（由调用方
/// 传入 `tool.tier()`），保留为显式参数便于未来在 yolo 模式下按 tier
/// 自动批准 Read 工具。
pub fn resolve_policy(
    tool_id: &str,
    tier: ToolTier,
    input: &Value,
    tool: &dyn Tool,
    policy: &ToolPolicyConfig,
) -> SecurityPolicy {
    // 1. 配置显式 denied 优先
    if policy.denied.iter().any(|t| t == tool_id) {
        return SecurityPolicy::Denied;
    }
    // 2. 配置显式 approved 次之
    if policy.approved.iter().any(|t| t == tool_id) {
        return SecurityPolicy::Approved;
    }
    // 3. 动态 approval_for（可能比静态 tier 更严格，如 run_command 危险命令）
    let _effective_tier = tool.approval_for(input);
    // 4. 按 tier 推导默认策略：
    //    - `UiOnly`（编辑器动作，仅 UI 状态变更，无副作用）→ `Approved`
    //    - `Read`/`Write`/`Exec` → `NeedsConfirmation`（MVP 默认全需确认）
    match tier {
        ToolTier::UiOnly => SecurityPolicy::Approved,
        ToolTier::Read | ToolTier::Write | ToolTier::Exec => SecurityPolicy::NeedsConfirmation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{Concurrency, ToolCtx, ToolError, ToolResult};
    use async_trait::async_trait;
    use serde_json::json;
    use xgent_core::chat::ToolSchema;

    /// 测试用工具：tier 可配置，approval_for 可 override。
    struct MockTool {
        id: &'static str,
        tier: ToolTier,
        approval: Option<ToolTier>,
    }

    #[async_trait]
    impl Tool for MockTool {
        fn id(&self) -> &str {
            self.id
        }
        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: self.id.into(),
                description: "mock".into(),
                input_schema: json!({"type":"object"}),
            }
        }
        fn tier(&self) -> ToolTier {
            self.tier
        }
        fn approval_for(&self, _input: &Value) -> ToolTier {
            self.approval.unwrap_or(self.tier)
        }
        fn concurrency(&self) -> Concurrency {
            Concurrency::Shared
        }
        fn summarize(&self, _input: &Value) -> String {
            "mock".into()
        }
        async fn execute(
            &self,
            _input: Value,
            _ctx: &ToolCtx,
            _signal: tokio_util::sync::CancellationToken,
            _on_update: Option<&crate::tool::ToolUpdateCallback>,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult {
                output: "ok".into(),
                is_error: false,
                side_effect: None,
            })
        }
    }

    fn policy(approved: &[&str], denied: &[&str]) -> ToolPolicyConfig {
        ToolPolicyConfig {
            approved: approved.iter().map(|s| s.to_string()).collect(),
            denied: denied.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn denied_config_returns_denied() {
        // 配置 denied 命中 → Denied
        let p = policy(&["read_file"], &["read_file"]);
        let tool = MockTool {
            id: "read_file",
            tier: ToolTier::Read,
            approval: None,
        };
        assert_eq!(
            resolve_policy("read_file", ToolTier::Read, &json!({}), &tool, &p),
            SecurityPolicy::Denied
        );
    }

    #[test]
    fn approved_config_returns_approved() {
        // 配置 approved 命中（无 denied）→ Approved
        let p = policy(&["read_file"], &[]);
        let tool = MockTool {
            id: "read_file",
            tier: ToolTier::Read,
            approval: None,
        };
        assert_eq!(
            resolve_policy("read_file", ToolTier::Read, &json!({}), &tool, &p),
            SecurityPolicy::Approved
        );
    }

    #[test]
    fn dynamic_approval_for_called() {
        // 未配置时走 approval_for 动态 tier，MVP 仍映射 NeedsConfirmation
        let p = ToolPolicyConfig::default();
        let tool = MockTool {
            id: "run_command",
            tier: ToolTier::Exec,
            approval: Some(ToolTier::Exec),
        };
        assert_eq!(
            resolve_policy(
                "run_command",
                ToolTier::Exec,
                &json!({"command": "rm -rf /"}),
                &tool,
                &p
            ),
            SecurityPolicy::NeedsConfirmation
        );
    }

    #[test]
    fn default_needs_confirmation_for_all_tiers() {
        // MVP：Read/Write/Exec 全映射 NeedsConfirmation
        let p = ToolPolicyConfig::default();
        for tier in [ToolTier::Read, ToolTier::Write, ToolTier::Exec] {
            let tool = MockTool {
                id: "x",
                tier,
                approval: None,
            };
            assert_eq!(
                resolve_policy("x", tier, &json!({}), &tool, &p),
                SecurityPolicy::NeedsConfirmation,
                "tier {tier:?} 应映射 NeedsConfirmation"
            );
        }
    }

    #[test]
    fn denied_overrides_approved_config() {
        // denied 优先于 approved
        let p = policy(&["write_file"], &["write_file"]);
        let tool = MockTool {
            id: "write_file",
            tier: ToolTier::Write,
            approval: None,
        };
        assert_eq!(
            resolve_policy("write_file", ToolTier::Write, &json!({}), &tool, &p),
            SecurityPolicy::Denied
        );
    }

    /// 9 组合矩阵：Read/Write/Exec × (denied 命中/approved 命中/默认)
    /// 断言：denied → Denied；approved → Approved；默认 → NeedsConfirmation（MVP 全 tier）
    #[test]
    fn policy_matrix_9_combinations() {
        for tier in [ToolTier::Read, ToolTier::Write, ToolTier::Exec] {
            let tool_id = match tier {
                ToolTier::Read => "read_file",
                ToolTier::Write => "write_file",
                ToolTier::Exec => "run_command",
                ToolTier::UiOnly => "editor",
            };
            let tool = MockTool {
                id: tool_id,
                tier,
                approval: None,
            };
            // denied 命中 → Denied
            let p_denied = policy(&[], &[tool_id]);
            assert_eq!(
                resolve_policy(tool_id, tier, &json!({}), &tool, &p_denied),
                SecurityPolicy::Denied,
                "tier {tier:?} denied 应为 Denied"
            );
            // approved 命中 → Approved
            let p_approved = policy(&[tool_id], &[]);
            assert_eq!(
                resolve_policy(tool_id, tier, &json!({}), &tool, &p_approved),
                SecurityPolicy::Approved,
                "tier {tier:?} approved 应为 Approved"
            );
            // 默认（无配置）→ NeedsConfirmation
            let p_default = ToolPolicyConfig::default();
            assert_eq!(
                resolve_policy(tool_id, tier, &json!({}), &tool, &p_default),
                SecurityPolicy::NeedsConfirmation,
                "tier {tier:?} 默认应为 NeedsConfirmation"
            );
        }
    }

    /// UiOnly tier 默认为 Approved（不走 NeedsConfirmation），
    /// 但配置 denied 仍可拒绝、approved 显式批准仍生效。
    /// 详见 `doc/design/editor-design.md` 6.4 节。
    #[test]
    fn uionly_tier_default_approved() {
        let tool = MockTool {
            id: "editor.open_file",
            tier: ToolTier::UiOnly,
            approval: None,
        };
        let p_default = ToolPolicyConfig::default();
        assert_eq!(
            resolve_policy(
                "editor.open_file",
                ToolTier::UiOnly,
                &json!({}),
                &tool,
                &p_default
            ),
            SecurityPolicy::Approved,
            "UiOnly 默认应为 Approved"
        );
    }

    /// UiOnly 仍可被配置 denied 拒绝。
    #[test]
    fn uionly_tier_denied_overrides() {
        let tool = MockTool {
            id: "editor.open_file",
            tier: ToolTier::UiOnly,
            approval: None,
        };
        let p = policy(&[], &["editor.open_file"]);
        assert_eq!(
            resolve_policy("editor.open_file", ToolTier::UiOnly, &json!({}), &tool, &p),
            SecurityPolicy::Denied,
            "UiOnly 被 denied 配置拒绝"
        );
    }
}
