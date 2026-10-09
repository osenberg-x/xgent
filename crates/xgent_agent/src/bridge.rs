//! tokio 与 Bevy ECS 的桥接层。
//!
//! `AgentBridge` 作为 Bevy Resource 持有 tokio runtime 与命令/事件 channel。
//! 异步任务（agent loop）在 tokio 上运行，调 provider/tools/context，
//! 结果经 channel 回 ECS，由 [`crate::agent_loop`] 系统每帧非阻塞轮询。
//!
//! 确认流程：工具执行时，确认请求经事件回 ECS 弹窗，决策经命令回 task
//! （通过 `SharedConfirm` 共享 oneshot）。

use async_trait::async_trait;
use bevy::prelude::*;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};
use xgent_core::chat::{ChatEvent, ChatMessage, ChatRequest};
use xgent_core::ids::StreamId;
use xgent_settings_core::project::ToolPolicyConfig;
use xgent_tools::confirm::{ConfirmDecision, ConfirmRequest};
use xgent_tools::{SideEffect, ToolCtx, ToolExecutor};

/// 对 provider 的调用抽象。
///
/// MVP 本地实现直接调 `LlmProvider`；未来 IPC 实现经 daemon 路由。
/// trait 使两者可互换，调用方无感。
#[async_trait]
pub trait ProviderClient: Send + Sync {
    /// 发起流式对话，返回 (StreamId, ChatEvent 接收端)。
    async fn chat(
        &self,
        req: ChatRequest,
    ) -> Result<
        (StreamId, mpsc::Receiver<ChatEvent>),
        (xgent_core::chat::ErrorKind, String, Option<u64>),
    >;

    /// 取消指定流（默认空实现：本地 provider 由调用方 drop 接收端即可终止）。
    ///
    /// 经 daemon 的 provider 实现必须真正把取消送到上游请求——否则用户
    /// 按下中断后，daemon 侧仍把 LLM 流消费到自然结束，token 照常计费（R1-2）。
    /// 取消应幂等：流不存在或已结束也视为成功。
    async fn cancel(&self, _stream_id: StreamId) {}
}

/// 重试配置：从 [`ProviderConfig`](xgent_settings_core::global::ProviderConfig) 派生，
/// 驱动 agent loop 对可重试错误的自动重试。
///
/// - `max_retries`：`None` 表示无限重试（直到成功或被中断）；`Some(n)` 表示最多重试 n 次。
/// - 仅对可重试错误（`Network`/`StreamParse`）重试；其余错误立即失败。
/// - `mode`：固定间隔或指数退避。
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// 最大重试次数。`None` = 无限。
    pub max_retries: Option<u32>,
    /// 重试模式
    pub mode: xgent_settings_core::global::RetryMode,
    /// 初始间隔毫秒（固定模式为每次等待值；指数模式为退避基准）
    pub initial_delay_ms: u64,
    /// 指数退避上限（固定模式忽略）
    pub max_delay_ms: u64,
    /// 指数退避乘数（固定模式忽略）
    pub backoff_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: Some(2),
            mode: xgent_settings_core::global::RetryMode::Fixed,
            initial_delay_ms: 500,
            max_delay_ms: 30_000,
            backoff_factor: 2.0,
        }
    }
}

impl From<&xgent_settings_core::global::ProviderConfig> for RetryConfig {
    fn from(pc: &xgent_settings_core::global::ProviderConfig) -> Self {
        Self {
            max_retries: pc.max_retries,
            mode: pc.retry_mode,
            initial_delay_ms: pc.retry_initial_delay_ms,
            max_delay_ms: pc.retry_max_delay_ms,
            backoff_factor: pc.retry_backoff_factor,
        }
    }
}

impl RetryConfig {
    /// 判断错误是否可重试。
    ///
    /// 可重试：连接/超时（`Network`）、SSE/JSON 解析（`StreamParse`）、
    /// 服务端限流（`RateLimited`）、服务端错误（`ServerError`）。
    /// 立即失败：`NotConfigured`/`AuthFailed`/`ProviderError`（重试无意义）。
    ///
    /// 限流与服务端错误纳入可重试是 R1-7 的修复：此前 429/5xx 落到
    /// `ProviderError` 直接失败，限流下用户只能手动重发。
    pub fn is_retryable(kind: xgent_core::chat::ErrorKind) -> bool {
        use xgent_core::chat::ErrorKind;
        matches!(
            kind,
            ErrorKind::Network
                | ErrorKind::StreamParse
                | ErrorKind::RateLimited
                | ErrorKind::ServerError
        )
    }

    /// 计算第 `attempt` 次重试（1-based）前的等待时长。
    ///
    /// 固定模式：恒为 `initial_delay_ms`。
    /// 指数模式：`min(initial * factor^(attempt-1), max_delay)`。
    ///
    /// `retry_after_secs` 为服务端 `Retry-After` 时**优先**采用它——
    /// 限流场景下按客户端退避重试只会更快撞上限、加剧拥塞（R1-7）。
    /// 该值仍受 `max_delay_ms` 约束，防止服务端给出离谱的长等待。
    pub fn delay_for_with_retry_after(
        &self,
        attempt: u32,
        retry_after_secs: Option<u64>,
    ) -> std::time::Duration {
        if let Some(secs) = retry_after_secs {
            let ms = secs.saturating_mul(1000).min(self.max_delay_ms);
            return std::time::Duration::from_millis(ms);
        }
        self.delay_for(attempt)
    }

    /// 计算第 `attempt` 次重试（1-based）前的等待时长（不含服务端覆盖）。
    pub fn delay_for(&self, attempt: u32) -> std::time::Duration {
        let ms = match self.mode {
            xgent_settings_core::global::RetryMode::Fixed => self.initial_delay_ms,
            xgent_settings_core::global::RetryMode::Exponential => {
                // attempt >= 1，factor^(attempt-1)；用乘法循环避免 f64 powf 精度问题
                let mut delay = self.initial_delay_ms as f64;
                for _ in 1..attempt {
                    delay *= self.backoff_factor;
                    if delay >= self.max_delay_ms as f64 {
                        delay = self.max_delay_ms as f64;
                        break;
                    }
                }
                delay.min(self.max_delay_ms as f64) as u64
            }
        };
        std::time::Duration::from_millis(ms)
    }

    /// 是否还有重试机会。
    ///
    /// `max_retries == None` 时退到有限缺省值 [`Self::bounded_max_retries`]——
    /// 无界重试会让网络长期故障时 agent 任务永久占用并持续计费（R1-7）。
    /// `max_retries == Some(n)` → 当 `attempt < n` 时可继续。
    pub fn can_retry(&self, attempt: u32) -> bool {
        attempt < self.effective_max_retries()
    }

    /// 实际生效的重试上限（永远有限）。
    ///
    /// `max_retries == None` 退到 [`Self::DEFAULT_MAX_RETRIES`]：无界重试会让
    /// 网络长期故障时 agent 任务永久占用、连接与配额持续消耗（R1-7）。
    pub fn effective_max_retries(&self) -> u32 {
        self.max_retries.unwrap_or(Self::DEFAULT_MAX_RETRIES)
    }

    /// 有限重试缺省次数（保守：正常请求远少于该值）。
    pub const DEFAULT_MAX_RETRIES: u32 = 5;
}

/// 共享确认状态：异步任务等待的 oneshot 由 ECS 回填决策。
#[derive(Clone, Default)]
pub struct SharedConfirm {
    inner: Arc<Mutex<Option<oneshot::Sender<ConfirmDecision>>>>,
}

impl SharedConfirm {
    /// 异步任务调用：发请求并返回等待决策的 oneshot。
    pub async fn take_sender(&self) -> Option<oneshot::Sender<ConfirmDecision>> {
        self.inner.lock().await.take()
    }

    /// ECS 调用：收到 ConfirmRequestEvent 后，把决策 sender 存入。
    pub async fn set_sender(&self, tx: oneshot::Sender<ConfirmDecision>) {
        *self.inner.lock().await = Some(tx);
    }
}

/// 桥接 Resource：持有 tokio runtime 与命令 channel。
#[derive(Resource)]
pub struct AgentBridge {
    /// tokio runtime
    pub runtime: tokio::runtime::Runtime,
    /// 发往异步任务的命令
    pub cmd_tx: mpsc::Sender<AgentCommand>,
    /// 异步任务回 ECS 的事件接收端
    pub event_rx: Mutex<mpsc::Receiver<AgentEvent>>,
    /// 共享确认状态：ECS 收到决策后回填给等待的 async task
    pub shared_confirm: SharedConfirm,
    /// 项目根（会话存储路径派生用，见 ADR-0008）
    pub project_root: std::path::PathBuf,
    /// 重试配置（与 task 共享同一 Arc，运行时可刷新）
    pub retry_config: Arc<parking_lot::RwLock<RetryConfig>>,
    /// 当前对话的 cancel_token（StartLoop 时由 task 设置，对话结束清 None）。
    ///
    /// ECS Abort handler 直接调 `cancel()` 即时中断——无需经 steering_rx
    /// 让 run_agent_loop 轮询（run_agent_loop 可能 park 在 executor.execute
    /// 或 stream_llm_response，无法及时消费 Abort 命令）。修复 Confirming/ToolRunning
    /// 态下 Abort 无响应的 bug。用 Arc 共享给 task 与 ECS。
    pub current_cancel: Arc<parking_lot::Mutex<Option<tokio_util::sync::CancellationToken>>>,
    /// 已注册工具的 schema 列表（启动时从 ToolExecutor 一次性提取，
    /// 运行期工具集合不变；ECS 侧构造 ChatRequest 时注入为 `tools` 字段，
    /// 修复工具 schema 从未注入导致 LLM 无法发起工具调用的 bug）。
    pub tool_schemas: Arc<Vec<xgent_core::chat::ToolSchema>>,
    /// 有界执行的运行时可调上限（与 task 共享，测试与运行时调整用）。
    ///
    /// `None` = 不限。用 `parking_lot::RwLock` 而非裸字段，使 ECS 与异步
    /// task 都能读写而不必重建 bridge。
    pub loop_bounds: Arc<parking_lot::RwLock<LoopBounds>>,
}

/// agent loop 的有界执行上限。
#[derive(Debug, Clone, Copy)]
pub struct LoopBounds {
    /// 单轮工具调用轮次上限
    pub max_tool_rounds: Option<u32>,
    /// 单轮累计 token 上限
    pub max_tokens_per_turn: Option<u64>,
}

impl Default for LoopBounds {
    fn default() -> Self {
        Self {
            max_tool_rounds: Some(loop_limits::MAX_TOOL_ROUNDS),
            max_tokens_per_turn: Some(loop_limits::MAX_TOKENS_PER_TURN),
        }
    }
}

/// 命令（ECS → 异步任务）。
pub enum AgentCommand {
    /// 发起对话
    StartLoop {
        req: ChatRequest,
        /// @ 引用解析的编辑器查询（供 context 检索注入）
        editor_queries: Vec<xgent_core::EditorQuery>,
    },
    /// 中断当前对话
    Abort,
    /// 用户确认决策
    ConfirmDecision(ConfirmDecision),
    /// Steering：用户在 agent 执行中插话（注入到当前对话，MVP 不中断工具）
    Steering { text: String },
    /// Follow-up：agent 停止后注入后续消息继续对话
    FollowUp { text: String },
}

/// 有界执行的终止原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundedReason {
    /// 达到单轮工具调用轮次上限
    IterationLimit,
    /// 达到单轮累计 token 上限
    TokenLimit,
}

/// 异步任务 → ECS 的事件。
pub enum AgentEvent {
    /// 流式文本增量
    Delta(String),
    /// 工具调用开始
    ToolCall {
        /// provider 返回的工具调用 id（用于配对 tool result，对齐 OpenAI 协议）
        call_id: String,
        /// 工具名（UI 展示与执行查找用）
        tool_id: String,
        input: serde_json::Value,
    },
    /// 工具执行完成
    ToolResult {
        /// 对应的 provider tool_call id（与 ToolCall 的 call_id 配对）
        call_id: String,
        tool_id: String,
        output: String,
        /// 是否为逻辑失败（语义反转：true 表示失败）
        is_error: bool,
        /// 是否被策略/用户拒绝（UI 显示「已拒绝」态）
        denied: bool,
        side_effect: Option<SideEffect>,
    },
    /// 工具执行中的中间结果（流式进度）。
    ///
    /// 长时工具（`run_command` 的 stdout 增量、插件工具的 `push-update`）
    /// 经 `ToolUpdateCallback` 推送中间文本，UI 据此实时呈现进度而不必
    /// 等工具结束。与最终 [`AgentEvent::ToolResult`] 是同一 `call_id` 的
    /// 两个阶段：ToolProgress 可多次，ToolResult 恰好一次且为终态。
    ToolProgress {
        call_id: String,
        tool_id: String,
        output: String,
    },
    /// 有界执行终止（agent → UI）。
    ///
    /// 循环因命中上限而**正常停止**（非错误、非中断）：对话状态保持一致，
    /// 用户发下一条消息即可继续。UI 据此提示命中的界，让用户知道本轮为何停下。
    Bounded {
        reason: BoundedReason,
        /// 面向用户的中文说明，含具体上限与已用量
        detail: String,
    },
    /// 需要用户确认
    ConfirmRequest(ConfirmRequest),
    /// 对话完成（assistant turn 结束）。
    ///
    /// `usage` 携带本次 stream 的 token 用量（来自 provider），
    /// 供 ECS 写入 AssistantMessage.usage 并更新 UI token 统计。
    /// `model` 携带生成该回复的模型名（供持久化）。
    Done {
        usage: Option<xgent_core::chat::TokenUsage>,
        model: Option<String>,
    },
    /// 流式期间被 steering 中断：半截 assistant 文本需固化为被中断消息。
    ///
    /// `partial_text` 是中断前已流的 assistant 文本（可能为空）。
    /// ECS 据此把半截文本 finalize 为一条 assistant 消息（标记被中断），
    /// 清空 `current_assistant_text`，然后 UI 会显示新一轮流式。
    /// 避免半截文本与新回复拼接在一起（修复 steering 中断后文本混乱 bug）。
    SteeringInterrupted { partial_text: String },
    /// 对话出错
    Error {
        kind: xgent_core::chat::ErrorKind,
        message: String,
        /// 服务端 `Retry-After`（秒），透传给 UI 供展示"X 秒后自动重试"
        retry_after_secs: Option<u64>,
    },
    /// 即将重试。
    ///
    /// 因可重试错误（`Network`/`StreamParse`）触发自动重试前发射。
    /// UI 据此清空当前半截助手文本并展示"重试中(第 `attempt` 次)"。
    /// `last_error` 供 UI 展示上次失败原因。
    RetryAttempt {
        /// 即将进行的重试序号（1-based：首次重试 = 1）
        attempt: u32,
        /// 是否为无限重试模式（`max_retries == None`）
        infinite: bool,
        /// 上次失败的错误类型
        kind: xgent_core::chat::ErrorKind,
        /// 上次失败的错误消息
        last_error: String,
    },
    /// 对话已压缩（compaction 触发后发射）。
    ///
    /// UI 据此提示用户「前序对话已摘要」，可刷新上下文展示。
    /// `new_messages` 为压缩后的 agent 层消息（含 summary 前置 + kept），
    /// ECS 侧据此替换 `conv.messages`，保持 conv 与 req 同步
    /// （修复下次 StartLoop 从未压缩的 conv 重建导致压缩丢失的 bug）。
    /// 注意：system prompt 不在此处（system 在 build_request 时动态注入）。
    Compacted {
        /// 压缩前 token 估算
        tokens_before: u32,
        /// 压缩后保留消息 token 估算
        tokens_after: u32,
        /// 压缩后的 agent 层消息（不含 system，ECS 据此替换 conv.messages）
        new_messages: Vec<xgent_core::chat::AgentMessage>,
    },
}

/// 桥接配置参数（供 Plugin / xgent_app 注入）。
pub struct AgentBridgeConfig {
    pub provider: Arc<dyn ProviderClient>,
    pub executor: Arc<ToolExecutor>,
    pub context: Arc<dyn xgent_context::ContextProvider>,
    /// 项目根（工具执行上下文）
    pub project_root: std::path::PathBuf,
    /// 工具策略配置（approved / denied 列表）
    pub tool_policy: ToolPolicyConfig,
    /// 重试配置（从 ProviderConfig 派生，运行时可经
    /// [`AgentBridge::update_retry_config`] 刷新）
    pub retry_config: Arc<parking_lot::RwLock<RetryConfig>>,
    /// Compaction provider（None 则禁用压缩）。
    pub compaction: Option<Arc<dyn crate::compaction::CompactionProvider>>,
    /// 上下文窗口大小（token，从 ModelInfo 派生），compaction 触发依据。
    pub context_window: u32,
    /// Compaction 配置（阈值/reserve）。
    pub compaction_settings: crate::compaction::CompactionSettings,
    /// 单轮工具调用轮次上限（`None` = 不限，慎用）。
    ///
    /// LLM 若反复返回同一 tool_call（命令失败后重试同一条），无此闸会
    /// 无限执行工具并无限计费（R1-1）。
    pub max_tool_rounds: Option<u32>,
    /// 单轮累计 token 上限（`None` = 不限，慎用）。
    pub max_tokens_per_turn: Option<u64>,
}

/// agent loop 的默认有界值。
///
/// 刻意保守：正常一轮对话的工具调用远少于 20 次，累计 token 远少于
/// 100k；这两个上限只用于拦住失控循环，正常使用不会触达。
pub mod loop_limits {
    /// 单轮工具调用轮次上限
    pub const MAX_TOOL_ROUNDS: u32 = 20;
    /// 单轮累计 token 上限
    pub const MAX_TOKENS_PER_TURN: u64 = 200_000;
}

impl std::fmt::Debug for AgentBridgeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentBridgeConfig")
            .field("project_root", &self.project_root)
            .finish_non_exhaustive()
    }
}

impl AgentBridge {
    /// 构造桥接并 spawn 异步 agent loop task。
    pub fn new(cfg: AgentBridgeConfig) -> Self {
        let project_root = cfg.project_root.clone();
        // 启动时一次性提取工具 schema（运行期工具集合不变），
        // 供 ECS 侧构造 ChatRequest 时注入为 tools 字段。
        let tool_schemas = Arc::new(cfg.executor.schemas());
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("无法创建 tokio runtime");
        let (cmd_tx, cmd_rx) = mpsc::channel::<AgentCommand>(32);
        let (event_tx, event_rx) = mpsc::channel::<AgentEvent>(64);
        let shared_confirm = SharedConfirm::default();
        let retry_config = cfg.retry_config.clone();
        // 当前对话 cancel_token：task 与 ECS 共享，StartLoop 时 task 设置，
        // ECS Abort handler 直接 cancel（绕过 steering_rx 即时中断）。
        let current_cancel: Arc<parking_lot::Mutex<Option<tokio_util::sync::CancellationToken>>> =
            Arc::new(parking_lot::Mutex::new(None));
        // 有界执行上限：cfg 的初始值写入共享结构，ECS 与 task 共用
        let loop_bounds = Arc::new(parking_lot::RwLock::new(LoopBounds {
            max_tool_rounds: cfg.max_tool_rounds,
            max_tokens_per_turn: cfg.max_tokens_per_turn,
        }));

        let shared_for_task = shared_confirm.clone();
        let cancel_for_task = current_cancel.clone();
        let bounds_for_task = loop_bounds.clone();
        runtime.spawn(async move {
            agent_loop_task(
                cfg,
                cmd_rx,
                event_tx,
                shared_for_task,
                cancel_for_task,
                bounds_for_task,
            )
            .await;
        });

        Self {
            runtime,
            cmd_tx,
            event_rx: Mutex::new(event_rx),
            shared_confirm: shared_confirm.clone(),
            project_root,
            retry_config,
            current_cancel,
            tool_schemas,
            loop_bounds,
        }
    }

    /// 运行时刷新重试配置（下次 `StartLoop` 生效）。
    pub fn update_retry_config(&self, cfg: RetryConfig) {
        *self.retry_config.write() = cfg;
    }

    /// 调整单轮工具调用轮次上限（`None` = 不限）。
    pub fn set_max_tool_rounds(&self, v: Option<u32>) {
        self.loop_bounds.write().max_tool_rounds = v;
    }

    /// 调整单轮累计 token 上限（`None` = 不限）。
    pub fn set_max_tokens_per_turn(&self, v: Option<u64>) {
        self.loop_bounds.write().max_tokens_per_turn = v;
    }
}

///
/// 顶层循环：StartLoop 启动 run_agent_loop（双层循环）；
/// Abort 中断当前对话；Steering/FollowUp 由 run_agent_loop 内部消费。
async fn agent_loop_task(
    cfg: AgentBridgeConfig,
    mut cmd_rx: mpsc::Receiver<AgentCommand>,
    event_tx: mpsc::Sender<AgentEvent>,
    shared_confirm: SharedConfirm,
    current_cancel: Arc<parking_lot::Mutex<Option<tokio_util::sync::CancellationToken>>>,
    loop_bounds: Arc<parking_lot::RwLock<LoopBounds>>,
) {
    let tool_ctx = ToolCtx {
        project_root: cfg.project_root.clone(),
        tool_policy: cfg.tool_policy.clone(),
    };
    // 中断信号：每次对话创建独立 token（见下方 StartLoop 分支）。
    // Abort 时 cancel，传给 executor 与 stream_llm_response。
    // 注意：token 不能跨对话复用——CancellationToken::cancel() 是永久的，
    // 一旦 cancel 后该 token 永远处于已取消态，后续对话会立即触发 abort 分支。
    // current_cancel 与 ECS 共享：ECS Abort handler 直接 cancel（绕过
    // steering_rx），修复 run_agent_loop park 在 executor.execute 时无法
    // 及时消费 Abort 命令的 bug。

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            AgentCommand::StartLoop {
                mut req,
                mut editor_queries,
            } => {
                // run_agent_loop 可能在停等期收到迟到的新 StartLoop（ECS Done 置
                // Idle 后的新用户输入）并上返回——此处循环接手，立即开新对话。
                loop {
                    // 每次对话创建独立的 cancel_token——CancellationToken 是一次性的，
                    // cancel() 后无法撤销。若跨对话复用，首次 Abort 后该 token 永远
                    // 处于已取消态，后续对话的 stream_llm_response 会立即命中
                    // cancel_token.cancelled() 分支，导致 agent 永久无法流式。
                    let cancel_token = tokio_util::sync::CancellationToken::new();
                    // 暴露给 ECS：Abort handler 直接 cancel（即时中断 confirm 等待）
                    *current_cancel.lock() = Some(cancel_token.clone());
                    // 上下文检索：ECS 系统同步无法 await，故在此异步侧检索并刷新
                    // req 的首条 system 消息（修复上下文从未注入的 bug）。
                    // 用最近一条 user 消息作为 query；检索失败不阻塞对话（用空结果）。
                    if let Some(user_text) = crate::format::last_user_text(&req.messages) {
                        // 把 @ 引用查询转为 hints（File → 路径，Cursor → 描述）
                        let hints: Vec<String> = editor_queries
                            .iter()
                            .map(|q| match q {
                                xgent_core::EditorQuery::File { path } => {
                                    format!("@file:{}", path.display())
                                }
                                xgent_core::EditorQuery::Cursor => "@cursor".into(),
                            })
                            .collect();
                        let query = xgent_context::provider::ContextQuery {
                            user_message: user_text,
                            current_file: editor_queries.iter().find_map(|q| match q {
                                xgent_core::EditorQuery::File { path } => Some(path.clone()),
                                _ => None,
                            }),
                            hints,
                            max_tokens: 8_000,
                        };
                        let result = cfg.context.retrieve(&query).await;
                        crate::format::refresh_system_message(&mut req, &result);
                    }
                    let next = run_agent_loop(
                        &cfg.provider,
                        &cfg.executor,
                        &tool_ctx,
                        req,
                        &event_tx,
                        &shared_confirm,
                        &cancel_token,
                        &mut cmd_rx,
                        &cfg.retry_config,
                        cfg.compaction.as_ref(),
                        cfg.context_window,
                        &cfg.compaction_settings,
                        &loop_bounds,
                    )
                    .await;
                    // 对话结束：清除 current_cancel，ECS Abort 不再误触发
                    *current_cancel.lock() = None;
                    match next {
                        Some((new_req, new_queries)) => {
                            req = new_req;
                            editor_queries = new_queries;
                        }
                        None => break,
                    }
                }
            }
            AgentCommand::Abort => {
                // Abort 在无活跃对话时到达（run_agent_loop 已返回）：
                // cancel_token 已随 StartLoop 作用域销毁，无需 cancel。
                // 发 Done 通知 ECS（若对话刚结束 ECS 可能已在 Idle，幂等）。
                let _ = event_tx
                    .send(AgentEvent::Done {
                        usage: None,
                        model: None,
                    })
                    .await;
            }
            AgentCommand::ConfirmDecision(d) => {
                // 决策由 ECS 经 SharedConfirm 回填给等待的 task，此处无需处理
                let _ = d;
            }
            // Steering/FollowUp 在无对话运行时到达：忽略（MVP 不排队）
            AgentCommand::Steering { .. } | AgentCommand::FollowUp { .. } => {}
        }
    }
}

/// 流式调用结果。
/// 承载 tool_calls、usage（compaction 触发依据）、stop_reason，
/// 以及 steering 中断标记（流式期间用户插话，stream 被中断，
/// `pending_steering` 为待注入的 steering 文本）。
struct StreamOutcome {
    tool_calls: Vec<(String, String, serde_json::Value)>,
    usage: Option<xgent_core::chat::TokenUsage>,
    stop_reason: xgent_core::chat::StopReason,
    /// 流式期间被 steering 中断时，待注入的 steering 文本（非空表示中断发生）。
    pending_steering: Option<String>,
    /// steering 中断前已流的 assistant 文本（供 ECS 固化为被中断消息）。
    /// 非 None 且 `pending_steering` 非空时有意义。
    partial_text: Option<String>,
}

/// 驱动 agent 对话循环（双层）。
///
/// 外层：Follow-up 消息驱动（agent 准备停止时注入新消息继续）。
/// 内层：tool-call + steering（LLM → tool → continue，直到无 tool_calls）。
/// abort：CancellationToken，stream_llm_response 与 executor.execute 都监听。
/// steering：流式期间即时中断当前流（race abort），工具完成后注入到 req.messages；
///           停止边界（外层等 FollowUp 前）重新 try_recv，防止 steer 在 yield 点丢失。
/// compaction：每次 stream 拿到 usage 后检查 should_compact，触发则压缩 req.messages。
#[allow(clippy::too_many_arguments)]
async fn run_agent_loop(
    provider: &Arc<dyn ProviderClient>,
    executor: &Arc<ToolExecutor>,
    ctx: &ToolCtx,
    mut req: ChatRequest,
    event_tx: &mpsc::Sender<AgentEvent>,
    shared_confirm: &SharedConfirm,
    cancel_token: &tokio_util::sync::CancellationToken,
    steering_rx: &mut mpsc::Receiver<AgentCommand>,
    retry_config: &Arc<parking_lot::RwLock<RetryConfig>>,
    compaction: Option<&Arc<dyn crate::compaction::CompactionProvider>>,
    context_window: u32,
    compaction_settings: &crate::compaction::CompactionSettings,
    loop_bounds: &Arc<parking_lot::RwLock<LoopBounds>>,
) -> Option<(ChatRequest, Vec<xgent_core::EditorQuery>)> {
    use xgent_core::chat::{ContentBlock, Role};
    // 对话级快照：本对话期间配置固定，运行时刷新下次对话生效
    let retry_cfg = retry_config.read().clone();
    // 有界上限在每次 StartLoop 时读一次快照（parking_lot guard 不跨 await）
    let bounds = *loop_bounds.read();
    let max_tool_rounds = bounds.max_tool_rounds;
    let max_tokens_per_turn = bounds.max_tokens_per_turn;
    loop {
        let mut has_tool_calls = true;
        // 本轮最后一次 stream 的 usage 与 model，供 Done 事件携带
        let mut last_usage: Option<xgent_core::chat::TokenUsage> = None;
        let mut last_model: Option<String> = None;
        // 有界执行：tool-call 轮次计数与累计 token 计数（R1-1）
        let mut tool_rounds: u32 = 0;
        let mut tokens_used: u64 = 0;

        // 内层循环：tool-call + steering
        while has_tool_calls {
            // 轮询 steering（非阻塞，工具完成后注入）
            while let Ok(cmd) = steering_rx.try_recv() {
                match cmd {
                    AgentCommand::Steering { text } => {
                        // 注入 steering 消息到当前对话
                        req.messages
                            .push(xgent_core::chat::ChatMessage::text(Role::User, text));
                    }
                    AgentCommand::Abort => {
                        cancel_token.cancel();
                        let _ = event_tx
                            .send(AgentEvent::Done {
                                usage: None,
                                model: None,
                            })
                            .await;
                        return None;
                    }
                    AgentCommand::FollowUp { .. } | AgentCommand::StartLoop { .. } => {
                        // 不可达：ECS 仅在 Idle 态发送 FollowUp/StartLoop，
                        // 内层运行中 conv.status 必为 Thinking/ToolRunning。
                        // 若未来放宽该守卫，此处丢弃会让会话卡死，须改为上返回。
                    }
                    AgentCommand::ConfirmDecision(_) => {}
                }
            }
            // 流式调用 LLM（带自动重试：仅 Network/StreamParse 可重试）
            let outcome = match stream_with_retry(
                provider,
                &mut req,
                event_tx,
                cancel_token,
                &retry_cfg,
                steering_rx,
            )
            .await
            {
                Ok(o) => o,
                Err((kind, message, retry_after_secs)) => {
                    let _ = event_tx
                        .send(AgentEvent::Error {
                            kind,
                            message,
                            retry_after_secs,
                        })
                        .await;
                    return None;
                }
            };
            // 累计 token：每轮 stream 一结束即累加（含带 tool_calls 的轮次），
            // 否则只在末轮累加，token 闸形同虚设（R1-1）
            if let Some(u) = outcome.usage.as_ref() {
                tokens_used += (u.prompt as u64) + (u.completion as u64);
            }
            // 累计 token 超限：结束本轮并报告命中的界（与轮次闸同层，
            // 每轮检查一次——不能只在末轮检查）
            if let Some(limit) = max_tokens_per_turn
                && tokens_used >= limit
            {
                let _ = event_tx
                    .send(AgentEvent::Done {
                        usage: outcome.usage.clone(),
                        model: Some(req.model.clone()),
                    })
                    .await;
                let _ = event_tx
                    .send(AgentEvent::Bounded {
                        reason: BoundedReason::TokenLimit,
                        detail: format!(
                            "达到单轮 token 上限（{limit}，已用 {tokens_used}），已停止本轮执行"
                        ),
                    })
                    .await;
                return None;
            }

            // 流式期间被 steering 中断：发 SteeringInterrupted 让 ECS 固化半截文本，
            // 然后把被中断的 assistant 文本 + steering 文本注入 req.messages，
            // 本轮重新调用 LLM。这样 UI 不会把半截文本与新回复拼接（修复文本混乱），
            // 且 LLM 下一轮能看到自己被中断的半截话（修复 req/conv 不同步导致
            // LLM 丢失被中断 assistant 文本的连续性 bug）。
            if let Some(steer_text) = &outcome.pending_steering {
                let partial = outcome.partial_text.clone().unwrap_or_default();
                let _ = event_tx
                    .send(AgentEvent::SteeringInterrupted {
                        partial_text: partial.clone(),
                    })
                    .await;
                // 先回灌被中断的 assistant 文本（与 conv 侧 finalize_assistant 对称），
                // 让 LLM 下一轮看到自己刚说的半截话，避免重复/矛盾。
                if !partial.is_empty() {
                    req.messages.push(ChatMessage {
                        role: Role::Assistant,
                        content: vec![ContentBlock::Text { text: partial }],
                    });
                }
                req.messages.push(xgent_core::chat::ChatMessage::text(
                    Role::User,
                    steer_text.clone(),
                ));
                // 中断后不执行 tool_calls（可能不完整），直接 continue 重新流式
                continue;
            }

            // compaction 检查：每次 stream 完成后据 usage 判断
            if let (Some(compactor), Some(usage)) = (compaction, outcome.usage.as_ref())
                && let Some(new_messages) = maybe_compact(
                    compactor,
                    &req.messages,
                    usage.prompt,
                    context_window,
                    compaction_settings,
                    event_tx,
                    cancel_token,
                )
                .await
            {
                req.messages = new_messages;
            }

            if outcome.tool_calls.is_empty() {
                if outcome.stop_reason == xgent_core::chat::StopReason::Aborted {
                    // stream 被 abort 中断（cancel_token 或 steering Abort）：
                    // stream_llm_response 已发 Done，直接退出循环，避免走到外层
                    // select! 吞掉后续 StartLoop 命令（run_agent_loop 的外层等待
                    // 会消费并丢弃 StartLoop，导致下一次对话无法启动）。
                    return None;
                }
                // LLM 停止、无工具调用：本轮结束，记下 usage/model 供 Done 事件。
                //
                // 回灌本轮 assistant 文本到 req.messages（与 conv 侧 finalize_assistant
                // 对称）。修复前漏回灌：对话内 FollowUp/steering 时 req.messages 缺
                // 最后一轮 assistant 回复，LLM 看不到自己刚说的话，上下文断裂。
                // （tool 执行分支在第 635 行回灌 partial_text，此分支须同样处理。）
                if let Some(text) = outcome.partial_text.as_ref()
                    && !text.is_empty()
                {
                    req.messages.push(ChatMessage {
                        role: Role::Assistant,
                        content: vec![ContentBlock::Text { text: text.clone() }],
                    });
                }
                last_usage = outcome.usage.clone();
                last_model = Some(req.model.clone());
                has_tool_calls = false;
            } else if outcome.stop_reason == xgent_core::chat::StopReason::Length {
                // max_tokens 截断：tool_calls 可能参数不完整，不执行，
                // 为每个补占位 skipped result（对齐 omp createAbortedToolResult），
                // 让 LLM 在下一轮重新生成完整 tool_call。
                for (call_id, name, _args) in &outcome.tool_calls {
                    // 先发 ToolCall 事件，让 ECS 侧 conv.messages 记录 assistant tool_call，
                    // 与后续 ToolResult 配对（修复 conv.messages 孤儿 tool_result 导致
                    // 下次 StartLoop 时 OpenAI 协议配对断裂的 bug）
                    let _ = event_tx
                        .send(AgentEvent::ToolCall {
                            call_id: call_id.clone(),
                            tool_id: name.clone(),
                            input: serde_json::Value::Null,
                        })
                        .await;
                    let _ = event_tx
                        .send(AgentEvent::ToolResult {
                            call_id: call_id.clone(),
                            tool_id: name.clone(),
                            output: "工具调用因 max_tokens 截断而未执行，请重新发起完整调用。"
                                .into(),
                            is_error: true,
                            denied: false,
                            side_effect: None,
                        })
                        .await;
                    req.messages.push(ChatMessage {
                        role: Role::Assistant,
                        content: vec![ContentBlock::ToolCall {
                            id: call_id.clone(),
                            name: name.clone(),
                            args: serde_json::Value::Null,
                        }],
                    });
                    req.messages.push(ChatMessage {
                        role: Role::Tool,
                        content: vec![ContentBlock::ToolResult {
                            tool_call_id: call_id.clone(),
                            content: "工具调用因 max_tokens 截断而未执行".into(),
                            is_error: true,
                        }],
                    });
                }
                has_tool_calls = true;
            } else {
                // 执行工具调用，结果回灌为 ChatMessage 追加到 req.messages。
                //
                // OpenAI 协议要求：一条 assistant 消息可含多个 tool_calls，后跟多条
                // tool role 消息。因此先逐个执行（保持 UI 实时性：ToolCall/ToolResult
                // 事件逐个发），收集结果后**一次性**回灌一条含所有 ToolCall 块
                // （+首个文本块）的 assistant ChatMessage + N 条 tool result。
                // 修复之前每个 tool_call 都 push 一条独立 assistant 消息破坏协议的 bug。
                let assistant_text = outcome.partial_text.clone().unwrap_or_default();
                let mut executed: Vec<(String, String, serde_json::Value, String, bool)> =
                    Vec::with_capacity(outcome.tool_calls.len());
                let mut aborted = false;
                // 先发所有 ToolCall 事件，让 conv 侧累积到同一条 assistant 消息的
                // pending 批次（push_tool_call 累积，首个 ToolResult 时 flush），
                // UI 侧 ToolCall 卡片先全部出现。然后再逐个执行+发 ToolResult。
                for (call_id, name, args) in &outcome.tool_calls {
                    let _ = event_tx
                        .send(AgentEvent::ToolCall {
                            call_id: call_id.clone(),
                            tool_id: name.clone(),
                            input: args.clone(),
                        })
                        .await;
                }

                // 逐个执行工具，发 ToolResult 事件
                for (call_id, name, args) in &outcome.tool_calls {
                    let cb = BridgeConfirm {
                        event_tx: event_tx.clone(),
                        shared: shared_confirm.clone(),
                    };
                    // 流式更新回调：长时工具（run_command 的 stdout 增量、
                    // 插件工具的 push-update）经此把中间结果回灌 UI。
                    let update_tx = event_tx.clone();
                    let update_call_id = call_id.clone();
                    let update_tool_id = name.clone();
                    let update_cb: Arc<xgent_tools::ToolUpdateCallback> =
                        Arc::new(move |partial| {
                            let tx = update_tx.clone();
                            let cid = update_call_id.clone();
                            let tid = update_tool_id.clone();
                            let out = partial.output.clone();
                            tokio::spawn(async move {
                                let _ = tx
                                    .send(AgentEvent::ToolProgress {
                                        call_id: cid,
                                        tool_id: tid,
                                        output: out,
                                    })
                                    .await;
                            });
                        });
                    let result = executor
                        .execute(
                            name,
                            args.clone(),
                            ctx,
                            cancel_token.clone(),
                            &cb,
                            Some(update_cb),
                        )
                        .await;
                    match result {
                        Ok(r) => {
                            let _ = event_tx
                                .send(AgentEvent::ToolResult {
                                    call_id: call_id.clone(),
                                    tool_id: name.clone(),
                                    output: r.output.clone(),
                                    is_error: r.is_error,
                                    denied: r.denied,
                                    side_effect: r.side_effect,
                                })
                                .await;
                            executed.push((
                                call_id.clone(),
                                name.clone(),
                                args.clone(),
                                r.output,
                                r.is_error,
                            ));
                        }
                        Err(xgent_tools::ToolError::Aborted) => {
                            // 中断：当前 tool 透传为 ToolResult 逻辑失败，
                            // 已发 ToolCall 但未执行的 tool_calls 补占位 ToolResult，
                            // 保证所有 tool_call 都有配对的 tool result（对齐 Length 路径）。
                            let _ = event_tx
                                .send(AgentEvent::ToolResult {
                                    call_id: call_id.clone(),
                                    tool_id: name.clone(),
                                    output: "工具执行被中断".into(),
                                    is_error: true,
                                    denied: false,
                                    side_effect: None,
                                })
                                .await;
                            executed.push((
                                call_id.clone(),
                                name.clone(),
                                args.clone(),
                                "工具执行被中断".into(),
                                true,
                            ));
                            // 剩余已发 ToolCall 但未执行的补占位 ToolResult
                            let remaining = &outcome.tool_calls[executed.len()..];
                            for (cid, nm, _) in remaining {
                                let _ = event_tx
                                    .send(AgentEvent::ToolResult {
                                        call_id: cid.clone(),
                                        tool_id: nm.clone(),
                                        output: "工具执行因中断而未执行".into(),
                                        is_error: true,
                                        denied: false,
                                        side_effect: None,
                                    })
                                    .await;
                                executed.push((
                                    cid.clone(),
                                    nm.clone(),
                                    serde_json::Value::Null,
                                    "工具执行因中断而未执行".into(),
                                    true,
                                ));
                            }
                            aborted = true;
                            break;
                        }
                        Err(e) => {
                            let output = format!("工具异常: {e}");
                            let _ = event_tx
                                .send(AgentEvent::ToolResult {
                                    call_id: call_id.clone(),
                                    tool_id: name.clone(),
                                    output: output.clone(),
                                    is_error: true,
                                    denied: false,
                                    side_effect: None,
                                })
                                .await;
                            executed.push((
                                call_id.clone(),
                                name.clone(),
                                args.clone(),
                                output,
                                true,
                            ));
                        }
                    }
                }

                // 一次性回灌：一条 assistant（文本块 + 所有 ToolCall 块）
                // + N 条 tool result，符合 OpenAI 协议。
                let mut asst_content = Vec::with_capacity(executed.len() + 1);
                if !assistant_text.is_empty() {
                    asst_content.push(ContentBlock::Text {
                        text: assistant_text,
                    });
                }
                for (cid, nm, args, _, _) in &executed {
                    asst_content.push(ContentBlock::ToolCall {
                        id: cid.clone(),
                        name: nm.clone(),
                        args: args.clone(),
                    });
                }
                req.messages.push(ChatMessage {
                    role: Role::Assistant,
                    content: asst_content,
                });
                for (call_id, _, _, output, is_error) in executed {
                    req.messages.push(ChatMessage {
                        role: Role::Tool,
                        content: vec![ContentBlock::ToolResult {
                            tool_call_id: call_id,
                            content: output,
                            is_error,
                        }],
                    });
                }

                if aborted {
                    let _ = event_tx
                        .send(AgentEvent::Done {
                            usage: None,
                            model: None,
                        })
                        .await;
                    return None;
                }
                has_tool_calls = true;
            }

            // 每完成一轮工具调用即计数并检查上限：LLM 反复返回同一 tool_call
            // （如命令失败后重试同一条）时，无此闸会无限执行并无限计费（R1-1）。
            tool_rounds += 1;
            if let Some(limit) = max_tool_rounds
                && tool_rounds >= limit
            {
                let _ = event_tx
                    .send(AgentEvent::Done {
                        usage: last_usage,
                        model: last_model,
                    })
                    .await;
                let _ = event_tx
                    .send(AgentEvent::Bounded {
                        reason: BoundedReason::IterationLimit,
                        detail: format!("达到单轮工具调用上限（{limit} 次），已停止本轮执行"),
                    })
                    .await;
                return None;
            }
        }

        // 内层结束（无 tool_calls）→ 发 Done，携带本次 stream 的 usage 与 model
        // 供 ECS 写入 AssistantMessage.usage 并更新 UI token 统计（修复 usage 丢失 bug）。
        let _ = event_tx
            .send(AgentEvent::Done {
                usage: last_usage,
                model: last_model,
            })
            .await;

        // 停止边界：先 try_recv steering（防止 steer 在 yield 点丢失，对齐 omp）
        let mut late_steer: Option<String> = None;
        let mut pending_start: Option<(ChatRequest, Vec<xgent_core::EditorQuery>)> = None;
        while let Ok(cmd) = steering_rx.try_recv() {
            match cmd {
                AgentCommand::Steering { text } => {
                    late_steer = Some(text);
                    break;
                }
                AgentCommand::Abort => {
                    return None;
                }
                AgentCommand::StartLoop {
                    req,
                    editor_queries,
                } => {
                    // Done 已发、ECS 即将 Idle：新对话请求可能已在本函数阻塞
                    // 前入队。立即上返回顶层循环处理，不得丢弃（丢弃会让
                    // ECS 已置 Thinking 的会话永久卡死）。
                    pending_start = Some((req, editor_queries));
                    break;
                }
                _ => {}
            }
        }
        if let Some(start) = pending_start {
            return Some(start);
        }
        if let Some(text) = late_steer {
            req.messages
                .push(xgent_core::chat::ChatMessage::text(Role::User, text));
            continue;
        }

        // 外层：等待 FollowUp / Steering / Abort / 新 StartLoop
        loop {
            match steering_rx.recv().await {
                Some(AgentCommand::FollowUp { text }) => {
                    req.messages
                        .push(xgent_core::chat::ChatMessage::text(Role::User, text));
                    break; // 继续外层对话循环
                }
                Some(AgentCommand::Steering { text }) => {
                    // 外层等待期到达的 steering 也应继续对话
                    req.messages
                        .push(xgent_core::chat::ChatMessage::text(Role::User, text));
                    break;
                }
                Some(AgentCommand::StartLoop {
                    req: new_req,
                    editor_queries,
                }) => {
                    // 新对话请求在停等期到达：ECS 收到 Done 置 Idle 后，新用户
                    // 输入走 StartLoop。若在此丢弃，ECS 已置 Thinking 的会话
                    // 永久卡死——上返回顶层循环立即开新对话。
                    return Some((new_req, editor_queries));
                }
                Some(AgentCommand::Abort) | None => {
                    // 中断对话：发 Done 通知 ECS 切回 Idle（修复前直接 return 不发 Done，
                    // 导致 ECS 永久停留在 Aborting 态）
                    cancel_token.cancel();
                    let _ = event_tx
                        .send(AgentEvent::Done {
                            usage: None,
                            model: None,
                        })
                        .await;
                    return None;
                }
                Some(AgentCommand::ConfirmDecision(_)) => {
                    // 迟到的确认决策（确认窗口已关闭）：忽略，继续等待
                    continue;
                }
            }
        }
    }
}

/// 检查并执行 compaction。
///
/// 触发条件：`should_compact(max(provider_prompt, 本地估算), window, settings)`。
/// 触发后调 compactor.compact，apply_compaction 重建消息，发 `Compacted` 事件。
///
/// 返回 `Some(new_messages)` 表示已压缩；`None` 表示未触发或失败（失败不阻塞对话）。
async fn maybe_compact(
    compactor: &Arc<dyn crate::compaction::CompactionProvider>,
    messages: &[ChatMessage],
    provider_prompt_tokens: u32,
    context_window: u32,
    settings: &crate::compaction::CompactionSettings,
    event_tx: &mpsc::Sender<AgentEvent>,
    _cancel_token: &tokio_util::sync::CancellationToken,
) -> Option<Vec<ChatMessage>> {
    use xgent_core::chat::{AgentMessage, ContentBlock, Role};

    // 分离 system 消息：system prompt 不参与压缩（压缩会把它混入摘要后丢失，
    // 导致后续请求缺失系统指令与项目上下文）。压缩仅作用于对话部分，
    // 压缩后把 system 重新前置。
    let (system_msg, conversation_msgs) = match messages.split_first() {
        Some((first, rest)) if first.role == Role::System => (Some(first.clone()), rest),
        _ => (None, messages),
    };

    let agent_msgs: Vec<AgentMessage> = conversation_msgs
        .iter()
        .map(|m| match m.role {
            Role::User => AgentMessage::User(xgent_core::chat::UserMessage {
                content: m.content.clone(),
                timestamp: 0,
            }),
            Role::Assistant => AgentMessage::Assistant(xgent_core::chat::AssistantMessage {
                content: m.content.clone(),
                model: None,
                usage: None,
                timestamp: 0,
            }),
            Role::Tool => {
                // Tool 消息取首个 ToolResult block
                let (tool_call_id, content, is_error) = m
                    .content
                    .first()
                    .map(|b| match b {
                        ContentBlock::ToolResult {
                            tool_call_id,
                            content,
                            is_error,
                        } => (tool_call_id.clone(), content.clone(), *is_error),
                        _ => (String::new(), String::new(), false),
                    })
                    .unwrap_or_default();
                AgentMessage::ToolResult(xgent_core::chat::ToolResultMessage {
                    tool_call_id,
                    tool_name: String::new(),
                    content,
                    is_error,
                    timestamp: 0,
                })
            }
            // System 已在上方分离，此处不应再出现
            Role::System => AgentMessage::User(xgent_core::chat::UserMessage {
                content: m.content.clone(),
                timestamp: 0,
            }),
        })
        .collect();

    let local_estimate = crate::tokenizer::estimate_messages_tokens(&agent_msgs);
    let ctx_tokens =
        crate::compaction::compaction_context_tokens(provider_prompt_tokens, local_estimate);
    if !crate::compaction::should_compact(ctx_tokens, context_window, settings) {
        return None;
    }

    let model = ""; // compactor 内部用自身 model 字段，此处占位
    let result = match compactor.compact(&agent_msgs, model).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[compaction] 压缩失败，对话继续未压缩: {e}");
            return None;
        }
    };
    let tokens_after = crate::tokenizer::estimate_messages_tokens(&result.kept_messages);
    let new_agent_msgs = crate::compaction::apply_compaction(result);
    let mut new_messages = xgent_core::chat::convert_to_llm(&new_agent_msgs);
    // 把 system prompt 重新前置（压缩前已分离，此处恢复，保证后续请求仍有
    // 系统指令与项目上下文）
    if let Some(sys) = system_msg {
        new_messages.insert(0, sys);
    }
    let _ = event_tx
        .send(AgentEvent::Compacted {
            tokens_before: ctx_tokens,
            tokens_after,
            new_messages: new_agent_msgs,
        })
        .await;
    Some(new_messages)
}

/// 带自动重试的流式调用。
///
/// 包装 [`stream_llm_response`]：失败时按 [`RetryConfig`] 重试。
/// 可重试错误见 [`RetryConfig::is_retryable`]：网络、流解析、限流、服务端错误；
/// 其余立即返回。重试前发 [`AgentEvent::RetryAttempt`]，UI 据此清空半截文本。
///
/// 重试次数**永远有限**（`None` 退到缺省值，见
/// [`RetryConfig::effective_max_retries`]）。限流时优先采用服务端
/// `Retry-After` 而非客户端计算的退避（R1-7）。
///
/// 重试等待期间监听 `cancel_token`，用户 abort 立即中断重试循环。
async fn stream_with_retry(
    provider: &Arc<dyn ProviderClient>,
    req: &mut ChatRequest,
    event_tx: &mpsc::Sender<AgentEvent>,
    cancel_token: &tokio_util::sync::CancellationToken,
    retry_config: &RetryConfig,
    steering_rx: &mut mpsc::Receiver<AgentCommand>,
) -> Result<StreamOutcome, (xgent_core::chat::ErrorKind, String, Option<u64>)> {
    use xgent_core::chat::{ContentBlock, Role};
    let mut attempt: u32 = 0;
    loop {
        match stream_llm_response(provider, req, event_tx, cancel_token, steering_rx).await {
            Ok(o) => return Ok(o),
            Err((kind, message, partial_text, retry_after_secs)) => {
                // 不可重试错误立即失败
                if !RetryConfig::is_retryable(kind) {
                    return Err((kind, message, retry_after_secs));
                }
                // 可重试：检查是否还有重试机会
                // attempt 是已失败次数；下一次重试序号 = attempt + 1
                if !retry_config.can_retry(attempt + 1) {
                    return Err((kind, message, retry_after_secs));
                }
                attempt += 1;
                // 重试永远有限（None 退到缺省），infinite 仅用于 UI 提示
                let infinite = false;
                // 通知 UI 即将重试（清空半截文本 + 展示进度）
                let _ = event_tx
                    .send(AgentEvent::RetryAttempt {
                        attempt,
                        infinite,
                        kind,
                        last_error: message.clone(),
                    })
                    .await;
                // 重试前把半截 assistant 文本回灌到 req.messages（与 conv 侧
                // RetryAttempt 的 finalize_assistant 对称），让 LLM 下一轮看到
                // 自己重试前说的半截话，避免重复/矛盾。修复 req/conv 不同步。
                if let Some(text) = &partial_text
                    && !text.is_empty()
                {
                    req.messages.push(ChatMessage {
                        role: Role::Assistant,
                        content: vec![ContentBlock::Text { text: text.clone() }],
                    });
                }
                // 等待退避时长，期间可被 abort 中断。
                // 限流时优先采用服务端 Retry-After（R1-7）。
                let delay = retry_config.delay_for_with_retry_after(attempt, retry_after_secs);
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = cancel_token.cancelled() => {
                        // 中断重试：发 Done 后返回空（对齐 abort 语义）
                        let _ = event_tx
                            .send(AgentEvent::Done {
                                usage: None,
                                model: None,
                            })
                            .await;
                        return Ok(StreamOutcome {
                            tool_calls: Vec::new(),
                            usage: None,
                            stop_reason: xgent_core::chat::StopReason::Aborted,
                            pending_steering: None,
                            partial_text: None,
                        });
                    }
                }
                // 继续循环重试
            }
        }
    }
}

/// 流式调用 LLM，返回工具调用列表（id, name, args）与 usage。
///
/// 用 tokio::select! 监听流式事件、abort 信号、steering 插话。
/// steering 到达时**即时中断当前流**（race abort），返回已收集的 tool_calls
/// 与 `pending_steering`，由 run_agent_loop 注入后重新流式（对齐 omp
/// `streamAssistantResponse` 的 abort race 语义）。
async fn stream_llm_response(
    provider: &Arc<dyn ProviderClient>,
    req: &ChatRequest,
    event_tx: &mpsc::Sender<AgentEvent>,
    cancel_token: &tokio_util::sync::CancellationToken,
    steering_rx: &mut mpsc::Receiver<AgentCommand>,
) -> Result<
    StreamOutcome,
    (
        xgent_core::chat::ErrorKind,
        String,
        Option<String>,
        Option<u64>,
    ),
> {
    // 建流失败也可能带 Retry-After（如请求即被 429 限流），一并透传（R1-7）
    let (sid, mut stream) = match provider.chat(req.clone()).await {
        Ok((sid, rx)) => (sid, rx),
        Err((kind, msg, ra)) => return Err((kind, msg, None, ra)),
    };
    // abort 时把取消送到上游：顶层 select 在 cancel_token 触发时经 provider
    // 取消该流。经 daemon 的实现会真正取消 HTTP 请求，被放弃的流不再继续
    // 消费与计费（R1-2）。用 select 而非常驻 watcher task，避免每次流结束
    // 都泄漏一个等待 cancel 的 task。
    let sid_for_cancel = sid;

    // 累积 ToolCallStart 的 id/name（按 index），ToolCallEnd 时配对
    let mut pending_tool_calls: std::collections::HashMap<u32, (String, String)> =
        std::collections::HashMap::new();
    let mut collected: Vec<(String, String, serde_json::Value)> = Vec::new();
    // 累积已流式 assistant 文本，供 steering 中断时返回（ECS 固化为被中断消息）
    let mut partial_text = String::new();

    loop {
        tokio::select! {
            ev = stream.recv() => {
                match ev {
                    Some(ChatEvent::TextDelta { text }) => {
                        partial_text.push_str(&text);
                        let _ = event_tx.send(AgentEvent::Delta(text)).await;
                    }
                    Some(ChatEvent::ToolCallStart { index, id, name }) => {
                        pending_tool_calls.insert(index, (id, name));
                    }
                    Some(ChatEvent::ToolCallEnd { index, args }) => {
                        if let Some((id, name)) = pending_tool_calls.remove(&index) {
                            collected.push((id, name, args));
                        }
                    }
                    Some(ChatEvent::Done { reason, usage: u }) => {
                        return Ok(StreamOutcome {
                            tool_calls: collected,
                            usage: Some(u),
                            stop_reason: reason,
                            pending_steering: None,
                            // 正常完成时也携带已流式文本，供 run_agent_loop 在
                            // tool 执行分支回灌 assistant 消息时包含文本块
                            // （修复 LLM 同时返回文本+tool_calls 时文本丢失的 bug）
                            partial_text: Some(partial_text.clone()),
                        });
                    }
                    Some(ChatEvent::Error {
                        kind,
                        message,
                        retry_after_secs: ra,
                    }) => {
                        return Err((kind, message, Some(partial_text.clone()), ra));
                    }
                    Some(_) => {} // 忽略其他细粒度事件
                    None => {
                        // provider 流提前断开（未发 Done）：视为流解析错误，
                        // 返回可重试错误（StreamParse）让 stream_with_retry 重试。
                        // 不返回 Ok，避免上层误执行可能不完整的 tool_calls。
                        // 携带 partial_text 供 stream_with_retry 重试前回灌到 req
                        // （与 conv 侧 RetryAttempt finalize_assistant 对称）。
                        return Err((
                            xgent_core::chat::ErrorKind::StreamParse,
                            "stream ended without Done event".into(),
                            Some(partial_text.clone()),
                            None,
                        ));
                    }
                }
            }
            _ = cancel_token.cancelled() => {
                // abort：先把取消送到上游，再发 Done 后返回（停止循环）。
                // 经 daemon 的 provider 会真正取消 HTTP 请求——否则 daemon 侧
                // 推送 task 继续把流消费到自然结束，被放弃的请求照常产生 token（R1-2）。
                provider.cancel(sid_for_cancel).await;
                let _ = event_tx
                    .send(AgentEvent::Done {
                        usage: None,
                        model: None,
                    })
                    .await;
                return Ok(StreamOutcome {
                    tool_calls: Vec::new(),
                    usage: None,
                    stop_reason: xgent_core::chat::StopReason::Aborted,
                    pending_steering: None,
                    partial_text: None,
                });
            }
            cmd = steering_rx.recv() => {
                // 流式期间 steering：即时中断当前流
                match cmd {
                    Some(AgentCommand::Steering { text }) => {
                        // 不发 Done（对话继续，只是中断当前流）。
                        // steering 同样中止当前 LLM 请求，故也要取消上游（R1-2）。
                        provider.cancel(sid_for_cancel).await;
                        return Ok(StreamOutcome {
                            tool_calls: collected,
                            usage: None,
                            stop_reason: xgent_core::chat::StopReason::Aborted,
                            pending_steering: Some(text),
                            partial_text: Some(partial_text.clone()),
                        });
                    }
                    Some(AgentCommand::Abort) => {
                        cancel_token.cancel();
                        // 本分支与 `cancel_token.cancelled()` 分支竞争同一中断信号，
                        // 谁先被 select 选中是不确定的——两条路径都必须取消上游，
                        // 否则 Abort 命令路径下流仍被消费到自然结束（R1-2）。
                        provider.cancel(sid_for_cancel).await;
                        let _ = event_tx
                            .send(AgentEvent::Done {
                                usage: None,
                                model: None,
                            })
                            .await;
                        return Ok(StreamOutcome {
                            tool_calls: Vec::new(),
                            usage: None,
                            stop_reason: xgent_core::chat::StopReason::Aborted,
                            pending_steering: None,
                            partial_text: None,
                        });
                    }
                    _ => {} // 其他命令在流式期间到达：忽略，继续流
                }
            }
        }
    }
}

/// 桥接确认回调：发 ConfirmRequest 事件，并通过 SharedConfirm 等待决策。
struct BridgeConfirm {
    event_tx: mpsc::Sender<AgentEvent>,
    shared: SharedConfirm,
}

#[async_trait]
impl xgent_tools::ConfirmCallback for BridgeConfirm {
    async fn confirm(&self, req: ConfirmRequest) -> oneshot::Receiver<ConfirmDecision> {
        let (tx, rx) = oneshot::channel();
        // 存入共享状态，等 ECS 收到决策命令后回填
        self.shared.set_sender(tx).await;
        let _ = self.event_tx.send(AgentEvent::ConfirmRequest(req)).await;
        rx
    }
}

/// EditorCommandSink 桥接实现：持有 mpsc::Sender，emit 写 channel。
/// agent_poll_system 消费 receiver 端发 EditorCommandRequestMessage。
#[derive(Clone)]
pub struct ChannelEditorCommandSink {
    tx: Arc<std::sync::Mutex<mpsc::Sender<xgent_tools::EditorCommandRequest>>>,
}

impl ChannelEditorCommandSink {
    /// 构造，注入 channel 发送端。
    pub fn new(tx: mpsc::Sender<xgent_tools::EditorCommandRequest>) -> Self {
        Self {
            tx: Arc::new(std::sync::Mutex::new(tx)),
        }
    }
}

impl xgent_tools::EditorCommandSink for ChannelEditorCommandSink {
    fn emit(&self, req: xgent_tools::EditorCommandRequest) -> Result<(), String> {
        self.tx
            .lock()
            .unwrap()
            .try_send(req)
            .map_err(|e| e.to_string())
    }
}

/// 持有 editor 命令 channel 的接收端，由 agent_poll_system 每帧 drain。
#[derive(Resource)]
pub struct EditorCommandRx {
    /// tokio mpsc 接收端。
    ///
    /// ECS 系统线程**不得** `blocking_lock()`：tokio Mutex 被异步 task 持有时
    /// 会整帧阻塞主线程（Bevy 渲染与输入全部卡死）。改为 `try_lock()`——
    /// 抢不到就跳过本帧，事件留在 channel 里下帧再取，不丢数据。
    pub rx: Mutex<mpsc::Receiver<xgent_tools::EditorCommandRequest>>,
}
