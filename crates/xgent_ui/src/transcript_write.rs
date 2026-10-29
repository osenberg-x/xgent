//! Transcript 写入侧（M6.5-T1）：把 UI 消息流投影进 [`Transcript`]。
//!
//! Transcript 是消息顺序的唯一权威：本文件只做「消息 → 块」的投影，
//! 渲染由 `transcript_render.rs` 跟随。每条系统都要**排空**自己的 reader，
//! 否则当帧没有对应轮次时，消息会在下一轮被重放（跨轮串内容）。

use bevy::prelude::*;
use xgent_agent::{
    BoundedMessage, CompactedMessage, Conversation, ConversationStatus, DeltaMessage, DoneMessage,
    ErrorMessage, RetryMessage, SessionClearedMessage, SessionRestoredMessage, ToolCallMessage,
    ToolProgressMessage, ToolResultMessage,
};
use xgent_core::chat::{AgentMessage, ContentBlock as CoreContentBlock};
use xgent_settings::Localizer;
use xui::input::ChatInputSubmitted;

use crate::chat_panel::ChatInputMarker;
use crate::transcript::{BlockRef, ContentBlock, NoticeKind, Transcript};
use crate::undo::UndoNoticeMessage;

/// 写入系统集合：渲染系统据此排序。
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TranscriptWriteSet;

/// Transcript 写入插件。
pub struct TranscriptWritePlugin;

impl Plugin for TranscriptWritePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Transcript>().add_systems(
            Update,
            (
                record_delta,
                record_done,
                record_tool_call,
                record_tool_progress,
                record_tool_result,
                record_compacted,
                record_undo_notice,
                record_error,
                record_retry,
                record_bounded,
                record_session_cleared,
                record_session_restored,
                // 提交放最后：同帧迟到的 Done 先结算上一轮，避免把新一轮的空
                // AssistantMessage 误标为终稿。
                record_user_submission,
                record_steering,
            )
                .chain()
                .in_set(TranscriptWriteSet)
                .after(xgent_agent::agent_loop::agent_poll_system),
        );
    }
}

/// 无当前轮次时排空 reader 后计数告警（消息不得延后到下一轮重放）。
fn warn_dropped(kind: &str, dropped: usize) {
    if dropped > 0 {
        tracing::warn!("Transcript 无当前轮次，丢弃 {dropped} 条{kind}（避免延后重放到下一轮）");
    }
}

fn record_user_submission(
    mut reader: MessageReader<ChatInputSubmitted>,
    q_input: Query<Entity, With<ChatInputMarker>>,
    conv: Res<Conversation>,
    consumed: Option<Res<crate::completion_ui::CompletionConsumed>>,
    mut transcript: ResMut<Transcript>,
) {
    let input_entity = q_input.single().ok();
    let completion_enter = consumed.as_deref().is_some_and(|c| c.enter);
    let mut dropped = 0usize;
    for ev in reader.read() {
        // 输入卡尚未 spawn（启动首帧）时提交无意义：排空而不是延后重放。
        let Some(expected) = input_entity else {
            dropped += 1;
            continue;
        };
        if completion_enter || ev.entity != expected || ev.text.is_empty() {
            continue;
        }
        // 忙碌时 chat_panel 走 SteeringMessage，由 record_steering 落块，避免重复。
        if conv.status != ConversationStatus::Idle && conv.status != ConversationStatus::Error {
            continue;
        }
        let (turn, _) = transcript.push_user_message(ev.text.clone());
        transcript.begin_assistant_message(turn);
    }
    warn_dropped("ChatInputSubmitted", dropped);
}

/// 运行中追加的引导消息：同样占一个用户块，后续增量自然落到新一轮。
fn record_steering(
    mut reader: MessageReader<xgent_agent::SteeringMessage>,
    mut transcript: ResMut<Transcript>,
) {
    for ev in reader.read() {
        if ev.text.is_empty() {
            continue;
        }
        transcript.push_user_message(ev.text.clone());
    }
}

fn record_delta(mut reader: MessageReader<DeltaMessage>, mut transcript: ResMut<Transcript>) {
    let turn = transcript.current_turn();
    let mut dropped = 0usize;
    for ev in reader.read() {
        let Some(turn) = turn else {
            dropped += 1;
            continue;
        };
        transcript.append_text_delta(turn, &ev.text);
    }
    warn_dropped("DeltaMessage", dropped);
}

fn record_done(mut reader: MessageReader<DoneMessage>, mut transcript: ResMut<Transcript>) {
    let turn = transcript.current_turn();
    let mut arrived = false;
    let mut dropped = 0usize;
    for _ in reader.read() {
        if turn.is_some() {
            arrived = true;
        } else {
            dropped += 1;
        }
    }
    if let Some(turn) = turn
        && arrived
    {
        transcript.finalize_assistant(turn);
    }
    warn_dropped("DoneMessage", dropped);
}

fn record_tool_call(
    mut reader: MessageReader<ToolCallMessage>,
    mut transcript: ResMut<Transcript>,
) {
    let turn = transcript.current_turn();
    let mut dropped = 0usize;
    for ev in reader.read() {
        let Some(turn) = turn else {
            dropped += 1;
            continue;
        };
        transcript.push_tool_call(
            turn,
            ev.tool_call_id.clone(),
            ev.tool_id.clone(),
            ev.input.to_string(),
        );
        transcript.set_tool_status(&ev.tool_call_id, crate::transcript::ToolCallStatus::Running);
    }
    warn_dropped("ToolCallMessage", dropped);
}

fn record_tool_progress(
    mut reader: MessageReader<ToolProgressMessage>,
    mut transcript: ResMut<Transcript>,
) {
    for ev in reader.read() {
        if !transcript.set_tool_progress(&ev.tool_call_id, &ev.output) {
            tracing::warn!(tool_call_id = %ev.tool_call_id, "progress 找不到对应的工具调用块");
        }
    }
}

fn record_tool_result(
    mut reader: MessageReader<ToolResultMessage>,
    mut transcript: ResMut<Transcript>,
) {
    let turn = transcript.current_turn();
    for ev in reader.read() {
        let Some(turn) = turn else {
            tracing::warn!(tool_call_id = %ev.tool_call_id, "无当前轮次，丢弃工具结果");
            continue;
        };
        if transcript
            .set_tool_result(
                turn,
                ev.tool_call_id.clone(),
                ev.output.clone(),
                ev.is_error,
                ev.denied,
            )
            .is_none()
        {
            // Spec §1.1：未知 id 记 warn 并忽略，不新建孤立结果块。
            tracing::warn!(tool_call_id = %ev.tool_call_id, "工具结果找不到对应的工具调用块，忽略");
        }
    }
}

fn record_compacted(
    mut reader: MessageReader<CompactedMessage>,
    mut transcript: ResMut<Transcript>,
) {
    let turn = transcript.current_turn();
    let mut dropped = 0usize;
    for ev in reader.read() {
        let Some(turn) = turn else {
            dropped += 1;
            continue;
        };
        transcript.push_compression_notice(turn, ev.tokens_before, ev.tokens_after);
    }
    warn_dropped("CompactedMessage", dropped);
}

fn record_undo_notice(
    mut reader: MessageReader<UndoNoticeMessage>,
    mut transcript: ResMut<Transcript>,
) {
    let turn = transcript.current_turn();
    let mut dropped = 0usize;
    for ev in reader.read() {
        let Some(turn) = turn else {
            dropped += 1;
            continue;
        };
        transcript.push_notice(turn, ev.text.clone(), NoticeKind::Info);
    }
    warn_dropped("UndoNoticeMessage", dropped);
}

fn record_error(
    mut reader: MessageReader<ErrorMessage>,
    loc: Res<Localizer>,
    mut transcript: ResMut<Transcript>,
) {
    let turn = transcript.current_turn();
    let mut dropped = 0usize;
    for ev in reader.read() {
        let Some(turn) = turn else {
            dropped += 1;
            continue;
        };
        let prefix = match ev.kind {
            xgent_core::chat::ErrorKind::NotConfigured => {
                crate::i18n::tr(&loc, "error-not-configured")
            }
            xgent_core::chat::ErrorKind::AuthFailed => crate::i18n::tr(&loc, "error-auth-failed"),
            xgent_core::chat::ErrorKind::Network => crate::i18n::tr(&loc, "error-network"),
            xgent_core::chat::ErrorKind::StreamParse => crate::i18n::tr(&loc, "error-stream-parse"),
            xgent_core::chat::ErrorKind::RateLimited => crate::i18n::tr(&loc, "error-rate-limited"),
            xgent_core::chat::ErrorKind::ServerError => crate::i18n::tr(&loc, "error-server"),
            xgent_core::chat::ErrorKind::ProviderError => crate::i18n::tr(&loc, "error-provider"),
        };
        let retry_hint = crate::i18n::tr(&loc, "error-retry-hint");
        transcript.push_notice(
            turn,
            format!(
                "{prefix} {}

{retry_hint}",
                ev.message
            ),
            NoticeKind::Error,
        );
    }
    warn_dropped("ErrorMessage", dropped);
}

fn record_retry(
    mut reader: MessageReader<RetryMessage>,
    loc: Res<Localizer>,
    mut transcript: ResMut<Transcript>,
) {
    let turn = transcript.current_turn();
    let mut dropped = 0usize;
    for ev in reader.read() {
        let Some(turn) = turn else {
            dropped += 1;
            continue;
        };
        let label = if ev.infinite {
            crate::i18n::tr_with(
                &loc,
                "retry-attempt-infinite",
                &[("n", ev.attempt.to_string())],
            )
            .to_string()
        } else {
            crate::i18n::tr_with(&loc, "retry-attempt", &[("n", ev.attempt.to_string())])
                .to_string()
        };
        let last_error = crate::i18n::tr_with(
            &loc,
            "retry-last-error",
            &[("error", ev.last_error.clone())],
        );
        transcript.push_notice(
            turn,
            format!(
                "{label}
{last_error}"
            ),
            NoticeKind::Retry,
        );
    }
    warn_dropped("RetryMessage", dropped);
}

fn record_bounded(mut reader: MessageReader<BoundedMessage>, mut transcript: ResMut<Transcript>) {
    let turn = transcript.current_turn();
    let mut dropped = 0usize;
    for ev in reader.read() {
        let Some(turn) = turn else {
            dropped += 1;
            continue;
        };
        transcript.push_notice(turn, ev.detail.clone(), NoticeKind::Info);
    }
    warn_dropped("BoundedMessage", dropped);
}

fn record_session_cleared(
    mut reader: MessageReader<SessionClearedMessage>,
    mut transcript: ResMut<Transcript>,
) {
    if reader.read().next().is_some() {
        transcript.clear();
    }
}

fn record_session_restored(
    mut reader: MessageReader<SessionRestoredMessage>,
    mut transcript: ResMut<Transcript>,
) {
    for ev in reader.read() {
        let mut blocks: Vec<BlockRef> = Vec::new();
        let mut next_turn = 0u64;
        for message in &ev.messages {
            match message {
                AgentMessage::User(user) => {
                    let text = user
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            CoreContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    blocks.push(BlockRef::UserMessage {
                        turn_index: next_turn,
                        block_index: blocks.len(),
                        content: text,
                    });
                    next_turn += 1;
                }
                AgentMessage::Assistant(assistant) => {
                    let turn = next_turn.saturating_sub(1);
                    next_turn = next_turn.max(turn + 1);
                    let mut pending: Vec<ContentBlock> = Vec::new();
                    for block in &assistant.content {
                        match block {
                            CoreContentBlock::Text { text } => {
                                pending.push(ContentBlock::Text(text.clone()));
                            }
                            CoreContentBlock::ToolCall { id, name, args } => {
                                flush_assistant_text(&mut blocks, turn, &mut pending);
                                blocks.push(BlockRef::ToolCall {
                                    turn_index: turn,
                                    block_index: blocks.len(),
                                    tool_call_id: id.clone(),
                                    tool_name: name.clone(),
                                    arguments: args.to_string(),
                                    status: crate::transcript::ToolCallStatus::Pending,
                                    progress: None,
                                });
                            }
                            CoreContentBlock::ToolResult {
                                tool_call_id,
                                content: text,
                                is_error,
                            } => {
                                flush_assistant_text(&mut blocks, turn, &mut pending);
                                blocks.push(BlockRef::ToolResult {
                                    turn_index: turn,
                                    block_index: blocks.len(),
                                    tool_call_id: tool_call_id.clone(),
                                    content: text.clone(),
                                    is_error: *is_error,
                                    // 会话 JSONL 不记录拒绝原因，恢复时按失败处理。
                                    denied: false,
                                });
                            }
                            CoreContentBlock::Image { .. } => {}
                        }
                    }
                    flush_assistant_text(&mut blocks, turn, &mut pending);
                }
                AgentMessage::ToolResult(result) => {
                    let turn = next_turn.saturating_sub(1);
                    blocks.push(BlockRef::ToolResult {
                        turn_index: turn,
                        block_index: blocks.len(),
                        tool_call_id: result.tool_call_id.clone(),
                        content: result.content.clone(),
                        is_error: result.is_error,
                        denied: false,
                    });
                }
                AgentMessage::Notification(note) => {
                    let turn = next_turn.saturating_sub(1);
                    blocks.push(BlockRef::Notice {
                        turn_index: turn,
                        block_index: blocks.len(),
                        text: note.text.clone(),
                        kind: NoticeKind::Info,
                    });
                }
            }
        }
        apply_restored_tool_status(&mut blocks);
        transcript.restore(blocks);
    }
}

fn flush_assistant_text(blocks: &mut Vec<BlockRef>, turn: u64, pending: &mut Vec<ContentBlock>) {
    if pending.is_empty() {
        return;
    }
    blocks.push(BlockRef::AssistantMessage {
        turn_index: turn,
        block_index: blocks.len(),
        blocks: std::mem::take(pending),
        finalized: true,
    });
}

fn apply_restored_tool_status(blocks: &mut [BlockRef]) {
    for i in 0..blocks.len() {
        let (call_id, is_error) = match &blocks[i] {
            BlockRef::ToolResult {
                tool_call_id,
                is_error,
                ..
            } => (tool_call_id.clone(), *is_error),
            _ => continue,
        };
        for block in blocks.iter_mut() {
            if let BlockRef::ToolCall {
                tool_call_id,
                status,
                ..
            } = block
                && *tool_call_id == call_id
            {
                *status = if is_error {
                    crate::transcript::ToolCallStatus::Failed
                } else {
                    crate::transcript::ToolCallStatus::Completed
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xgent_agent::{DoneMessage, ToolCallMessage, ToolProgressMessage, ToolResultMessage};

    fn app_with_systems() -> App {
        let mut app = App::new();
        app.add_message::<ChatInputSubmitted>()
            .add_message::<DeltaMessage>()
            .add_message::<DoneMessage>()
            .add_message::<ToolCallMessage>()
            .add_message::<ToolProgressMessage>()
            .add_message::<ToolResultMessage>()
            .add_message::<CompactedMessage>()
            .add_message::<UndoNoticeMessage>()
            .add_message::<SessionClearedMessage>()
            .add_message::<SessionRestoredMessage>()
            .add_message::<xgent_agent::SteeringMessage>()
            .insert_resource(Conversation::default())
            .insert_resource(Transcript::default())
            .add_systems(
                Update,
                (
                    record_delta,
                    record_done,
                    record_tool_call,
                    record_tool_progress,
                    record_tool_result,
                    record_compacted,
                    record_undo_notice,
                    record_session_cleared,
                    record_user_submission,
                    record_steering,
                )
                    .chain(),
            );
        app
    }

    #[test]
    fn stragglers_after_clear_do_not_leak_into_next_turn() {
        let mut app = app_with_systems();
        let input = app.world_mut().spawn(ChatInputMarker).id();
        app.world_mut().write_message(ChatInputSubmitted {
            text: "first".into(),
            entity: input,
        });
        app.update();
        app.world_mut().write_message(DeltaMessage {
            text: "partial".into(),
        });
        app.update();
        // 流式途中新建会话：Transcript 清空后没有当前轮。
        app.world_mut().write_message(SessionClearedMessage);
        app.update();
        // 迟到的增量与 Done 必须在本帧被排空，不得延后重放到下一轮。
        app.world_mut().write_message(DeltaMessage {
            text: "leak".into(),
        });
        app.world_mut().write_message(DoneMessage {
            usage: None,
            model: None,
        });
        app.update();
        app.world_mut().write_message(ChatInputSubmitted {
            text: "second".into(),
            entity: input,
        });
        app.update();

        let transcript = app.world().resource::<Transcript>();
        let blocks = transcript.blocks();
        assert_eq!(blocks.len(), 2, "残留增量/Done 不得进入新一轮：{blocks:?}");
        match &blocks[1] {
            BlockRef::AssistantMessage {
                blocks: inner,
                finalized,
                ..
            } => {
                assert!(inner.is_empty(), "新一轮被写入上一轮文本：{inner:?}");
                assert!(!*finalized, "上一轮遗留的 Done 提前终稿了新一轮");
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn steering_text_gets_its_own_user_block() {
        let mut app = app_with_systems();
        let input = app.world_mut().spawn(ChatInputMarker).id();
        app.world_mut().write_message(ChatInputSubmitted {
            text: "go".into(),
            entity: input,
        });
        app.update();
        app.world_mut().write_message(xgent_agent::SteeringMessage {
            text: "先看测试".into(),
        });
        app.update();

        let transcript = app.world().resource::<Transcript>();
        let users: Vec<&str> = transcript
            .blocks()
            .iter()
            .filter_map(|block| match block {
                BlockRef::UserMessage { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(users, vec!["go", "先看测试"], "引导文本也要进对话流");
    }

    #[test]
    fn unknown_tool_call_id_is_ignored_without_orphan_block() {
        let mut app = app_with_systems();
        let input = app.world_mut().spawn(ChatInputMarker).id();
        app.world_mut().write_message(ChatInputSubmitted {
            text: "run".into(),
            entity: input,
        });
        app.update();
        app.world_mut().write_message(ToolResultMessage {
            tool_call_id: "ghost".into(),
            tool_id: "read_file".into(),
            output: "out".into(),
            is_error: false,
            denied: false,
        });
        app.update();

        let transcript = app.world().resource::<Transcript>();
        let results = transcript
            .blocks()
            .iter()
            .filter(|block| matches!(block, BlockRef::ToolResult { .. }))
            .count();
        assert_eq!(results, 0, "未知 id 不得生成孤立结果块");
    }

    #[test]
    fn submission_and_stream_project_in_order() {
        let mut app = app_with_systems();
        let input = app.world_mut().spawn(ChatInputMarker).id();
        app.world_mut().write_message(ChatInputSubmitted {
            text: "hello".into(),
            entity: input,
        });
        app.update();
        app.world_mut()
            .write_message(DeltaMessage { text: "Hi".into() });
        app.update();
        app.world_mut().write_message(DeltaMessage {
            text: " there".into(),
        });
        app.update();
        app.world_mut().write_message(DoneMessage {
            usage: None,
            model: None,
        });
        app.update();

        let transcript = app.world().resource::<Transcript>();
        assert_eq!(transcript.blocks().len(), 2);
        match &transcript.blocks()[1] {
            BlockRef::AssistantMessage {
                blocks, finalized, ..
            } => {
                assert_eq!(blocks, &vec![ContentBlock::Text("Hi there".into())]);
                assert!(*finalized);
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn tool_blocks_split_assistant_segments() {
        let mut app = app_with_systems();
        let input = app.world_mut().spawn(ChatInputMarker).id();
        app.world_mut().write_message(ChatInputSubmitted {
            text: "run".into(),
            entity: input,
        });
        app.update();
        app.world_mut().write_message(DeltaMessage {
            text: "before".into(),
        });
        app.update();
        app.world_mut().write_message(ToolCallMessage {
            tool_call_id: "tc1".into(),
            tool_id: "run_command".into(),
            input: serde_json::json!({"cmd": "ls"}),
        });
        app.update();
        app.world_mut().write_message(ToolProgressMessage {
            tool_call_id: "tc1".into(),
            tool_id: "run_command".into(),
            output: "partial".into(),
        });
        app.update();
        app.world_mut().write_message(ToolResultMessage {
            tool_call_id: "tc1".into(),
            tool_id: "run_command".into(),
            output: "done".into(),
            is_error: false,
            denied: false,
        });
        app.update();
        app.world_mut().write_message(DeltaMessage {
            text: "after".into(),
        });
        app.update();
        app.world_mut().write_message(DoneMessage {
            usage: None,
            model: None,
        });
        app.update();

        let transcript = app.world().resource::<Transcript>();
        let kinds: Vec<&str> = transcript
            .blocks()
            .iter()
            .map(|b| match b {
                BlockRef::UserMessage { .. } => "user",
                BlockRef::AssistantMessage { finalized, .. } => {
                    if *finalized {
                        "assistant-final"
                    } else {
                        "assistant"
                    }
                }
                BlockRef::ToolCall { .. } => "tool-call",
                BlockRef::ToolResult { .. } => "tool-result",
                BlockRef::CompressionNotice { .. } => "compression",
                BlockRef::Notice { .. } => "notice",
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "user",
                "assistant-final",
                "tool-call",
                "tool-result",
                "assistant-final"
            ]
        );
        let call = transcript
            .blocks()
            .iter()
            .find_map(|b| match b {
                BlockRef::ToolCall {
                    status, progress, ..
                } => Some((*status, progress.clone())),
                _ => None,
            })
            .unwrap();
        assert_eq!(call.0, crate::transcript::ToolCallStatus::Completed);
        assert_eq!(call.1.as_deref(), Some("partial"));
    }
}
