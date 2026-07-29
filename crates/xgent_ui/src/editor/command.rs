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
pub fn handle_editor_commands(
    mut reader: MessageReader<EditorCommandRequestMessage>,
    mut open_writer: MessageWriter<OpenFileRequest>,
    tabs: Res<crate::editor::tabs::EditorTabs>,
    q_buffers: Query<&crate::editor::buffer::EditorBuffer>,
    mut q_editors: Query<&mut xui::TextEditor>,
    mut close_writer: MessageWriter<crate::editor::tabs::CloseTabRequest>,
    project_root: Option<Res<crate::file_panel::ProjectRoot>>,
    mut commands: Commands,
) {
    use xgent_tools::EditorCommandRequest as R;
    for msg in reader.read() {
        match &msg.0 {
            R::OpenFile { path, line } => {
                // 相对路径拼 project_root 转绝对路径
                let abs_path = if path.is_absolute() {
                    path.clone()
                } else if let Some(root) = &project_root {
                    root.path.join(path)
                } else {
                    path.clone()
                };
                open_writer.write(OpenFileRequest {
                    path: abs_path,
                    line: *line,
                });
            }
            R::GoTo { line, col } => {
                if let Some(active) = tabs.active_entity() {
                    if let Ok(mut editor) = q_editors.get_mut(active) {
                        editor.cursor = (*line, col.unwrap_or(1));
                        commands
                            .entity(active)
                            .insert(crate::editor::buffer::PendingGoTo { line: *line });
                    }
                }
            }
            R::SetSelection { start: _, end: _ } => {
                // MVP：选区设置留待后续
            }
            R::ScrollTo { line } => {
                if let Some(active) = tabs.active_entity() {
                    if let Ok(mut editor) = q_editors.get_mut(active) {
                        editor.cursor = (*line, 1);
                        commands
                            .entity(active)
                            .insert(crate::editor::buffer::PendingGoTo { line: *line });
                    }
                }
            }
            R::CloseTab { path } => {
                if let Some(entity) = tabs.find_by_path(path, &q_buffers) {
                    close_writer.write(crate::editor::tabs::CloseTabRequest { entity });
                }
            }
        }
    }
}
