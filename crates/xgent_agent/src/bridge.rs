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
use xgent_core::chat::{ChatEvent, ChatRequest};
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
    async fn chat(&self, req: ChatRequest)
    -> Result<(StreamId, mpsc::Receiver<ChatEvent>), (xgent_core::chat::ErrorKind, String)>;
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
}

/// 命令（ECS → 异步任务）。
pub enum AgentCommand {
    /// 发起对话
    StartLoop { req: ChatRequest },
    /// 中断
    Abort,
    /// 用户确认决策
    ConfirmDecision(ConfirmDecision),
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
        success: bool,
        side_effect: Option<SideEffect>,
    },
    /// 需要用户确认
    ConfirmRequest(ConfirmRequest),
    /// 对话完成
    Done,
    /// 对话出错
    Error { kind: xgent_core::chat::ErrorKind, message: String },
}

/// 桥接配置参数（供 Plugin / xgent_app 注入）。
pub struct AgentBridgeConfig {
    pub provider: Arc<dyn ProviderClient>,
    pub executor: Arc<ToolExecutor>,
    pub context: Arc<dyn xgent_context::ContextProvider>,
    /// 项目根（工具执行上下文）
    pub project_root: std::path::PathBuf,
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
        }
    }
}

/// 异步 agent loop 任务。
async fn agent_loop_task(
    cfg: AgentBridgeConfig,
    mut cmd_rx: mpsc::Receiver<AgentCommand>,
    event_tx: mpsc::Sender<AgentEvent>,
    shared_confirm: SharedConfirm,
) {
    let tool_ctx = ToolCtx {
        project_root: cfg.project_root.clone(),
        tool_policy: ToolPolicyConfig::default(),
    };

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            AgentCommand::StartLoop { req } => {
                run_conversation(
                    &cfg.provider,
                    &cfg.executor,
                    &tool_ctx,
                    req,
                    &event_tx,
                    &shared_confirm,
                )
                .await;
            }
            AgentCommand::Abort => {
                let _ = event_tx.send(AgentEvent::Done).await;
            }
            AgentCommand::ConfirmDecision(d) => {
                // 决策由 ECS 经 SharedConfirm 回填给等待的 task，此处无需处理
                let _ = d;
            }
        }
    }
}

/// 驱动一次对话：调 provider 流式，遇工具调用则执行（含确认）。
async fn run_conversation(
    provider: &Arc<dyn ProviderClient>,
    executor: &Arc<ToolExecutor>,
    ctx: &ToolCtx,
    req: ChatRequest,
    event_tx: &mpsc::Sender<AgentEvent>,
    shared_confirm: &SharedConfirm,
) {
    let (_sid, mut stream) = match provider.chat(req).await {
        Ok(s) => s,
        Err((kind, msg)) => {
            let _ = event_tx.send(AgentEvent::Error { kind, message: msg }).await;
            return;
        }
    };
    while let Some(ev) = stream.recv().await {
        match ev {
            ChatEvent::Delta { text } => {
                let _ = event_tx.send(AgentEvent::Delta(text)).await;
            }
            ChatEvent::ToolCall { id: _, name, args } => {
                let _ = event_tx
                    .send(AgentEvent::ToolCall {
                        tool_id: name.clone(),
                        input: args.clone(),
                    })
                    .await;
                // 确认回调：发请求 + 等待 SharedConfirm 回填的 oneshot
                let cb = BridgeConfirm {
                    event_tx: event_tx.clone(),
                    shared: shared_confirm.clone(),
                };
                let result = executor.execute(&name, args, ctx, &cb).await;
                let _ = event_tx
                    .send(AgentEvent::ToolResult {
                        tool_id: name,
                        output: result.output,
                        success: result.success,
                        side_effect: result.side_effect,
                    })
                    .await;
            }
            ChatEvent::Done { .. } => {
                let _ = event_tx.send(AgentEvent::Done).await;
                break;
            }
            ChatEvent::Error { kind, message } => {
                let _ = event_tx.send(AgentEvent::Error { kind, message }).await;
                break;
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
