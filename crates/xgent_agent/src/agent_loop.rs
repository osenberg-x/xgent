//! agent loop 轮询系统：每帧非阻塞从桥接 channel 读取事件转 Bevy Event，
//! 处理用户输入/中断/确认决策。

use bevy::prelude::*;
use tokio::sync::mpsc;

use crate::bridge::{AgentBridge, AgentCommand, AgentEvent};
use crate::conversation::{Conversation, ConversationStatus};
use crate::events::*;
use crate::format::build_request;

/// 每帧轮询桥接 channel，分发事件到 ECS；处理用户输入/中断/确认。
#[allow(clippy::too_many_arguments)]
pub fn agent_poll_system(
    bridge: Res<AgentBridge>,
    mut conv: ResMut<Conversation>,
    mut delta: MessageWriter<DeltaMessage>,
    mut tool_call: MessageWriter<ToolCallMessage>,
    mut tool_result: MessageWriter<ToolResultMessage>,
    mut confirm: MessageWriter<ConfirmRequestMessage>,
    mut done: MessageWriter<DoneMessage>,
    mut error: MessageWriter<ErrorMessage>,
    mut user_input: MessageReader<UserInputMessage>,
    mut abort: MessageReader<AbortMessage>,
    mut decision: MessageReader<ConfirmDecisionMessage>,
    provider: Res<crate::provider_state::ProviderInfo>,
    context: Res<crate::provider_state::ContextState>,
) {
    // 1. 处理用户输入
    for ev in user_input.read() {
        if conv.status != ConversationStatus::Idle && conv.status != ConversationStatus::Error {
            // 忙碌时忽略（UI 应已禁用输入）
            continue;
        }
        // 从 Error 恢复：清空残留的错误文本（错误不进历史）
        if conv.status == ConversationStatus::Error {
            conv.current_assistant_text.clear();
        }
        // 闸门：provider 未就绪时不构造请求，发引导错误
        if !provider.ready {
            error.write(ErrorMessage {
                kind: xgent_core::chat::ErrorKind::NotConfigured,
                message: "未配置 Provider，请先在设置中配置 API 信息".to_string(),
            });
            conv.status = ConversationStatus::Error;
            continue;
        }
        conv.push_user(&ev.text);
        conv.status = ConversationStatus::Thinking;
        // 构造请求（上下文已在 context Resource 中预检索）
        let req = build_request(
            &conv.messages,
            &context.result,
            &provider.id,
            &provider.model,
            None, // tools schema 后续注入
        );
        let _ = bridge.cmd_tx.try_send(AgentCommand::StartLoop { req });
    }

    // 2. 处理中断
    for _ in abort.read() {
        let _ = bridge.cmd_tx.try_send(AgentCommand::Abort);
        conv.status = ConversationStatus::Aborting;
    }

    // 3. 处理确认决策：经 SharedConfirm 回填给等待的 async task
    for ev in decision.read() {
        let handle = bridge.runtime.handle().clone();
        let shared = bridge.shared_confirm.clone();
        let d = ev.decision;
        handle.spawn(async move {
            if let Some(tx) = shared.take_sender().await {
                let _ = tx.send(d);
            }
        });
        conv.status = ConversationStatus::Streaming;
    }

    // 4. 非阻塞轮询事件 channel
    let mut event_rx = bridge.event_rx.blocking_lock();
    // 限制每帧处理数量，避免单帧过长
    let mut processed = 0;
    while processed < 64 {
        match event_rx.try_recv() {
            Ok(ev) => {
                processed += 1;
                handle_agent_event(
                    ev,
                    &mut conv,
                    &mut delta,
                    &mut tool_call,
                    &mut tool_result,
                    &mut confirm,
                    &mut done,
                    &mut error,
                );
            }
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                // 异步任务退出，视为完成
                done.write(DoneMessage);
                break;
            }
        }
    }
}

/// 处理单个 AgentEvent，更新状态并发 Bevy Message。
#[allow(clippy::too_many_arguments)]
fn handle_agent_event(
    ev: AgentEvent,
    conv: &mut Conversation,
    delta: &mut MessageWriter<DeltaMessage>,
    tool_call: &mut MessageWriter<ToolCallMessage>,
    tool_result: &mut MessageWriter<ToolResultMessage>,
    confirm: &mut MessageWriter<ConfirmRequestMessage>,
    done: &mut MessageWriter<DoneMessage>,
    error: &mut MessageWriter<ErrorMessage>,
) {
    match ev {
        AgentEvent::Delta(text) => {
            conv.status = ConversationStatus::Streaming;
            conv.current_assistant_text.push_str(&text);
            delta.write(DeltaMessage { text });
        }
        AgentEvent::ToolCall { tool_id, input } => {
            conv.status = ConversationStatus::ToolRunning;
            tool_call.write(ToolCallMessage { tool_id, input });
        }
        AgentEvent::ToolResult {
            tool_id,
            output,
            success,
            ..
        } => {
            tool_result.write(ToolResultMessage {
                tool_id,
                output,
                success,
            });
        }
        AgentEvent::ConfirmRequest(req) => {
            conv.status = ConversationStatus::Confirming;
            confirm.write(ConfirmRequestMessage(req));
        }
        AgentEvent::Done => {
            conv.finalize_assistant();
            conv.status = ConversationStatus::Idle;
            done.write(DoneMessage);
        }
        AgentEvent::Error { kind, message } => {
            conv.status = ConversationStatus::Error;
            error.write(ErrorMessage { kind, message });
        }
    }
}
