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
use crate::tool::{
    Concurrency, SecurityPolicy, Tool, ToolCtx, ToolError, ToolResult, ToolTier, ToolUpdateCallback,
};

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
    /// 运行参数（项目配置 `[tools]`，R3-17），**运行期可换**。
    ///
    /// 与 `RunCommand` 共享同一句柄，也由 `xgent_app` 的实时配置持有：
    /// 确认超时与命令上限因此在下一次调用就按新值走，不必重启。
    params: Arc<RwLock<crate::ToolParams>>,
    /// 检查点存储（`None` = 不记录，改动不可撤销）。
    ///
    /// 由宿主（`xgent_app`）注入，与 `AgentBridge` 共享同一 `Arc`；执行器是所有
    /// 写工具的唯一必经点，快照放在这里就不存在「某条路径忘了备份」。
    checkpoints: Option<Arc<crate::checkpoint::CheckpointStore>>,
}

impl ToolExecutor {
    /// 构造并注册内置工具（内置缺省参数）。
    pub fn with_defaults() -> Self {
        Self::with_params(&crate::ToolParams::default())
    }

    /// 用给定运行参数构造并注册内置工具（`xgent_app` 由项目配置派生）。
    ///
    /// 参数被拷进一个新的共享句柄；要运行期改参数请用
    /// [`ToolExecutor::with_shared_params`]。
    pub fn with_params(params: &crate::ToolParams) -> Self {
        Self::with_shared_params(Arc::new(RwLock::new(*params)))
    }

    /// 用**共享**参数句柄构造并注册内置工具。
    ///
    /// 宿主把同一个 `Arc` 既交给执行器又留作己用，写入即生效于下一次调用。
    pub fn with_shared_params(params: Arc<RwLock<crate::ToolParams>>) -> Self {
        let tools: Vec<Arc<dyn Tool>> = crate::default_tools_shared(params.clone());
        let map = tools.into_iter().map(|t| (t.id().to_string(), t)).collect();
        Self {
            tools: RwLock::new(map),
            allowed_all: tokio::sync::Mutex::new(HashSet::new()),
            params,
            checkpoints: None,
        }
    }

    /// 挂上检查点存储（链式构造，宿主在 `Arc::new` 之前调用）。
    pub fn attach_checkpoints(mut self, store: Arc<crate::checkpoint::CheckpointStore>) -> Self {
        self.checkpoints = Some(store);
        self
    }

    /// 当前挂接的检查点存储（宿主/测试确认接线用）。
    pub fn checkpoints(&self) -> Option<&Arc<crate::checkpoint::CheckpointStore>> {
        self.checkpoints.as_ref()
    }

    /// 用指定工具集合构造（测试用），运行参数取内置缺省的快照。
    pub fn new(tools: Vec<Arc<dyn Tool>>) -> Self {
        let map = tools.into_iter().map(|t| (t.id().to_string(), t)).collect();
        Self {
            tools: RwLock::new(map),
            allowed_all: tokio::sync::Mutex::new(HashSet::new()),
            params: Arc::new(RwLock::new(crate::ToolParams::default())),
            checkpoints: None,
        }
    }

    /// 当前运行参数句柄（宿主用来写入新值，执行器与 `run_command` 都读它）。
    pub fn params(&self) -> Arc<RwLock<crate::ToolParams>> {
        self.params.clone()
    }

    /// 已注册工具的 id 与信任层（设置面板的三态策略行据此列出）。
    ///
    /// 按 id 排序：面板行的顺序要稳定，否则每次重开面板都在换位置。
    pub fn policy_targets(&self) -> Vec<(String, ToolTier)> {
        let mut rows: Vec<(String, ToolTier)> = self
            .tools
            .read()
            .values()
            .map(|t| (t.id().to_string(), t.tier()))
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows
    }

    /// 该工具在给定策略配置下**解析后**的答案（设置面板显示用）。
    ///
    /// 走的是与执行同一条判据（[`crate::security::resolve_policy`]），只是这里用
    /// `Null` 输入取样：`run_command` 这类按输入升级的工具在空输入下返回静态 tier，
    /// 面板据此显示"配置的缺省"，具体危险命令仍由执行时的动态升级拦住——显示的
    /// 是配置层的答案，不是"任何输入都免确认"的承诺。
    pub fn resolved_policy(
        &self,
        tool_id: &str,
        policy: &xgent_settings_core::project::ToolPolicyConfig,
    ) -> Option<SecurityPolicy> {
        let tool = self.tools.read().get(tool_id).cloned()?;
        Some(resolve_policy(
            tool_id,
            tool.tier(),
            &serde_json::Value::Null,
            tool.as_ref(),
            policy,
        ))
    }

    /// 执行工具，并在写入前后与检查点存储打点。
    ///
    /// 三条执行路径（自动批准 / 本次允许 / 会话允许）都只经此方法调用工具，
    /// 因此「忘了记检查点」不可能由路径遗漏产生；被拒绝的调用在更早的分支就已
    /// 返回，不会留下记录。
    async fn run_tool(
        &self,
        tool: &Arc<dyn Tool>,
        input: serde_json::Value,
        ctx: &ToolCtx,
        signal: CancellationToken,
        on_update: Option<Arc<ToolUpdateCallback>>,
    ) -> Result<ToolResult, ToolError> {
        let targets = tool.write_targets(&input, ctx);
        if !targets.is_empty()
            && let Some(store) = self.checkpoints.as_ref()
        {
            store.snapshot_before_write(&targets, ctx).await;
        }
        let result = tool.execute(input, ctx, signal, on_update).await;
        let wrote = matches!(&result, Ok(r) if !r.is_error);
        if wrote
            && !targets.is_empty()
            && let Some(store) = self.checkpoints.as_ref()
        {
            // 回填「agent 写进去的那份」哈希：撤销靠它区分「还是我写的」与
            // 「用户后来改过」，不靠 mtime（touch / git checkout 会误判）
            store.finalize_after_write(&targets, ctx).await;
        }
        result
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

    /// 该调用能否与相邻调用**同时**执行（R3-15 的调度判据）。
    ///
    /// 三个互相独立的信号全过才算可以，缺一个就串行：
    /// 1. **不需要用户确认**——`Approved`，或 `NeedsConfirmation` 但该工具已在本会话
    ///    AllowAll 集合里。确认通道是单槽（`SharedConfirm` 持一个
    ///    `Mutex<Option<Sender>>`），两个并发请求会互相覆盖 sender，被覆盖那次的
    ///    等待只能看到 channel closed，用户决策就此丢失。
    /// 2. 工具自报 [`Concurrency::Shared`]。
    /// 3. 信任层**恰好**是 [`ToolTier::Read`]。这里不收 `UiOnly`，两个理由都是硬的：
    ///    - [`crate::security::resolve_policy`] 对 `UiOnly` **无条件放行**（不经确认），
    ///      所以若算上它，已发货的 `editor.*` 会在默认配置下就成组并发——"默认配置等价于
    ///      串行"这条不变量当场不成立；
    ///    - `open_file → goto` 这类编辑器命令**有顺序语义**，把它们并发等于把正确性
    ///      押在"该工具恰好在 emit 前不 await"这种隐性质上。顺序性要由判据保证。
    ///    - 第 3 条也不是第 2 条的重复：`Tool::concurrency()` 的 trait 默认值就是 `Shared`，
    ///      只信第 2 条等于让忘了声明 `Exclusive` 的写工具自动获得并发。
    ///
    /// 判定只是**建议**：`execute` 内部仍自行解析策略，真正是否弹窗由它决定。
    /// 两次读取非原子的唯一分歧方向是「用户在批次运行中点了 AllowAll」，后果是多问
    /// 一次确认——绝不会少问一次，也绝不会丢决策。
    pub async fn parallel_safe(
        &self,
        tool_id: &str,
        input: &serde_json::Value,
        ctx: &ToolCtx,
    ) -> bool {
        // 取工具句柄后立即放掉 tools 的读锁：下面要 await allowed_all 的异步锁
        let Some(tool) = self.tools.read().get(tool_id).cloned() else {
            return false;
        };
        if tool.concurrency() != Concurrency::Shared {
            return false;
        }
        let tier = tool.tier();
        if tier != ToolTier::Read {
            return false;
        }
        // 先取出策略再 match：读锁守卫必须在这条语句结束时释放，不能跨 await 持有
        let resolved = resolve_policy(tool_id, tier, input, tool.as_ref(), &ctx.tool_policy.read());
        match resolved {
            SecurityPolicy::Approved => true,
            SecurityPolicy::NeedsConfirmation => self.allowed_all.lock().await.contains(tool_id),
            // 被策略拒绝的调用交给 execute 返回拒绝结果，不参与并发调度
            SecurityPolicy::Denied => false,
        }
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
        // 策略读的是共享句柄的当前值：面板改完 [tool_policy] 后这一次调用就按新值判
        // （读锁守卫在语句结束即释放，下面要 await 确认与执行）
        let policy = resolve_policy(
            tool_id,
            tool.tier(),
            &input,
            tool.as_ref(),
            &ctx.tool_policy.read(),
        );
        match policy {
            SecurityPolicy::Denied => Ok(ToolResult {
                output: "工具被策略拒绝".into(),
                is_error: true,
                denied: true,
                side_effect: None,
            }),
            SecurityPolicy::Approved => self.run_tool(&tool, input, ctx, signal, on_update).await,
            SecurityPolicy::NeedsConfirmation => {
                // 会话级 AllowAll 命中则跳过确认
                if self.allowed_all.lock().await.contains(tool_id) {
                    return self.run_tool(&tool, input, ctx, signal, on_update).await;
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
                //
                // 超时须覆盖**两段**：取回 receiver 与等 `rx.await` 拿到决策。
                // 生产实现 `BridgeConfirm::confirm` 立即返回 receiver（用户的
                // 点击发生在之后），只包前一段等于超时永不触发——确认请求会一直
                // 挂着，除 Abort 外没有出口（2026-10-14 由 `[tools]
                // confirm_timeout_secs` 的行为测试暴露，见台账 §七）。
                // `biased` 让取消优先判定，Abort 不被超时抢跑。
                // 超时秒数在**发起这次确认时**读取（而不是构造执行器时）：
                // 面板改完 `[tools] confirm_timeout_secs` 后，下一次确认就按新值等。
                let confirm_timeout_secs = self.params.read().confirm_timeout_secs;
                let decision = tokio::select! {
                    biased;
                    _ = signal.cancelled() => {
                        return Err(ToolError::Aborted);
                    }
                    r = tokio::time::timeout(
                        std::time::Duration::from_secs(confirm_timeout_secs),
                        async {
                            let rx = confirm.confirm(req).await;
                            rx.await
                        },
                    ) => match r {
                        Ok(Ok(d)) => d,
                        Ok(Err(_)) => return Ok(ToolResult { output: "确认被取消".into(), is_error: true, denied: false, side_effect: None }),
                        Err(_) => return Ok(ToolResult { output: "确认请求超时".into(), is_error: true, denied: false, side_effect: None }),
                    },
                };
                match decision {
                    ConfirmDecision::Allow => {
                        self.run_tool(&tool, input, ctx, signal, on_update).await
                    }
                    ConfirmDecision::AllowAll => {
                        self.allowed_all.lock().await.insert(tool_id.to_string());
                        self.run_tool(&tool, input, ctx, signal, on_update).await
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

    /// 永不回应的 mock 回调：留住发送端，使接收端既不完成也不失败，
    /// 从而只能由确认超时结束等待（测 A8 的超时支路）。
    struct NeverAnswers(std::sync::Mutex<Vec<oneshot::Sender<ConfirmDecision>>>);
    impl NeverAnswers {
        fn new() -> Self {
            Self(std::sync::Mutex::new(Vec::new()))
        }
    }
    #[async_trait::async_trait]
    impl ConfirmCallback for NeverAnswers {
        async fn confirm(&self, _req: ConfirmRequest) -> oneshot::Receiver<ConfirmDecision> {
            let (tx, rx) = oneshot::channel();
            self.0.lock().expect("锁").push(tx);
            rx
        }
    }

    fn ctx(root: &std::path::Path, policy: ToolPolicyConfig) -> ToolCtx {
        ToolCtx::with_policy(root.to_path_buf(), policy)
    }

    fn policy(approved: &[&str], denied: &[&str]) -> ToolPolicyConfig {
        ToolPolicyConfig {
            approved: approved.iter().map(|s| s.to_string()).collect(),
            denied: denied.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// A8：确认等待上限取自 `[tools] confirm_timeout_secs`。
    ///
    /// 1 秒上限 + 永不回应的回调 → 应很快报「确认请求超时」，而不是内置的
    /// 5 分钟。改回硬编码 300s 时本测试会挂到 tokio 的 30s 测试超时。
    #[tokio::test]
    async fn configured_confirm_timeout_expires() {
        use std::time::Instant;
        let params = crate::ToolParams {
            confirm_timeout_secs: 1,
            ..Default::default()
        };
        let exec = ToolExecutor::with_params(&params);
        let dir = tempfile::tempdir().unwrap();
        let hold = NeverAnswers::new();
        let started = Instant::now();
        let r = exec
            .execute(
                "read_file",
                serde_json::json!({"path": "x.txt"}),
                &ctx(dir.path(), Default::default()),
                CancellationToken::new(),
                &hold,
                None,
            )
            .await
            .unwrap();
        let elapsed = started.elapsed();
        assert!(r.is_error, "超时应是错误: {}", r.output);
        assert!(
            r.output.contains("确认请求超时"),
            "应走超时支路而非取消支路: {}",
            r.output
        );
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "配置 1s 生效则不应等满内置 300s: {elapsed:?}"
        );
    }

    /// A8：等待期间 Abort 仍然优先于超时（保住 R1-2 修好的取消语义）。
    #[tokio::test]
    async fn abort_beats_confirm_timeout() {
        let params = crate::ToolParams {
            confirm_timeout_secs: 30,
            ..Default::default()
        };
        let exec = ToolExecutor::with_params(&params);
        let dir = tempfile::tempdir().unwrap();
        let hold = NeverAnswers::new();
        let signal = CancellationToken::new();
        let cancel = signal.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            cancel.cancel();
        });
        let r = exec
            .execute(
                "read_file",
                serde_json::json!({"path": "x.txt"}),
                &ctx(dir.path(), Default::default()),
                signal,
                &hold,
                None,
            )
            .await;
        assert!(
            matches!(r, Err(ToolError::Aborted)),
            "Abort 应即刻中断等待: {r:?}"
        );
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

    /// `tier = Write` 却自报 `Shared` 的工具：模拟"忘了声明 Exclusive"的新写工具。
    ///
    /// 存在的意义是给 tier 判据提供反证对象——`Tool::concurrency()` 的 trait 默认值
    /// 就是 `Shared`，若并行资格只看它，这类工具会被放进并发组里同时写文件。
    struct WriteSharedTool;

    #[async_trait::async_trait]
    impl Tool for WriteSharedTool {
        fn id(&self) -> &str {
            "write_shared"
        }
        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "write_shared".into(),
                description: "tier Write 但声明 Shared".into(),
                input_schema: json!({"type":"object"}),
            }
        }
        fn tier(&self) -> ToolTier {
            ToolTier::Write
        }
        fn concurrency(&self) -> Concurrency {
            Concurrency::Shared
        }
        async fn summarize(&self, _input: &Value) -> String {
            "write_shared".into()
        }
        async fn execute(
            &self,
            _input: Value,
            _ctx: &ToolCtx,
            _signal: CancellationToken,
            _on_update: Option<Arc<ToolUpdateCallback>>,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult {
                output: "ok".into(),
                is_error: false,
                denied: false,
                side_effect: None,
            })
        }
    }

    /// `tier = UiOnly` 且自报 `Shared` 的工具：形状等同已发货的 `EditorTool`。
    ///
    /// 它是"默认配置到底等不等价于串行"的判据所在：`resolve_policy` 对 `UiOnly`
    /// **不经确认直接放行**，所以只要资格里收了 `UiOnly`，`editor.*` 就会在用户
    /// 什么都没配置的情况下成组并发——而 `open_file → goto` 是有顺序语义的。
    struct UiOnlySharedTool;

    #[async_trait::async_trait]
    impl Tool for UiOnlySharedTool {
        fn id(&self) -> &str {
            "ui_only_shared"
        }
        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "ui_only_shared".into(),
                description: "tier UiOnly 且声明 Shared（策略自动放行）".into(),
                input_schema: json!({"type":"object"}),
            }
        }
        fn tier(&self) -> ToolTier {
            ToolTier::UiOnly
        }
        fn concurrency(&self) -> Concurrency {
            Concurrency::Shared
        }
        async fn summarize(&self, _input: &Value) -> String {
            "ui_only_shared".into()
        }
        async fn execute(
            &self,
            _input: Value,
            _ctx: &ToolCtx,
            _signal: CancellationToken,
            _on_update: Option<Arc<ToolUpdateCallback>>,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult {
                output: "ok".into(),
                is_error: false,
                denied: false,
                side_effect: None,
            })
        }
    }

    /// R3-15 的并行资格：三条独立判据（策略 / concurrency / tier）缺一条就不并行。
    #[tokio::test]
    async fn parallel_safe_requires_all_three_signals() {
        use crate::builtins::{ReadFile, WriteFile};
        let dir = tempfile::tempdir().unwrap();
        let approved = policy(&["sleep", "read_file", "write_shared", "write_file"], &[]);
        let exec = ToolExecutor::new(vec![
            Arc::new(SleepTool),
            Arc::new(ReadFile),
            Arc::new(WriteSharedTool),
            Arc::new(UiOnlySharedTool),
            Arc::new(WriteFile),
        ]);
        let c = ctx(dir.path(), approved.clone());
        // ① 三条全过：Approved + Shared + Read
        assert!(
            exec.parallel_safe("sleep", &json!({}), &c).await,
            "只读且无需确认的调用应可并行"
        );
        // ② 策略没过：默认全部 NeedsConfirmation
        let c2 = ctx(dir.path(), policy(&[], &[]));
        assert!(
            !exec.parallel_safe("sleep", &json!({}), &c2).await,
            "还要弹窗确认的调用不得并行——确认通道只有一个槽位"
        );
        // ③ tier 没过：自报 Shared 的写工具（trait 默认就是 Shared，不能只信它）
        assert!(
            !exec.parallel_safe("write_shared", &json!({}), &c).await,
            "Write 层的工具即使自报 Shared 也必须独占"
        );
        // ④ concurrency 没过：老实声明 Exclusive 的写工具
        assert!(
            !exec
                .parallel_safe("write_file", &json!({"path":"a.txt","content":"x"}), &c)
                .await,
            "声明 Exclusive 的写工具不得进并发组"
        );
        // ⑤ UiOnly 一律不并行——即便策略对它**自动放行**（不需要批准、不需要确认），
        //    资格仍要求 tier == Read。这一条钉住"默认配置等价于串行"这条不变量：
        //    已发货的 editor.* 属 UiOnly，若不排除就会在用户没配任何东西时成组并发
        let c4 = ctx(dir.path(), policy(&[], &[]));
        assert!(
            !exec.parallel_safe("ui_only_shared", &json!({}), &c4).await,
            "UiOnly 虽被策略自动放行，仍不得进并发组（编辑器命令有顺序语义）"
        );
        // ⑥ 未注册的 id：交给 execute 返回"未知工具"，不参与调度
        assert!(!exec.parallel_safe("nope", &json!({}), &c).await);
        // ⑦ 被策略拒绝的调用不并行（拒绝也在 execute 里统一处理）
        let c3 = ctx(dir.path(), policy(&["sleep"], &["sleep"]));
        assert!(!exec.parallel_safe("sleep", &json!({}), &c3).await);
    }

    /// 用户点过 AllowAll 之后，同一调用才获得并行资格：判据读的是活的会话状态。
    #[tokio::test]
    async fn parallel_safe_flips_after_allow_all() {
        struct AllowAllOnce;
        #[async_trait::async_trait]
        impl ConfirmCallback for AllowAllOnce {
            async fn confirm(&self, req: ConfirmRequest) -> oneshot::Receiver<ConfirmDecision> {
                let (tx, rx) = oneshot::channel();
                let _ = tx.send(ConfirmDecision::AllowAll);
                let _ = req;
                rx
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path(), policy(&[], &[]));
        let exec = ToolExecutor::new(vec![Arc::new(SleepTool)]);
        assert!(
            !exec.parallel_safe("sleep", &json!({}), &c).await,
            "尚未确认过的只读调用应保持串行"
        );
        // 走一次真实执行路径，让 AllowAll 决策写进会话集合
        let r = exec
            .execute(
                "sleep",
                json!({}),
                &c,
                CancellationToken::new(),
                &AllowAllOnce,
                None,
            )
            .await;
        assert!(matches!(r, Ok(ok) if !ok.is_error));
        assert!(
            exec.parallel_safe("sleep", &json!({}), &c).await,
            "AllowAll 之后无需再确认，应可并行"
        );
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
    }

    /// 写工具改由检查点记录改前状态：不再有旁置 `.xgent-bak`，且撤销可回到写入前。
    #[tokio::test]
    async fn write_tools_record_checkpoints_instead_of_backup_files() {
        use crate::checkpoint::{EntryKind, SkipReason};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let target = root.join("a.txt");
        tokio::fs::write(&target, "1\n2\n3\n").await.unwrap();
        let store = std::sync::Arc::new(crate::CheckpointStore::new(root.clone()));
        let exec = ToolExecutor::with_defaults().attach_checkpoints(store.clone());
        store.begin_turn(42).await;

        let r = exec
            .execute(
                "edit_file",
                json!({"path": "a.txt", "start_line": 2, "end_line": 2, "new_content": "TWO"}),
                &ctx(&root, policy(&["edit_file"], &[])),
                CancellationToken::new(),
                &AutoAllow,
                None,
            )
            .await
            .unwrap();
        assert!(!r.is_error, "edit_file 应成功: {}", r.output);

        // 不再留旁置备份文件
        let mut rd = tokio::fs::read_dir(&root).await.unwrap();
        while let Some(e) = rd.next_entry().await.unwrap() {
            let n = e.file_name().to_string_lossy().to_string();
            assert!(!n.ends_with(".xgent-bak"), "不该再产生旁置备份: {n}");
        }

        let report = store.undo_last_turn(42).await;
        assert_eq!(report.restored, vec!["a.txt".to_string()]);
        assert!(
            report
                .skipped
                .iter()
                .all(|(_, r)| *r != SkipReason::Unverified),
            "写入成功应已回填校验值"
        );
        assert_eq!(
            tokio::fs::read_to_string(&target).await.unwrap(),
            "1\n2\n3\n",
            "撤销后回到写入前"
        );
        let _ = EntryKind::Modified;
    }

    /// 只读工具不声明写入目标，因此不产生检查点。
    #[test]
    fn read_only_tools_declare_no_write_targets() {
        let dir = tempfile::tempdir().unwrap();
        let c = ToolCtx {
            project_root: dir.path().to_path_buf(),
            tool_policy: Default::default(),
        };
        use crate::builtins::{ReadFile, RunCommand, SearchFiles};
        assert!(ReadFile.write_targets(&json!({"path": "."}), &c).is_empty());
        assert!(
            SearchFiles
                .write_targets(&json!({"pattern": "x"}), &c)
                .is_empty()
        );
        // run_command 确实能改磁盘（sed -i / git apply），但它不声明目标：撤销覆盖不到它。
        // 这条断言钉住「走 trait 默认 = 不可撤销」，UI 文案的覆盖边界以此为据。
        assert!(
            RunCommand::default()
                .write_targets(&json!({"command": "true"}), &c)
                .is_empty()
        );
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

    // ---- R3-13：配置改完即生效（不需要重建执行器） ----

    /// 丢掉命令的 sink：只为把 `editor` 工具挂进执行器，让策略行列出它。
    struct NullSink;
    #[async_trait::async_trait]
    impl crate::EditorCommandSink for NullSink {
        fn emit(&self, _req: crate::EditorCommandRequest) -> Result<(), String> {
            Ok(())
        }
    }

    /// 数确认次数并一律放行的 mock。
    struct CountingAllow {
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl ConfirmCallback for CountingAllow {
        async fn confirm(&self, _req: ConfirmRequest) -> oneshot::Receiver<ConfirmDecision> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(ConfirmDecision::Allow);
            rx
        }
    }

    /// 共享策略句柄：把某工具提进 `approved` 后，**下一次调用**就不再问确认。
    ///
    /// 判据是确认次数（1 → 仍为 1），不是耗时：策略若被 `ToolCtx` 按值拷走，
    /// 第二次调用会再问一次，计数变 2。
    #[tokio::test]
    async fn approved_tool_stops_asking_after_policy_handle_changes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        let policy = std::sync::Arc::new(RwLock::new(ToolPolicyConfig::default()));
        let ctx = ToolCtx {
            project_root: dir.path().to_path_buf(),
            tool_policy: policy.clone(),
        };
        std::fs::write(dir.path().join("a.txt"), "hi").unwrap();
        let exec = ToolExecutor::with_defaults();
        let count = std::sync::Arc::new(AtomicUsize::new(0));
        let confirm = CountingAllow {
            calls: count.clone(),
        };

        exec.execute(
            "read_file",
            json!({"path": "a.txt"}),
            &ctx,
            CancellationToken::new(),
            &confirm,
            None,
        )
        .await
        .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1, "默认配置应问一次确认");

        policy.write().approved.push("read_file".to_string());
        exec.execute(
            "read_file",
            json!({"path": "a.txt"}),
            &ctx,
            CancellationToken::new(),
            &confirm,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "改完策略后下一次调用应自动放行（问过一次就不该再问）"
        );
    }

    /// 共享策略句柄：把工具提进 `denied` 后，下一次调用直接拒绝且不碰确认通道。
    ///
    /// 两个 id 都要跑：`read_file` 代表普通只读工具，`run_command` 是设置面板
    /// 三态行里最常被拒绝的那一个（Exec 层 + 按输入动态升级），只测前者就等于
    /// 没测到"拒绝优先于动态升级"这条次序。
    #[tokio::test]
    async fn denied_tool_is_refused_after_policy_handle_changes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        let policy = std::sync::Arc::new(RwLock::new(ToolPolicyConfig::default()));
        let ctx = ToolCtx {
            project_root: dir.path().to_path_buf(),
            tool_policy: policy.clone(),
        };
        std::fs::write(dir.path().join("a.txt"), "hi").unwrap();
        let exec = ToolExecutor::with_defaults();
        let count = std::sync::Arc::new(AtomicUsize::new(0));
        let confirm = CountingAllow {
            calls: count.clone(),
        };

        let cases = [
            ("read_file", json!({"path": "a.txt"})),
            ("run_command", json!({"command": "true"})),
        ];
        for (id, input) in cases {
            policy.write().denied.push(id.to_string());
            let r = exec
                .execute(id, input, &ctx, CancellationToken::new(), &confirm, None)
                .await
                .unwrap();
            assert!(r.denied, "{id} 被拒绝时应标 denied: {}", r.output);
            assert!(r.is_error, "{id} 的拒绝应作为逻辑失败回灌");
        }
        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "denied 不该先问一次确认（两个工具都不该问）"
        );
    }

    /// 设为自动执行**不能**让危险命令免确认：动态升级判据读的是输入，
    /// 与策略来自共享句柄这件事互不干扰（R1-5 的修复不得被 UI 通路弄回去）。
    #[tokio::test]
    async fn approved_run_command_still_confirms_dangerous_input() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        let policy = std::sync::Arc::new(RwLock::new(ToolPolicyConfig {
            approved: vec!["run_command".to_string()],
            denied: vec![],
        }));
        let ctx = ToolCtx {
            project_root: dir.path().to_path_buf(),
            tool_policy: policy.clone(),
        };
        let exec = ToolExecutor::with_defaults();
        let count = std::sync::Arc::new(AtomicUsize::new(0));
        let confirm = CountingAllow {
            calls: count.clone(),
        };

        // 普通命令：approved 命中，自动放行
        exec.execute(
            "run_command",
            json!({"command": "true"}),
            &ctx,
            CancellationToken::new(),
            &confirm,
            None,
        )
        .await
        .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 0, "普通命令应免确认");

        // 危险命令：同一份配置下仍要问
        exec.execute(
            "run_command",
            json!({"command": "rm -rf /tmp/definitely-not-here"}),
            &ctx,
            CancellationToken::new(),
            &confirm,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "配置 approved 后危险命令仍须询问一次"
        );
    }

    /// 确认超时读的是共享参数句柄：改小之后**下一次确认**就按新值到期。
    ///
    /// 执行器构造时句柄里是内置的 300s；若超时被缓存在执行器字段上，
    /// 这条测试会等 300s 而不是 1s。
    #[tokio::test]
    async fn confirm_timeout_written_to_handle_expires_next_request() {
        use std::time::Instant;
        let dir = tempfile::tempdir().unwrap();
        let params = std::sync::Arc::new(RwLock::new(crate::ToolParams::default()));
        assert_eq!(params.read().confirm_timeout_secs, 300);
        let exec = ToolExecutor::with_shared_params(params.clone());
        let ctx = ToolCtx {
            project_root: dir.path().to_path_buf(),
            tool_policy: Default::default(),
        };
        params.write().confirm_timeout_secs = 1;

        let hold = NeverAnswers::new();
        let started = Instant::now();
        let r = exec
            .execute(
                "read_file",
                json!({"path": "missing.txt"}),
                &ctx,
                CancellationToken::new(),
                &hold,
                None,
            )
            .await
            .unwrap();
        let elapsed = started.elapsed();
        assert!(
            r.output.contains("确认请求超时"),
            "应走超时支路: {}",
            r.output
        );
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "写入句柄的 1s 应被下一次确认为准: {elapsed:?}"
        );
    }

    /// 面板的三态显示与执行走同一条判据（显示错了比不显示更糟）。
    #[test]
    fn resolved_policy_agrees_with_execution_rules() {
        let exec = ToolExecutor::with_defaults();
        exec.register(std::sync::Arc::new(crate::EditorTool::new(
            std::sync::Arc::new(NullSink),
        )));

        // UiOnly：不写任何配置就是自动执行
        assert_eq!(
            exec.resolved_policy(crate::EditorTool::ID, &ToolPolicyConfig::default()),
            Some(SecurityPolicy::Approved),
            "editor 工具应显示为自动执行"
        );
        // 默认：其余工具需确认
        assert_eq!(
            exec.resolved_policy("read_file", &ToolPolicyConfig::default()),
            Some(SecurityPolicy::NeedsConfirmation)
        );
        // denied 优先
        assert_eq!(
            exec.resolved_policy("read_file", &policy(&["read_file"], &["read_file"])),
            Some(SecurityPolicy::Denied)
        );
        // 未知工具没有行
        assert_eq!(
            exec.resolved_policy("no_such_tool", &Default::default()),
            None
        );
        // 三态行按 id 排序且含内置工具
        let targets = exec.policy_targets();
        let ids: Vec<&str> = targets.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&"read_file") && ids.contains(&"run_command"));
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "策略行应按 id 稳定排序");
    }
}
