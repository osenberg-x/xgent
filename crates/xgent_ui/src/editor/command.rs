//! 编辑器命令订阅与执行。
//!
//! 详见 `doc/design/editor-design.md` 第 3.2 节 / 3.4 节。
//!
//! `EditorCommandRequestMessage` 由 `xgent_agent` 桥接层从 `EditorTool`
//! （UI-only Tier，默认 Approved）的请求经 channel 转发到 ECS。
//! 本模块订阅并执行：切换到编辑器视图、打开标签、跳转行、关闭标签等。

use bevy::prelude::*;

use crate::editor::tabs::OpenFileRequest;
use xgent_agent::EditorCommandRequestMessage;

/// 订阅 EditorCommandRequestMessage 并执行。
///
/// MVP：OpenFile 转发为 `OpenFileRequest`（由 io + tabs 系统处理）；
/// GoTo/ScrollTo 直接更新激活 buffer 的 TextEditor cursor 并插 PendingGoTo；
/// CloseTab 转发为 CloseTabRequest。
///
/// **读取中跳转保护**：若激活 buffer 仍在异步读取（带 `FileReadPending`），
/// GoTo/ScrollTo 不插 `PendingGoTo`（此时 rope 为空、虚拟化占位高度 0，
/// `handle_pending_goto` 设的滚动量会被布局 clamp 回 0，跳转丢失）。
/// 改为更新 `FileReadPending.line`，让 `apply_file_read_results` 在读取完成
/// 后用最新行号插 `PendingGoTo`，保证跳转生效。
pub fn handle_editor_commands(
    mut reader: MessageReader<EditorCommandRequestMessage>,
    mut open_writer: MessageWriter<OpenFileRequest>,
    tabs: Res<crate::editor::tabs::EditorTabs>,
    q_buffers: Query<&crate::editor::buffer::EditorBuffer>,
    mut q_editors: Query<&mut xui::TextEditor>,
    mut q_pending: Query<&mut crate::editor::io::FileReadPending>,
    mut close_writer: MessageWriter<crate::editor::tabs::CloseTabRequest>,
    project_root: Option<Res<crate::file_panel::ProjectRoot>>,
    mut commands: Commands,
) {
    use xgent_tools::EditorCommandRequest as R;
    for msg in reader.read() {
        match &msg.0 {
            R::OpenFile { path, line } => {
                // 相对路径拼 project_root 转绝对路径，再 canonicalize 规范化
                // （消除 `./`、`../`、符号链接差异），避免同文件因路径表示
                // 不同被 find_by_path 判定为不同 buffer、开重复 tab。
                // canonicalize 要求路径存在；失败（文件不存在）保留原路径，
                // 由 io 系统报错。
                let abs_path = if path.is_absolute() {
                    path.clone()
                } else if let Some(root) = &project_root {
                    root.path.join(path)
                } else {
                    path.clone()
                };
                let normalized = abs_path.canonicalize().unwrap_or(abs_path);
                open_writer.write(OpenFileRequest {
                    path: normalized,
                    line: *line,
                });
            }
            R::GoTo { line, col } => {
                if let Some(active) = tabs.active_entity() {
                    if let Ok(mut editor) = q_editors.get_mut(active) {
                        editor.cursor = (*line, col.unwrap_or(1));
                    }
                    request_goto(&mut commands, &mut q_pending, active, *line);
                }
            }
            R::SetSelection { start: _, end: _ } => {
                // MVP：选区设置留待后续
            }
            R::ScrollTo { line } => {
                if let Some(active) = tabs.active_entity() {
                    if let Ok(mut editor) = q_editors.get_mut(active) {
                        editor.cursor = (*line, 1);
                    }
                    request_goto(&mut commands, &mut q_pending, active, *line);
                }
            }
            R::CloseTab { path } => {
                if let Some(entity) = tabs.find_by_path(path, &q_buffers) {
                    close_writer.write(crate::editor::tabs::CloseTabRequest {
                        entity,
                        force: false,
                    });
                }
            }
        }
    }
}

/// 提交跳转请求：buffer 仍在读取时更新 `FileReadPending.line`（读取完成后
/// 由 `apply_file_read_results` 据此插 `PendingGoTo`）；否则直接插 `PendingGoTo`。
fn request_goto(
    commands: &mut Commands,
    q_pending: &mut Query<&mut crate::editor::io::FileReadPending>,
    entity: Entity,
    line: usize,
) {
    if let Ok(mut pending) = q_pending.get_mut(entity) {
        // 仍在读取：覆盖跳转行，避免 rope 空时 PendingGoTo 被无效消费
        pending.line = Some(line);
    } else {
        commands
            .entity(entity)
            .insert(crate::editor::buffer::PendingGoTo { line });
    }
}
