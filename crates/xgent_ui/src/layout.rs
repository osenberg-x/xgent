//! 布局：顶栏 + 主区（图标轨 + 对话 + 分隔条 + 上下文面板）+ 状态栏。
//!
//! v7 目标形态（M5-T6 起）：文件面板抽屉化（`FileDrawerOpen` + 左侧 overlay），
//! 主区收四列——图标轨 → 对话主区 → 右手柄 → 上下文面板。
//!
//! 各区域挂 marker 组件，供子系统在启动时向其挂子节点。

use bevy::prelude::*;

use crate::theme::{Theme, size};

/// 根 UI 节点（全屏 flex 列容器）。
#[derive(Component, Default)]
pub struct UiRoot;

/// 顶栏容器。
#[derive(Component, Default)]
pub struct TopBarMarker;

/// 活动栏容器（48px 窄条，左侧图标导航）。
#[derive(Component, Default)]
pub struct ActivityBarMarker;

/// 侧栏容器（文件面板）。
#[derive(Component, Default)]
pub struct FilePanelMarker;

/// 对话主区容器。
#[derive(Component, Default)]
pub struct ChatPanelMarker;

/// 右侧分屏容器（编辑器/文件预览/终端，默认隐藏）。
#[derive(Component, Default)]
pub struct SideViewMarker;

/// 状态栏容器。
#[derive(Component, Default)]
pub struct StatusBarMarker;

/// 主区容器（活动栏 + 侧栏 + 对话 + 分屏的父节点）。
#[derive(Component, Default)]
pub struct MainAreaMarker;

/// 文件抽屉开关（v7 抽屉化：左侧 overlay，rail 文件钮 / `filepanel.toggle` 切换）。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileDrawerOpen(pub bool);

/// 右侧分屏（上下文面板）折叠状态。
///
/// `false`（默认）= 展开，与对话主区并排（v7）；
/// `true` = 收起，对话主区独占。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SideViewCollapsed(pub bool);

/// 布局插件。
pub struct LayoutPlugin;

impl Plugin for LayoutPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Theme>()
            .init_resource::<FileDrawerOpen>()
            .init_resource::<SideViewCollapsed>()
            .init_resource::<crate::resize::PanelWidths>()
            .add_systems(Startup, spawn_layout)
            .add_systems(
                Update,
                toggle_panel_visibility.after(crate::shortcuts::handle_hotkey_triggers),
            );
    }
}

/// 启动时 spawn 全屏根节点与各区域容器。
pub(crate) fn spawn_layout(
    mut commands: Commands,
    theme: Res<Theme>,
    widths: Res<crate::resize::PanelWidths>,
) {
    commands.spawn(Camera2d);

    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme.bg),
            UiRoot,
        ))
        .with_children(|root| {
            // ===== 顶栏 =====
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: px(size::TOP_BAR_H),
                    padding: UiRect::horizontal(px(crate::theme::space::LG)),
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(crate::theme::space::MD),
                    border: UiRect::bottom(px(1.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(theme.surface),
                BorderColor::all(theme.border),
                TopBarMarker,
            ));

            // ===== 主区（图标轨 + 对话 + 分屏；文件面板已抽屉化）=====
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Row,
                    min_height: Val::ZERO,
                    overflow: Overflow::clip(),
                    ..default()
                },
                MainAreaMarker,
            ))
            .with_children(|main| {
                // 活动栏（52px 固定宽度）
                main.spawn((
                    Node {
                        width: px(size::RAIL_W),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::vertical(px(crate::theme::space::SM)),
                        row_gap: px(crate::theme::space::XS),
                        flex_shrink: 0.0,
                        border: UiRect::right(px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme.surface),
                    BorderColor::all(theme.line),
                    ActivityBarMarker,
                ));

                // 对话主区
                main.spawn((
                    Node {
                        flex_grow: 1.0,
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        min_width: Val::ZERO,
                        min_height: Val::ZERO,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(theme.bg),
                    ChatPanelMarker,
                ));

                // 右拖拽手柄
                main.spawn(crate::resize::handle_bundle(
                    crate::resize::ResizeEdge::Right,
                ));
                // 上下文面板（预览/差异/终端；默认展开，宽度走 PanelWidths）
                main.spawn((
                    Node {
                        width: px(widths.side_view),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        border: UiRect::left(px(1.0)),
                        min_width: Val::ZERO,
                        min_height: Val::ZERO,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(theme.surface),
                    BorderColor::all(theme.line),
                    SideViewMarker,
                ));
            });

            // ===== 状态栏 =====
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: px(size::STATUS_BAR_H),
                    padding: UiRect::horizontal(px(crate::theme::space::LG)),
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(crate::theme::space::SM),
                    border: UiRect::top(px(1.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(theme.surface),
                BorderColor::all(theme.border),
                StatusBarMarker,
            ));
        });
}

/// 上下文面板折叠状态变化时更新面板与右手柄显隐（文件抽屉显隐由 file_panel 模块自理）。
pub(crate) fn toggle_panel_visibility(
    side_collapsed: Res<SideViewCollapsed>,
    mut q_side: Query<&mut Node, With<SideViewMarker>>,
    mut q_handles: Query<(&crate::resize::ResizeEdgeMarker, &mut Node), Without<SideViewMarker>>,
) {
    if !side_collapsed.is_changed() {
        return;
    }
    let display = if side_collapsed.0 {
        Display::None
    } else {
        Display::Flex
    };
    if let Ok(mut node) = q_side.single_mut() {
        node.display = display;
    }
    for (marker, mut node) in q_handles.iter_mut() {
        if marker.0 == crate::resize::ResizeEdge::Right {
            node.display = display;
        }
    }
}
