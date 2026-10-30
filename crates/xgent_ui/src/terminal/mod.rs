//! XGent 业务终端层（F-19）。
//!
//! 详见 `docs/design/terminal-design.md` §2-§3、ADR-0011/0012、`CONTEXT.md`
//! 「终端（F-19，P1）」。
//!
//! 职责：
//! - 多 tab 管理（每 tab 一个独立 PTY 会话，[`TerminalTabs`] Resource）
//! - PTY IO 桥接（ECS Messages → async backend，[`io`] 模块）
//! - 输出历史渲染（vte 解析 → `RenderHistory` → 虚拟滚动，[`output`] 模块）
//! - UI 侧行编辑（光标/Backspace/Enter，[`input`] 模块）
//! - 视图切换（终端是 `Slot::SideDock` 的注册面板，与编辑器/差异互斥）
//!
//! # 输入模式
//!
//! PTY 保持 cooked 模式，shell 自带 readline（行编辑/历史/补全）。UI 侧**透传**
//! 按键为原始字节直接发 PTY，不本地镜像字符——避免「输入框 + shell 回显」双显。
//! shell 回显是输入的唯一显示源。代价是放弃 UI 侧行编辑（光标移动/编辑已输入
//! 文本交由 shell readline 承担），收益是跨平台一致 + 获得 shell 原生历史/补全
//! （设计本列为「不支持」，cooked 模式反而白送）。控制字符 Ctrl+C/D 即时单字节
//! 发送，行编辑键（←→/Home/End/Backspace/Delete）发对应转义序列让 shell 处理。

pub mod input;
pub mod io;
pub mod metrics;
pub mod output;
pub mod tabs;

use std::path::PathBuf;

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;

use crate::fonts::{UiFonts, mono_text};
use crate::i18n::tr;
use crate::theme::{Theme, px, space, type_scale};
use xgent_settings::Localizer;
use xgent_terminal::{LocalPtyBackend, TerminalBackend};
use xui::scroll_frame::ScrollFrame;

/// 终端视图容器标记（挂于 [`crate::layout::SideViewMarker`] 下，初始隐藏）。
///
/// 显隐由 `derive_panel_visibility` 依据注册表中的 `terminal` 面板派生。
#[derive(Component, Default)]
pub struct TerminalViewMarker;

/// 终端 tab 条容器标记（tv-head 内，动态 spawn tab 项）。
#[derive(Component, Default)]
pub struct TerminalTabBarMarker;

/// 单个终端 tab 按钮标记。
#[derive(Component)]
pub struct TerminalTabMarker {
    /// 此 tab 对应的 [`TerminalTab`] 实体。
    pub tab: Entity,
}

/// 终端 tab 关闭按钮标记（×，挂于 tab 项内）。
#[derive(Component, Default)]
pub struct TerminalTabCloseMarker;

/// 终端视图关闭分屏按钮标记（tv-head 右侧 ✕）。
#[derive(Component, Default)]
pub struct TerminalCloseButtonMarker;

/// 新建终端 tab 按钮标记（tv-head 的 ＋）。
#[derive(Component, Default)]
pub struct TerminalNewTabButtonMarker;

/// 清屏按钮标记。
#[derive(Component, Default)]
pub struct TerminalClearButtonMarker;

/// 终端输出历史容器标记（tv-body，可滚动，每 tab 一个）。
#[derive(Component, Default)]
pub struct TerminalOutputMarker;

/// 终端行编辑输入框标记（tv-inputline）。
#[derive(Component, Default)]
pub struct TerminalInputMarker;

/// 终端输入行块状光标标记（1s 周期闪烁）。
#[derive(Component, Default)]
pub struct TerminalInputCaretMarker;

/// 终端状态栏标记（tv-statusbar，显示运行状态/shell/cwd）。
#[derive(Component, Default)]
pub struct TerminalStatusBarMarker;

/// 终端 IO runtime（由 `xgent_app` 注入 tokio handle + backend）。
///
/// `backend` 默认注入 [`LocalPtyBackend`]；若未注入，终端功能不可用（spawn 请求
/// 静默丢弃并记 warn）。对齐 [`crate::editor::io::EditorIoRuntime`] 的注入模式。
#[derive(Resource, Default)]
pub struct TerminalIoRuntime {
    /// tokio runtime handle。
    pub handle: Option<tokio::runtime::Handle>,
    /// PTY 后端实例（`Arc` 共享给 spawn 的 task）。
    pub backend: Option<std::sync::Arc<dyn TerminalBackend>>,
}

impl TerminalIoRuntime {
    /// 注入 handle + backend。
    pub fn new(handle: tokio::runtime::Handle, backend: LocalPtyBackend) -> Self {
        Self {
            handle: Some(handle),
            backend: Some(std::sync::Arc::new(backend)),
        }
    }
}

/// 终端 tab 状态（每个 PTY 会话一个实体）。
#[derive(Component, Debug, Clone)]
pub struct TerminalTab {
    /// PTY 会话 id（backend 返回前为 None）。
    pub pty_id: Option<xgent_terminal::TerminalId>,
    /// 会话标题（用于 tab 显示，如 "shell #1"）。
    pub title: String,
    /// 运行状态。
    pub status: TerminalTabStatus,
    /// shell 选择（spawn 时记录，statusbar 显示）。
    pub shell: xgent_terminal::ShellSpec,
    /// 初始 cwd（spawn 时记录，statusbar 显示；cd 后不追踪）。
    pub cwd: PathBuf,
    /// 退出码（PTY 退出后填）。
    pub exit_code: Option<i32>,
}

/// 终端 tab 运行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerminalTabStatus {
    /// tab Entity 已 spawn，PTY 尚未就绪（瞬态）。
    #[default]
    Created,
    /// PTY 活跃。
    Running,
    /// PTY 已退出（tab 保留标灰）。
    Exited,
}

/// 多 tab 管理 Resource。
#[derive(Resource, Debug, Default)]
pub struct TerminalTabs {
    /// 打开的 tab 实体列表（按创建顺序）。
    pub tabs: Vec<Entity>,
    /// 当前激活 tab 下标。
    pub active: Option<usize>,
    /// 下一 tab 序号（标题 "shell #N"）。
    pub next_seq: u32,
}

impl TerminalTabs {
    /// 注册新 tab，设为激活。
    pub fn open(&mut self, entity: Entity) {
        self.tabs.push(entity);
        self.active = Some(self.tabs.len() - 1);
    }

    /// 关闭 tab，返回需 despawn 的实体 + 新激活下标。
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

    /// 是否无 tab。
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// 激活 tab 实体。
    pub fn active_entity(&self) -> Option<Entity> {
        self.active.and_then(|i| self.tabs.get(i).copied())
    }
}

/// 终端插件。
pub struct TerminalPlugin;

impl Plugin for TerminalPlugin {
    fn build(&self, app: &mut App) {
        // bridge channel：tokio drain task → ECS 系统（crossbeam Receiver 是 Sync）
        let (bridge_tx, bridge_rx) = io::PtyBridge::pair();
        app.add_message::<tabs::SpawnTabRequest>()
            .add_message::<tabs::CloseTabRequest>()
            .add_message::<tabs::SwitchTabRequest>()
            .add_message::<tabs::ClearTabRequest>()
            .add_message::<io::TerminalInput>()
            .add_message::<io::TerminalResize>()
            .add_message::<io::TerminalSpawned>()
            .add_message::<io::TerminalOutputChunk>()
            .add_message::<io::TerminalExited>();
        // 面板注册表（幂等：宿主可能已装配）
        if !app.is_plugin_added::<crate::panels::PanelsPlugin>() {
            app.add_plugins(crate::panels::PanelsPlugin);
        }
        if !app.is_plugin_added::<crate::panels::visibility::VisibilityPlugin>() {
            app.add_plugins(crate::panels::visibility::VisibilityPlugin);
        }
        app.init_resource::<TerminalTabs>()
            .init_resource::<TerminalIoRuntime>()
            .init_resource::<input::TerminalLineBuf>()
            .init_resource::<output::TerminalResizeTracker>()
            .init_resource::<metrics::CellGrid>()
            .insert_resource(bridge_tx)
            .insert_resource(bridge_rx)
            .add_systems(
                Startup,
                spawn_terminal_view.after(crate::layout::spawn_layout),
            )
            .add_systems(
                Update,
                (
                    tabs::handle_spawn_tab_requests,
                    tabs::handle_close_tab_requests,
                    tabs::handle_switch_tab_requests,
                    tabs::handle_clear_tab_requests,
                    tabs::handle_terminal_tab_click,
                    io::handle_pty_events,
                    io::handle_terminal_input,
                    output::handle_terminal_resize,
                    io::handle_terminal_resize,
                    output::append_output_chunks,
                    tabs::rebuild_terminal_tabs,
                    input::handle_terminal_keyboard,
                    input::update_input_line,
                    input::update_input_after,
                    output::update_output_visibility,
                    output::update_status_bar,
                    handle_close_button,
                    handle_new_tab_button,
                    handle_clear_button,
                    input::update_input_caret,
                )
                    .chain(),
            )
            .add_systems(Update, apply_terminal_view_focus)
            // 等宽度量实测：探针生成（首帧）+ 读探针排版结果（须在 bevy 文本排版之后）
            .add_systems(Update, metrics::spawn_mono_probes)
            .add_systems(
                PostUpdate,
                metrics::collect_mono_metrics.after(bevy::ui::UiSystems::PostLayout),
            );
    }
}

/// 启动时在右侧分屏容器内 spawn 终端视图（初始隐藏）。
///
/// 终端视图是右侧分屏的内容之一（与编辑器/差异互斥）；分屏本身由
/// [`crate::layout::SideViewMarker`] 容器承载，展开/收起由
/// [`crate::panels::SlotLayout`] 的 `Slot::SideDock` 统一控制。
fn spawn_terminal_view(
    mut commands: Commands,
    q_side: Query<Entity, With<crate::layout::SideViewMarker>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    fonts: Res<UiFonts>,
    mut register: MessageWriter<crate::panels::PanelRegister>,
) {
    let Ok(side) = q_side.single() else {
        return;
    };
    // 终端视图容器：初始隐藏（由 `derive_panel_visibility` 据注册表派生）
    // v8 M3-T3：同列有 38px 页签条，页面用 `height:100%` 会把底部状态条顶出
    // 视口（审计 D2：永久掉出 38px）。改用 `region_fill`（flex 填满剩余空间）。
    // v8 M5-T3：初始隐藏改为构造期字面量（不再是运行时写手）。
    let terminal_view = commands
        .spawn((
            Node {
                display: Display::None,
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..xui::region_fill()
            },
            BackgroundColor(theme.bg),
            TerminalViewMarker,
            crate::panels::PanelVisibility::of_panel(
                crate::panels::ids::TERMINAL,
                crate::panels::Slot::SideDock,
            ),
        ))
        .id();
    commands.entity(side).add_child(terminal_view);
    // v8 M5：注册为 `Slot::SideDock` 面板。
    register.write(crate::panels::PanelRegister {
        desc: crate::panels::PanelDescriptor::new(
            crate::panels::ids::TERMINAL,
            crate::panels::Slot::SideDock,
            "panel-terminal",
            "terminal",
            2,
        ),
        view: Some(terminal_view),
    });
    commands.entity(terminal_view).with_children(|view| {
        {
            // tv-head：标题 + tab 条 + 新建/清屏/关闭
            view.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: px(crate::theme::size::TOP_BAR_H),
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(space::XS),
                    padding: UiRect::horizontal(px(space::SM)),
                    border: UiRect::bottom(px(1.0)),
                    ..default()
                },
                BackgroundColor(theme.surface),
                BorderColor::all(theme.border),
            ))
            .with_children(|head| {
                // tab 条容器（动态 spawn tab 项，见 rebuild_terminal_tabs）
                head.spawn((
                    Node {
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: px(space::XS),
                        overflow: Overflow::clip_x(),
                        ..default()
                    },
                    TerminalTabBarMarker,
                ));
                // ＋ 新建 tab
                head.spawn((
                    Button,
                    Hovered::default(),
                    Node {
                        width: px(24.0),
                        height: px(24.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(px(4.0)),
                        ..default()
                    },
                    mono_text(
                        &fonts,
                        tr(&loc, "terminal-new-tab"),
                        type_scale::BODY,
                        theme.text_dim,
                        type_scale::line_height::TERM,
                    ),
                    TerminalNewTabButtonMarker,
                ));
                // 清屏
                head.spawn((
                    Button,
                    Hovered::default(),
                    Node {
                        width: px(24.0),
                        height: px(24.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(px(4.0)),
                        ..default()
                    },
                    mono_text(
                        &fonts,
                        tr(&loc, "terminal-clear"),
                        type_scale::CAPTION,
                        theme.text_dim,
                        type_scale::line_height::TERM,
                    ),
                    TerminalClearButtonMarker,
                ));
                // ✕ 关闭分屏
                head.spawn((
                    Button,
                    Hovered::default(),
                    Node {
                        width: px(24.0),
                        height: px(24.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(px(4.0)),
                        ..default()
                    },
                    mono_text(
                        &fonts,
                        tr(&loc, "terminal-close"),
                        type_scale::BODY,
                        theme.text_dim,
                        type_scale::line_height::TERM,
                    ),
                    TerminalCloseButtonMarker,
                ));
            });

            // tv-body：输出历史容器（ScrollFrame 贴底滚动，每 tab 动态 spawn 行）
            // v8 M3-T2：滚动语义只由 ScrollFrame 决定。
            let (mut output_area, output_stick) = ScrollFrame::stick_bottom();
            output_area.style().padding = UiRect::horizontal(px(space::SM));
            view.spawn((
                output_area,
                output_stick,
                BackgroundColor(theme.bg),
                TerminalOutputMarker,
                output::RenderedCursor::default(),
            ));

            // tv-inputline：行编辑输入框
            view.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: px(28.0),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(space::SM),
                    padding: UiRect::horizontal(px(space::SM)),
                    border: UiRect::top(px(1.0)),
                    ..default()
                },
                BackgroundColor(theme.surface),
                BorderColor::all(theme.border),
            ))
            .with_children(|line| {
                // prompt ❯
                line.spawn((mono_text(
                    &fonts,
                    tr(&loc, "terminal-prompt"),
                    type_scale::BODY,
                    theme.accent,
                    type_scale::line_height::TERM,
                ),));
                // 输入文本节点（光标前部分，input 模块本地行编辑更新）
                line.spawn((
                    mono_text(
                        &fonts,
                        "",
                        type_scale::BODY,
                        theme.text,
                        type_scale::line_height::TERM,
                    ),
                    TerminalInputMarker,
                ));
                // 块状光标（accent_interactive，1s 周期闪烁；随输入右移——
                // 其左侧文本节点增长把它推后，天然随字符宽度移动）
                line.spawn((
                    mono_text(
                        &fonts,
                        "\u{2588}",
                        type_scale::BODY,
                        theme.accent_interactive,
                        type_scale::line_height::TERM,
                    ),
                    TerminalInputCaretMarker,
                ));
                // 光标后文本节点（光标在行中编辑时显示右侧内容）
                line.spawn((
                    mono_text(
                        &fonts,
                        "",
                        type_scale::BODY,
                        theme.text,
                        type_scale::line_height::TERM,
                    ),
                    input::TerminalInputAfterMarker,
                ));
            });

            // tv-statusbar
            view.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: px(crate::theme::size::STATUS_BAR_H),
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(space::SM),
                    padding: UiRect::horizontal(px(space::SM)),
                    border: UiRect::top(px(1.0)),
                    ..default()
                },
                BackgroundColor(theme.surface),
                BorderColor::all(theme.border),
            ))
            .with_children(|bar| {
                bar.spawn((
                    mono_text(
                        &fonts,
                        "",
                        type_scale::CAPTION,
                        theme.text_dim,
                        type_scale::line_height::TERM,
                    ),
                    TerminalStatusBarMarker,
                ));
            });
        }
    });
}

/// 终端面板激活时清除 `InputFocus`，使终端独占键盘（`handle_terminal_keyboard`
/// 在无焦点时才捕获），避免与对话输入区同步输入。
///
/// 显隐本身由 `derive_panel_visibility` 统一派生（v8 M5-T3），本系统只负责
/// 键盘焦点的移交。
pub fn apply_terminal_view_focus(
    registry: Res<crate::panels::PanelRegistry>,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
) {
    if registry.is_changed() && registry.is_visible(crate::panels::ids::TERMINAL) {
        focus.clear();
    }
}

/// 处理 ✕ 关闭分屏按钮：切回对话 + 收起分屏。
fn handle_close_button(
    q_btn: Query<(), (With<TerminalCloseButtonMarker>, Added<Pressed>)>,
    mut deactivate: MessageWriter<crate::panels::PanelDeactivate>,
    mut activate: MessageWriter<crate::panels::PanelActivate>,
    mut layout: ResMut<crate::panels::SlotLayout>,
) {
    for _ in q_btn.iter() {
        deactivate.write(crate::panels::PanelDeactivate {
            id: crate::panels::ids::TERMINAL,
        });
        activate.write(crate::panels::PanelActivate {
            id: crate::panels::ids::CHAT,
        });
        layout.set_open(crate::panels::Slot::SideDock, false);
    }
}

/// 处理 ＋ 新建 tab 按钮：发 [`tabs::SpawnTabRequest`]。
fn handle_new_tab_button(
    q_btn: Query<(), (With<TerminalNewTabButtonMarker>, Added<Pressed>)>,
    mut writer: MessageWriter<tabs::SpawnTabRequest>,
    project_root: Option<Res<crate::file_panel::ProjectRoot>>,
) {
    for _ in q_btn.iter() {
        let cwd = project_root
            .as_deref()
            .map(|r| r.path.clone())
            .unwrap_or_else(std::env::temp_dir);
        writer.write(tabs::SpawnTabRequest { cwd });
    }
}

/// 处理「清屏」按钮：给当前激活 tab 发 [`tabs::ClearTabRequest`]。
///
/// 这条按压处理曾经缺失——`TerminalClearButtonMarker` 只有 spawn、`tabs::
/// handle_clear_tab_requests` 只有 reader，没人写请求 → 「清屏」是死钮。
fn handle_clear_button(
    q_btn: Query<(), (With<TerminalClearButtonMarker>, Added<Pressed>)>,
    tabs: Res<TerminalTabs>,
    mut writer: MessageWriter<tabs::ClearTabRequest>,
) {
    if q_btn.iter().count() == 0 {
        return;
    }
    let Some(tab) = tabs.active.and_then(|idx| tabs.tabs.get(idx)).copied() else {
        tracing::warn!("终端没有激活 tab，忽略清屏请求");
        return;
    };
    writer.write(tabs::ClearTabRequest { tab });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 收集本帧写出的 `ClearTabRequest` 目标 tab。
    #[derive(Resource, Default)]
    struct Collected(Vec<Entity>);

    fn collect(mut reader: MessageReader<tabs::ClearTabRequest>, mut out: ResMut<Collected>) {
        for req in reader.read() {
            out.0.push(req.tab);
        }
    }

    fn clear_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<tabs::ClearTabRequest>()
            .init_resource::<Collected>()
            .init_resource::<TerminalTabs>()
            .add_systems(Update, (handle_clear_button, collect).chain());
        app
    }

    fn set_tabs(app: &mut App, tabs: Vec<Entity>, active: Option<usize>) {
        let mut res = app.world_mut().resource_mut::<TerminalTabs>();
        res.tabs = tabs;
        res.active = active;
    }

    /// 按下清屏钮 → 请求落在激活 tab 上。
    #[test]
    fn clear_button_targets_the_active_tab() {
        let mut app = clear_app();
        let first = app.world_mut().spawn(()).id();
        let active = app.world_mut().spawn(()).id();
        set_tabs(&mut app, vec![first, active], Some(1));
        app.world_mut()
            .spawn((Button, TerminalClearButtonMarker, Pressed));
        app.update();
        assert_eq!(
            app.world().resource::<Collected>().0,
            vec![active],
            "清屏应只作用于当前激活 tab"
        );
    }

    /// 没按按钮不发请求；无激活 tab 时按了也不发（且不能 panic）。
    #[test]
    fn clear_button_writes_nothing_without_press_or_active_tab() {
        let mut app = clear_app();
        app.update();
        assert!(app.world().resource::<Collected>().0.is_empty());

        let dead = app.world_mut().spawn(()).id();
        set_tabs(&mut app, vec![dead], None);
        app.world_mut()
            .spawn((Button, TerminalClearButtonMarker, Pressed));
        app.update();
        assert!(
            app.world().resource::<Collected>().0.is_empty(),
            "无激活 tab 时不该发出清屏请求"
        );
    }

    /// 接线护栏：处理系统必须既定义又注册进 Update（只定义 = 死钮）。
    #[test]
    fn clear_button_handler_is_registered() {
        let src = include_str!("mod.rs");
        assert!(
            src.matches("handle_clear_button").count() >= 2,
            "handle_clear_button 必须定义并出现在插件的 Update 注册里"
        );
    }
}
