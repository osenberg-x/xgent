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

/// 脏关闭确认弹窗根节点标记。
#[derive(Component, Default)]
pub struct DirtyCloseDialogMarker;

/// 脏关闭弹窗关联的 buffer 实体（决策时定位）。
#[derive(Component)]
pub struct DirtyCloseDialogFor {
    /// 待关闭的 buffer 实体
    pub buffer: Entity,
}

/// 脏关闭弹窗"丢弃修改"按钮标记。
#[derive(Component, Default)]
pub struct DirtyCloseDiscardMarker;

/// 脏关闭弹窗"取消"按钮标记。
#[derive(Component, Default)]
pub struct DirtyCloseCancelMarker;

/// 打开文件请求（由命令面板/文件面板点击/EditorTool 触发）。
#[derive(Message, Debug, Clone)]
pub struct OpenFileRequest {
    /// 文件绝对路径
    pub path: PathBuf,
    /// 可选跳转行号（1-based）
    pub line: Option<usize>,
}

/// 关闭标签请求。
///
/// `force` 为真时跳过脏 buffer 确认（用户已在确认弹窗中同意丢弃修改）。
#[derive(Message, Debug, Clone)]
pub struct CloseTabRequest {
    /// buffer 实体
    pub entity: Entity,
    /// 是否跳过脏 buffer 确认弹窗
    pub force: bool,
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
    mut q_pending: Query<&mut crate::editor::io::FileReadPending>,
    q_area: Query<Entity, With<crate::editor::EditorAreaMarker>>,
    mut view: ResMut<crate::editor::EditorView>,
    mut content: ResMut<crate::editor::SideViewContent>,
    editor_theme: Res<xui::text_editor::render::EditorTheme>,
    mut commands: Commands,
) {
    for req in reader.read() {
        if let Some(entity) = tabs.find_by_path(&req.path, &q_buffers) {
            tabs.open(entity);
            if let Some(line) = req.line {
                // 仍在读取时更新 FileReadPending.line（读取完成后由
                // apply_file_read_results 据此插 PendingGoTo）；否则直接插 PendingGoTo。
                // 避免对空 rope 立即设滚动被 clamp 回 0、跳转丢失。
                if let Ok(mut pending) = q_pending.get_mut(entity) {
                    pending.line = Some(line);
                } else {
                    commands
                        .entity(entity)
                        .insert(crate::editor::buffer::PendingGoTo { line });
                }
            }
        } else {
            // spawn 新 buffer：滚动容器 + 虚拟化占位 + 行号列 + 光标条。
            // 文本显示走 `update_virtual_lines` 动态 spawn 可见行；
            let line_num_entity = commands
                .spawn((
                    Text::new(String::new()),
                    TextFont {
                        font_size: FontSize::Px(editor_theme.font_size),
                        ..default()
                    },
                    TextColor(bevy::color::palettes::tailwind::GRAY_400.into()),
                    xui::LineNumbersMarker,
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(0.0),
                        left: Val::Px(0.0),
                        width: Val::Px(48.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                ))
                .id();
            let cursor_bar_entity = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(0.0),
                        left: Val::Px(48.0),
                        width: Val::Px(2.0),
                        height: Val::Px(20.0),
                        ..default()
                    },
                    BackgroundColor(bevy::color::palettes::tailwind::AMBER_400.into()),
                    xui::CursorBarMarker,
                ))
                .id();
            let buffer_entity = commands
                .spawn((
                    xui::ScrollArea::vertical(),
                    xui::Scrollbar::default(),
                    crate::editor::buffer::EditorBuffer::from_disk(req.path.clone(), String::new()),
                    xui::TextEditor::default(),
                    xui::HighlightCache::default(),
                    xui::TextEditorChildren {
                        line_numbers: Some(line_num_entity),
                        highlight_layer: None,
                        cursor_bar: Some(cursor_bar_entity),
                    },
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
                            height: Val::Px(0.0),
                            position_type: PositionType::Relative,
                            flex_shrink: 0.0,
                            ..default()
                        },
                        xui::text_editor::virtual_render::VirtualContentMarker,
                    ));
                })
                .id();
            // 行号列 + 光标条挂为 buffer 子节点
            commands.entity(buffer_entity).add_child(line_num_entity);
            commands.entity(buffer_entity).add_child(cursor_bar_entity);
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
///
/// **脏保护**：非 `force` 关闭脏 buffer 时弹出确认弹窗，避免静默丢失
/// 未保存修改（对齐 AGENTS.md §5.4 安全模型）。用户确认后以 `force=true`
/// 重新发 `CloseTabRequest` 才真正 despawn。
pub fn handle_close_tab_requests(
    mut reader: MessageReader<CloseTabRequest>,
    mut tabs: ResMut<EditorTabs>,
    q_buffers: Query<&crate::editor::buffer::EditorBuffer>,
    q_dialog: Query<Entity, With<DirtyCloseDialogMarker>>,
    mut view: ResMut<crate::editor::EditorView>,
    mut content: ResMut<crate::editor::SideViewContent>,
    rt: ResMut<crate::editor::io::EditorIoRuntime>,
    theme: Res<crate::theme::Theme>,
    mut commands: Commands,
) {
    for req in reader.read() {
        // 脏 buffer + 非强制：弹确认或等待已有弹窗处理，绝不静默关闭
        if !req.force {
            if let Ok(buf) = q_buffers.get(req.entity) {
                if buf.state.is_dirty() {
                    // 无弹窗才弹新窗；已有弹窗则静默跳过（等用户处理完当前确认）
                    if q_dialog.single().is_err() {
                        spawn_dirty_close_dialog(&mut commands, req.entity, buf.path(), &theme);
                    }
                    continue;
                }
            }
        }
        // 清理该 buffer 的 pending 写入，避免写完成后误匹配
        rt.cancel_pending_writes(req.entity);
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

/// spawn 脏关闭确认弹窗（丢弃修改 / 取消）。
///
/// 样式对齐 `conflict::spawn_conflict_dialog`，保持弹窗视觉一致。
fn spawn_dirty_close_dialog(
    commands: &mut Commands,
    buffer: Entity,
    path: &std::path::Path,
    theme: &crate::theme::Theme,
) {
    let accent = theme.accent;
    let danger = theme.st_fail;
    let panel = theme.panel;
    let border = theme.border;
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(theme.overlay),
            DirtyCloseDialogMarker,
            DirtyCloseDialogFor { buffer },
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    padding: UiRect::all(Val::Px(16.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(12.0),
                    min_width: Val::Px(360.0),
                    ..default()
                },
                BackgroundColor(panel),
                BorderColor::all(border),
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new(format!(
                        "关闭未保存的标签？\n\n{} 有未保存的修改，关闭将丢失。",
                        path.display()
                    )),
                    TextColor(Color::WHITE),
                ));
                card.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(12.0),
                    ..default()
                },))
                    .with_children(|btns| {
                        btns.spawn((
                            Button,
                            Node {
                                padding: UiRect::all(Val::Px(8.0)),
                                ..default()
                            },
                            BackgroundColor(danger),
                            Text::new("丢弃修改"),
                            TextColor(Color::WHITE),
                            DirtyCloseDiscardMarker,
                        ));
                        btns.spawn((
                            Button,
                            Node {
                                padding: UiRect::all(Val::Px(8.0)),
                                ..default()
                            },
                            BackgroundColor(accent),
                            Text::new("取消"),
                            TextColor(Color::WHITE),
                            DirtyCloseCancelMarker,
                        ));
                    });
            });
        });
}

/// 处理脏关闭弹窗决策：丢弃修改 → 以 `force=true` 重发 `CloseTabRequest`；
/// 取消 → 仅关闭弹窗。
pub fn handle_dirty_close_decision(
    q_dialog: Query<(Entity, &DirtyCloseDialogFor), With<DirtyCloseDialogMarker>>,
    q_discard: Query<&Interaction, (With<DirtyCloseDiscardMarker>, Changed<Interaction>)>,
    q_cancel: Query<&Interaction, (With<DirtyCloseCancelMarker>, Changed<Interaction>)>,
    mut close_writer: MessageWriter<CloseTabRequest>,
    mut commands: Commands,
) {
    let Ok((dialog, for_buf)) = q_dialog.single() else {
        return;
    };
    let discard = q_discard
        .iter()
        .any(|i| *i == Interaction::Pressed);
    let cancel = q_cancel
        .iter()
        .any(|i| *i == Interaction::Pressed);
    if discard {
        close_writer.write(CloseTabRequest {
            entity: for_buf.buffer,
            force: true,
        });
        commands.entity(dialog).despawn();
    } else if cancel {
        commands.entity(dialog).despawn();
    }
}
