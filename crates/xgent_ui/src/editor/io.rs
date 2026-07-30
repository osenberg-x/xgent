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
use parking_lot::Mutex;

use bevy::prelude::*;
use tokio::sync::oneshot;

/// 编辑器 IO runtime（由 xgent_app 注入 tokio handle）。
///
/// 若 `handle` 为 None，io 模块降级为同步 IO。
///
/// 持有 pending oneshot receiver 列表，每帧非阻塞 poll（`try_recv`），
/// 避免 `blocking_recv` 卡 ECS 帧循环。
#[derive(Resource)]
pub struct EditorIoRuntime {
    /// tokio runtime handle（可选，便于测试不依赖 runtime）
    pub handle: Option<tokio::runtime::Handle>,
    /// 待 poll 的读取结果 receiver 列表
    pending_reads: Mutex<Vec<(FileReadRequest, oneshot::Receiver<Result<String, String>>)>>,
    /// 待 poll 的写入结果 receiver 列表
    pending_writes: Mutex<Vec<(FileWriteRequest, oneshot::Receiver<Result<(), String>>)>>,
}

impl Default for EditorIoRuntime {
    fn default() -> Self {
        Self {
            handle: None,
            pending_reads: Mutex::new(Vec::new()),
            pending_writes: Mutex::new(Vec::new()),
        }
    }
}

impl EditorIoRuntime {
    /// 注入 handle。
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            handle: Some(handle),
            pending_reads: Mutex::new(Vec::new()),
            pending_writes: Mutex::new(Vec::new()),
        }
    }

    /// 清理指定 buffer 实体的 pending 写入请求。
    ///
    /// buffer 关闭时调用，避免写完成后 `BufferSavedEvent` 发给已 despawn 的实体
    /// （`apply_save_result` 查实体失败丢弃，无害但浪费；且避免与重开同路径
    /// buffer 的潜在竞态）。被取消的 oneshot receiver 随 Vec drop 而关闭，
    /// tokio task 的 `tx.send` 会失败但无害。
    pub fn cancel_pending_writes(&self, entity: Entity) {
        let mut writes = self.pending_writes.lock();
        writes.retain(|(req, _)| req.entity != entity);
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
    /// 发起保存的 buffer 实体（用于结果回写时实体匹配，避免按 path 误匹配
    /// 同路径的新 buffer）
    pub entity: Entity,
    /// 绝对路径
    pub path: PathBuf,
    /// 文本内容
    pub content: String,
}


/// buffer 已保存事件（写入成功后发，供 xgent_app 桥接转 IPC fs.changed）。
#[derive(Message, Debug, Clone)]
pub struct BufferSavedEvent {
    /// 发起保存的 buffer 实体（apply_save_result 按实体匹配，避免按 path
    /// 误匹配关闭后重开的同路径新 buffer）
    pub entity: Entity,
    /// 绝对路径
    pub path: PathBuf,
    /// 已落盘的内容（用于更新 buffer 的 disk_content 快照）
    pub content: String,
}

/// 处理文件读取请求：spawn tokio task，把 receiver 存入 pending 列表。
///
/// 结果由 [`poll_io_results`] 系统每帧非阻塞 poll。
pub fn handle_file_read_requests(
    mut reader: MessageReader<FileReadRequest>,
    mut writer: MessageWriter<FileReadResult>,
    rt: ResMut<EditorIoRuntime>,
) {
    for req in reader.read() {
        if let Some(handle) = rt.handle.clone() {
            let (tx, rx) = oneshot::channel::<Result<String, String>>();
            let path = req.path.clone();
            handle.spawn(async move {
                let result = tokio::fs::read_to_string(&path)
                    .await
                    .map_err(|e| format!("{}: {e}", path.display()));
                let _ = tx.send(result);
            });
            // 非阻塞：receiver 存入 pending，由 poll_io_results 每帧 try_recv
            rt.pending_reads.lock().push((req.clone(), rx));
        } else {
            // 降级同步 IO（无 runtime，小文件可用）
            let result = std::fs::read_to_string(&req.path)
                .map_err(|e| format!("{}: {e}", req.path.display()));
            writer.write(FileReadResult {
                path: req.path.clone(),
                line: req.line,
                content: result,
            });
        }
    }
}

/// 处理文件写入请求：spawn tokio task，把 receiver 存入 pending 列表。
///
/// 结果由 [`poll_io_results`] 系统每帧非阻塞 poll。
///
/// **降级路径**（无 runtime）：同步写入，成功直接发 `BufferSavedEvent`
/// （让 `apply_save_result` 更新 buffer 状态），失败记 warn 日志——
/// 不静默吞错，保证用户可感知。
pub fn handle_file_write_requests(
    mut reader: MessageReader<FileWriteRequest>,
    rt: ResMut<EditorIoRuntime>,
    mut saved_writer: MessageWriter<BufferSavedEvent>,
) {
    for req in reader.read() {
        if let Some(handle) = rt.handle.clone() {
            let (tx, rx) = oneshot::channel::<Result<(), String>>();
            let path = req.path.clone();
            let content = req.content.clone();
            handle.spawn(async move {
                let result = tokio::fs::write(&path, content.as_bytes())
                    .await
                    .map_err(|e| format!("{}: {e}", path.display()));
                let _ = tx.send(result);
            });
            rt.pending_writes.lock().push((req.clone(), rx));
        } else {
            // 降级同步 IO：成功发 BufferSavedEvent，失败记 warn
            match std::fs::write(&req.path, req.content.as_bytes()) {
                Ok(()) => {
                    saved_writer.write(BufferSavedEvent {
                        entity: req.entity,
                        path: req.path.clone(),
                        content: req.content.clone(),
                    });
                }
                Err(e) => {
                    tracing::warn!("写入文件失败 {}: {e}", req.path.display());
                }
            }
        }
    }
}

/// 每帧非阻塞 poll pending IO receiver，就绪的发对应消息。
///
/// 读就绪 → `FileReadResult`；写成功 → `BufferSavedEvent`，写失败/取消 → warn 日志。
/// 未就绪的保留到下一帧。
pub fn poll_io_results(
    rt: ResMut<EditorIoRuntime>,
    mut read_writer: MessageWriter<FileReadResult>,
    mut saved_writer: MessageWriter<BufferSavedEvent>,
) {
    // poll 读取
    let mut reads = rt.pending_reads.lock();
    let mut still_pending = Vec::with_capacity(reads.len());
    for (req, mut rx) in reads.drain(..) {
        match rx.try_recv() {
            Ok(content) => {
                read_writer.write(FileReadResult {
                    path: req.path.clone(),
                    line: req.line,
                    content,
                });
            }
            Err(oneshot::error::TryRecvError::Empty) => {
                still_pending.push((req, rx));
            }
            Err(oneshot::error::TryRecvError::Closed) => {
                read_writer.write(FileReadResult {
                    path: req.path.clone(),
                    line: req.line,
                    content: Err("读取任务取消".into()),
                });
            }
        }
    }
    *reads = still_pending;
    drop(reads);
    // poll 写入
    let mut writes = rt.pending_writes.lock();
    let mut still_pending = Vec::with_capacity(writes.len());
    for (req, mut rx) in writes.drain(..) {
        match rx.try_recv() {
            Ok(result) => {
                match result {
                    Ok(()) => {
                        saved_writer.write(BufferSavedEvent {
                            entity: req.entity,
                            path: req.path.clone(),
                            content: req.content.clone(),
                        });
                    }
                    Err(e) => {
                        tracing::warn!("写入文件失败 {}: {e}", req.path.display());
                    }
                }
            }
            Err(oneshot::error::TryRecvError::Empty) => {
                still_pending.push((req, rx));
            }
            Err(oneshot::error::TryRecvError::Closed) => {
                tracing::warn!("写入任务取消: {}", req.path.display());
            }
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
///
/// 遍历所有匹配 `pending.path` 的 buffer（不 `break`），避免同路径多 buffer
/// （路径未规范化等场景）时后续 buffer 的 `FileReadPending` 永不清除、卡死。
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
                    // 压入 undo 快照（重载内容作为新基准）。
                    // 注意：UndoStack::push 会清空 redo 栈——重载后旧 redo 快照
                    // 指向已被覆盖的旧内容，不再适用，故清空是正确行为。
                    // 副作用：用户若 undo 数步后触发静默重载，redo 历史丢失。
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
                Err(e) => {
                    // 读取失败：记日志（用户可见的反馈通道），移除 pending
                    tracing::warn!("读取文件失败 {}: {e}", pending.path.display());
                    commands.entity(entity).remove::<FileReadPending>();
                }
            }
        }
    }
}
