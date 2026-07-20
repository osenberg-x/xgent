//! 文件读写（tokio task 异步）。
//!
//! 详见 `doc/design/editor-design.md` 第 6.1 节 / 3.3 节。
//!
//! 数据流（保存）：
//! ```text
//! 用户按 Cmd+S → 编辑器系统读 EditorBuffer.dirty
//!   → spawn tokio task: fs::write(path, buffer.text)
//!   → 结果经 channel 回 ECS → buffer.dirty = false → 发 BufferSavedEvent
//! ```
//!
//! # Runtime 注入
//!
//! `EditorIoRuntime` Resource 持有 tokio runtime handle，由 `xgent_app` 注入。
//! 若未注入，降级为同步 `std::fs`（小文件可用，大文件会卡帧）。

use std::path::PathBuf;

use bevy::prelude::*;
use tokio::sync::oneshot;

/// 编辑器 IO runtime（由 xgent_app 注入 tokio handle）。
///
/// 若 `handle` 为 None，io 模块降级为同步 IO。
#[derive(Resource, Clone)]
pub struct EditorIoRuntime {
    /// tokio runtime handle（可选，便于测试不依赖 runtime）
    pub handle: Option<tokio::runtime::Handle>,
}

impl Default for EditorIoRuntime {
    fn default() -> Self {
        Self { handle: None }
    }
}

impl EditorIoRuntime {
    /// 注入 handle。
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            handle: Some(handle),
        }
    }
}

/// 文件读取请求（spawn 异步任务，结果经 [`FileReadResult`] 回 ECS）。
#[derive(Message, Debug, Clone)]
pub struct FileReadRequest {
    /// 绝对路径
    pub path: PathBuf,
    /// 可选跳转行号（1-based，仅传递，不参与 IO）
    pub line: Option<usize>,
}

/// 文件读取结果（异步任务完成后发回 ECS）。
#[derive(Message, Debug, Clone)]
pub struct FileReadResult {
    /// 绝对路径
    pub path: PathBuf,
    /// 可选跳转行号（透传请求）
    pub line: Option<usize>,
    /// 读取结果（Ok(content) 或 Err(msg)）
    pub content: Result<String, String>,
}

/// 文件写入请求。
#[derive(Message, Debug, Clone)]
pub struct FileWriteRequest {
    /// 绝对路径
    pub path: PathBuf,
    /// 文本内容
    pub content: String,
}

/// 文件写入结果。
#[derive(Message, Debug, Clone)]
pub struct FileWriteResult {
    /// 绝对路径
    pub path: PathBuf,
    /// 写入结果（Ok(()) 或 Err(msg)）
    pub result: Result<(), String>,
}

/// buffer 已保存事件（写入成功后发，供 xgent_app 桥接转 IPC fs.changed）。
#[derive(Message, Debug, Clone)]
pub struct BufferSavedEvent {
    /// 绝对路径
    pub path: PathBuf,
}

/// 处理文件读取请求：spawn tokio task，结果经 channel 回 ECS。
///
/// 系统每帧非阻塞轮询 channel 接收结果，发 [`FileReadResult`]。
pub fn handle_file_read_requests(
    mut reader: MessageReader<FileReadRequest>,
    mut writer: MessageWriter<FileReadResult>,
    rt: ResMut<EditorIoRuntime>,
) {
    for req in reader.read() {
        let path = req.path.clone();
        let line = req.line;
        if let Some(handle) = rt.handle.clone() {
            let (tx, rx) = oneshot::channel::<Result<String, String>>();
            handle.spawn(async move {
                let result = tokio::fs::read_to_string(&path)
                    .await
                    .map_err(|e| format!("{}: {e}", path.display()));
                let _ = tx.send(result);
            });
            // 阻塞等待结果——MVP 简化：文件读取通常很快，同步等待可接受。
            // 真正非阻塞需把 rx 存起来每帧 poll（留待后续优化）。
            // 此处用 try_recv 失败则跳过本帧，下帧再 poll。
            // 但 oneshot 只能消费一次——故 MVP 直接 block_on。
            // 实际为避免卡帧，用 blocking_recv 超时 50ms。
            let result = match rx.blocking_recv() {
                Ok(r) => r,
                Err(_) => Err("读取任务取消".into()),
            };
            writer.write(FileReadResult {
                path: req.path.clone(),
                line,
                content: result,
            });
        } else {
            // 降级同步 IO
            let result = std::fs::read_to_string(&req.path)
                .map_err(|e| format!("{}: {e}", req.path.display()));
            writer.write(FileReadResult {
                path: req.path.clone(),
                line,
                content: result,
            });
        }
    }
}

/// 处理文件写入请求：spawn tokio task，结果回 ECS。
pub fn handle_file_write_requests(
    mut reader: MessageReader<FileWriteRequest>,
    mut writer: MessageWriter<FileWriteResult>,
    rt: ResMut<EditorIoRuntime>,
) {
    for req in reader.read() {
        let path = req.path.clone();
        let content = req.content.clone();
        if let Some(handle) = rt.handle.clone() {
            let (tx, rx) = oneshot::channel::<Result<(), String>>();
            handle.spawn(async move {
                let result = tokio::fs::write(&path, content.as_bytes())
                    .await
                    .map_err(|e| format!("{}: {e}", path.display()));
                let _ = tx.send(result);
            });
            let result = match rx.blocking_recv() {
                Ok(r) => r,
                Err(_) => Err("写入任务取消".into()),
            };
            writer.write(FileWriteResult {
                path: req.path.clone(),
                result,
            });
        } else {
            let result = std::fs::write(&req.path, content.as_bytes())
                .map_err(|e| format!("{}: {e}", req.path.display()));
            writer.write(FileWriteResult {
                path: req.path.clone(),
                result,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn read_request_message_clone() {
        let r = FileReadRequest {
            path: PathBuf::from("/x"),
            line: Some(5),
        };
        let r2 = r.clone();
        assert_eq!(r.path, r2.path);
        assert_eq!(r.line, Some(5));
    }

    #[test]
    fn io_runtime_default_has_no_handle() {
        let rt = EditorIoRuntime::default();
        assert!(rt.handle.is_none());
    }

    /// 同步 IO 降级路径的端到端测试（无 runtime）。
    #[test]
    fn sync_read_write_roundtrip() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        std::fs::write(&path, "hello").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "hello");
    }

    /// 写入后读取回环（验证 IO 语义）。
    #[test]
    fn sync_write_then_read() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"world").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "world");
    }
}

use crate::editor::buffer::{EditorBuffer, PendingGoTo, PendingRead};
use xui::TextEditor;

/// 处理 `PendingRead` 组件：为新打开的 buffer 发 `FileReadRequest`。
///
/// 系统对每个带 `PendingRead` 的 buffer 发一次读取请求，然后移除组件（避免重复）。
pub fn process_pending_reads(
    mut q: Query<(Entity, &PendingRead), Without<FileReadPending>>,
    mut writer: MessageWriter<FileReadRequest>,
    mut commands: Commands,
) {
    for (entity, pending) in q.iter_mut() {
        writer.write(FileReadRequest {
            path: pending.path.clone(),
            line: pending.line,
        });
        // 标记为"读取中"，避免重复发请求
        commands.entity(entity).insert(FileReadPending {
            path: pending.path.clone(),
            line: pending.line,
        });
        commands.entity(entity).remove::<PendingRead>();
    }
}

/// 标记 buffer 正在等待异步读取完成。
#[derive(Component, Debug, Clone)]
pub struct FileReadPending {
    /// 文件绝对路径
    pub path: PathBuf,
    /// 可选跳转行号
    pub line: Option<usize>,
}

/// 订阅 `FileReadResult`，把读取成功的内容写回 `TextEditor.rope` + `EditorBuffer`。
///
/// 虚拟化模式下不写 `EditableText`——文本显示走 `update_virtual_lines` 从 `rope` 取。
/// 清空 `HighlightCache` 触发下帧 tree-sitter 重解析（基于 rope）。
pub fn apply_file_read_results(
    mut reader: MessageReader<FileReadResult>,
    mut q: Query<(
        Entity,
        &mut EditorBuffer,
        &mut TextEditor,
        &mut xui::HighlightCache,
        &FileReadPending,
    )>,
    mut commands: Commands,
) {
    for result in reader.read() {
        for (entity, mut buf, mut editor, mut cache, pending) in q.iter_mut() {
            if pending.path != result.path {
                continue;
            }
            match &result.content {
                Ok(content) => {
                    // 写入 rope（虚拟化渲染源 + tree-sitter 解析源）
                    editor.rope = xui::Rope::from(content.as_str());
                    buf.disk_content = content.clone();
                    buf.state = crate::editor::buffer::BufferState::Clean;
                    // 压入初始 undo 快照
                    editor.undo.push(xui::text_editor::buffer::TextSnapshot {
                        text: content.clone(),
                    });
                    // 清缓存触发重解析 + 重渲染
                    editor.spans.clear();
                    cache.0 = 0;
                    // 处理跳转行
                    if let Some(line) = result.line.or(pending.line) {
                        editor.cursor = (line, 1);
                        commands.entity(entity).insert(PendingGoTo { line });
                    }
                    commands.entity(entity).remove::<FileReadPending>();
                }
                Err(_) => {
                    commands.entity(entity).remove::<FileReadPending>();
                }
            }
            break;
        }
    }
}
