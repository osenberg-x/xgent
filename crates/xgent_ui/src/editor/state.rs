//! EditorState Resource（impl `xgent_core::EditorState` trait）。
//!
//! 详见 `doc/design/editor-design.md` 第 6.3 节。
//!
//! `xgent_context` 经 trait 查询编辑器状态，`xgent_ui` 实现该 trait。
//! 反转依赖避免 `xgent_context` 依赖 `xgent_ui`（成环）。

use std::path::{Path, PathBuf};

use bevy::prelude::*;

use xgent_core::editor::{BufferStatus, EditorState as EditorStateTrait};

use crate::editor::buffer::EditorBuffer;
use crate::editor::tabs::EditorTabs;

/// 编辑器状态只读视图 Resource（impl `xgent_core::EditorState`）。
///
/// 作为 Resource 供 `ContextProvider` 查询。实际数据从 `EditorTabs` + `EditorBuffer`
/// 组件查询派生——本 Resource 只持有查询所需的 ECS 访问句柄。
///
/// 注意：trait 方法需要访问 ECS 查询，而 trait 定义在 `xgent_core` 无 Bevy 依赖。
/// 故采用"快照"模式：每帧由系统把当前编辑器状态快照写入此 Resource，
/// `ContextProvider` 读快照而非直接查 ECS。
#[derive(Resource, Debug, Default, Clone)]
pub struct EditorStateSnapshot {
    /// 当前活跃 buffer 路径
    pub active_path: Option<PathBuf>,
    /// 当前光标位置（行，列，1-based）
    pub cursor: Option<(usize, usize)>,
    /// 当前选区文本
    pub selection: Option<String>,
    /// 所有打开 buffer 的状态
    pub buffers: Vec<(PathBuf, BufferStatus)>,
}

impl EditorStateTrait for EditorStateSnapshot {
    fn active_path(&self) -> Option<&Path> {
        self.active_path.as_deref()
    }

    fn cursor(&self) -> Option<(usize, usize)> {
        self.cursor
    }

    fn selection(&self) -> Option<&str> {
        self.selection.as_deref()
    }

    fn buffer_status(&self, path: &Path) -> Option<BufferStatus> {
        self.buffers
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, s)| *s)
    }
}

/// 每帧更新 `EditorStateSnapshot`（供 ContextProvider 查询）。
pub fn update_editor_state_snapshot(
    mut snapshot: ResMut<EditorStateSnapshot>,
    tabs: Res<EditorTabs>,
    q_buffers: Query<(&EditorBuffer, &xui::TextEditor)>,
) {
    snapshot.active_path = None;
    snapshot.cursor = None;
    snapshot.selection = None;
    snapshot.buffers.clear();

    for &e in &tabs.tabs {
        let Ok((buf, _editor)) = q_buffers.get(e) else {
            continue;
        };
        let status = BufferStatus {
            open: true,
            dirty: buf.state.is_dirty(),
        };
        snapshot.buffers.push((buf.path.clone(), status));
    }

    if let Some(active) = tabs.active_entity() {
        if let Ok((buf, editor)) = q_buffers.get(active) {
            snapshot.active_path = Some(buf.path.clone());
            snapshot.cursor = Some(editor.cursor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_default_has_no_active() {
        let s = EditorStateSnapshot::default();
        assert!(s.active_path.is_none());
        assert!(s.cursor.is_none());
        assert!(s.selection.is_none());
        assert!(s.buffers.is_empty());
    }

    #[test]
    fn trait_impl_active_path() {
        let mut s = EditorStateSnapshot::default();
        s.active_path = Some(PathBuf::from("/x/main.rs"));
        assert_eq!(s.active_path(), Some(Path::new("/x/main.rs")));
    }

    #[test]
    fn trait_impl_cursor() {
        let mut s = EditorStateSnapshot::default();
        s.cursor = Some((3, 5));
        assert_eq!(s.cursor(), Some((3, 5)));
    }

    #[test]
    fn trait_impl_buffer_status() {
        let mut s = EditorStateSnapshot::default();
        s.buffers.push((
            PathBuf::from("/x"),
            BufferStatus {
                open: true,
                dirty: true,
            },
        ));
        let st = s.buffer_status(Path::new("/x")).unwrap();
        assert!(st.open);
        assert!(st.dirty);
        assert!(s.buffer_status(Path::new("/other")).is_none());
    }
}
