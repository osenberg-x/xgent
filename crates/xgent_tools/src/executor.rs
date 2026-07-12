//! 工具执行器：调度工具、处理安全策略与确认流程。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::oneshot;

use crate::confirm::{ConfirmDecision, ConfirmRequest};
use crate::security::resolve_policy;
use crate::tool::{SecurityPolicy, Tool, ToolCtx, ToolResult};

/// 确认回调 trait：传入确认请求，返回一个可 await 的决策接收端。
///
/// 由调用方（xgent_agent 的 ECS 桥接）实现：发起 UI 弹窗，
/// 用户决策后通过 oneshot 回传 [`ConfirmDecision`]。
#[async_trait::async_trait]
pub trait ConfirmCallback: Send + Sync {
    async fn confirm(&self, req: ConfirmRequest) -> oneshot::Receiver<ConfirmDecision>;
}

/// 工具执行器。
pub struct ToolExecutor {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// 会话级 AllowAll 集合：用户选过 AllowAll 的工具不再确认
    allowed_all: tokio::sync::Mutex<HashSet<String>>,
}

impl ToolExecutor {
    /// 构造并注册内置工具。
    pub fn with_defaults() -> Self {
        let tools: Vec<Arc<dyn Tool>> = crate::default_tools();
        let map = tools.into_iter().map(|t| (t.id().to_string(), t)).collect();
        Self {
            tools: map,
            allowed_all: tokio::sync::Mutex::new(HashSet::new()),
        }
    }

    /// 用指定工具集合构造（测试用）。
    pub fn new(tools: Vec<Arc<dyn Tool>>) -> Self {
        let map = tools.into_iter().map(|t| (t.id().to_string(), t)).collect();
        Self {
            tools: map,
            allowed_all: tokio::sync::Mutex::new(HashSet::new()),
        }
    }

    /// 注册工具。
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.id().to_string(), tool);
    }

    /// 列出所有已注册工具的 schema（供 provider 的 tools 参数）。
    pub fn schemas(&self) -> Vec<xgent_core::chat::ToolSchema> {
        self.tools.values().map(|t| t.schema()).collect()
    }

    /// 执行工具调用。
    ///
    /// 流程：
    /// 1. 解析最终策略（配置覆盖工具默认）；
    /// 2. `Denied` 直接拒绝；
    /// 3. `Approved` 或会话级 AllowAll 命中 → 直接执行；
    /// 4. `NeedsConfirmation` → 经 `confirm` 获取决策，Allow/AllowAll 后执行。
    pub async fn execute(
        &self,
        tool_id: &str,
        input: serde_json::Value,
        ctx: &ToolCtx,
        confirm: &dyn ConfirmCallback,
    ) -> ToolResult {
        let tool = match self.tools.get(tool_id) {
            Some(t) => t,
            None => {
                return ToolResult {
                    output: format!("未知工具: {tool_id}"),
                    success: false,
                    side_effect: None,
                };
            }
        };
        let policy = resolve_policy(tool_id, tool.policy(), &ctx.tool_policy);
        match policy {
            SecurityPolicy::Denied => ToolResult {
                output: "已被策略拒绝（denied）".into(),
                success: false,
                side_effect: None,
            },
            SecurityPolicy::Approved => tool.execute(input, ctx).await,
            SecurityPolicy::NeedsConfirmation => {
                // 会话级 AllowAll 命中则跳过确认
                if self.allowed_all.lock().await.contains(tool_id) {
                    return tool.execute(input, ctx).await;
                }
                let req = ConfirmRequest {
                    tool_id: tool_id.to_string(),
                    input: input.clone(),
                    summary: tool.summarize(&input),
                };
                let rx = match tokio::time::timeout(
                    std::time::Duration::from_secs(300),
                    confirm.confirm(req),
                )
                .await
                {
                    Ok(rx) => rx,
                    Err(_) => {
                        return ToolResult {
                            output: "确认请求超时".into(),
                            success: false,
                            side_effect: None,
                        };
                    }
                };
                match rx.await {
                    Ok(ConfirmDecision::Allow) => tool.execute(input, ctx).await,
                    Ok(ConfirmDecision::AllowAll) => {
                        self.allowed_all.lock().await.insert(tool_id.to_string());
                        tool.execute(input, ctx).await
                    }
                    Ok(ConfirmDecision::Deny) => ToolResult {
                        output: "用户拒绝".into(),
                        success: false,
                        side_effect: None,
                    },
                    Err(_) => ToolResult {
                        output: "确认被取消".into(),
                        success: false,
                        side_effect: None,
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SideEffect;
    use serde_json::json;
    use xgent_settings_core::project::ToolPolicyConfig;

    /// 自动允许所有确认的 mock 回调。
    struct AutoAllow;
    #[async_trait::async_trait]
    impl ConfirmCallback for AutoAllow {
        async fn confirm(&self, req: ConfirmRequest) -> oneshot::Receiver<ConfirmDecision> {
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(ConfirmDecision::Allow);
            let _ = req;
            rx
        }
    }

    /// 自动拒绝的 mock 回调。
    struct AutoDeny;
    #[async_trait::async_trait]
    impl ConfirmCallback for AutoDeny {
        async fn confirm(&self, req: ConfirmRequest) -> oneshot::Receiver<ConfirmDecision> {
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(ConfirmDecision::Deny);
            let _ = req;
            rx
        }
    }

    fn ctx(root: &std::path::Path, policy: ToolPolicyConfig) -> ToolCtx {
        ToolCtx {
            project_root: root.to_path_buf(),
            tool_policy: policy,
        }
    }

    fn policy(approved: &[&str], denied: &[&str]) -> ToolPolicyConfig {
        ToolPolicyConfig {
            approved: approved.iter().map(|s| s.to_string()).collect(),
            denied: denied.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[tokio::test]
    async fn unknown_tool_errors() {
        let exec = ToolExecutor::with_defaults();
        let dir = tempfile::tempdir().unwrap();
        let r = exec
            .execute(
                "nope",
                serde_json::json!({}),
                &ctx(dir.path(), Default::default()),
                &AutoAllow,
            )
            .await;
        assert!(!r.success);
        assert!(r.output.contains("未知工具"));
    }

    #[tokio::test]
    async fn denied_tool_rejected() {
        let exec = ToolExecutor::with_defaults();
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("a.txt"), "hi")
            .await
            .unwrap();
        let r = exec
            .execute(
                "read_file",
                json!({"path": "a.txt"}),
                &ctx(dir.path(), policy(&[], &["read_file"])),
                &AutoAllow,
            )
            .await;
        assert!(!r.success);
        assert!(r.output.contains("拒绝"));
    }

    #[tokio::test]
    async fn approved_tool_auto_executes() {
        let exec = ToolExecutor::with_defaults();
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("a.txt"), "hi")
            .await
            .unwrap();
        let r = exec
            .execute(
                "read_file",
                json!({"path": "a.txt"}),
                &ctx(dir.path(), policy(&["read_file"], &[])),
                &AutoDeny,
            )
            .await;
        assert!(r.success);
        assert_eq!(r.output, "hi");
    }

    #[tokio::test]
    async fn needs_confirmation_allow_executes() {
        let exec = ToolExecutor::with_defaults();
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("a.txt"), "hi")
            .await
            .unwrap();
        let r = exec
            .execute(
                "read_file",
                json!({"path": "a.txt"}),
                &ctx(dir.path(), Default::default()),
                &AutoAllow,
            )
            .await;
        assert!(r.success);
        assert_eq!(r.output, "hi");
    }

    #[tokio::test]
    async fn needs_confirmation_deny_rejected() {
        let exec = ToolExecutor::with_defaults();
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("a.txt"), "hi")
            .await
            .unwrap();
        let r = exec
            .execute(
                "read_file",
                json!({"path": "a.txt"}),
                &ctx(dir.path(), Default::default()),
                &AutoDeny,
            )
            .await;
        assert!(!r.success);
        assert!(r.output.contains("用户拒绝"));
    }

    #[tokio::test]
    async fn allow_all_skips_future_confirmations() {
        let exec = ToolExecutor::with_defaults();
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("a.txt"), "hi")
            .await
            .unwrap();
        // 用 AllowAll 回调
        struct AllowAll;
        #[async_trait::async_trait]
        impl ConfirmCallback for AllowAll {
            async fn confirm(&self, req: ConfirmRequest) -> oneshot::Receiver<ConfirmDecision> {
                let (tx, rx) = oneshot::channel();
                let _ = tx.send(ConfirmDecision::AllowAll);
                let _ = req;
                rx
            }
        }
        exec.execute(
            "read_file",
            json!({"path": "a.txt"}),
            &ctx(dir.path(), Default::default()),
            &AllowAll,
        )
        .await;
        // 第二次调用应无需确认——用 AutoDeny 验证（若仍确认会被拒绝）
        let r = exec
            .execute(
                "read_file",
                json!({"path": "a.txt"}),
                &ctx(dir.path(), Default::default()),
                &AutoDeny,
            )
            .await;
        assert!(r.success, "AllowAll 后应跳过确认直接执行");
        assert_eq!(r.output, "hi");
    }

    #[tokio::test]
    async fn write_file_returns_side_effect() {
        let exec = ToolExecutor::with_defaults();
        let dir = tempfile::tempdir().unwrap();
        let r = exec
            .execute(
                "write_file",
                json!({"path": "out.txt", "content": "x"}),
                &ctx(dir.path(), policy(&["write_file"], &[])),
                &AutoDeny,
            )
            .await;
        assert!(r.success);
        assert!(matches!(r.side_effect, Some(SideEffect::FileWritten(_))));
    }

    #[tokio::test]
    async fn schemas_present() {
        let exec = ToolExecutor::with_defaults();
        let s = exec.schemas();
        let ids: Vec<&str> = s.iter().map(|s| s.name.as_str()).collect();
        assert!(ids.contains(&"read_file"));
        assert!(ids.contains(&"write_file"));
        assert!(ids.contains(&"search_files"));
        assert!(ids.contains(&"run_command"));
    }
}
