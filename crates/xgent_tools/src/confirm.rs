//! 确认请求与决策类型。
//!
//! `NeedsConfirmation` 工具产生 [`ConfirmRequest`]，经 ECS 事件触发 UI 弹窗，
//! 用户决策后回传 [`ConfirmDecision`]，执行器据此执行或拒绝。

use serde::{Deserialize, Serialize};

/// 请求用户确认。
///
/// `old_content` / `new_content` 用于确认弹窗展示 diff（写文件类工具）。
/// 二者均为 `Some` 时 UI 渲染增删行；否则展示 `summary` 文本。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmRequest {
    /// 工具 id
    pub tool_id: String,
    /// 工具输入参数
    pub input: serde_json::Value,
    /// 人类可读摘要，如 `"写入文件 /path/to/x.rs"`
    pub summary: String,
    /// 旧文件内容（若工具将覆盖已存在文件），None 表示新建文件或非写操作
    #[serde(default)]
    pub old_content: Option<String>,
    /// 新文件内容（工具将写入的内容），None 表示非写操作
    #[serde(default)]
    pub new_content: Option<String>,
}

/// 用户对确认请求的决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfirmDecision {
    /// 本次允许
    Allow,
    /// 此类工具本次会话全允许（便利特性）
    AllowAll,
    /// 拒绝
    Deny,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_request_roundtrip() {
        let r = ConfirmRequest {
            tool_id: "write_file".into(),
            input: serde_json::json!({"path": "/x.rs"}),
            summary: "写入文件 /x.rs".into(),
            old_content: Some("old".into()),
            new_content: Some("new".into()),
        };
        let j = serde_json::to_string(&r).unwrap();
        let r2: ConfirmRequest = serde_json::from_str(&j).unwrap();
        assert_eq!(r2.tool_id, "write_file");
        assert_eq!(r2.summary, "写入文件 /x.rs");
        assert_eq!(r2.old_content.as_deref(), Some("old"));
        assert_eq!(r2.new_content.as_deref(), Some("new"));
    }

    #[test]
    fn confirm_request_defaults_none() {
        // 缺省序列化（旧格式无 diff 字段）应能反序列化，diff 为 None
        let j = r#"{"tool_id":"t","input":{},"summary":"s"}"#;
        let r: ConfirmRequest = serde_json::from_str(j).unwrap();
        assert_eq!(r.old_content, None);
        assert_eq!(r.new_content, None);
    }

    #[test]
    fn confirm_decision_roundtrip() {
        for d in [
            ConfirmDecision::Allow,
            ConfirmDecision::AllowAll,
            ConfirmDecision::Deny,
        ] {
            let j = serde_json::to_string(&d).unwrap();
            let d2: ConfirmDecision = serde_json::from_str(&j).unwrap();
            assert_eq!(d2, d);
        }
    }
}
