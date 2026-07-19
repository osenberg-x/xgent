//! 文本缓冲抽象与快照类型。
//!
//! `EditorText` 是 TextEditor 的文本载体（基于 `Rope`，与 `EditableText` 同步）。
//! `TextSnapshot` 用于 undo/redo 栈——基于文本快照（简单可靠，MVP 不做操作级 diff）。

pub use ropey::Rope;

use bevy::prelude::*;

/// 编辑器文本快照（undo/redo 栈条目）。
///
/// MVP 用文本快照而非操作日志：实现简单、可靠，代价是内存占用 O(历史 × 文本大小)。
/// 大文件场景可后续升级为操作级 diff（设计文档 5.3 节）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSnapshot {
    /// 快照时的完整文本
    pub text: String,
}

/// 编辑器文本载体（占位组件，标记含编辑器文本的实体）。
///
/// 实际文本存储在 `bevy::text::EditableText` 中；本组件仅用于业务层
/// 区分编辑器文本节点与普通文本节点。
#[derive(Component, Default)]
pub struct EditorText;
