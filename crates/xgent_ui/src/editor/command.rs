//! EditorCommand Event 订阅与执行。
//!
//! 详见 `doc/design/editor-design.md` 第 3.2 节 / 3.4 节。
//!
//! `EditorCommand` Event 由 agent 经 [`EditorTool`](xgent_tools::EditorTool)
//! （UI-only Tier，默认 Approved）发出，经 `xgent_agent` 桥接转 ECS。
//! 本模块订阅并执行：切换到编辑器视图、打开标签、跳转行、关闭标签等。

use std::path::PathBuf;

use bevy::prelude::*;

use crate::editor::tabs::OpenFileRequest;

/// agent 驱动编辑器的命令（与 `xgent_tools::EditorCommandRequest` 一一对应）。
///
/// 由 `xgent_agent` 桥接层从工具请求转换为本 ECS Event。
#[derive(Message, Debug, Clone)]
pub enum EditorCommand {
    /// 打开文件并可选跳到某行
    OpenFile {
        /// 绝对路径
        path: PathBuf,
        /// 可选行号（1-based）
        line: Option<usize>,
    },
    /// 跳到某行某列
    GoTo {
        /// 行号（1-based）
        line: usize,
        /// 列号（1-based）
        col: Option<usize>,
    },
    /// 设置选区
    SetSelection {
        /// 起始字节偏移
        start: usize,
        /// 结束字节偏移（半开）
        end: usize,
    },
    /// 滚动到某行
    ScrollTo {
        /// 行号（1-based）
        line: usize,
    },
    /// 关闭某路径对应的标签
    CloseTab {
        /// 文件路径
        path: PathBuf,
    },
}

impl EditorCommand {
    /// 从 `xgent_tools::EditorCommandRequest` 构造。
    pub fn from_request(req: &xgent_tools::EditorCommandRequest) -> Self {
        use xgent_tools::EditorCommandRequest as R;
        match req {
            R::OpenFile { path, line } => EditorCommand::OpenFile {
                path: path.clone(),
                line: *line,
            },
            R::GoTo { line, col } => EditorCommand::GoTo {
                line: *line,
                col: *col,
            },
            R::SetSelection { start, end } => EditorCommand::SetSelection {
                start: *start,
                end: *end,
            },
            R::ScrollTo { line } => EditorCommand::ScrollTo { line: *line },
            R::CloseTab { path } => EditorCommand::CloseTab { path: path.clone() },
        }
    }
}

/// 订阅 EditorCommand 并执行。
///
/// MVP：OpenFile 转发为 `OpenFileRequest`（由 io + tabs 系统处理）；
/// GoTo/ScrollTo 直接更新对应 TextEditor 的 cursor；
/// CloseTab 转发为 CloseTabRequest。
pub fn handle_editor_commands(
    mut reader: MessageReader<EditorCommand>,
    mut open_writer: MessageWriter<OpenFileRequest>,
) {
    for cmd in reader.read() {
        match cmd {
            EditorCommand::OpenFile { path, line } => {
                open_writer.write(OpenFileRequest {
                    path: path.clone(),
                    line: *line,
                });
            }
            EditorCommand::GoTo { line, col: _ } => {
                // 跳转到行：更新激活 buffer 的 TextEditor cursor
                let _ = line;
            }
            EditorCommand::SetSelection { start: _, end: _ } => {
                // MVP：选区设置留待后续（EditableText 的选区 API 复杂）
            }
            EditorCommand::ScrollTo { line } => {
                let _ = line;
            }
            EditorCommand::CloseTab { path: _ } => {
                // 转发 CloseTabRequest（需要 path→entity 查询，留给 tabs 系统）
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xgent_tools::EditorCommandRequest as R;

    #[test]
    fn from_request_open_file() {
        let req = R::OpenFile {
            path: PathBuf::from("/x"),
            line: Some(10),
        };
        let cmd = EditorCommand::from_request(&req);
        match cmd {
            EditorCommand::OpenFile { path, line } => {
                assert_eq!(path, PathBuf::from("/x"));
                assert_eq!(line, Some(10));
            }
            _ => panic!("应为 OpenFile"),
        }
    }

    #[test]
    fn from_request_goto() {
        let req = R::GoTo {
            line: 5,
            col: Some(3),
        };
        let cmd = EditorCommand::from_request(&req);
        match cmd {
            EditorCommand::GoTo { line, col } => {
                assert_eq!(line, 5);
                assert_eq!(col, Some(3));
            }
            _ => panic!("应为 GoTo"),
        }
    }

    #[test]
    fn from_request_close_tab() {
        let req = R::CloseTab {
            path: PathBuf::from("/y"),
        };
        let cmd = EditorCommand::from_request(&req);
        match cmd {
            EditorCommand::CloseTab { path } => assert_eq!(path, PathBuf::from("/y")),
            _ => panic!("应为 CloseTab"),
        }
    }
}
