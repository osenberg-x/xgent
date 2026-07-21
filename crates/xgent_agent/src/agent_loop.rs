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
    mut retry: MessageWriter<RetryMessage>,
    mut compacted: MessageWriter<CompactedMessage>,
    mut session_cleared: MessageWriter<SessionClearedMessage>,
    // ParamSet 合并所有 MessageReader，突破 SystemParam 数量上限
    mut readers: ParamSet<(
        MessageReader<UserInputMessage>,
        MessageReader<AbortMessage>,
        MessageReader<ConfirmDecisionMessage>,
        MessageReader<SteeringMessage>,
        MessageReader<FollowUpMessage>,
        MessageReader<NewSessionMessage>,
    )>,
    provider: Res<crate::provider_state::ProviderInfo>,
    context: Res<crate::provider_state::ContextState>,
) {
    // 1. 处理用户输入
    for ev in readers.p0().read() {
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
        conv.ensure_session_store(&bridge.project_root);
        conv.push_user(&ev.text);
        conv.status = ConversationStatus::Thinking;
        // 构造请求（上下文已在 context Resource 中预检索）
        // 注：tool_schemas 必须注入，否则 LLM 无法发起工具调用（修复关键 bug）。
        // 上下文检索由 bridge 异步侧在 StartLoop 时调 context.retrieve 完成，
        // 此处用空 ContextResult 占位，bridge 会用真实结果覆盖首条 system 消息。
        let req = build_request(
            &conv.messages,
            &context.result,
            &provider.id,
            &provider.model,
            Some(bridge.tool_schemas.as_ref().clone()),
        );
        let _ = bridge.cmd_tx.try_send(AgentCommand::StartLoop { req });
    }

    // 2. 处理中断
    for _ in readers.p1().read() {
        let _ = bridge.cmd_tx.try_send(AgentCommand::Abort);
        conv.status = ConversationStatus::Aborting;
    }

    // 2b. 处理 steering：用户在 agent 执行中插话（注入到当前对话，MVP 不中断工具）
    for ev in readers.p3().read() {
        // 注入 conv.messages（UI 展示 + 持久化）；bridge 侧另注入 req.messages（LLM 上下文）
        conv.push_user(&ev.text);
        let _ = bridge.cmd_tx.try_send(AgentCommand::Steering {
            text: ev.text.clone(),
        });
    }

    // 2c. 处理 follow-up：agent 停止后注入后续消息
    for ev in readers.p4().read() {
        if conv.status != ConversationStatus::Idle {
            // 仅 Idle 时接受 follow-up（非 Idle 用 steering）
            continue;
        }
        conv.push_user(&ev.text);
        // FollowUp 只传 text：bridge 内部 run_agent_loop 会把 text 追加到
        // 当前 req.messages。不在此重建 req（bridge 的 req 是 StartLoop 时的快照，
        // 重建会丢失对话中已积累的 tool_call/tool_result 消息）。
        // conv.messages 已 push，下次 StartLoop 时会用完整历史重建。
        let _ = bridge.cmd_tx.try_send(AgentCommand::FollowUp {
            text: ev.text.clone(),
        });
    }

    // 2d. 处理新建会话：仅 Idle/Error 接受（忙碌时忽略，避免丢失进行中的对话）
    for _ in readers.p5().read() {
        if conv.status != ConversationStatus::Idle && conv.status != ConversationStatus::Error {
            continue;
        }
        conv.reset();
        // 通知 UI 清空消息列表
        session_cleared.write(SessionClearedMessage);
    }

    // 3. 处理确认决策：经 SharedConfirm 回填给等待的 async task
    for ev in readers.p2().read() {
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
                    &mut retry,
                    &mut compacted,
                );
            }
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                // 异步任务退出，视为完成
                done.write(DoneMessage {
                    usage: None,
                    model: None,
                });
                break;
            }
        }
    }
}

/// 处理单个 AgentEvent，更新状态并发 Bevy Message。
fn handle_agent_event(
    ev: AgentEvent,
    conv: &mut Conversation,
    delta: &mut MessageWriter<DeltaMessage>,
    tool_call: &mut MessageWriter<ToolCallMessage>,
    tool_result: &mut MessageWriter<ToolResultMessage>,
    confirm: &mut MessageWriter<ConfirmRequestMessage>,
    done: &mut MessageWriter<DoneMessage>,
    error: &mut MessageWriter<ErrorMessage>,
    retry: &mut MessageWriter<RetryMessage>,
    compacted: &mut MessageWriter<CompactedMessage>,
) {
    match ev {
        AgentEvent::Delta(text) => {
            conv.status = ConversationStatus::Streaming;
            conv.current_assistant_text.push_str(&text);
            delta.write(DeltaMessage { text });
        }
        AgentEvent::ToolCall {
            call_id,
            tool_id,
            input,
        } => {
            // 记录 assistant tool_call 到 conv.messages，与后续 tool result 配对
            // （修复多轮工具调用后 conv 缺 tool_call 导致 LLM 请求被拒的 bug）
            conv.push_tool_call(&call_id, &tool_id, &input);
            conv.status = ConversationStatus::ToolRunning;
            tool_call.write(ToolCallMessage {
                tool_call_id: call_id,
                tool_id,
                input,
            });
        }
        AgentEvent::ToolResult {
            call_id,
            tool_id,
            output,
            is_error,
            denied,
            ..
        } => {
            // 记录 tool result，与 push_tool_call 的 call_id 配对
            // （OpenAI 要求 tool result 的 tool_call_id 与前述 tool_call 的 id 一致）
            conv.push_tool_result(&call_id, &tool_id, &output, is_error);
            tool_result.write(ToolResultMessage {
                tool_call_id: call_id,
                tool_id,
                output,
                is_error,
                denied,
            });
        }
        AgentEvent::ConfirmRequest(req) => {
            conv.status = ConversationStatus::Confirming;
            confirm.write(ConfirmRequestMessage(req));
        }
        AgentEvent::SteeringInterrupted { partial_text } => {
            // 流式被 steering 中断：把半截文本固化为被中断的 assistant 消息，
            // 清空 current_assistant_text，避免与新一轮流式拼接。
            // 复用 DoneMessage 让 UI 把半截文本固化为历史气泡并清空当前节点
            // （usage 为 None，token 统计无害）。
            if !partial_text.is_empty() {
                conv.current_assistant_text = partial_text;
                conv.finalize_assistant(None, None);
                conv.persist_last_assistant();
            } else {
                conv.current_assistant_text.clear();
            }
            // status 保持 Streaming/Thinking 语义：对话未结束，steering 后继续流式
            conv.status = ConversationStatus::Thinking;
            done.write(DoneMessage {
                usage: None,
                model: None,
            });
        }
        AgentEvent::Done { usage, model } => {
            // 先固化助手消息（写入 usage/model），再清空 current_assistant_text，
            // 然后发 DoneMessage 供 UI 用真实 usage 累加 token（修复读取空文本的 bug）。
            conv.finalize_assistant(usage.clone(), model.clone());
            conv.persist_last_assistant();
            conv.status = ConversationStatus::Idle;
            done.write(DoneMessage { usage, model });
        }
        AgentEvent::RetryAttempt {
            attempt,
            infinite,
            kind,
            last_error,
        } => {
            // 清空半截助手文本（重试后重新流式输出，避免拼接）
            conv.current_assistant_text.clear();
            // 状态保持 Streaming（重试中），不切到 Error
            conv.status = ConversationStatus::Streaming;
            retry.write(RetryMessage {
                attempt,
                infinite,
                kind,
                last_error,
            });
        }
        AgentEvent::Error { kind, message } => {
            // 错误不进 conv.messages（不发给 LLM），但持久化为独立 entry 供审计
            conv.persist_error(kind, &message);
            conv.status = ConversationStatus::Error;
            error.write(ErrorMessage { kind, message });
        }
        AgentEvent::Compacted {
            tokens_before,
            tokens_after,
        } => {
            // 持久化 compaction 记录（不重写历史，append CompactionEntry）
            conv.persist_compaction(
                &format!("[compacted: {tokens_before}→{tokens_after} tokens]"),
                "kept",
                tokens_before,
            );
            compacted.write(CompactedMessage {
                tokens_before,
                tokens_after,
            });
        }
    }
}
