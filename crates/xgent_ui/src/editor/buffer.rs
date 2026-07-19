//! EditorBuffer 组件与状态机。
//!
//! 详见 `doc/design/editor-design.md` 第 4.1 节。
//!
//! # 状态机
//!
//! ```text
//!         open file (read)
//! Clean ────────────────────→ Dirty
//!   ↑                           │
//!   │ save (fs::write)          │ 外部修改
//!   │                           ↓
//!   │                      ConflictDetected
//!   │                           │
//!   │                           ├─ 丢弃本地 ─→ Clean（重载）
//!   │                           ├─ 保留本地 ─→ Dirty(LocalPreferred)
//!   │                           └─ 对比合并 ─→ Dirty（用户手动取舍后）
//!   └──────────────────────────
//! ```

use std::path::{Path, PathBuf};

use bevy::prelude::*;

/// buffer 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BufferState {
    /// buffer 与磁盘一致
    #[default]
    Clean,
    /// 有未保存本地修改
    Dirty,
    /// 脏 buffer + 外部修改到达，等待用户决策
    ConflictDetected,
    /// 用户选保留本地，下次保存覆盖外部
    LocalPreferred,
}

impl BufferState {
    /// 是否脏（有未保存修改）。
    pub fn is_dirty(self) -> bool {
        matches!(self, BufferState::Dirty | BufferState::ConflictDetected | BufferState::LocalPreferred)
    }
}

/// EditorBuffer 组件：一个打开的文件 buffer。
///
/// 文本内容存储在同实体的 `bevy::text::EditableText` 中；
/// 本组件只持有元数据（路径、状态、磁盘内容快照）。
#[derive(Component, Debug)]
pub struct EditorBuffer {
    /// 文件绝对路径
    pub path: PathBuf,
    /// buffer 状态机
    pub state: BufferState,
    /// 上次加载/保存时的磁盘内容（用于冲突检测与静默重载比较）
    pub disk_content: String,
}

impl EditorBuffer {
    /// 构造：刚从磁盘读取，状态 Clean。
    pub fn from_disk(path: PathBuf, content: String) -> Self {
        Self {
            path,
            state: BufferState::Clean,
            disk_content: content,
        }
    }

    /// 标记为脏（用户编辑）。
    pub fn mark_dirty(&mut self) {
        if self.state == BufferState::Clean {
            self.state = BufferState::Dirty;
        }
    }

    /// 标记为已保存（落盘后）。
    pub fn mark_saved(&mut self, content: &str) {
        self.disk_content = content.to_string();
        self.state = BufferState::Clean;
    }

    /// 检测外部修改：当前磁盘内容与 `disk_content` 不同则有外部修改。
    pub fn detect_external_change(&self, current_disk: &str) -> bool {
        current_disk != self.disk_content
    }

    /// 进入冲突检测态（脏 + 外部修改到达）。
    pub fn enter_conflict(&mut self) {
        if self.state == BufferState::Dirty {
            self.state = BufferState::ConflictDetected;
        }
    }

    /// 丢弃本地修改，重载磁盘内容。
    pub fn reload(&mut self, content: &str) {
        self.disk_content = content.to_string();
        self.state = BufferState::Clean;
    }

    /// 保留本地，标记 LocalPreferred。
    pub fn keep_local(&mut self) {
        self.state = BufferState::LocalPreferred;
    }

    /// 路径引用。
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_disk_is_clean() {
        let b = EditorBuffer::from_disk(PathBuf::from("/x"), "hi".into());
        assert_eq!(b.state, BufferState::Clean);
        assert!(!b.state.is_dirty());
    }

    #[test]
    fn mark_dirty_transitions_clean_to_dirty() {
        let mut b = EditorBuffer::from_disk(PathBuf::from("/x"), "hi".into());
        b.mark_dirty();
        assert_eq!(b.state, BufferState::Dirty);
        assert!(b.state.is_dirty());
    }

    #[test]
    fn mark_dirty_does_not_override_conflict() {
        let mut b = EditorBuffer::from_disk(PathBuf::from("/x"), "hi".into());
        b.state = BufferState::ConflictDetected;
        b.mark_dirty();
        assert_eq!(b.state, BufferState::ConflictDetected);
    }

    #[test]
    fn save_clears_dirty_and_updates_disk() {
        let mut b = EditorBuffer::from_disk(PathBuf::from("/x"), "hi".into());
        b.mark_dirty();
        b.mark_saved("new");
        assert_eq!(b.state, BufferState::Clean);
        assert_eq!(b.disk_content, "new");
    }

    #[test]
    fn detect_external_change() {
        let b = EditorBuffer::from_disk(PathBuf::from("/x"), "hi".into());
        assert!(!b.detect_external_change("hi"));
        assert!(b.detect_external_change("changed"));
    }

    #[test]
    fn enter_conflict_only_from_dirty() {
        let mut b = EditorBuffer::from_disk(PathBuf::from("/x"), "hi".into());
        b.enter_conflict();
        assert_eq!(b.state, BufferState::Clean); // Clean 不进冲突
        b.mark_dirty();
        b.enter_conflict();
        assert_eq!(b.state, BufferState::ConflictDetected);
    }

    #[test]
    fn reload_resets_to_clean() {
        let mut b = EditorBuffer::from_disk(PathBuf::from("/x"), "hi".into());
        b.mark_dirty();
        b.reload("newdisk");
        assert_eq!(b.state, BufferState::Clean);
        assert_eq!(b.disk_content, "newdisk");
    }

    #[test]
    fn keep_local_sets_local_preferred() {
        let mut b = EditorBuffer::from_disk(PathBuf::from("/x"), "hi".into());
        b.state = BufferState::ConflictDetected;
        b.keep_local();
        assert_eq!(b.state, BufferState::LocalPreferred);
        assert!(b.state.is_dirty());
    }

    #[test]
    fn local_preferred_saved_to_clean() {
        let mut b = EditorBuffer::from_disk(PathBuf::from("/x"), "hi".into());
        b.state = BufferState::LocalPreferred;
        b.mark_saved("overwritten");
        assert_eq!(b.state, BufferState::Clean);
    }
}

/// 标记 buffer 等待异步文件读取完成（路径+行号）。
#[derive(Component, Debug, Clone)]
pub struct PendingRead {
    /// 文件绝对路径
    pub path: PathBuf,
    /// 可选跳转行号
    pub line: Option<usize>,
}

/// 标记 buffer 需要跳转到某行（异步读取完成后处理）。
#[derive(Component, Debug, Clone)]
pub struct PendingGoTo {
    /// 行号（1-based）
    pub line: usize,
}
