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
use crate::layout::ChatPanelMarker;
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
    CloseTabRequest, CycleTabRequest, EditorTabs, OpenFileRequest, handle_close_tab_requests,
    handle_cycle_tab_requests, handle_open_file_requests,
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
                )
                    .chain()
                    .after(xui::TextEditorUpdateSet),
            );
    }
}

/// 启动时在对话主区内 spawn 编辑器视图容器（顶部栏 + 编辑器区 + 状态条）。
fn spawn_editor_view(
    mut commands: Commands,
    q_chat: Query<Entity, With<ChatPanelMarker>>,
    theme: Res<Theme>,
    loc: Res<xgent_settings::Localizer>,
) {
    let Ok(chat) = q_chat.single() else {
        return;
    };
    let font = theme.font_size;
    // 编辑器视图容器：覆盖在对话主区上，初始隐藏
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                // 兜底裁剪：子节点（编辑器区）被 buffer 内容撑高时不应溢出此视口。
                overflow: Overflow::clip(),
                display: Display::None,
                ..default()
            },
            BackgroundColor(theme.bg),
            EditorViewMarker,
        ))
        .with_children(|p| {
            // 顶部栏：返回对话按钮 + 标题
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(crate::theme::size::TOP_BAR_H),
                    padding: UiRect::horizontal(px(crate::theme::space::MD)),
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(crate::theme::space::SM),
                    border: UiRect::bottom(px(1.0)),
                    ..default()
                },
                BackgroundColor(theme.bar),
                BorderColor::all(theme.border),
            ))
            .with_children(|bar| {
                // 返回对话视图按钮（←）
                bar.spawn((
                    Button,
                    Node {
                        padding: UiRect::horizontal(px(crate::theme::space::SM)),
                        ..default()
                    },
                    BackgroundColor(theme.accent),
                    Text::new(crate::i18n::tr(&loc, "editor-back-to-chat")),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text),
                    EditorBackButtonMarker,
                ));
            });
            // 编辑器区：填充顶部栏以下空间，buffer 实体动态挂入
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    // flex 主轴纵向：min_height:0 允许收缩到视口高度，
                    // 否则被 buffer 子节点（含撑高占位）撑到内容高度，
                    // 导致 buffer.size.y == content_size.y、max_offset≈0、滚轮无效。
                    min_height: Val::ZERO,
                    overflow: Overflow::clip_y(),
                    ..default()
                },
                ScrollPosition::default(),
                EditorAreaMarker,
            ));
        });
    // 编辑器视图用 PositionType::Absolute 全屏覆盖，无需挂到特定父节点
    let _ = chat;
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
/// 按当前 `EditorView` 切换编辑器视图显隐。
pub fn apply_editor_view_visibility(
    view: Res<EditorView>,
    mut q: Query<&mut Node, With<EditorViewMarker>>,
) {
    for mut node in &mut q {
        let display = if *view == EditorView::Editor {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
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

/// 处理返回对话按钮点击：切回对话视图。
pub fn handle_back_button_click(
    q_btn: Query<&Interaction, (With<EditorBackButtonMarker>, Changed<Interaction>)>,
    mut view: ResMut<EditorView>,
) {
    for interaction in q_btn.iter() {
        if *interaction == Interaction::Pressed {
            *view = EditorView::Chat;
        }
    }
}

/// 更新各 buffer 实体显隐：仅激活标签显示，其余隐藏。
///
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
