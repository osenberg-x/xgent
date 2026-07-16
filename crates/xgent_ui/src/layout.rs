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

/// 状态栏容器。
#[derive(Component, Default)]
pub struct StatusBarMarker;

/// 文件面板折叠状态。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FilePanelCollapsed(pub bool);

/// 布局插件。
pub struct LayoutPlugin;

impl Plugin for LayoutPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Theme>()
            .init_resource::<FilePanelCollapsed>()
            .add_systems(Startup, spawn_layout)
            .add_systems(Update, toggle_file_panel_width.after(crate::shortcuts::handle_hotkey_triggers));
    }
}

/// 启动时 spawn 全屏根节点与三区容器。
pub(crate) fn spawn_layout(mut commands: Commands, theme: Res<Theme>) {
    // 相机（UI 渲染需要）
    commands.spawn(Camera2d);

    let font = theme.font_size;
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
                ))
                .with_children(|bar| {
                    bar.spawn((
                        Text::new("XGent"),
                        TextFont {
                            font_size: px_size(font + 2.0),
                            ..default()
                        },
                        TextColor(theme.text),
                    ));
                });

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
                            width: px(size::FILE_PANEL_W),
                            height: Val::Percent(100.0),
                            padding: UiRect::all(px(crate::theme::space::SM)),
                            flex_direction: FlexDirection::Column,
                            border: UiRect::right(px(1.0)),
                            overflow: Overflow::clip_y(),
                            ..default()
                        },
                        BackgroundColor(theme.panel),
                        BorderColor::all(theme.border),
                        FilePanelMarker,
                    ));

                    // 对话主区
                    main.spawn((
                        Node {
                            flex_grow: 1.0,
                            height: Val::Percent(100.0),
                            padding: UiRect::all(px(crate::theme::space::SM)),
                            flex_direction: FlexDirection::Column,
                            border: UiRect::left(px(1.0)),
                            // 主轴纵向需 min_height:0 防 message_list 撑破；
                            // 交叉轴横向需 min_width:0 防文本宽内容挤占文件面板。
                            min_width: Val::ZERO,
                            min_height: Val::ZERO,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(theme.panel),
                        BorderColor::all(theme.border),
                        ChatPanelMarker,
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

/// 文件面板折叠状态变化时更新宽度。
fn toggle_file_panel_width(
    collapsed: Res<FilePanelCollapsed>,
    mut q: Query<&mut Node, With<FilePanelMarker>>,
) {
    if !collapsed.is_changed() {
        return;
    }
    let Ok(mut node) = q.single_mut() else {
        return;
    };
    node.width = if collapsed.0 {
        Val::Px(0.0)
    } else {
        px(size::FILE_PANEL_W)
    };
}

/// 把 f32 转为 [`FontSize`]。
fn px_size(v: f32) -> FontSize {
    FontSize::Px(v)
}
