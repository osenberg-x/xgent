//! XGent 业务编辑器层。
//!
//! 详见 `doc/design/editor-design.md` 第 6 节。
//!
//! 依赖 `xui::TextEditor` + `xgent_core` + `xgent_agent`。
//! 职责：
//! - 多标签页管理（EditorBuffer 集合）
//! - 文件 IO（fs::read / fs::write，tokio task 异步）
//! - 外部修改冲突协调（订阅 FileChangedEvent）
//! - EditorState Resource（impl trait，供 ContextProvider 查询）
//! - EditorCommand Event 订阅与执行
//! - 视图切换（对话/编辑器/文件预览）
//! - @ 引用解析（输入预处理）

pub mod at_syntax;
pub mod buffer;
pub mod command;
pub mod conflict;
pub mod io;
pub mod state;
pub mod tabs;

use bevy::prelude::*;

use crate::editor::buffer::EditorBuffer;
use crate::theme::Theme;

/// 便捷：f32 → Val::Px
fn px(v: f32) -> Val {
    Val::Px(v)
}
use crate::editor::command::EditorCommand;
use crate::editor::conflict::{FileChangedEvent, handle_conflict_decision, handle_file_changed};
use crate::editor::io::{
    BufferSavedEvent, EditorIoRuntime, FileReadRequest, FileReadResult, FileWriteRequest,
    FileWriteResult, apply_file_read_results, handle_file_read_requests,
    handle_file_write_requests, process_pending_reads,
};
use crate::editor::state::{EditorStateSnapshot, update_editor_state_snapshot};
use crate::editor::tabs::{
    CloseTabRequest, CycleTabRequest, EditorTabs, EditorTabBarMarker, EditorTabMarker,
    OpenFileRequest, handle_close_tab_requests, handle_cycle_tab_requests,
    handle_open_file_requests,
};

/// 编辑器视图状态（对话/编辑器/文件预览切换）。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorView {
    /// 对话视图（默认）
    #[default]
    Chat,
    /// 编辑器视图
    Editor,
}

/// 右侧分屏内容类型：编辑器视图 / 文件预览 / 无（收起）。
///
/// 由 [`crate::file_panel::handle_file_click`] 据文件类型设置；
/// [`apply_editor_view_visibility`] 统一据它切换 `EditorViewMarker` 与
/// `FilePreviewMarker` 的显隐，避免多系统并发写同一组件（B0001）。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SideViewContent {
    /// 无内容（分屏收起或初始）
    #[default]
    None,
    /// 编辑器视图（代码文件）
    Editor,
    /// 文件预览（非代码文件）
    Preview,
}

/// 编辑器视图标记节点（编辑器容器，初始隐藏）。
#[derive(Component, Default)]
pub struct EditorViewMarker;

/// 编辑器插件。
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<OpenFileRequest>()
            .add_message::<CloseTabRequest>()
            .add_message::<CycleTabRequest>()
            .add_message::<FileReadRequest>()
            .add_message::<FileReadResult>()
            .add_message::<FileWriteRequest>()
            .add_message::<FileWriteResult>()
            .add_message::<BufferSavedEvent>()
            .add_message::<FileChangedEvent>()
            .add_message::<EditorCommand>()
            .init_resource::<EditorTabs>()
            .init_resource::<EditorView>()
            .init_resource::<SideViewContent>()
            .init_resource::<EditorIoRuntime>()
            .init_resource::<EditorStateSnapshot>()
            .add_systems(
                Startup,
                spawn_editor_view.after(crate::layout::spawn_layout),
            )
            .add_systems(
                Update,
                (
                    handle_open_file_requests,
                    handle_close_tab_requests,
                    handle_cycle_tab_requests,
                    process_pending_reads,
                    apply_file_read_results,
                    handle_file_read_requests,
                    handle_editor_save_requests,
                    apply_editor_view_visibility,
                    update_buffer_visibility,
                    update_editor_state_snapshot,
                    rebuild_editor_tabs,
                    handle_editor_tab_click,
                )
                    .chain()
                    .after(xui::TextEditorUpdateSet),
            );
    }
}

/// 启动时在右侧分屏容器内 spawn 编辑器视图（顶部标签栏 + 编辑器区）。
///
/// 编辑器视图是右侧分屏的内容之一（另一为文件预览）；分屏本身由
/// [`crate::layout::SideViewMarker`] 容器承载，展开/收起由
/// [`crate::layout::SideViewCollapsed`] 统一控制。
fn spawn_editor_view(
    mut commands: Commands,
    q_side: Query<Entity, With<crate::layout::SideViewMarker>>,
    theme: Res<Theme>,
    loc: Res<xgent_settings::Localizer>,
) {
    let Ok(side) = q_side.single() else {
        return;
    };
    let font = theme.font_size;
    // 编辑器视图容器：作为分屏内容，初始隐藏（由 buffer 显隐 + 分屏显隐共同决定）
    let editor_view = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                display: Display::None,
                ..default()
            },
            BackgroundColor(theme.bg),
            EditorViewMarker,
        ))
        .with_children(|p| {
            // 顶部栏：tab 条（EditorTabBarMarker，动态 spawn tab 项）+ spacer + ✕ 关分屏
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(crate::theme::size::TOP_BAR_H),
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Row,
                    border: UiRect::bottom(px(1.0)),
                    ..default()
                },
                BackgroundColor(theme.bar),
                BorderColor::all(theme.border),
            ))
            .with_children(|bar| {
                // tab 条容器（动态 spawn tab 项，见 rebuild_editor_tabs）
                bar.spawn((
                    Node {
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        overflow: Overflow::clip_x(),
                        ..default()
                    },
                    EditorTabBarMarker,
                ));
                // ✕ 关闭分屏按钮（收起 SideView）
                bar.spawn((
                    Button,
                    Node {
                        width: px(28.0),
                        height: px(28.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(px(4.0)),
                        ..default()
                    },
                    Text::new("✕"),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                    EditorBackButtonMarker,
                ));
            });
            // 编辑器区：填充顶部栏以下空间，buffer 实体动态挂入
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    min_height: Val::ZERO,
                    overflow: Overflow::clip_y(),
                    ..default()
                },
                ScrollPosition::default(),
                EditorAreaMarker,
            ));
        })
        .id();
    commands.entity(side).add_child(editor_view);
}
/// 返回对话按钮标记。
#[derive(Component, Default)]
pub struct EditorBackButtonMarker;

/// 编辑器区容器标记（buffer 实体动态挂入）。
#[derive(Component, Default)]
pub struct EditorAreaMarker;

/// 订阅 xui 的 EditorSaveRequested，触发文件写入 + 发 BufferSavedEvent。
///
/// 文本从 `TextEditor.rope` 重建（虚拟化模式下无 EditableText）。
pub fn handle_editor_save_requests(
    mut reader: MessageReader<xui::EditorSaveRequested>,
    mut q_editors: Query<(&mut EditorBuffer, &xui::TextEditor)>,
    mut write_writer: MessageWriter<FileWriteRequest>,
    mut saved_writer: MessageWriter<BufferSavedEvent>,
) {
    for ev in reader.read() {
        let Ok((mut buf, editor)) = q_editors.get_mut(ev.entity) else {
            continue;
        };
        let content = editor.rope.to_string();
        write_writer.write(FileWriteRequest {
            path: buf.path.clone(),
            content: content.clone(),
        });
        // 标记 saved（实际写入结果由 FileWriteResult 处理，此处乐观更新）
        buf.mark_saved(&content);
        saved_writer.write(BufferSavedEvent {
            path: buf.path.clone(),
        });
    }
}
/// 据右侧分屏内容（`SideViewContent`）+ 编辑器视图（`EditorView`）切换
/// `EditorViewMarker` 与 `FilePreviewMarker` 的显隐，并展开分屏。
///
/// 由本系统统一写两个容器的 `Node.display`，避免 [`crate::file_panel::handle_file_click`]
/// 也写同一组件导致 B0001 query 冲突。
pub fn apply_editor_view_visibility(
    content: Res<SideViewContent>,
    mut collapsed: ResMut<crate::layout::SideViewCollapsed>,
    mut q: ParamSet<(
        Query<&mut Node, With<EditorViewMarker>>,
        Query<&mut Node, With<crate::file_panel::FilePreviewMarker>>,
    )>,
) {
    // 有内容时展开分屏；None 时不主动收（收起由返回按钮/Ctrl+\ 触发）
    if *content != SideViewContent::None && collapsed.0 {
        collapsed.0 = false;
    }
    let editor_display = if *content == SideViewContent::Editor {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut q.p0() {
        if node.display != editor_display {
            node.display = editor_display;
        }
    }
    let preview_display = if *content == SideViewContent::Preview {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut q.p1() {
        if node.display != preview_display {
            node.display = preview_display;
        }
    }
}

/// 切换到编辑器视图。
pub fn switch_to_editor_view(mut view: ResMut<EditorView>) {
    *view = EditorView::Editor;
}

/// 切换到对话视图。
pub fn switch_to_chat_view(mut view: ResMut<EditorView>) {
    *view = EditorView::Chat;
}

/// 处理返回对话按钮点击：切回对话视图 + 收起右侧分屏 + 清空分屏内容。
pub fn handle_back_button_click(
    q_btn: Query<&Interaction, (With<EditorBackButtonMarker>, Changed<Interaction>)>,
    mut view: ResMut<EditorView>,
    mut content: ResMut<SideViewContent>,
    mut collapsed: ResMut<crate::layout::SideViewCollapsed>,
) {
    for interaction in q_btn.iter() {
        if *interaction == Interaction::Pressed {
            *view = EditorView::Chat;
            *content = SideViewContent::None;
            collapsed.0 = true;
        }
    }
}
/// 解决"打开第二个文件时旧内容仍显示"——多标签下所有 buffer 都挂在
/// `EditorAreaMarker` 下，需显式控制各自 `Display`。
/// 编辑器视图整体隐藏时（`EditorView::Chat`），所有 buffer 也隐藏
/// （由容器 `Display::None` 级联，但显式设置更稳）。
pub fn update_buffer_visibility(
    tabs: Res<EditorTabs>,
    view: Res<EditorView>,
    mut q: Query<&mut Node, With<EditorBuffer>>,
) {
    let active = tabs.active_entity();
    let editor_active = *view == EditorView::Editor;
    for (i, &entity) in tabs.tabs.iter().enumerate() {
        let Ok(mut node) = q.get_mut(entity) else {
            continue;
        };
        let is_active = Some(i) == tabs.active;
        let show = editor_active && is_active;
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    let _ = active;
}
/// 编辑器 tab 关闭按钮标记（×，挂于 tab 项内）。
#[derive(Component, Default)]
pub struct EditorTabCloseMarker;

/// 据 `EditorTabs` Resource 重建 tab 条 UI。
///
/// tabs 列表变化时（打开/关闭文件）despawn 旧 tab 项、spawn 新 tab 项；
/// 每个 tab 项 = Button(row: 文件名 + 脏标记● + 关闭×)，active 态高亮。
pub fn rebuild_editor_tabs(
    tabs: Res<EditorTabs>,
    q_buffers: Query<&crate::editor::buffer::EditorBuffer>,
    q_bar: Query<Entity, With<EditorTabBarMarker>>,
    q_existing: Query<Entity, With<EditorTabMarker>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    // 仅在 tabs 列表变化时重建
    if !tabs.is_changed() && !tabs.is_added() {
        return;
    }
    let Ok(bar) = q_bar.single() else {
        return;
    };
    // despawn 旧 tab 项
    for entity in q_existing.iter() {
        commands.entity(entity).despawn();
    }
    let font = theme.font_size;
    let active_idx = tabs.active;
    for (i, &buf_entity) in tabs.tabs.iter().enumerate() {
        let Ok(buf) = q_buffers.get(buf_entity) else {
            continue;
        };
        let name = buf
            .path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let dirty = buf.state.is_dirty();
        let is_active = Some(i) == active_idx;
        let bg = if is_active {
            BackgroundColor(theme.bg)
        } else {
            BackgroundColor(theme.panel)
        };
        let txt_color = if is_active {
            theme.text
        } else {
            theme.text_dim
        };
        let border = if is_active {
            BorderColor::all(theme.accent)
        } else {
            BorderColor::all(theme.border)
        };
        commands.entity(bar).with_children(|bar| {
            bar.spawn((
                Button,
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(crate::theme::space::SM),
                    padding: UiRect::horizontal(px(crate::theme::space::MD)),
                    border: UiRect::right(px(1.0)),
                    ..default()
                },
                bg,
                border,
                EditorTabMarker { buffer: buf_entity },
            ))
            .with_children(|tab| {
                // 脏标记●
                if dirty {
                    tab.spawn((
                        Text::new("●"),
                        TextFont {
                            font_size: FontSize::Px(font),
                            ..default()
                        },
                        TextColor(theme.st_pending),
                    ));
                }
                // 文件名
                tab.spawn((
                    Text::new(name.clone()),
                    TextFont {
                        font_size: FontSize::Px(font - 1.5),
                        ..default()
                    },
                    TextColor(txt_color),
                ));
                // 关闭×
                tab.spawn((
                    Button,
                    Node {
                        width: px(16.0),
                        height: px(16.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    Text::new("×"),
                    TextFont {
                        font_size: FontSize::Px(font + 1.0),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                    EditorTabCloseMarker,
                ));
            });
        });
    }
}

/// 处理 tab 项点击：切换激活 tab；处理关闭×点击：发 CloseTabRequest。
pub fn handle_editor_tab_click(
    q_tabs: Query<(&EditorTabMarker, &Interaction), Changed<Interaction>>,
    q_close: Query<(&EditorTabMarker, &Interaction, &ChildOf), (With<EditorTabCloseMarker>, Changed<Interaction>)>,
    mut tabs: ResMut<EditorTabs>,
    mut close_writer: MessageWriter<CloseTabRequest>,
) {
    // 关闭× 优先
    for (marker, interaction, _parent) in q_close.iter() {
        if *interaction == Interaction::Pressed {
            close_writer.write(CloseTabRequest {
                entity: marker.buffer,
            });
        }
    }
    // tab 项点击切换
    for (marker, interaction) in q_tabs.iter() {
        if *interaction == Interaction::Pressed {
            tabs.open(marker.buffer);
        }
    }
}
