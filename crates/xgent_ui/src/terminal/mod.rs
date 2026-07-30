//! XGent 业务终端层（F-19）。
//!
//! 详见 `doc/design/terminal-design.md` §2-§3、ADR-0011/0012、`CONTEXT.md`
//! 「终端（F-19，P1）」。
//!
//! 职责：
//! - 多 tab 管理（每 tab 一个独立 PTY 会话，[`TerminalTabs`] Resource）
//! - PTY IO 桥接（ECS Messages → async backend，[`io`] 模块）
//! - 输出历史渲染（vte 解析 → `RenderHistory` → 虚拟滚动，[`output`] 模块）
//! - UI 侧行编辑（光标/Backspace/Enter，[`input`] 模块）
//! - 视图切换（[`crate::editor::SideViewContent::Terminal`] 互斥子视图）
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
pub mod output;
pub mod tabs;

use std::path::PathBuf;

use bevy::prelude::*;

use crate::editor::SideViewContent;
use crate::i18n::tr;
use crate::theme::{Theme, px, space};
use xgent_settings::Localizer;
use xgent_terminal::{LocalPtyBackend, TerminalBackend};
use xui::scroll_area::{ScrollArea, StickToBottom};

/// 把 f32 转为 [`FontSize`]。
fn px_size(v: f32) -> FontSize {
    FontSize::Px(v)
}

/// 终端视图容器标记（挂于 [`crate::layout::SideViewMarker`] 下，初始隐藏）。
///
/// 显隐由 [`apply_terminal_view_visibility`] 据 [`SideViewContent`] 统一写，
/// 避免多系统并发写同一 `Node.display`（B0001）。
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

/// 终端状态栏标记（tv-statusbar，显示运行状态/shell/cwd）。
#[derive(Component, Default)]
pub struct TerminalStatusBarMarker;

/// 终端 IO runtime（由 `xgent_app` 注入 tokio handle + backend）。
///
/// `backend` 默认注入 [`LocalPtyBackend`]；若未注入，终端功能不可用（spawn 请求
/// 静默丢弃并记 warn）。对齐 [`crate::editor::io::EditorIoRuntime`] 的注入模式。
#[derive(Resource)]
pub struct TerminalIoRuntime {
    /// tokio runtime handle。
    pub handle: Option<tokio::runtime::Handle>,
    /// PTY 后端实例（`Arc` 共享给 spawn 的 task）。
    pub backend: Option<std::sync::Arc<dyn TerminalBackend>>,
}

impl Default for TerminalIoRuntime {
    fn default() -> Self {
        Self {
            handle: None,
            backend: None,
        }
    }
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
            .add_message::<io::TerminalExited>()
            .init_resource::<TerminalTabs>()
            .init_resource::<TerminalIoRuntime>()
            .init_resource::<output::TerminalResizeTracker>()
            .insert_resource(bridge_tx)
            .insert_resource(bridge_rx)
            .add_systems(Startup, spawn_terminal_view.after(crate::layout::spawn_layout))
            .add_systems(
                Update,
                (
                    tabs::handle_spawn_tab_requests,
                    tabs::handle_close_tab_requests,
                    tabs::handle_switch_tab_requests,
                    tabs::handle_clear_tab_requests,
                    io::handle_pty_events,
                    io::handle_terminal_input,
                    output::handle_terminal_resize,
                    io::handle_terminal_resize,
                    output::append_output_chunks,
                    apply_terminal_view_visibility,
                    tabs::rebuild_terminal_tabs,
                    input::handle_terminal_keyboard,
                    output::update_output_visibility,
                    output::update_status_bar,
                    handle_close_button,
                    handle_new_tab_button,
                )
                    .chain(),
            );
    }
}

/// 启动时在右侧分屏容器内 spawn 终端视图（初始隐藏）。
///
/// 终端视图是右侧分屏的内容之一（与编辑器/文件预览互斥）；分屏本身由
/// [`crate::layout::SideViewMarker`] 容器承载，展开/收起由
/// [`crate::layout::SideViewCollapsed`] 统一控制。
fn spawn_terminal_view(
    mut commands: Commands,
    q_side: Query<Entity, With<crate::layout::SideViewMarker>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
) {
    let Ok(side) = q_side.single() else {
        return;
    };
    let font = theme.font_size;
    commands
        .entity(side)
        .with_children(|p| {
            // 终端视图容器：初始隐藏（由 apply_terminal_view_visibility 据 SideViewContent 切换）
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip(),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(theme.bg),
                TerminalViewMarker,
            ))
            .with_children(|view| {
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
                    BackgroundColor(theme.bar),
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
                        Node {
                            width: px(24.0),
                            height: px(24.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(px(4.0)),
                            ..default()
                        },
                        Text::new(tr(&loc, "terminal-new-tab")),
                        TextFont {
                            font_size: px_size(font),
                            ..default()
                        },
                        TextColor(theme.text_dim),
                        TerminalNewTabButtonMarker,
                    ));
                    // 清屏
                    head.spawn((
                        Button,
                        Node {
                            width: px(24.0),
                            height: px(24.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(px(4.0)),
                            ..default()
                        },
                        Text::new(tr(&loc, "terminal-clear")),
                        TextFont {
                            font_size: px_size(font - 2.0),
                            ..default()
                        },
                        TextColor(theme.text_dim),
                        TerminalClearButtonMarker,
                    ));
                    // ✕ 关闭分屏
                    head.spawn((
                        Button,
                        Node {
                            width: px(24.0),
                            height: px(24.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(px(4.0)),
                            ..default()
                        },
                        Text::new(tr(&loc, "terminal-close")),
                        TextFont {
                            font_size: px_size(font),
                            ..default()
                        },
                        TextColor(theme.text_dim),
                        TerminalCloseButtonMarker,
                    ));
                });

                // tv-body：输出历史容器（ScrollArea 贴底滚动，每 tab 动态 spawn 行）
                let mut output_area = ScrollArea::vertical();
                output_area.node.padding = UiRect::horizontal(px(space::SM));
                view.spawn((
                    output_area,
                    StickToBottom::default(),
                    BackgroundColor(theme.bg),
                    TerminalOutputMarker,
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
                    BackgroundColor(theme.bar),
                    BorderColor::all(theme.border),
                ))
                .with_children(|line| {
                    // prompt ❯
                    line.spawn((
                        Text::new(tr(&loc, "terminal-prompt")),
                        TextFont {
                            font_size: px_size(font),
                            ..default()
                        },
                        TextColor(theme.accent),
                    ));
                    // 输入文本节点（由 input 模块更新内容）
                    line.spawn((
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::ZERO,
                            ..default()
                        },
                        Text::new(""),
                        TextFont {
                            font_size: px_size(font),
                            ..default()
                        },
                        TextColor(theme.text),
                        TerminalInputMarker,
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
                    BackgroundColor(theme.bar),
                    BorderColor::all(theme.border),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        Text::new(""),
                        TextFont {
                            font_size: px_size(font - 2.0),
                            ..default()
                        },
                        TextColor(theme.text_dim),
                        TerminalStatusBarMarker,
                    ));
                });
            });
        });
}

/// 据 [`SideViewContent`] 切换 [`TerminalViewMarker`] 显隐，并展开分屏。
///
/// 与 [`crate::editor::apply_editor_view_visibility`] 对称——本系统只写
/// `TerminalViewMarker` 的 `Node.display`，不碰编辑器/预览容器（由编辑器模块
/// 自管），避免 B0001 跨系统可变访问冲突。
///
/// 切到终端视图时清除 `InputFocus`，使终端独占键盘（`handle_terminal_keyboard`
/// 在无焦点时才捕获），避免与对话输入区同步输入。
pub fn apply_terminal_view_visibility(
    content: Res<SideViewContent>,
    mut collapsed: ResMut<crate::layout::SideViewCollapsed>,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    mut q: Query<&mut Node, With<TerminalViewMarker>>,
) {
    // Terminal 时展开分屏（与编辑器模块的展开逻辑各自独立触发，幂等）
    if *content == SideViewContent::Terminal && collapsed.0 {
        collapsed.0 = false;
    }
    // 切到终端视图时清除焦点，让终端捕获键盘
    if content.is_changed() && *content == SideViewContent::Terminal {
        focus.clear();
    }
    let display = if *content == SideViewContent::Terminal {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut q {
        if node.display != display {
            node.display = display;
        }
    }
}

/// 处理 ✕ 关闭分屏按钮：切回对话 + 收起分屏。
fn handle_close_button(
    q_btn: Query<&Interaction, (With<TerminalCloseButtonMarker>, Changed<Interaction>)>,
    mut content: ResMut<SideViewContent>,
    mut collapsed: ResMut<crate::layout::SideViewCollapsed>,
) {
    for interaction in q_btn.iter() {
        if *interaction == Interaction::Pressed {
            *content = SideViewContent::None;
            collapsed.0 = true;
        }
    }
}

/// 处理 ＋ 新建 tab 按钮：发 [`tabs::SpawnTabRequest`]。
fn handle_new_tab_button(
    q_btn: Query<&Interaction, (With<TerminalNewTabButtonMarker>, Changed<Interaction>)>,
    mut writer: MessageWriter<tabs::SpawnTabRequest>,
    project_root: Option<Res<crate::file_panel::ProjectRoot>>,
) {
    for interaction in q_btn.iter() {
        if *interaction == Interaction::Pressed {
            let cwd = project_root
                .as_deref()
                .map(|r| r.path.clone())
                .unwrap_or_else(|| std::env::temp_dir());
            writer.write(tabs::SpawnTabRequest { cwd });
        }
    }
}
