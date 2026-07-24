//! 三区布局：顶栏 + 主区（文件面板 + 对话主区）+ 状态栏。
//!
//! 各区域挂 marker 组件（如 [`TopBarMarker`]），供子系统（chat_panel、status_bar 等）
//! 在启动时向其挂子节点。
//!
//! 文件面板可通过 `Cmd+B` 切换折叠/展开，折叠时宽度变为 0。

use bevy::prelude::*;

use crate::theme::{Theme, size};

/// 根 UI 节点（全屏 flex 列容器）。
#[derive(Component, Default)]
pub struct UiRoot;

/// 顶栏容器。
#[derive(Component, Default)]
pub struct TopBarMarker;

/// 主区容器（flex:1）。
#[derive(Component, Default)]
pub struct MainAreaMarker;

/// 文件面板容器。
#[derive(Component, Default)]
pub struct FilePanelMarker;

/// 对话主区容器。
#[derive(Component, Default)]
pub struct ChatPanelMarker;

/// 右侧分屏容器（编辑器/文件预览，默认隐藏，点击文件后展开，与对话主区并排占一半）。
#[derive(Component, Default)]
pub struct SideViewMarker;

/// 状态栏容器。
#[derive(Component, Default)]
pub struct StatusBarMarker;

/// 文件面板折叠状态。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FilePanelCollapsed(pub bool);

/// 右侧分屏折叠状态。
///
/// `true`（默认）= 分屏收起（`display:none`），对话主区独占；
/// `false` = 分屏展开，与对话主区各占一半并排显示。
/// 由点击文件节点（代码文件→编辑器、非代码文件→预览）或 `Ctrl+\` 切换。
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
                toggle_panel_visibility
                    .after(crate::shortcuts::handle_hotkey_triggers),
            );
    }
}

/// 启动时 spawn 全屏根节点与三区容器。
pub(crate) fn spawn_layout(mut commands: Commands, theme: Res<Theme>, widths: Res<crate::resize::PanelWidths>) {
    // 相机（UI 渲染需要）
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
        .with_children(|parent| {
            // 顶栏
            parent
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: px(size::TOP_BAR_H),
                        padding: UiRect::horizontal(px(crate::theme::space::MD)),
                        align_items: AlignItems::Center,
                        flex_direction: FlexDirection::Row,
                        column_gap: px(crate::theme::space::SM),
                        border: UiRect::bottom(px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme.bar),
                    BorderColor::all(theme.border),
                    TopBarMarker,
                ));

            // 主区
            parent
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Row,
                        // 防止子容器（文件面板/对话主区）被各自内容撑破后溢出：
                        // min_height: 0 允许纵向收缩，overflow: clip 兜底裁剪。
                        min_height: Val::ZERO,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    MainAreaMarker,
                ))
                .with_children(|main| {
                    // 文件面板
                    main.spawn((
                        Node {
                            width: px(widths.file_panel),
                            height: Val::Percent(100.0),
                            padding: UiRect::all(px(crate::theme::space::SM)),
                            flex_direction: FlexDirection::Column,
                            flex_shrink: 0.0,
                            overflow: Overflow::clip_y(),
                            border: UiRect::right(px(1.0)),
                            ..default()
                        },
                        BackgroundColor(theme.panel),
                        BorderColor::all(theme.border),
                        FilePanelMarker,
                    ));

                    // 左拖拽手柄（文件面板 ↔ 对话主区）
                    main.spawn(crate::resize::handle_bundle(crate::resize::ResizeEdge::Left));

                    // 对话主区
                    main.spawn((
                        Node {
                            flex_grow: 1.0,
                            height: Val::Percent(100.0),
                            padding: UiRect::all(px(crate::theme::space::SM)),
                            flex_direction: FlexDirection::Column,
                            // 主轴纵向需 min_height:0 防 message_list 撑破；
                            // 交叉轴横向需 min_width:0 防文本宽内容挤占文件面板。
                            min_width: Val::ZERO,
                            min_height: Val::ZERO,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(theme.panel),
                        ChatPanelMarker,
                    ));

                    // 右拖拽手柄（对话主区 ↔ 右侧分屏，分屏收起时隐藏）
                    main.spawn(crate::resize::handle_bundle(crate::resize::ResizeEdge::Right));

                    // 右侧分屏容器（默认隐藏；展开时据 PanelWidths.side_view 显式宽度）
                    // 编辑器视图 / 文件预览 由 editor 模块 spawn 挂入此容器。
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
                        BackgroundColor(theme.bg),
                        BorderColor::all(theme.border),
                        SideViewMarker,
                    ));
                });

            // 状态栏
            parent.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: px(size::STATUS_BAR_H),
                    padding: UiRect::horizontal(px(crate::theme::space::SM)),
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Row,
                    border: UiRect::top(px(1.0)),
                    ..default()
                },
                BackgroundColor(theme.bar),
                BorderColor::all(theme.border),
                StatusBarMarker,
            ));
        });
}

/// 折叠状态变化时更新面板宽度与手柄显隐。
///
/// - 文件面板折叠：宽 0 + 左手柄 `display:none`；
/// - 右侧分屏折叠：分屏 + 右手柄 `display:none`。
///
/// 合并为单系统以避免两个系统各自 `Query<(&ResizeEdgeMarker, &mut Node)>`
/// 产生的 B0001 跨系统可变访问冲突（With 无法证明按枚举值不相交）。
/// 展开态的宽度由 [`crate::resize::apply_panel_widths`] 据 [`crate::resize::PanelWidths`] 写入。
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
        let display = if file_collapsed.0 { Display::None } else { Display::Flex };
        for (marker, mut node) in q_handles.iter_mut() {
            if marker.0 == crate::resize::ResizeEdge::Left {
                node.display = display;
            }
        }
    }
    if side_changed {
        let display = if side_collapsed.0 { Display::None } else { Display::Flex };
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

