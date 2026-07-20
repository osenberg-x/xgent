//! 外部修改冲突协调 + 弹窗。
//!
//! 详见 `doc/design/editor-design.md` 第 3.6 节 / 4.1 节 / 2.4 节。
//!
//! 流程：
//! ```text
//! daemon FileChanged → FileChangedEvent → 编辑器系统查 EditorBuffer.state
//!   → Clean：静默重载（fs::read → 替换 buffer → 重置 undo → dirty=false）
//!   → Dirty：enter_conflict → 弹外部修改冲突弹窗 → 用户三选
//!       → 丢弃本地：同 Clean 路径
//!       → 保留本地：keep_local（下次保存覆盖）
//!       → 对比合并：打开 diff 视图（MVP 降级为并排只读）
//! ```

use std::path::PathBuf;

use bevy::prelude::*;

use crate::editor::buffer::{BufferState, EditorBuffer};
use crate::editor::io::FileReadRequest;
use crate::editor::tabs::EditorTabs;

/// 外部文件变更事件（由 daemon 桥接转发，见 `doc/design/editor-design.md` 3.2）。
#[derive(Message, Debug, Clone)]
pub struct FileChangedEvent {
    /// 变更文件绝对路径
    pub path: PathBuf,
}

/// 冲突弹窗根节点标记。
#[derive(Component, Default)]
pub struct ConflictDialogMarker;

/// 冲突弹窗关联的 buffer 实体（用于决策时定位）。
#[derive(Component)]
pub struct ConflictDialogFor {
    /// 冲突的 buffer 实体
    pub buffer: Entity,
}

/// 冲突决策按钮标记。
#[derive(Component, Default)]
pub struct ConflictDiscardMarker;
#[derive(Component, Default)]
pub struct ConflictKeepLocalMarker;
#[derive(Component, Default)]
pub struct ConflictDiffMarker;
/// 冲突决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictDecision {
    /// 丢弃本地，重载磁盘
    Discard,
    /// 保留本地，下次保存覆盖
    KeepLocal,
    /// 对比合并（MVP 降级为并排只读）
    Diff,
}

/// 订阅 `FileChangedEvent`，按 buffer 状态分发：Clean 静默重载，Dirty 进冲突态。
///
/// 注意：用单个 `Query<&mut EditorBuffer>` 同时承担查找与修改，
/// 避免同系统内 `Query<&EditorBuffer>` + `Query<&mut EditorBuffer>` 并存的 B0001 冲突。
pub fn handle_file_changed(
    mut reader: MessageReader<FileChangedEvent>,
    tabs: Res<EditorTabs>,
    mut q_buffers: Query<&mut EditorBuffer>,
    mut read_writer: MessageWriter<FileReadRequest>,
    q_dialog: Query<Entity, With<ConflictDialogMarker>>,
    mut commands: Commands,
) {
    for ev in reader.read() {
        // 在 tabs 中按路径定位 buffer 实体（不借用第二个 Query）
        let Some(entity) = tabs.tabs.iter().copied().find(|&e| {
            q_buffers
                .get(e)
                .ok()
                .is_some_and(|b| b.path() == ev.path.as_path())
        }) else {
            continue; // 未打开，忽略
        };
        let Ok(mut buf) = q_buffers.get_mut(entity) else {
            continue;
        };
        match buf.state {
            BufferState::Clean => {
                // 静默重载：发 FileReadRequest
                read_writer.write(FileReadRequest {
                    path: ev.path.clone(),
                    line: None,
                });
            }
            BufferState::Dirty | BufferState::LocalPreferred => {
                // 进入冲突态，弹窗
                buf.enter_conflict();
                if q_dialog.single().is_err() {
                    spawn_conflict_dialog(&mut commands, entity, &ev.path);
                }
            }
            BufferState::ConflictDetected => {
                // 已在冲突态，不重复弹窗
            }
        }
    }
}

/// spawn 外部修改冲突弹窗（三选）。
fn spawn_conflict_dialog(commands: &mut Commands, buffer: Entity, path: &std::path::Path) {
    let accent = Color::srgb(0.36, 0.62, 0.92);
    let panel = Color::srgb(0.13, 0.14, 0.17);
    let border = Color::srgb(0.25, 0.26, 0.30);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
            ConflictDialogMarker,
            ConflictDialogFor { buffer },
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    padding: UiRect::all(Val::Px(16.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(12.0),
                    min_width: Val::Px(360.0),
                    ..default()
                },
                BackgroundColor(panel),
                BorderColor::all(border),
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new(format!(
                        "文件已被外部修改\n\n{} 在编辑器外被修改。\n你有未保存的本地修改。",
                        path.display()
                    )),
                    TextColor(Color::WHITE),
                ));
                card.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(12.0),
                    ..default()
                },))
                    .with_children(|btns| {
                        btns.spawn((
                            Button,
                            Node {
                                padding: UiRect::all(Val::Px(8.0)),
                                ..default()
                            },
                            BackgroundColor(accent),
                            Text::new("丢弃本地"),
                            TextColor(Color::WHITE),
                            ConflictDiscardMarker,
                        ));
                        btns.spawn((
                            Button,
                            Node {
                                padding: UiRect::all(Val::Px(8.0)),
                                ..default()
                            },
                            BackgroundColor(accent),
                            Text::new("保留本地"),
                            TextColor(Color::WHITE),
                            ConflictKeepLocalMarker,
                        ));
                        btns.spawn((
                            Button,
                            Node {
                                padding: UiRect::all(Val::Px(8.0)),
                                ..default()
                            },
                            BackgroundColor(accent),
                            Text::new("对比合并"),
                            TextColor(Color::WHITE),
                            ConflictDiffMarker,
                        ));
                    });
            });
        });
}

/// 处理冲突决策按钮点击。
pub fn handle_conflict_decision(
    q_dialog: Query<(Entity, &ConflictDialogFor), With<ConflictDialogMarker>>,
    q_discard: Query<&Interaction, (With<ConflictDiscardMarker>, Changed<Interaction>)>,
    q_keep: Query<&Interaction, (With<ConflictKeepLocalMarker>, Changed<Interaction>)>,
    q_diff: Query<&Interaction, (With<ConflictDiffMarker>, Changed<Interaction>)>,
    mut q_buffers: Query<&mut EditorBuffer>,
    mut read_writer: MessageWriter<FileReadRequest>,
    mut commands: Commands,
) {
    let Ok((dialog, for_buf)) = q_dialog.single() else {
        return;
    };
    let decision = None
        .or_else(|| {
            q_discard
                .iter()
                .any(|i| *i == Interaction::Pressed)
                .then_some(ConflictDecision::Discard)
        })
        .or_else(|| {
            q_keep
                .iter()
                .any(|i| *i == Interaction::Pressed)
                .then_some(ConflictDecision::KeepLocal)
        })
        .or_else(|| {
            q_diff
                .iter()
                .any(|i| *i == Interaction::Pressed)
                .then_some(ConflictDecision::Diff)
        });
    let Some(decision) = decision else {
        return;
    };
    if let Ok(mut buf) = q_buffers.get_mut(for_buf.buffer) {
        match decision {
            ConflictDecision::Discard => {
                read_writer.write(FileReadRequest {
                    path: buf.path.clone(),
                    line: None,
                });
            }
            ConflictDecision::KeepLocal => {
                buf.keep_local();
            }
            ConflictDecision::Diff => {
                // MVP 降级：标记 LocalPreferred，用户手动取舍
                buf.keep_local();
            }
        }
    }
    commands.entity(dialog).despawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_decision_variants() {
        assert_eq!(ConflictDecision::Discard, ConflictDecision::Discard);
        assert_ne!(ConflictDecision::Discard, ConflictDecision::KeepLocal);
        assert_ne!(ConflictDecision::Diff, ConflictDecision::Discard);
    }

    #[test]
    fn file_changed_event_clone() {
        let e = FileChangedEvent {
            path: PathBuf::from("/x"),
        };
        let e2 = e.clone();
        assert_eq!(e.path, e2.path);
    }
}
