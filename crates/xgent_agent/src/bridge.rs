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
    ) -> Result<(StreamId, mpsc::Receiver<ChatEvent>), (xgent_core::chat::ErrorKind, String)>;
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
}

/// 命令（ECS → 异步任务）。
pub enum AgentCommand {
    /// 发起对话
    StartLoop { req: ChatRequest },
    /// 中断当前对话
    Abort,
    /// 用户确认决策
    ConfirmDecision(ConfirmDecision),
    /// Steering：用户在 agent 执行中插话（注入到当前对话，MVP 不中断工具）
    Steering { text: String },
    /// Follow-up：agent 停止后注入后续消息继续对话
    FollowUp { text: String },
}

/// 异步任务 → ECS 的事件。
pub enum AgentEvent {
    /// 流式文本增量
    Delta(String),
    /// 工具调用开始
    ToolCall {
        tool_id: String,
        input: serde_json::Value,
    },
    /// 工具执行完成
    ToolResult {
        tool_id: String,
        output: String,
        /// 是否为逻辑失败（语义反转：true 表示失败）
        is_error: bool,
        side_effect: Option<SideEffect>,
    },
    /// 需要用户确认
    ConfirmRequest(ConfirmRequest),
    /// 对话完成
    Done,
    /// 对话出错
    Error {
        kind: xgent_core::chat::ErrorKind,
        message: String,
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
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("无法创建 tokio runtime");
        let (cmd_tx, cmd_rx) = mpsc::channel::<AgentCommand>(32);
        let (event_tx, event_rx) = mpsc::channel::<AgentEvent>(64);
        let shared_confirm = SharedConfirm::default();

        let shared_for_task = shared_confirm.clone();
        runtime.spawn(async move {
            agent_loop_task(cfg, cmd_rx, event_tx, shared_for_task).await;
        });

        Self {
            runtime,
            cmd_tx,
            event_rx: Mutex::new(event_rx),
            shared_confirm: shared_confirm.clone(),
            project_root,
        }
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
) {
    let tool_ctx = ToolCtx {
        project_root: cfg.project_root.clone(),
        tool_policy: cfg.tool_policy.clone(),
    };
    // 中断信号：每次对话创建独立 token，Abort 时 cancel，传给 executor
    let cancel_token = tokio_util::sync::CancellationToken::new();

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            AgentCommand::StartLoop { req } => {
                run_agent_loop(
                    &cfg.provider,
                    &cfg.executor,
                    &tool_ctx,
                    req,
                    &event_tx,
                    &shared_confirm,
                    &cancel_token,
                    &mut cmd_rx,
                )
                .await;
            }
            AgentCommand::Abort => {
                // 中断当前对话：cancel token 触发 stream/工具中断
                cancel_token.cancel();
                let _ = event_tx.send(AgentEvent::Done).await;
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

/// 驱动 agent 对话循环（双层）。
///
/// 外层：Follow-up 消息驱动（agent 准备停止时注入新消息继续）。
/// 内层：tool-call + steering（LLM → tool → continue，直到无 tool_calls）。
/// abort：CancellationToken，stream_llm_response 与 executor.execute 都监听。
/// steering：MVP 不中断工具，在工具完成后 try_recv 注入到 req.messages。
async fn run_agent_loop(
    provider: &Arc<dyn ProviderClient>,
    executor: &Arc<ToolExecutor>,
    ctx: &ToolCtx,
    mut req: ChatRequest,
    event_tx: &mpsc::Sender<AgentEvent>,
    shared_confirm: &SharedConfirm,
    cancel_token: &tokio_util::sync::CancellationToken,
    steering_rx: &mut mpsc::Receiver<AgentCommand>,
) {
    use xgent_core::chat::{ContentBlock, Role};

    // 外层循环：follow-up 驱动
    loop {
        let mut has_tool_calls = true;

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
                        let _ = event_tx.send(AgentEvent::Done).await;
                        return;
                    }
                    AgentCommand::FollowUp { .. } | AgentCommand::StartLoop { .. } => {
                        // FollowUp 在外层处理，StartLoop 不应在运行中到达：暂存回队列？MVP 忽略
                    }
                    AgentCommand::ConfirmDecision(_) => {}
                }
            }

            // 流式调用 LLM，返回工具调用列表
            let tool_calls = match stream_llm_response(provider, &req, event_tx, cancel_token).await
            {
                Ok(tc) => tc,
                Err((kind, message)) => {
                    let _ = event_tx.send(AgentEvent::Error { kind, message }).await;
                    return;
                }
            };

            if tool_calls.is_empty() {
                has_tool_calls = false;
            } else {
                // 执行工具调用，结果回灌为 ChatMessage 追加到 req.messages
                for (call_id, name, args) in tool_calls {
                    let _ = event_tx
                        .send(AgentEvent::ToolCall {
                            tool_id: name.clone(),
                            input: args.clone(),
                        })
                        .await;

                    let cb = BridgeConfirm {
                        event_tx: event_tx.clone(),
                        shared: shared_confirm.clone(),
                    };
                    let result = executor
                        .execute(&name, args, ctx, cancel_token.clone(), &cb)
                        .await;
                    let (output, is_error, side_effect) = match result {
                        Ok(r) => (r.output, r.is_error, r.side_effect),
                        Err(xgent_tools::ToolError::Aborted) => {
                            // 中断：透传为 ToolResult 逻辑失败 + 结束本轮
                            let _ = event_tx
                                .send(AgentEvent::ToolResult {
                                    tool_id: name.clone(),
                                    output: "工具执行被中断".into(),
                                    is_error: true,
                                    side_effect: None,
                                })
                                .await;
                            let _ = event_tx.send(AgentEvent::Done).await;
                            return;
                        }
                        Err(e) => (format!("工具异常: {e}"), true, None),
                    };
                    let _ = event_tx
                        .send(AgentEvent::ToolResult {
                            tool_id: name.clone(),
                            output: output.clone(),
                            is_error,
                            side_effect,
                        })
                        .await;

                    // 工具结果回灌为 ChatMessage：
                    // assistant tool_call 消息（带 ContentBlock::ToolCall）+ tool 结果消息
                    req.messages.push(ChatMessage {
                        role: Role::Assistant,
                        content: vec![ContentBlock::ToolCall {
                            id: call_id.clone(),
                            name: name.clone(),
                            args: serde_json::Value::Null, // args 已消费，回灌用 Null
                        }],
                    });
                    req.messages.push(ChatMessage {
                        role: Role::Tool,
                        content: vec![ContentBlock::ToolResult {
                            tool_call_id: call_id,
                            content: output,
                            is_error,
                        }],
                    });
                }
                has_tool_calls = true;
            }
        }

        // 内层结束（无 tool_calls）→ 发 Done，等待外层 FollowUp
        let _ = event_tx.send(AgentEvent::Done).await;

        // 外层：等待 FollowUp 或 Abort
        match steering_rx.recv().await {
            Some(AgentCommand::FollowUp { text }) => {
                req.messages
                    .push(xgent_core::chat::ChatMessage::text(Role::User, text));
                continue; // 继续外层循环
            }
            Some(AgentCommand::Abort) | None => {
                return;
            }
            // Steering/StartLoop/ConfirmDecision 在外层等待时到达：MVP 忽略，退出
            _ => return,
        }
    }
}

/// 流式调用 LLM，返回工具调用列表（id, name, args）。
///
/// 用 tokio::select! 监听流式事件与 abort 信号。Done 时返回收集的 tool_calls。
async fn stream_llm_response(
    provider: &Arc<dyn ProviderClient>,
    req: &ChatRequest,
    event_tx: &mpsc::Sender<AgentEvent>,
    cancel_token: &tokio_util::sync::CancellationToken,
) -> Result<Vec<(String, String, serde_json::Value)>, (xgent_core::chat::ErrorKind, String)> {
    let (_sid, mut stream) = match provider.chat(req.clone()).await {
        Ok(s) => s,
        Err((kind, msg)) => return Err((kind, msg)),
    };

    // 累积 ToolCallStart 的 id/name（按 index），ToolCallEnd 时配对
    let mut pending_tool_calls: std::collections::HashMap<u32, (String, String)> =
        std::collections::HashMap::new();
    let mut collected: Vec<(String, String, serde_json::Value)> = Vec::new();

    loop {
        tokio::select! {
            ev = stream.recv() => {
                match ev {
                    Some(ChatEvent::TextDelta { text }) => {
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
                    Some(ChatEvent::Done { .. }) => {
                        return Ok(collected);
                    }
                    Some(ChatEvent::Error { kind, message }) => {
                        return Err((kind, message));
                    }
                    Some(_) => {} // 忽略其他细粒度事件
                    None => return Ok(collected),
                }
            }
            _ = cancel_token.cancelled() => {
                // abort：发 Done 后返回空（停止循环）
                let _ = event_tx.send(AgentEvent::Done).await;
                return Ok(Vec::new());
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
