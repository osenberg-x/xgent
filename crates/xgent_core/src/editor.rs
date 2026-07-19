//! 编辑器状态共享类型。
//!
//! 定义在 `xgent_core` 以反转依赖：`xgent_context` 经 trait 查询编辑器状态，
//! `xgent_ui` 实现该 trait，避免 `xgent_context` 依赖 `xgent_ui`（成环）。
//!
//! 对齐 `xui_i18n::StringSource` 的反转依赖模式。
//!
//! 详见 `doc/design/editor-design.md` 第 6.3 节。

use std::path::{Path, PathBuf};

/// 编辑器状态只读视图，供 `ContextProvider` 查询。
///
/// 定义在 `xgent_core`，`xgent_ui` 实现，`xgent_context` 经 trait 调用——
/// 避免 `xgent_context` 依赖 `xgent_ui`（成环）。
pub trait EditorState: Send + Sync {
    /// 当前活跃 buffer 的路径
    fn active_path(&self) -> Option<&Path>;
    /// 当前光标位置（行，列）
    fn cursor(&self) -> Option<(usize, usize)>;
    /// 当前选区文本（若有）
    fn selection(&self) -> Option<&str>;
    /// 指定路径 buffer 是否存在 + 是否脏
    fn buffer_status(&self, path: &Path) -> Option<BufferStatus>;
}

/// buffer 状态只读视图。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferStatus {
    /// 是否已打开
    pub open: bool,
    /// 是否脏（有未保存修改）
    pub dirty: bool,
}

/// `ContextProvider` 查询 @ 引用的载荷。
///
/// 由 `xgent_ui::editor::at_syntax::parse_at_references` 从用户输入解析得到，
/// 注入 `ContextQuery` 供 `ContextProvider` 处理。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorQuery {
    /// `@file:<path>`：拉取该文件内容作为上下文
    File {
        /// 文件路径
        path: PathBuf,
    },
    /// `@cursor`：拉取当前光标位置所在符号 + 周边若干行
    Cursor,
    /// `@selection`：拉取当前选区文本
    Selection,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_status_fields() {
        let b = BufferStatus { open: true, dirty: false };
        assert!(b.open);
        assert!(!b.dirty);
    }

    #[test]
    fn editor_query_variants() {
        let q = EditorQuery::Cursor;
        assert_eq!(q, EditorQuery::Cursor);
        let q = EditorQuery::Selection;
        assert_eq!(q, EditorQuery::Selection);
        let q = EditorQuery::File { path: PathBuf::from("src/main.rs") };
        assert_eq!(q.path(), std::path::Path::new("src/main.rs"));
    }

    /// 辅助：File 变体取路径（仅用于测试）。
    impl EditorQuery {
        fn path(&self) -> &Path {
            match self {
                EditorQuery::File { path } => path,
                _ => Path::new(""),
            }
        }
    }
}
