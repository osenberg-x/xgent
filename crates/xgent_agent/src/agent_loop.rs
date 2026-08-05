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
    mut writers: ParamSet<(
        MessageWriter<DeltaMessage>,
        MessageWriter<ToolCallMessage>,
        MessageWriter<ToolResultMessage>,
        MessageWriter<ConfirmRequestMessage>,
        MessageWriter<DoneMessage>,
        MessageWriter<ErrorMessage>,
        MessageWriter<RetryMessage>,
        MessageWriter<CompactedMessage>,
    )>,
    mut session_cleared: MessageWriter<SessionClearedMessage>,
    mut session_list: MessageWriter<SessionListMessage>,
    mut session_restored: MessageWriter<SessionRestoredMessage>,
    // ParamSet 合并所有 MessageReader，突破 SystemParam 数量上限
    mut readers: ParamSet<(
        MessageReader<UserInputMessage>,
        MessageReader<AbortMessage>,
        MessageReader<ConfirmDecisionMessage>,
        MessageReader<SteeringMessage>,
        MessageReader<FollowUpMessage>,
        MessageReader<NewSessionMessage>,
        MessageReader<ListSessionsMessage>,
        MessageReader<RestoreSessionMessage>,
    )>,
    provider: Res<crate::provider_state::ProviderInfo>,
    context: Res<crate::provider_state::ContextState>,
    // editor 命令 channel（可选：测试环境未注入时为 None）
    editor_cmd_rx: Option<ResMut<crate::bridge::EditorCommandRx>>,
    mut editor_cmd: MessageWriter<EditorCommandRequestMessage>,
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
            writers.p5().write(ErrorMessage {
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
        let _ = bridge.cmd_tx.try_send(AgentCommand::StartLoop {
            req,
            editor_queries: ev.editor_queries.clone(),
        });
    }

    // 2. 处理中断
    for _ in readers.p1().read() {
        // 直接 cancel 当前对话的 token——即时中断 stream/confirm/重试等待，
        // 无需等 run_agent_loop 轮询 steering_rx 消费 Abort 命令
        // （run_agent_loop 可能 park 在 executor.execute，无法及时轮询）。
        if let Some(token) = bridge.current_cancel.lock().as_ref() {
            token.cancel();
        }
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

    // 2e. 处理列出历史会话：扫描 sessions 目录，返回摘要列表
    for _ in readers.p6().read() {
        let sessions = crate::session_store::list_sessions();
        session_list.write(SessionListMessage { sessions });
    }

    // 2f. 处理恢复会话：仅 Idle/Error 接受（忙碌时忽略）
    for ev in readers.p7().read() {
        if conv.status != ConversationStatus::Idle && conv.status != ConversationStatus::Error {
            continue;
        }
        if let Some(messages) = crate::session_store::restore_session(&ev.session_id) {
            conv.restore(&ev.session_id, messages.clone());
            session_restored.write(SessionRestoredMessage { messages });
        }
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
        conv.status = ConversationStatus::ToolRunning;
    }

    // 3b. drain editor 命令 channel：EditorTool → EditorCommandRequestMessage
    if let Some(editor_rx) = editor_cmd_rx {
        let mut rx = editor_rx.rx.blocking_lock();
        while let Ok(req) = rx.try_recv() {
            editor_cmd.write(EditorCommandRequestMessage(req));
        }
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
                    &mut writers,
                    &mut session_cleared,
                    &mut session_restored,
                );
            }
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                // 异步任务退出，视为完成
                writers.p4().write(DoneMessage {
                    usage: None,
                    model: None,
                });
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
    writers: &mut ParamSet<(
        MessageWriter<DeltaMessage>,
        MessageWriter<ToolCallMessage>,
        MessageWriter<ToolResultMessage>,
        MessageWriter<ConfirmRequestMessage>,
        MessageWriter<DoneMessage>,
        MessageWriter<ErrorMessage>,
        MessageWriter<RetryMessage>,
        MessageWriter<CompactedMessage>,
    )>,
    _session_cleared: &mut MessageWriter<SessionClearedMessage>,
    _session_restored: &mut MessageWriter<SessionRestoredMessage>,
) {
    match ev {
        AgentEvent::Delta(text) => {
            conv.status = ConversationStatus::Streaming;
            conv.current_assistant_text.push_str(&text);
            writers.p0().write(DeltaMessage { text });
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
            writers.p1().write(ToolCallMessage {
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
            writers.p2().write(ToolResultMessage {
                tool_call_id: call_id,
                tool_id,
                output,
                is_error,
                denied,
            });
        }
        AgentEvent::ConfirmRequest(req) => {
            conv.status = ConversationStatus::Confirming;
            writers.p3().write(ConfirmRequestMessage(req));
        }
        AgentEvent::SteeringInterrupted { partial_text } => {
            // 流式被 steering 中断：把半截文本固化为被中断的 assistant 消息，
            // 清空 current_assistant_text，避免与新一轮流式拼接。
            // 复用 DoneMessage 让 UI 把半截文本固化为历史气泡并清空当前节点
            // （usage 为 None，token 统计无害）。
            //
            // 清空 pending_tool_calls：中断轮已发 ToolCallStart 事件累积的
            // tool_call 块不执行（bridge 侧 steering 分支不执行 tool_calls、
            // 不回灌到 req），不应残留到 conv.messages。否则新一轮 push_tool_result
            // 触发 flush_pending_tool_calls 时，会把中断轮的孤儿 tool_call 与
            // 新一轮的 tool_call 一起固化进一条 assistant 消息，破坏 OpenAI
            // tool_call/tool_result 配对（中断轮 tool_call 无对应 tool_result）。
            conv.pending_tool_calls.clear();
            if !partial_text.is_empty() {
                conv.current_assistant_text = partial_text;
                conv.finalize_assistant(None, None);
                conv.persist_last_assistant();
            } else {
                conv.current_assistant_text.clear();
            }
            // status 保持 Streaming/Thinking 语义：对话未结束，steering 后继续流式
            conv.status = ConversationStatus::Thinking;
            writers.p4().write(DoneMessage {
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
            writers.p4().write(DoneMessage { usage, model });
        }
        AgentEvent::RetryAttempt {
            attempt,
            infinite,
            kind,
            last_error,
        } => {
            // 把半截助手文本固化为历史气泡（被中断的回复），清空当前节点，
            // 避免 retry 后新一轮流式与残留半截文本拼接。
            // 复用 DoneMessage 让 UI finalize_on_done 把半截文本固化为历史气泡
            // 并清空 CurrentAssistantText 实体（修复前 UI 实体未被清空，文本残留）。
            conv.finalize_assistant(None, None);
            conv.persist_last_assistant();
            // 状态保持 Streaming（重试中），不切到 Error
            conv.status = ConversationStatus::Streaming;
            writers.p4().write(DoneMessage {
                usage: None,
                model: None,
            });
            writers.p6().write(RetryMessage {
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
            writers.p5().write(ErrorMessage { kind, message });
        }
        AgentEvent::Compacted {
            tokens_before,
            tokens_after,
            new_messages,
        } => {
            // 用压缩后的 agent 层消息替换 conv.messages，保持 conv 与 req 同步
            // （修复下次 StartLoop 从未压缩的 conv 重建导致压缩丢失的 bug）。
            // conv.messages 不含 system（system 在 build_request 时动态注入），
            // new_messages 也不含 system（maybe_compact 已分离），语义一致。
            conv.messages = new_messages;
            // 持久化 compaction 记录（不重写历史，append CompactionEntry）
            conv.persist_compaction(
                &format!("[compacted: {tokens_before}→{tokens_after} tokens]"),
                "kept",
                tokens_before,
            );
            writers.p7().write(CompactedMessage {
                tokens_before,
                tokens_after,
            });
        }
    }
}
