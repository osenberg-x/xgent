//! 三区布局：顶栏 + 活动栏 + 侧栏 + 主区（对话 + 分屏）+ 状态栏。
//!
//! v2 重构：引入 ActivityBar（48px 窄条），将文件/编辑器/终端/设置入口
//! 抽象为图标导航。主区为对话 + SideView（编辑器/预览/终端互斥子视图）。
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

/// 文件面板折叠状态。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FilePanelCollapsed(pub bool);

/// 右侧分屏折叠状态。
///
/// `true`（默认）= 分屏收起，对话主区独占；
/// `false` = 分屏展开，与对话主区并排。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SideViewCollapsed(pub bool);

/// 布局插件。
pub struct LayoutPlugin;

impl Plugin for LayoutPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Theme>()
            .init_resource::<FilePanelCollapsed>()
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
                BackgroundColor(theme.panel),
                BorderColor::all(theme.border),
                TopBarMarker,
            ));

            // ===== 主区（活动栏 + 侧栏 + 对话 + 分屏）=====
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
                // 活动栏（48px 固定宽度）
                main.spawn((
                    Node {
                        width: px(size::ACTIVITY_BAR_W),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::vertical(px(crate::theme::space::SM)),
                        row_gap: px(crate::theme::space::XS),
                        flex_shrink: 0.0,
                        border: UiRect::right(px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme.panel),
                    BorderColor::all(theme.line),
                    ActivityBarMarker,
                ));

                // 文件面板（侧栏）
                main.spawn((
                    Node {
                        width: px(widths.file_panel),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        flex_shrink: 0.0,
                        overflow: Overflow::clip_y(),
                        border: UiRect::right(px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme.panel),
                    BorderColor::all(theme.line),
                    FilePanelMarker,
                ));

                // 左拖拽手柄
                main.spawn(crate::resize::handle_bundle(
                    crate::resize::ResizeEdge::Left,
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
                    BackgroundColor(theme.surface),
                    ChatPanelMarker,
                ));

                // 右拖拽手柄
                main.spawn(crate::resize::handle_bundle(
                    crate::resize::ResizeEdge::Right,
                ));

                // 右侧分屏容器
                main.spawn((
                    Node {
                        width: px(widths.side_view),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        border: UiRect::left(px(1.0)),
                        min_width: Val::ZERO,
                        min_height: Val::ZERO,
                        overflow: Overflow::clip(),
                        display: Display::None,
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
                BackgroundColor(theme.panel),
                BorderColor::all(theme.border),
                StatusBarMarker,
            ));
        });
}

/// 折叠状态变化时更新面板宽度与手柄显隐。
pub(crate) fn toggle_panel_visibility(
    file_collapsed: Res<FilePanelCollapsed>,
    side_collapsed: Res<SideViewCollapsed>,
    mut q_file: Query<&mut Node, (With<FilePanelMarker>, Without<SideViewMarker>)>,
    mut q_side: Query<&mut Node, (With<SideViewMarker>, Without<FilePanelMarker>)>,
    mut q_handles: Query<
        (&crate::resize::ResizeEdgeMarker, &mut Node),
        (Without<FilePanelMarker>, Without<SideViewMarker>),
    >,
) {
    let file_changed = file_collapsed.is_changed();
    let side_changed = side_collapsed.is_changed();
    if !file_changed && !side_changed {
        return;
    }
    if file_changed {
        if file_collapsed.0 {
            if let Ok(mut node) = q_file.single_mut() {
                node.width = Val::Px(0.0);
            }
        }
        let display = if file_collapsed.0 {
            Display::None
        } else {
            Display::Flex
        };
        for (marker, mut node) in q_handles.iter_mut() {
            if marker.0 == crate::resize::ResizeEdge::Left {
                node.display = display;
            }
        }
    }
    if side_changed {
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
}
