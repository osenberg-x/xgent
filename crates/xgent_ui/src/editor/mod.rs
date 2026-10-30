//! XGent 业务编辑器层。
//!
//! 详见 `docs/design/editor-design.md` 第 6 节。
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
pub mod diff_view;
pub mod io;
pub mod state;
pub mod tabs;

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;

use crate::editor::buffer::EditorBuffer;
use crate::editor::command::handle_editor_commands;
use crate::editor::conflict::{FileChangedEvent, handle_conflict_decision, handle_file_changed};
use crate::editor::io::{
    BufferSavedEvent, EditorIoRuntime, FileReadRequest, FileReadResult, FileWriteRequest,
    apply_file_read_results, handle_file_read_requests, handle_file_write_requests,
    poll_io_results, process_pending_reads,
};
use crate::editor::state::{EditorStateSnapshot, update_editor_state_snapshot};
use crate::editor::tabs::{
    CloseTabRequest, CycleTabRequest, EditorTabBarMarker, EditorTabMarker, EditorTabs,
    OpenFileRequest, handle_close_tab_requests, handle_cycle_tab_requests,
    handle_dirty_close_decision, handle_open_file_requests,
};
use crate::fonts::ui_text;

use crate::theme::{Theme, px, type_scale};
use xgent_agent::EditorCommandRequestMessage;

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
        // 面板注册表（幂等：宿主可能已装配）
        if !app.is_plugin_added::<crate::panels::PanelsPlugin>() {
            app.add_plugins(crate::panels::PanelsPlugin);
        }
        if !app.is_plugin_added::<crate::panels::visibility::VisibilityPlugin>() {
            app.add_plugins(crate::panels::visibility::VisibilityPlugin);
        }
        app.add_message::<OpenFileRequest>()
            .add_message::<CloseTabRequest>()
            .add_message::<CycleTabRequest>()
            .add_message::<FileReadRequest>()
            .add_message::<FileReadResult>()
            .add_message::<FileWriteRequest>()
            .add_message::<BufferSavedEvent>()
            .add_message::<FileChangedEvent>()
            .add_message::<EditorCommandRequestMessage>()
            .add_message::<xui::EditorDirtyChanged>()
            .init_resource::<EditorTabs>()
            .init_resource::<EditorView>()
            .init_resource::<EditorIoRuntime>()
            .init_resource::<EditorStateSnapshot>()
            .add_systems(Update, sync_editor_theme.before(xui::TextEditorUpdateSet))
            .add_systems(Update, (diff_view::rebuild_diff_view,))
            .add_systems(
                Startup,
                (spawn_editor_view, diff_view::spawn_diff_view)
                    .after(crate::layout::spawn_layout)
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    (
                        handle_editor_commands,
                        handle_open_file_requests,
                        handle_close_tab_requests,
                        handle_cycle_tab_requests,
                        process_pending_reads,
                        handle_file_read_requests,
                        handle_editor_save_requests,
                        handle_file_write_requests,
                        poll_io_results,
                        apply_save_result,
                    )
                        .chain(),
                    (
                        apply_file_read_results,
                        handle_pending_goto,
                        update_buffer_visibility,
                        sync_dirty_state,
                        update_editor_state_snapshot,
                        rebuild_editor_tabs,
                        handle_editor_tab_click,
                        handle_file_changed,
                        handle_conflict_decision,
                        handle_dirty_close_decision,
                    )
                        .chain(),
                )
                    .chain()
                    .after(xui::TextEditorUpdateSet),
            );
    }
}

/// 启动时在右侧分屏容器内 spawn 编辑器视图（顶部标签栏 + 编辑器区）。
///
/// 编辑器视图是右侧分屏的内容之一（另一为文件预览）；分屏本身由
/// [`crate::layout::CenterHostMarker`] 容器承载，展开/收起由
/// [`crate::panels::SlotLayout`] 的 `Slot::Center` 统一控制。
fn spawn_editor_view(
    mut commands: Commands,
    q_side: Query<Entity, With<crate::layout::CenterHostMarker>>,
    theme: Res<Theme>,
    mut register: MessageWriter<crate::panels::PanelRegister>,
) {
    let Ok(side) = q_side.single() else {
        return;
    };
    // 编辑器视图容器：作为分屏内容，初始隐藏（由 buffer 显隐 + 分屏显隐共同决定）
    // v8 M3-T3：同列还有 38px 页签条，页面若用 `height:100%` 会把底部顶出视口
    // （审计 D2：终端状态条永久掉出 38px）。改用 `region_fill`（flex 填满剩余空间）。
    // v8 M5-T3：初始隐藏改为**构造期字面量**（不再是运行时写手）；
    // 之后的显隐由 `derive_panel_visibility` 依据注册表派生。
    let view_node = Node {
        display: Display::None,
        width: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        overflow: Overflow::clip(),
        ..xui::region_fill()
    };
    let editor_view = commands
        .spawn((
            view_node,
            BackgroundColor(theme.code_bg),
            EditorViewMarker,
            crate::panels::PanelVisibility::of_panel(
                crate::panels::ids::EDITOR,
                crate::panels::Slot::Center,
            ),
        ))
        .with_children(|p| {
            // 顶部栏：**文档列表**（打开的代码文件）+ spacer。
            // v8 M5-T4：这里的 ✕ 返回入口被移除——侧区顶部只保留**一个**返回入口
            // （槽位页签条上的 `EditorBackBsttonMarker`）。
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: px(crate::theme::size::EDITOR_TABS_H),
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Row,
                    border: UiRect::bottom(px(1.0)),
                    ..default()
                },
                BackgroundColor(theme.surface),
                BorderColor::all(theme.border),
            ))
            .with_children(|bar| {
                // 文档列表容器（动态 spawn tab 项，见 rebuild_editor_tabs）
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
            });
            // 编辑器区：填充顶部栏以下空间，buffer 实体动态挂入。
            // 不挂 ScrollPosition——滚动职责在 buffer 实体自身的
            // `xui::ScrollFrame::vertical()`（含 ScrollPosition），外层容器
            // 仅做硬裁剪（clip）与布局，避免双层 ScrollPosition 嵌套干扰
            // 内层的跳转行定位（handle_pending_goto 写的是 buffer 的滚动）。
            // v8 M3-T2：clip_y → clip（clip_y 是「假滚动」的信号，grep 归零）。
            let mut area = xui::region_fill();
            area.width = Val::Percent(100.0);
            area.flex_direction = FlexDirection::Column;
            area.overflow = Overflow::clip();
            p.spawn((area, EditorAreaMarker));
        })
        .id();
    commands.entity(side).add_child(editor_view);
    // v8 M5：注册为 `Slot::Center` 面板（页签标题取 `title_key`）。
    register.write(crate::panels::PanelRegister {
        desc: crate::panels::PanelDescriptor::new(
            crate::panels::ids::EDITOR,
            crate::panels::Slot::Center,
            "panel-editor",
            "file",
            0,
        ),
        view: Some(editor_view),
    });
}

/// 把 xgent_ui [`Theme`]（用户/系统可配的单一字号源）同步到 xui [`EditorTheme`]。
///
/// 设计动机（对齐 zed 编辑器字号模型 + ui-prototype.html）：
/// zed 的 `buffer_font_size` 是用户可配的单一字号源，UI chrome 经 `rem_size` 派生。
/// xgent 此前 `EditorTheme.font_size` 硬编码 14.0 且与 [`Theme`] 脱节——本系统让
/// 编辑器正文字号跟随 [`Theme::font_size`]，未来 `Theme` 接入 settings（NF-04）后
/// 即可「跟随系统/用户偏好」调整编辑器字号，无需改 xui。颜色（text / text_dim）
/// 一并同步，保持编辑器与 UI 主题一致。
///
/// 字号偏移（对齐 ui-prototype.html `.ed-content`/`.gutter` 的 12.5px）：
/// 原型中 UI 正文 14px、编辑器代码 12.5px（等宽代码字号略小于 UI 正文，符合
/// 主流编辑器惯例）。故编辑器字号 = `Theme.font_size - 1.5`，而非直接等于。
/// 行高比对齐原型 1.55（而非 1.5），让代码行间距更贴近设计预期。
///
/// 跑在 `xui::TextEditorUpdateSet` 之前，确保 `update_virtual_lines` 读到最新值。
fn sync_editor_theme(
    theme: Res<Theme>,
    fonts: Res<crate::fonts::UiFonts>,
    mut editor_theme: ResMut<xui::text_editor::render::EditorTheme>,
) {
    // 仅在 Theme 变化（或等宽槽位变化）时写（避免每帧无谓 mutation 触发 change detection）。
    if !theme.is_changed() && !editor_theme.is_added() && fonts.mono == editor_theme.font {
        return;
    }
    editor_theme.font_size = (theme.font_size - 1.5).max(10.0);
    editor_theme.line_height_ratio = 1.55;
    editor_theme.text = theme.text;
    editor_theme.text_dim = theme.text_dim;
    // v8 M4-T3：代码正文走等宽槽位（比例字体的数字宽度不等 → 行号列与代码列错位）。
    editor_theme.font = fonts.mono.clone();
    // 语法色注入（v7 冷调低饱和；xui 侧 None 类别回落内置映射）
    editor_theme.kw = Some(theme.kw);
    editor_theme.fn_ = Some(theme.fn_);
    editor_theme.str_ = Some(theme.str_);
    editor_theme.num = Some(theme.num);
    editor_theme.ty = Some(theme.ty);
    editor_theme.com = Some(theme.com);
    editor_theme.punc = Some(theme.punc);
    editor_theme.plain = Some(theme.code_text);
}
/// 编辑器区容器标记（buffer 实体动态挂入）。
#[derive(Component, Default)]
pub struct EditorAreaMarker;

/// 订阅 xui 的 EditorSaveRequested，触发文件写入。
///
/// 文本从 `TextEditor.rope` 重建（虚拟化模式下无 EditableText）。
/// `BufferSavedEvent` 由 `poll_io_results` 在写入成功后发出（非乐观）。
/// buffer 的 saved 标记也由 `poll_io_results` 路径经 `BufferSavedEvent` →
/// `apply_save_result` 完成，避免写失败时 buffer 状态与磁盘不一致。
///
/// **冲突态保护**：buffer 处于 `ConflictDetected`（外部修改已到达、等待用户
/// 三选）时禁止落盘，避免静默覆盖外部修改（设计 §3.6）。用户须先在冲突弹窗
/// 决策（丢弃本地 / 保留本地 / 对比合并）后，`LocalPreferred` 才允许保存覆盖。
pub fn handle_editor_save_requests(
    mut reader: MessageReader<xui::EditorSaveRequested>,
    mut q_editors: Query<(&mut EditorBuffer, &xui::TextEditor)>,
    mut write_writer: MessageWriter<FileWriteRequest>,
) {
    for ev in reader.read() {
        let Ok((buf, editor)) = q_editors.get_mut(ev.entity) else {
            continue;
        };
        // 冲突未解决：禁止覆盖磁盘，保护用户数据
        if buf.state == crate::editor::buffer::BufferState::ConflictDetected {
            continue;
        }
        let content = editor.rope.to_string();
        write_writer.write(FileWriteRequest {
            entity: ev.entity,
            path: buf.path.clone(),
            content,
        });
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

/// 解决"打开第二个文件时旧内容仍显示"——多标签下所有 buffer 都挂在
/// `EditorAreaMarker` 下，需显式控制各自 `Display`。
///
/// **内容态可见性**（当前文档页签），不属于面板 / 区域可见性——按 M5-T3 的
/// 确认口径保留在各自模块，不并入 `derive_panel_visibility`。
pub fn update_buffer_visibility(
    tabs: Res<EditorTabs>,
    view: Res<EditorView>,
    mut q: Query<&mut Node, With<EditorBuffer>>,
) {
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
}
/// 编辑器 tab 关闭按钮标记（×，挂于 tab 项内）。
#[derive(Component, Default)]
pub struct EditorTabCloseMarker;

/// 据 `EditorTabs` Resource 重建 tab 条 UI。
///
/// tabs 列表变化时（打开/关闭文件）despawn 旧 tab 项、spawn 新 tab 项；
/// 每个 tab 项 = Button(row: 文件名 + 脏标记● + 关闭×)，active 态高亮。
///
/// **重建触发**：`EditorTabs` 变化（open/close/switch）或任意 `EditorBuffer`
/// 变化（dirty 标记随保存/编辑更新）时重建。仅 `tabs.is_changed()` 不够——
/// 保存（`apply_save_result`）与编辑（`sync_dirty_state`）只改 buffer 状态，
/// 不触发 tabs changed，会导致脏标记 `*` 不随状态更新。
pub fn rebuild_editor_tabs(
    tabs: Res<EditorTabs>,
    q_buffers: Query<&crate::editor::buffer::EditorBuffer>,
    q_changed: Query<(), Changed<crate::editor::buffer::EditorBuffer>>,
    q_bar: Query<Entity, With<EditorTabBarMarker>>,
    q_existing: Query<Entity, With<EditorTabMarker>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    // tabs 列表变化，或任意 buffer 状态变化（脏标记需更新）时重建。
    // 保存（apply_save_result）与编辑（sync_dirty_state）只改 EditorBuffer，
    // 不触发 tabs changed，故需额外检测 Changed<EditorBuffer>。
    let tabs_changed = tabs.is_changed() || tabs.is_added();
    let buffer_changed = !q_changed.is_empty();
    if !tabs_changed && !buffer_changed {
        return;
    }
    let Ok(bar) = q_bar.single() else {
        return;
    };
    // despawn 旧 tab 项
    for entity in q_existing.iter() {
        commands.entity(entity).despawn();
    }
    let active_idx = tabs.active;
    // 维护 xui::EditorActive 标记：编辑器快捷键（Cmd+S/Z/F/H）只作用于
    // 带标记的激活 buffer，否则 Cmd+S 会保存所有打开的 tab
    for (i, &buf_entity) in tabs.tabs.iter().enumerate() {
        if Some(i) == active_idx {
            commands.entity(buf_entity).insert(xui::EditorActive);
        } else {
            commands.entity(buf_entity).remove::<xui::EditorActive>();
        }
    }
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
            BackgroundColor(theme.surface)
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
                Hovered::default(),
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
                    tab.spawn(ui_text(
                        "*",
                        type_scale::BODY,
                        400,
                        theme.st_pending,
                        type_scale::line_height::UI,
                    ));
                }
                // 文件名
                // v8 M3-T5：文件名过长时省略（审计 C3），不挤掉关闭钮。
                tab.spawn((
                    Node {
                        flex_shrink: 1.0,
                        min_width: Val::ZERO,
                        max_width: px(crate::theme::size::TAB_TITLE_MAX_W),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    crate::fonts::ellipsis_text(
                        name.clone(),
                        type_scale::SMALL,
                        400,
                        txt_color,
                        type_scale::line_height::UI,
                    ),
                ));
                // 关闭×
                tab.spawn((
                    Button,
                    Hovered::default(),
                    Node {
                        width: px(16.0),
                        height: px(16.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    ui_text(
                        "×",
                        type_scale::H3,
                        400,
                        theme.text_dim,
                        type_scale::line_height::UI,
                    ),
                    EditorTabCloseMarker,
                ));
            });
        });
    }
}

/// 处理 tab 项点击：切换激活 tab；处理关闭×点击：发 CloseTabRequest。
///
/// × 按钮实体只挂 `EditorTabCloseMarker`（`EditorTabMarker{buffer}` 在其父
/// tab 节点上），经 `ChildOf` 回溯父实体取 buffer。× 是 `Button`
/// 点击 × 时事件由 ButtonPlugin 在最内层 Button 消费（propagate(false)），
/// 父 tab 不会同时 Pressed，两查询天然互斥。
pub fn handle_editor_tab_click(
    q_tabs: Query<(&EditorTabMarker,), Added<Pressed>>,
    q_close: Query<(&ChildOf,), (With<EditorTabCloseMarker>, Added<Pressed>)>,
    q_tab_marker: Query<&EditorTabMarker>,
    mut tabs: ResMut<EditorTabs>,
    mut close_writer: MessageWriter<CloseTabRequest>,
) {
    // 关闭× 优先
    for (parent,) in q_close.iter() {
        if let Ok(marker) = q_tab_marker.get(parent.0) {
            close_writer.write(CloseTabRequest {
                entity: marker.buffer,
                force: false,
            });
        }
    }
    // tab 项点击切换
    for (marker,) in q_tabs.iter() {
        tabs.open(marker.buffer);
    }
}
/// 同步 xui 的 EditorDirtyChanged 到 EditorBuffer 状态机。
///
/// - `dirty=true`（用户编辑）→ `mark_dirty`（Clean→Dirty）。
/// - `dirty=false`（undo 回到原始态）→ 若当前 `Dirty` 则回 `Clean`。
///   `ConflictDetected`/`LocalPreferred` 不被覆盖（用户已决策或待决策）。
pub fn sync_dirty_state(
    mut reader: MessageReader<xui::EditorDirtyChanged>,
    mut q_buffers: Query<&mut crate::editor::buffer::EditorBuffer>,
) {
    for ev in reader.read() {
        let Some(mut buf) = q_buffers.get_mut(ev.entity).ok() else {
            continue;
        };
        if ev.dirty {
            buf.mark_dirty();
        } else if buf.state == crate::editor::buffer::BufferState::Dirty {
            // undo 回到原始态：Dirty → Clean（不覆盖冲突态）
            buf.state = crate::editor::buffer::BufferState::Clean;
        }
    }
}

/// 订阅 `BufferSavedEvent`，把对应 buffer 标记为已保存。
///
/// `poll_io_results` 在写入成功后发 `BufferSavedEvent`，本系统据此
/// 更新 `EditorBuffer` 状态为 Clean + 更新 `disk_content`，
pub fn apply_save_result(
    mut reader: MessageReader<crate::editor::io::BufferSavedEvent>,
    mut q: Query<(&mut EditorBuffer, &mut xui::TextEditor)>,
) {
    for ev in reader.read() {
        // 按实体匹配（非 path）：避免关闭 buffer A 后重开同路径 B 时，
        // A 的保存结果误匹配 B、覆盖 B 的状态。实体已 despawn 则 q.get_mut 失败，安全丢弃。
        let Ok((mut buf, mut editor)) = q.get_mut(ev.entity) else {
            continue;
        };
        // 用事件携带的 content（写盘时的内容）更新 disk_content，
        // 避免再次 rope.to_string()（handle_editor_save_requests 已遍历一次）
        buf.mark_saved(&ev.content);
        editor.dirty = false;
    }
}

/// 消费 `PendingGoTo`：滚动到目标行 + 移除组件。
///
/// 目标行号 1-based，`ScrollPosition.y` 设为 `(line-1) * line_height`
/// 让目标行对齐视口顶部。行高从 `TextEditor.line_height` 取。
pub fn handle_pending_goto(
    mut q: Query<(
        Entity,
        &crate::editor::buffer::PendingGoTo,
        &mut bevy::ui::ScrollPosition,
        &xui::TextEditor,
    )>,
    mut commands: Commands,
) {
    for (entity, goto, mut scroll, editor) in q.iter_mut() {
        let target_y = goto.line.saturating_sub(1) as f32 * editor.line_height;
        scroll.y = target_y;
        commands
            .entity(entity)
            .remove::<crate::editor::buffer::PendingGoTo>();
    }
}
