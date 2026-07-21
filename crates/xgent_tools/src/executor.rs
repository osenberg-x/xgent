//! 工具执行器：调度工具、处理安全策略与确认流程。
//!
//! 对齐 ADR-0007：`execute` 签名加入 `CancellationToken`，返回
//! `Result<ToolResult, ToolError>`；`resolve_policy` 用新签名
//! （传 `tool.tier()` + `tool` 引用 + `input`）。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::confirm::{ConfirmDecision, ConfirmRequest};
use crate::security::resolve_policy;
use crate::tool::{SecurityPolicy, Tool, ToolCtx, ToolError, ToolResult};

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
    /// 1. 解析最终策略（配置 denied → approved → tool.approval_for → MVP 默认）；
    /// 2. `Denied` → `Ok(ToolResult{is_error:true})`（逻辑失败回灌 LLM）；
    /// 3. `Approved` 或会话级 AllowAll 命中 → 直接执行；
    /// 4. `NeedsConfirmation` → 经 `confirm` 获取决策，Allow/AllowAll 后执行；
    ///    `Deny` → 同 Denied 逻辑。
    ///
    /// `ToolError::Aborted` 透传给调用方（agent loop 走 abort 路径）。
    /// 工具返回 `Ok(ToolResult{is_error:true})` 时 executor 仍返回 `Ok`
    /// （非异常失败，错误文本回灌 LLM）。
    pub async fn execute(
        &self,
        tool_id: &str,
        input: serde_json::Value,
        ctx: &ToolCtx,
        signal: CancellationToken,
        confirm: &dyn ConfirmCallback,
    ) -> Result<ToolResult, ToolError> {
        let tool = match self.tools.get(tool_id) {
            Some(t) => t,
            None => {
                return Ok(ToolResult { output: format!("未知工具: {tool_id}"), is_error: true, denied: false, side_effect: None });
            }
        };
        let policy = resolve_policy(
            tool_id,
            tool.tier(),
            &input,
            tool.as_ref(),
            &ctx.tool_policy,
        );
        match policy {
            SecurityPolicy::Denied => Ok(ToolResult {
                output: "工具被策略拒绝".into(),
                is_error: true,
                denied: true,
                side_effect: None,
            }),
            SecurityPolicy::Approved => tool.execute(input, ctx, signal, None).await,
            SecurityPolicy::NeedsConfirmation => {
                // 会话级 AllowAll 命中则跳过确认
                if self.allowed_all.lock().await.contains(tool_id) {
                    return tool.execute(input, ctx, signal, None).await;
                }
                let (old_content, new_content) = match tool.preview_diff(&input, ctx).await {
                    Some((old, new)) => (Some(old), Some(new)),
                    None => (None, None),
                };
                let req = ConfirmRequest {
                    tool_id: tool_id.to_string(),
                    input: input.clone(),
                    summary: tool.summarize(&input),
                    old_content,
                    new_content,
                };
                let rx = match tokio::time::timeout(
                    std::time::Duration::from_secs(300),
                    confirm.confirm(req),
                )
                .await
                {
                    Ok(rx) => rx,
                    Err(_) => {
                        return Ok(ToolResult { output: "确认请求超时".into(), is_error: true, denied: false, side_effect: None });
                    }
                };
                match rx.await {
                    Ok(ConfirmDecision::Allow) => tool.execute(input, ctx, signal, None).await,
                    Ok(ConfirmDecision::AllowAll) => {
                        self.allowed_all.lock().await.insert(tool_id.to_string());
                        tool.execute(input, ctx, signal, None).await
                    }
                    Ok(ConfirmDecision::Deny) => Ok(ToolResult {
                        output: "用户拒绝".into(),
                        is_error: true,
                        denied: true,
                        side_effect: None,
                    }),
                    Err(_) => Ok(ToolResult { output: "确认被取消".into(), is_error: true, denied: false, side_effect: None }),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{Concurrency, ToolError, ToolTier, ToolUpdateCallback};
    use serde_json::{Value, json};
    use xgent_core::chat::ToolSchema;
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
                CancellationToken::new(),
                &AutoAllow,
            )
            .await
            .unwrap();
        assert!(r.is_error);
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
                CancellationToken::new(),
                &AutoAllow,
            )
            .await
            .unwrap();
        assert!(r.is_error);
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
                CancellationToken::new(),
                &AutoDeny,
            )
            .await
            .unwrap();
        assert!(!r.is_error);
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
                CancellationToken::new(),
                &AutoAllow,
            )
            .await
            .unwrap();
        assert!(!r.is_error);
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
                CancellationToken::new(),
                &AutoDeny,
            )
            .await
            .unwrap();
        assert!(r.is_error);
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
            CancellationToken::new(),
            &AllowAll,
        )
        .await
        .unwrap();
        // 第二次调用应无需确认——用 AutoDeny 验证（若仍确认会被拒绝）
        let r = exec
            .execute(
                "read_file",
                json!({"path": "a.txt"}),
                &ctx(dir.path(), Default::default()),
                CancellationToken::new(),
                &AutoDeny,
            )
            .await
            .unwrap();
        assert!(!r.is_error, "AllowAll 后应跳过确认直接执行");
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
                CancellationToken::new(),
                &AutoDeny,
            )
            .await
            .unwrap();
        assert!(!r.is_error);
        assert!(matches!(
            r.side_effect,
            Some(crate::tool::SideEffect::FileWritten(_))
        ));
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

    /// 可中断的 mock 工具：execute 内 tokio::select! 监听 signal.cancelled()，
    /// 未取消则等待 1s 完成。用于测试 CancellationToken 中断路径。
    struct SleepTool;

    #[async_trait::async_trait]
    impl Tool for SleepTool {
        fn id(&self) -> &str {
            "sleep"
        }
        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "sleep".into(),
                description: "sleep 1s".into(),
                input_schema: json!({"type":"object"}),
            }
        }
        fn tier(&self) -> ToolTier {
            ToolTier::Read
        }
        fn concurrency(&self) -> Concurrency {
            Concurrency::Shared
        }
        fn summarize(&self, _input: &Value) -> String {
            "sleep".into()
        }
        async fn execute(
            &self,
            _input: Value,
            _ctx: &ToolCtx,
            signal: CancellationToken,
            _on_update: Option<&ToolUpdateCallback>,
        ) -> Result<ToolResult, ToolError> {
            tokio::select! {
                _ = signal.cancelled() => Err(ToolError::Aborted),
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                    Ok(ToolResult {
                        output: "done".into(),
                        is_error: false,
                        denied: false,
                        side_effect: None,
                    })
                }
            }
        }
    }

    #[tokio::test]
    async fn cancel_returns_aborted_error() {
        // CancellationToken cancel 后 execute 返回 ToolError::Aborted
        let exec = ToolExecutor::new(vec![Arc::new(SleepTool)]);
        let dir = tempfile::tempdir().unwrap();
        let token = CancellationToken::new();
        // 先 cancel，再执行（模拟中断已发生）
        token.cancel();
        let r = exec
            .execute(
                "sleep",
                json!({}),
                &ctx(dir.path(), Default::default()),
                token,
                &AutoAllow,
            )
            .await;
        match r {
            Err(ToolError::Aborted) => {} // 期望
            other => panic!("期望 ToolError::Aborted，得到 {other:?}"),
        }
    }
}
