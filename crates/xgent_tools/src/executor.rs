//! 工具执行器：调度工具、处理安全策略与确认流程。
//!
//! 对齐 ADR-0007：`execute` 签名加入 `CancellationToken`，返回
//! `Result<ToolResult, ToolError>`；`resolve_policy` 用新签名
//! （传 `tool.tier()` + `tool` 引用 + `input`）。

use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::confirm::{ConfirmDecision, ConfirmRequest};
use crate::security::resolve_policy;
use crate::tool::{SecurityPolicy, Tool, ToolCtx, ToolError, ToolResult, ToolUpdateCallback};

/// 确认回调 trait：传入确认请求，返回一个可 await 的决策接收端。
///
/// 由调用方（xgent_agent 的 ECS 桥接）实现：发起 UI 弹窗，
/// 用户决策后通过 oneshot 回传 [`ConfirmDecision`]。
#[async_trait::async_trait]
pub trait ConfirmCallback: Send + Sync {
    async fn confirm(&self, req: ConfirmRequest) -> oneshot::Receiver<ConfirmDecision>;
}

/// 工具执行器。
///
/// 照设计文档 §7.2 + 插件系统需求：`tools` 用 `RwLock` 包裹实现 interior
/// mutability，使 `register`/`remove_by_prefix` 可经 `&self` 调用（插件
/// 宿主经 `Arc<ToolExecutor>` 共享并动态注册工具）。
///
/// 作为 Bevy Resource 插入 World 时用 `ToolExecutorResource(Arc<ToolExecutor>)`
/// 包装（见下方），与 `AgentBridge` 共享同一 Arc。
pub struct ToolExecutor {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
    /// 会话级 AllowAll 集合：用户选过 AllowAll 的工具不再确认
    allowed_all: tokio::sync::Mutex<HashSet<String>>,
}

impl ToolExecutor {
    /// 构造并注册内置工具。
    pub fn with_defaults() -> Self {
        let tools: Vec<Arc<dyn Tool>> = crate::default_tools();
        let map = tools.into_iter().map(|t| (t.id().to_string(), t)).collect();
        Self {
            tools: RwLock::new(map),
            allowed_all: tokio::sync::Mutex::new(HashSet::new()),
        }
    }

    /// 用指定工具集合构造（测试用）。
    pub fn new(tools: Vec<Arc<dyn Tool>>) -> Self {
        let map = tools.into_iter().map(|t| (t.id().to_string(), t)).collect();
        Self {
            tools: RwLock::new(map),
            allowed_all: tokio::sync::Mutex::new(HashSet::new()),
        }
    }

    pub fn register(&self, tool: Arc<dyn Tool>) {
        self.tools.write().insert(tool.id().to_string(), tool);
    }

    /// 按前缀移除（插件卸载时清理 plugin.<id>. 工具）。
    ///
    /// 照设计文档 §7.2。`&self`（interior mutability via RwLock），在 ECS 主线程
    /// system 内调用。`allowed_all` 是 `tokio::sync::Mutex`，主线程同步上下文
    /// 用 `try_lock()`：失败时记 warn 而非静默跳过。
    pub fn remove_by_prefix(&self, prefix: &str) {
        self.tools.write().retain(|k, _| !k.starts_with(prefix));
        match self.allowed_all.try_lock() {
            Ok(mut set) => set.retain(|k| !k.starts_with(prefix)),
            Err(_) => tracing::warn!(
                prefix,
                "ToolExecutor::remove_by_prefix: allowed_all 锁竞争，AllowAll 缓存未清理"
            ),
        }
    }

    /// 列出所有已注册工具的 schema（供 provider 的 tools 参数）。
    pub fn schemas(&self) -> Vec<xgent_core::chat::ToolSchema> {
        self.tools.read().values().map(|t| t.schema()).collect()
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
    ///
    /// `on_update` 为流式更新回调：转发给工具，长时工具（`run_command` 的
    /// stdout 增量、插件工具的 `push-update`）经它把中间结果回灌调用方。
    pub async fn execute(
        &self,
        tool_id: &str,
        input: serde_json::Value,
        ctx: &ToolCtx,
        signal: CancellationToken,
        confirm: &dyn ConfirmCallback,
        on_update: Option<Arc<ToolUpdateCallback>>,
    ) -> Result<ToolResult, ToolError> {
        let tool = match self.tools.read().get(tool_id).cloned() {
            Some(t) => t,
            None => {
                return Ok(ToolResult {
                    output: format!("未知工具: {tool_id}"),
                    is_error: true,
                    denied: false,
                    side_effect: None,
                });
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
            SecurityPolicy::Approved => tool.execute(input, ctx, signal, on_update).await,
            SecurityPolicy::NeedsConfirmation => {
                // 会话级 AllowAll 命中则跳过确认
                if self.allowed_all.lock().await.contains(tool_id) {
                    return tool.execute(input, ctx, signal, on_update).await;
                }
                let (old_content, new_content) = match tool.preview_diff(&input, ctx).await {
                    Some((old, new)) => (Some(old), Some(new)),
                    None => (None, None),
                };
                let req = ConfirmRequest {
                    tool_id: tool_id.to_string(),
                    input: input.clone(),
                    summary: tool.summarize(&input).await,
                    old_content,
                    new_content,
                };
                // 发起确认请求 + 等待用户决策，全程监听 cancel_token。
                // 修复之前 timeout(confirm.confirm) 不监听 cancel，Abort 在
                // confirm.confirm 卡住（event_tx 满）时需等满 300s 才生效。
                let decision = tokio::select! {
                    r = tokio::time::timeout(
                        std::time::Duration::from_secs(300),
                        confirm.confirm(req),
                    ) => match r {
                        Ok(rx) => match rx.await {
                            Ok(d) => d,
                            Err(_) => return Ok(ToolResult { output: "确认被取消".into(), is_error: true, denied: false, side_effect: None }),
                        },
                        Err(_) => return Ok(ToolResult { output: "确认请求超时".into(), is_error: true, denied: false, side_effect: None }),
                    },
                    _ = signal.cancelled() => {
                        return Err(ToolError::Aborted);
                    }
                };
                match decision {
                    ConfirmDecision::Allow => tool.execute(input, ctx, signal, on_update).await,
                    ConfirmDecision::AllowAll => {
                        self.allowed_all.lock().await.insert(tool_id.to_string());
                        tool.execute(input, ctx, signal, on_update).await
                    }
                    ConfirmDecision::Deny => Ok(ToolResult {
                        output: "用户拒绝".into(),
                        is_error: true,
                        denied: true,
                        side_effect: None,
                    }),
                }
            }
        }
    }
}

/// `ToolExecutor` 的 Bevy Resource 包装（共享 `Arc<ToolExecutor>`）。
///
/// 插件系统需把工具注册到 `ToolExecutor`，但 `AgentBridge` 也持 `Arc<ToolExecutor>`。
/// 经此包装，World 与 bridge 共享同一实例：`xgent_app` 构造 `Arc<ToolExecutor>`，
/// clone 给 bridge，原 Arc 包入此 Resource 插入 World。
#[derive(bevy::prelude::Resource, Clone)]
pub struct ToolExecutorResource(pub Arc<ToolExecutor>);

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
                None,
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
                None,
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
                None,
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
                None,
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
                None,
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
            None,
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
                None,
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
                None,
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
        async fn summarize(&self, _input: &Value) -> String {
            "sleep".into()
        }
        async fn execute(
            &self,
            _input: Value,
            _ctx: &ToolCtx,
            signal: CancellationToken,
            _on_update: Option<Arc<ToolUpdateCallback>>,
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
                None,
            )
            .await;
        match r {
            Err(ToolError::Aborted) => {} // 期望
            other => panic!("期望 ToolError::Aborted，得到 {other:?}"),
        }
    }

    /// 发出流式更新的 mock 工具：执行期间调用一次 `on_update`。
    struct StreamingTool;

    #[async_trait::async_trait]
    impl Tool for StreamingTool {
        fn id(&self) -> &str {
            "streaming"
        }
        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "streaming".into(),
                description: "mock".into(),
                input_schema: json!({"type": "object"}),
            }
        }
        fn tier(&self) -> ToolTier {
            ToolTier::Exec
        }
        fn concurrency(&self) -> Concurrency {
            Concurrency::Exclusive
        }
        async fn summarize(&self, _input: &Value) -> String {
            "streaming".into()
        }
        async fn execute(
            &self,
            _input: Value,
            _ctx: &ToolCtx,
            _signal: CancellationToken,
            on_update: Option<Arc<ToolUpdateCallback>>,
        ) -> Result<ToolResult, ToolError> {
            if let Some(cb) = on_update {
                cb(ToolResult {
                    output: "中间结果".into(),
                    is_error: false,
                    denied: false,
                    side_effect: None,
                });
            }
            Ok(ToolResult {
                output: "最终结果".into(),
                is_error: false,
                denied: false,
                side_effect: None,
            })
        }
    }

    /// executor 必须把 `on_update` 真实转发给工具（R1-9）——
    /// 修复前三条执行路径恒传 `None`，流式进度完全断链。
    #[tokio::test]
    async fn forwards_update_callback_to_tool() {
        let exec = ToolExecutor::new(vec![Arc::new(StreamingTool)]);
        let dir = tempfile::tempdir().unwrap();
        let updates = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink = updates.clone();
        let cb: Arc<ToolUpdateCallback> =
            Arc::new(move |r: ToolResult| sink.lock().unwrap().push(r.output));

        let r = exec
            .execute(
                "streaming",
                json!({}),
                &ctx(dir.path(), policy(&["streaming"], &[])),
                CancellationToken::new(),
                &AutoDeny,
                Some(cb),
            )
            .await
            .unwrap();

        assert_eq!(r.output, "最终结果");
        assert_eq!(
            updates.lock().unwrap().as_slice(),
            ["中间结果".to_string()],
            "工具的流式中间结果应到达回调"
        );
    }

    /// edit_file 走确认流程时会先取 preview_diff，diff 应基于替换结果而非原文。
    #[tokio::test]
    async fn edit_file_registered_by_default_and_edits() {
        let exec = ToolExecutor::with_defaults();
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("a.txt"), "1\n2\n3\n")
            .await
            .unwrap();
        let r = exec
            .execute(
                "edit_file",
                json!({"path": "a.txt", "start_line": 1, "new_content": "TWO"}),
                &ctx(dir.path(), policy(&["edit_file"], &[])),
                CancellationToken::new(),
                &AutoDeny,
                None,
            )
            .await
            .unwrap();
        assert!(!r.is_error, "edit_file 应成功: {}", r.output);
        let content = tokio::fs::read_to_string(dir.path().join("a.txt"))
            .await
            .unwrap();
        assert_eq!(content, "1\nTWO\n3\n");
        // 原文件备份应存在且保留原内容
        let bak = crate::atomic::backup_path_for(&dir.path().join("a.txt"));
        let bak_content = tokio::fs::read_to_string(&bak).await.unwrap();
        assert_eq!(bak_content, "1\n2\n3\n", "备份应保留原内容");
    }

    /// edit_file 的 old_content 不匹配时不得写入。
    #[tokio::test]
    async fn edit_file_rejects_content_mismatch() {
        let exec = ToolExecutor::with_defaults();
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        tokio::fs::write(&f, "1\n2\n3\n").await.unwrap();
        let r = exec
            .execute(
                "edit_file",
                json!({
                    "path": "a.txt", "start_line": 1,
                    "new_content": "TWO", "old_content": "NOT_THERE"
                }),
                &ctx(dir.path(), policy(&["edit_file"], &[])),
                CancellationToken::new(),
                &AutoDeny,
                None,
            )
            .await
            .unwrap();
        assert!(r.is_error, "内容不匹配应报错");
        assert!(
            tokio::fs::read_to_string(&f).await.unwrap() == "1\n2\n3\n",
            "报错时不得修改文件"
        );
    }

    /// read_file 经 executor 传递 offset/limit（R1-3）。
    #[tokio::test]
    async fn read_file_honors_offset_and_limit() {
        let exec = ToolExecutor::with_defaults();
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("a.txt"), "l0\nl1\nl2\nl3\n")
            .await
            .unwrap();
        let r = exec
            .execute(
                "read_file",
                json!({"path": "a.txt", "offset": 1, "limit": 2}),
                &ctx(dir.path(), policy(&["read_file"], &[])),
                CancellationToken::new(),
                &AutoDeny,
                None,
            )
            .await
            .unwrap();
        assert!(!r.is_error);
        assert_eq!(r.output, "l1\nl2");
    }

    /// read_file 超大文件输出被截断且带标记（R1-3）。
    #[tokio::test]
    async fn read_file_truncates_oversized_output() {
        let exec = ToolExecutor::with_defaults();
        let dir = tempfile::tempdir().unwrap();
        let big = "x".repeat(crate::MAX_TOOL_OUTPUT_BYTES * 2);
        tokio::fs::write(dir.path().join("big.txt"), &big)
            .await
            .unwrap();
        let r = exec
            .execute(
                "read_file",
                json!({"path": "big.txt"}),
                &ctx(dir.path(), policy(&["read_file"], &[])),
                CancellationToken::new(),
                &AutoDeny,
                None,
            )
            .await
            .unwrap();
        assert!(!r.is_error);
        assert!(
            r.output.len() < big.len(),
            "输出应被截断（原 {} 字节）",
            big.len()
        );
        assert!(r.output.contains("已截断"), "应含截断标记");
    }
}
