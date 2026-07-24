//! 多标签页管理。
//!
//! 详见 `doc/design/editor-design.md` 第 6.2 节 / 2.2 节。
//!
//! 每个标签对应一个 EditorBuffer 实体。`EditorTabs` Resource 跟踪所有打开的
//! buffer 实体与当前激活标签，提供打开/关闭/切换操作。
//!
//! 不含 split view（与中等能力边界一致）。

use std::path::PathBuf;

use bevy::prelude::*;

use crate::editor::buffer::EditorBuffer;
use crate::theme::px;

/// 多标签页管理 Resource。
#[derive(Resource, Debug, Default)]
pub struct EditorTabs {
    /// 打开的 buffer 实体列表（按打开顺序）
    pub tabs: Vec<Entity>,
    /// 当前激活标签下标（None 表示无激活）
    pub active: Option<usize>,
}

impl EditorTabs {
    /// 查找指定路径已打开的 buffer 实体。
    pub fn find_by_path(
        &self,
        path: &std::path::Path,
        buffers: &Query<&EditorBuffer>,
    ) -> Option<Entity> {
        for &e in &self.tabs {
            if let Ok(buf) = buffers.get(e) {
                if buf.path() == path {
                    return Some(e);
                }
            }
        }
        None
    }

    /// 注册一个新打开的 buffer 实体，设为激活。
    pub fn open(&mut self, entity: Entity) {
        if !self.tabs.contains(&entity) {
            self.tabs.push(entity);
        }
        self.active = Some(self.tabs.iter().position(|&e| e == entity).unwrap());
    }

    /// 关闭标签，返回需 despawn 的实体与新的激活下标。
    pub fn close(&mut self, entity: Entity) -> Option<(Entity, Option<usize>)> {
        let idx = self.tabs.iter().position(|&e| e == entity)?;
        self.tabs.remove(idx);
        let new_active = if self.tabs.is_empty() {
            None
        } else if idx == 0 {
            Some(0)
        } else {
            Some(idx - 1)
        };
        self.active = new_active;
        Some((entity, new_active))
    }

    /// 切换到下一个标签（循环）。
    pub fn next(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        let i = self.active.unwrap_or(0);
        self.active = Some((i + 1) % self.tabs.len());
    }

    /// 切换到上一个标签（循环）。
    pub fn prev(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        let i = self.active.unwrap_or(0);
        self.active = Some((i + self.tabs.len() - 1) % self.tabs.len());
    }

    /// 激活标签的实体。
    pub fn active_entity(&self) -> Option<Entity> {
        self.active.and_then(|i| self.tabs.get(i).copied())
    }

    /// 标签数。
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// 是否无标签。
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

/// 标签条 UI 节点标记（含子标签按钮）。
#[derive(Component, Default)]
pub struct EditorTabBarMarker;

/// 单个标签按钮标记（挂在其对应 buffer 实体上或独立实体）。
#[derive(Component)]
pub struct EditorTabMarker {
    /// 此标签对应的 buffer 实体
    pub buffer: Entity,
}

/// 打开文件请求（由命令面板/文件面板点击/EditorTool 触发）。
#[derive(Message, Debug, Clone)]
pub struct OpenFileRequest {
    /// 文件绝对路径
    pub path: PathBuf,
    /// 可选跳转行号（1-based）
    pub line: Option<usize>,
}

/// 关闭标签请求。
#[derive(Message, Debug, Clone)]
pub struct CloseTabRequest {
    /// buffer 实体
    pub entity: Entity,
}

/// 循环切换标签请求（Cmd+Tab）。
#[derive(Message, Debug, Clone)]
pub struct CycleTabRequest {
    /// true=下一个，false=上一个
    pub forward: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_sets_active() {
        let mut t = EditorTabs::default();
        let e1 = Entity::from_raw_u32(1).unwrap();
        let e2 = Entity::from_raw_u32(2).unwrap();
        t.open(e1);
        assert_eq!(t.active, Some(0));
        t.open(e2);
        assert_eq!(t.active, Some(1));
        // 重复 open 已存在实体，激活回到它
        t.open(e1);
        assert_eq!(t.active, Some(0));
    }

    #[test]
    fn close_returns_entity_and_new_active() {
        let mut t = EditorTabs::default();
        let e1 = Entity::from_raw_u32(1).unwrap();
        let e2 = Entity::from_raw_u32(2).unwrap();
        t.open(e1);
        t.open(e2);
        let r = t.close(e2).unwrap();
        assert_eq!(r.0, e2);
        assert_eq!(r.1, Some(0));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn close_last_tab_clears_active() {
        let mut t = EditorTabs::default();
        let e1 = Entity::from_raw_u32(1).unwrap();
        t.open(e1);
        let r = t.close(e1).unwrap();
        assert_eq!(r.0, e1);
        assert_eq!(r.1, None);
        assert!(t.is_empty());
    }

    #[test]
    fn next_wraps_around() {
        let mut t = EditorTabs::default();
        let e1 = Entity::from_raw_u32(1).unwrap();
        let e2 = Entity::from_raw_u32(2).unwrap();
        t.open(e1);
        t.open(e2);
        t.next();
        assert_eq!(t.active, Some(0)); // (1+1)%2 = 0
        t.next();
        assert_eq!(t.active, Some(1));
    }

    #[test]
    fn prev_wraps_around() {
        let mut t = EditorTabs::default();
        let e1 = Entity::from_raw_u32(1).unwrap();
        let e2 = Entity::from_raw_u32(2).unwrap();
        t.open(e1);
        t.open(e2);
        // active = Some(1)（e2）。prev: (1+2-1)%2 = 0 → e1
        t.prev();
        assert_eq!(t.active, Some(0));
        // 再 prev: (0+2-1)%2 = 1 → e2（wrap）
        t.prev();
        assert_eq!(t.active, Some(1));
    }

    #[test]
    fn active_entity_returns_correct() {
        let mut t = EditorTabs::default();
        let e1 = Entity::from_raw_u32(1).unwrap();
        t.open(e1);
        assert_eq!(t.active_entity(), Some(e1));
    }
}

/// 处理打开文件请求：若已打开则激活，否则 spawn 新 buffer + TextEditor。
///
/// 文件实际读取由 io 系统异步完成；此处先 spawn buffer 实体并切换视图。
/// buffer 实体挂到 `EditorAreaMarker` 容器下，随编辑器视图 `Display` 切换显隐。
pub fn handle_open_file_requests(
    mut reader: MessageReader<OpenFileRequest>,
    mut tabs: ResMut<EditorTabs>,
    q_buffers: Query<&crate::editor::buffer::EditorBuffer>,
    q_area: Query<Entity, With<crate::editor::EditorAreaMarker>>,
    mut view: ResMut<crate::editor::EditorView>,
    mut content: ResMut<crate::editor::SideViewContent>,
    mut commands: Commands,
) {
    for req in reader.read() {
        if let Some(entity) = tabs.find_by_path(&req.path, &q_buffers) {
            tabs.open(entity);
            if let Some(line) = req.line {
                commands
                    .entity(entity)
                    .insert(crate::editor::buffer::PendingGoTo { line });
            }
        } else {
            // spawn 新 buffer：滚动容器（契约由 `xui::ScrollArea` 通用提供）+
            // 虚拟化占位子节点。文本显示走 `update_virtual_lines` 动态 spawn
            // 可见行；EditableText 暂不挂（编辑功能留后续，当前聚焦查看+滚动+性能）。
            let buffer_entity = commands
                .spawn((
                    xui::ScrollArea::vertical(),
                    xui::Scrollbar::default(),
                    crate::editor::buffer::EditorBuffer::from_disk(req.path.clone(), String::new()),
                    xui::TextEditor::default(),
                    xui::HighlightCache::default(),
                    crate::editor::buffer::PendingRead {
                        path: req.path.clone(),
                        line: req.line,
                    },
                ))
                .with_children(|p| {
                    // 虚拟化占位节点：高度 = 行数 × 行高（撑出滚动范围）
                    p.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(0.0), // 由 update_virtual_lines 更新为 全文高
                            position_type: PositionType::Relative,
                            // 关键：禁止 flex 收缩——否则占位高度被父容器（视口）
                            // 压到 size.y，content_size ≈ size，max_offset ≈ 0，滚不到底。
                            flex_shrink: 0.0,
                            ..default()
                        },
                        xui::text_editor::virtual_render::VirtualContentMarker,
                    ));
                })
                .id();
            // 挂到编辑器区容器下
            if let Ok(area) = q_area.single() {
                commands.entity(area).add_child(buffer_entity);
            }
            tabs.open(buffer_entity);
        }
        // 切换到编辑器视图 + 分屏内容为编辑器
        *view = crate::editor::EditorView::Editor;
        *content = crate::editor::SideViewContent::Editor;
    }
}

/// 处理关闭标签请求：despawn buffer 实体，更新 tabs。
///
/// 关闭最后一个标签时自动收起右侧分屏（切回对话视图）。
pub fn handle_close_tab_requests(
    mut reader: MessageReader<CloseTabRequest>,
    mut tabs: ResMut<EditorTabs>,
    mut view: ResMut<crate::editor::EditorView>,
    mut content: ResMut<crate::editor::SideViewContent>,
    mut commands: Commands,
) {
    for req in reader.read() {
        if let Some((entity, _)) = tabs.close(req.entity) {
            commands.entity(entity).despawn();
            // 无剩余标签 → 收起分屏 + 清空内容
            if tabs.is_empty() {
                *view = crate::editor::EditorView::Chat;
                *content = crate::editor::SideViewContent::None;
            }
        }
    }
}

/// 处理循环切换标签请求。
pub fn handle_cycle_tab_requests(
    mut reader: MessageReader<CycleTabRequest>,
    mut tabs: ResMut<EditorTabs>,
) {
    for req in reader.read() {
        if req.forward {
            tabs.next();
        } else {
            tabs.prev();
        }
    }
}
