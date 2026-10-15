//! agent 文件改动的撤销（缺口 R3-11 的用户入口）。
//!
//! 命令面板发 [`UndoChangesMessage`] → 在本进程 tokio runtime 上向检查点存储请求
//! 「撤销当前会话最近一轮」→ 结果经 toast 汇总；有跳过/无检查点/写盘失败时再落一条
//! 常驻提示（toast 只活 2.2s，不足以读完一份清单）。
//!
//! 撤销**只改磁盘**：打开中的缓冲区沿用既有 fs-watch 语义——干净的 buffer 由
//! `editor/conflict.rs::handle_file_changed` 静默重载，脏 buffer 进冲突对话框，
//! 本模块不另开第二条重载通路（两处重载迟早会互相打断）。

use bevy::prelude::*;
use parking_lot::Mutex;
use tokio::sync::oneshot;
use xgent_agent::{Conversation, ConversationStatus};
use xgent_tools::{SkipReason, UndoReport};

use crate::chat_panel::ChatPanelEntities;
use crate::i18n::{tr, tr_with};
use crate::kit::ToastMessage;

/// 撤销请求（命令面板写入）。
#[derive(Message, Debug, Default)]
pub struct UndoChangesMessage;

/// 撤销详情的常驻提示（写入对话流，不像 toast 那样几秒消失）。
#[derive(Message, Debug, Clone)]
pub struct UndoNoticeMessage {
    /// 已本地化的提示文本
    pub text: String,
}

/// 撤销结果（含不可用的情形，便于文案区分）。
#[derive(Debug, Clone)]
pub enum UndoOutcome {
    /// 完成，携带逐文件结果
    Done(Box<UndoReport>),
    /// agent 正在执行，未执行撤销
    Busy,
    /// 本会话没有装配检查点存储（功能不可用）
    Unavailable,
}

/// 等待中的撤销任务回执。
#[derive(Resource, Default)]
struct PendingUndo(Mutex<Vec<oneshot::Receiver<UndoOutcome>>>);

/// 撤销插件。
pub struct UndoPlugin;

impl bevy::app::Plugin for UndoPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_message::<UndoChangesMessage>()
            .add_message::<UndoNoticeMessage>()
            .init_resource::<PendingUndo>()
            .add_systems(
                Update,
                (request_undo, collect_undo_results, append_undo_notice)
                    .chain()
                    // 与确认弹窗/面板同序：读到本帧最新的 Conversation.status，
                    // 避免用上一帧的 Idle 判断放行撤销
                    .after(xgent_agent::agent_loop::agent_poll_system),
            );
    }
}

/// 处理撤销请求：只在对话空闲时执行，避免与写入方抢同一份清单。
fn request_undo(
    mut reader: MessageReader<UndoChangesMessage>,
    conv: Res<Conversation>,
    bridge: Res<xgent_agent::AgentBridge>,
    pending: ResMut<PendingUndo>,
) {
    for _ in reader.read() {
        let (store, session) = match gate(bridge.checkpoints.clone(), conv.status, conv.id.0) {
            Gate::Run(store, session) => (store, session),
            Gate::Reject(outcome) => {
                // 忙 / 未装配存储：不 spawn，就地给结论（文案区分两种情形）
                pending.0.lock().push(completed(outcome));
                continue;
            }
        };
        let (tx, rx) = oneshot::channel();
        // 用 bridge 自带的 runtime：它是本进程真实存在的那一个。
        // 不用 `EditorIoRuntime.handle` —— 该 handle 在 xgent_app 里从未注入
        // （编辑器 IO 因此一直走同步降级），依赖它会让撤销在生产永远报「不可用」。
        bridge.runtime.handle().spawn(async move {
            let report = store.undo_last_turn(session).await;
            let _ = tx.send(UndoOutcome::Done(Box::new(report)));
        });
        pending.0.lock().push(rx);
    }
}

/// 放行判定：能不能撤销、用哪个存储与哪个会话。
///
/// 拆成纯函数是为了单测这两条分支——它们不需要 runtime，也不该只在人肉点 UI 时
/// 才被验证（撤销按钮在 agent 执行中把已经写入的文件回退，是真实的数据损失路径）。
enum Gate {
    Run(std::sync::Arc<xgent_tools::CheckpointStore>, u64),
    Reject(UndoOutcome),
}

fn gate(
    store: Option<std::sync::Arc<xgent_tools::CheckpointStore>>,
    status: ConversationStatus,
    session: u64,
) -> Gate {
    let Some(store) = store else {
        return Gate::Reject(UndoOutcome::Unavailable);
    };
    if status != ConversationStatus::Idle {
        return Gate::Reject(UndoOutcome::Busy);
    }
    Gate::Run(store, session)
}

/// 已有结论时伪造一个「已完成」的接收端，走同一条上报路径。
fn completed(outcome: UndoOutcome) -> oneshot::Receiver<UndoOutcome> {
    let (tx, rx) = oneshot::channel();
    let _ = tx.send(outcome);
    rx
}

/// 收结果：toast 汇总 + 需要时落常驻提示。
fn collect_undo_results(
    pending: ResMut<PendingUndo>,
    loc: Res<xgent_settings::Localizer>,
    mut toast: MessageWriter<ToastMessage>,
    mut notice: MessageWriter<UndoNoticeMessage>,
) {
    let mut done = Vec::new();
    pending.0.lock().retain_mut(|rx| match rx.try_recv() {
        Ok(outcome) => {
            done.push(outcome);
            false
        }
        Err(oneshot::error::TryRecvError::Empty) => true,
        Err(_) => false,
    });
    for outcome in done {
        let text = summarize(&loc, &outcome);
        toast.write(ToastMessage { text });
        if let Some(detail) = detail(&loc, &outcome) {
            notice.write(UndoNoticeMessage { text: detail });
        }
    }
}

/// 撤销结果摘要（一行，进 toast）。
pub fn summarize(loc: &xgent_settings::Localizer, outcome: &UndoOutcome) -> String {
    match outcome {
        UndoOutcome::Busy => tr(loc, "undo-busy"),
        UndoOutcome::Unavailable => tr(loc, "undo-unavailable"),
        UndoOutcome::Done(report) => {
            let changed = report.restored.len() + report.removed.len();
            if changed > 0 {
                let turn = report.turn.map(|t| t.turn.to_string()).unwrap_or_default();
                return tr_with(
                    loc,
                    "undo-done",
                    &[("count", changed.to_string()), ("turn", turn)],
                );
            }
            // 一个字节都没改：区分「全被跳过」「文件本就不在了」「压根没有记录」，
            // 否则 toast 会说「跳过 0 个」这种自相矛盾的话
            if !report.skipped.is_empty() {
                return tr_with(
                    loc,
                    "undo-part-skipped",
                    &[("count", report.skipped.len().to_string())],
                );
            }
            if report.turn.is_none() {
                return tr(loc, "undo-nothing");
            }
            tr(loc, "undo-noop")
        }
    }
}

/// 撤销详情（需要用户看清时才生成：有跳过 / 无检查点 / 写盘失败）。
pub fn detail(loc: &xgent_settings::Localizer, outcome: &UndoOutcome) -> Option<String> {
    let UndoOutcome::Done(report) = outcome else {
        return None;
    };
    if report.skipped.is_empty() && report.uncovered.is_empty() && report.failed.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if !report.restored.is_empty() {
        parts.push(tr_with(
            loc,
            "undo-part-restored",
            &[("count", report.restored.len().to_string())],
        ));
    }
    if !report.removed.is_empty() {
        parts.push(tr_with(
            loc,
            "undo-part-removed",
            &[("count", report.removed.len().to_string())],
        ));
    }
    if !report.skipped.is_empty() {
        let list = report
            .skipped
            .iter()
            .map(|(path, reason)| format!("{path}（{}）", skip_reason(loc, *reason)))
            .collect::<Vec<_>>()
            .join("、");
        parts.push(format!(
            "{}：{list}",
            tr_with(
                loc,
                "undo-part-skipped",
                &[("count", report.skipped.len().to_string())]
            )
        ));
    }
    if !report.uncovered.is_empty() {
        parts.push(format!(
            "{}：{}",
            tr_with(
                loc,
                "undo-part-uncovered",
                &[("count", report.uncovered.len().to_string())]
            ),
            report.uncovered.join("、")
        ));
    }
    if !report.failed.is_empty() {
        parts.push(format!(
            "{}：{}",
            tr_with(
                loc,
                "undo-part-failed",
                &[("count", report.failed.len().to_string())]
            ),
            report.failed.join("、")
        ));
    }
    Some(tr_with(
        loc,
        "undo-detail",
        &[("summary", parts.join(" · "))],
    ))
}

/// 跳过原因的可读文案。
fn skip_reason(loc: &xgent_settings::Localizer, reason: SkipReason) -> String {
    let key = match reason {
        SkipReason::ExternallyModified => "undo-skip-external",
        SkipReason::DeletedByUser => "undo-skip-deleted",
        SkipReason::BlobLost => "undo-skip-blob-lost",
        SkipReason::Unverified => "undo-skip-unverified",
    };
    tr(loc, key)
}

/// 把详情画进对话流（与压缩提示同一形状：居中 muted 小字）。
fn append_undo_notice(
    mut reader: MessageReader<UndoNoticeMessage>,
    entities: Res<ChatPanelEntities>,
    mut commands: Commands,
    theme: Res<crate::theme::Theme>,
) {
    let Some(list) = entities.message_list else {
        return;
    };
    for ev in reader.read() {
        let text = ev.text.clone();
        commands.entity(list).with_children(|p| {
            p.spawn((Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },))
                .with_children(|row| {
                    row.spawn((
                        Node {
                            padding: UiRect::all(px(crate::theme::space::XS)),
                            border_radius: BorderRadius::all(px(4.0)),
                            ..default()
                        },
                        BackgroundColor(theme.elevated),
                        crate::fonts::ui_text(
                            text,
                            crate::theme::type_scale::MICRO,
                            400,
                            theme.text_muted,
                            crate::theme::type_scale::line_height::UI,
                        ),
                    ));
                });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 去掉 fluent 插值两侧的 bidi 隔离符（U+2068/U+2069）。
    ///
    /// fluent 有意插入它们以隔离双向文本，既有键（如 `tool-fold-result`）同样如此，
    /// 渲染无差异；断言里去掉它们才能按人看到的字面比较。
    fn plain(s: &str) -> String {
        s.chars()
            .filter(|c| !matches!(c, '\u{2068}' | '\u{2069}'))
            .collect()
    }

    fn report() -> UndoReport {
        UndoReport {
            turn: Some(xgent_tools::TurnKey {
                session: 1,
                turn: 3,
            }),
            restored: vec!["a.rs".into(), "b.rs".into()],
            removed: vec!["new.rs".into()],
            already_absent: vec![],
            skipped: vec![("c.rs".into(), SkipReason::ExternallyModified)],
            uncovered: vec![],
            failed: vec![],
        }
    }

    #[test]
    fn busy_and_unavailable_have_their_own_text() {
        let loc = xgent_settings::Localizer::load("zh-CN");
        assert_eq!(summarize(&loc, &UndoOutcome::Busy), tr(&loc, "undo-busy"));
        assert_eq!(
            summarize(&loc, &UndoOutcome::Unavailable),
            tr(&loc, "undo-unavailable")
        );
        assert_eq!(
            summarize(&loc, &UndoOutcome::Unavailable),
            "撤销不可用：本会话未启用文件检查点"
        );
        // 忙/不可用不产详情
        assert!(detail(&loc, &UndoOutcome::Busy).is_none());
    }

    #[test]
    fn empty_and_skipped_results_say_different_things() {
        let loc = xgent_settings::Localizer::load("zh-CN");
        // 压根没有记录
        let none = UndoOutcome::Done(Box::<UndoReport>::default());
        assert_eq!(summarize(&loc, &none), tr(&loc, "undo-nothing"));
        // 有一轮但全部被跳过：不该说「跳过 0 个」，也不该说「没有可撤销」
        let skipped = UndoOutcome::Done(Box::new(UndoReport {
            turn: Some(xgent_tools::TurnKey {
                session: 1,
                turn: 1,
            }),
            skipped: vec![("a.rs".into(), SkipReason::ExternallyModified)],
            ..Default::default()
        }));
        assert_eq!(plain(&summarize(&loc, &skipped)), "跳过 1 个");
        // 有一轮但无事可做（agent 建的文件用户已删）
        let noop = UndoOutcome::Done(Box::new(UndoReport {
            turn: Some(xgent_tools::TurnKey {
                session: 1,
                turn: 2,
            }),
            already_absent: vec!["gone.rs".into()],
            ..Default::default()
        }));
        assert_eq!(summarize(&loc, &noop), tr(&loc, "undo-noop"));
    }

    #[test]
    fn clean_revert_has_no_detail() {
        let loc = xgent_settings::Localizer::load("zh-CN");
        let r = UndoReport {
            turn: Some(xgent_tools::TurnKey {
                session: 1,
                turn: 2,
            }),
            restored: vec!["a.rs".into()],
            ..Default::default()
        };
        assert!(detail(&loc, &UndoOutcome::Done(Box::new(r))).is_none());
    }

    #[test]
    fn skipped_files_are_listed_with_reasons() {
        let loc = xgent_settings::Localizer::load("zh-CN");
        let text =
            plain(&detail(&loc, &UndoOutcome::Done(Box::new(report()))).expect("有跳过就该给详情"));
        assert!(text.contains("c.rs"), "{text}");
        assert!(text.contains("已被外部修改"), "{text}");
        assert!(text.contains("恢复 2 个"), "{text}");
        assert!(text.contains("删除 1 个"), "{text}");
        // 摘要与详情同时在场时，详情必须带上「跳过」的具体路径（toast 2.2s 就没了）
        assert!(text.contains("跳过 1 个"), "{text}");
    }

    #[test]
    fn both_locales_define_every_new_key() {
        // 缺键时 Localizer 会原样返回 key（便于排查），故 != key 即证明键存在
        let zh = xgent_settings::Localizer::load("zh-CN");
        let en = xgent_settings::Localizer::load("en-US");
        for key in [
            "cmd-undo-changes",
            "undo-done",
            "undo-nothing",
            "undo-busy",
            "undo-unavailable",
            "undo-detail",
            "undo-part-restored",
            "undo-part-removed",
            "undo-part-skipped",
            "undo-part-uncovered",
            "undo-part-failed",
            "undo-skip-external",
            "undo-skip-deleted",
            "undo-skip-blob-lost",
            "undo-skip-unverified",
            "undo-noop",
        ] {
            let a = tr(&zh, key);
            let b = tr(&en, key);
            assert_ne!(a, key, "zh-CN 缺键 {key}");
            assert_ne!(b, key, "en-US 缺键 {key}");
            assert_ne!(a, b, "{key} 两语种同字面，说明 en 未翻译");
        }
    }

    #[test]
    fn gate_refuses_while_busy_and_without_store() {
        let dir = tempfile::tempdir().unwrap();
        let store =
            std::sync::Arc::new(xgent_tools::CheckpointStore::new(dir.path().to_path_buf()));
        assert!(
            matches!(
                gate(Some(store.clone()), ConversationStatus::Thinking, 1),
                Gate::Reject(UndoOutcome::Busy)
            ),
            "agent 执行中不得撤销"
        );
        assert!(
            matches!(
                gate(None, ConversationStatus::Idle, 1),
                Gate::Reject(UndoOutcome::Unavailable)
            ),
            "没装配存储要如实报不可用"
        );
        assert!(
            matches!(
                gate(Some(store), ConversationStatus::Idle, 7),
                Gate::Run(_, 7)
            ),
            "空闲且有存储才放行"
        );
    }
}
