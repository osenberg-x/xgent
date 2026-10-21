//! 布局：六个静态层根 + BASE 内的顶栏 / 主区 / 状态栏。
//!
//! v8 M1：所有浮层挂固定层根，业务模块不再裸写 `GlobalZIndex`；
//! BASE 层内的 `MainArea`/`ChatPanel` 保留 `Overflow::clip()`（区域内容确实要裁），
//! 但任何「要浮在外面」的东西必须迁到层根，不允许留在裁剪子树里。

use bevy::prelude::*;

use crate::theme::{ThemeHandle, ThemeRole, Themed, size, z};

/// 根 UI 节点（全屏，承载所有层根）。
#[derive(Component, Default)]
pub struct UiRoot;

/// 静态层根标记：携带该层的 z 值。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerRoot(pub i32);

/// 主布局层（顶栏 / 主区 / 状态栏）。
#[derive(Component, Default)]
pub struct BaseLayer;
/// 侧区内联浮层。
#[derive(Component, Default)]
pub struct DockLayer;
/// 抽屉层（文件 / 历史）。
#[derive(Component, Default)]
pub struct DrawerLayer;
/// 下拉层（补全 / 菜单 / 查找条）。
#[derive(Component, Default)]
pub struct DropdownLayer;
/// tooltip 层。
#[derive(Component, Default)]
pub struct TooltipLayer;
/// toast 层。
#[derive(Component, Default)]
pub struct ToastLayer;

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
///
/// v8 M5 起由面板注册表裁决（`Slot::Overlay` 内的 `file-drawer` 面板），
/// 本资源是**派生视图**（由 `panels::visibility::sync_file_drawer_open` 写回）。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileDrawerOpen(pub bool);

/// 布局插件。
pub struct LayoutPlugin;

impl Plugin for LayoutPlugin {
    fn build(&self, app: &mut App) {
        // 面板注册表（幂等：宿主可能已装配）
        if !app.is_plugin_added::<crate::panels::PanelsPlugin>() {
            app.add_plugins(crate::panels::PanelsPlugin);
        }
        app.init_resource::<crate::theme::ThemeHandle>()
            .init_resource::<crate::theme::Theme>()
            .init_resource::<FileDrawerOpen>()
            .add_systems(Startup, spawn_layout);
    }
}

/// 启动时 spawn 根节点、六个层根与各区域容器。
pub(crate) fn spawn_layout(
    mut commands: Commands,
    handle: Res<ThemeHandle>,
    tree: Res<crate::panels::split::SplitTree>,
    mut register: MessageWriter<crate::panels::PanelRegister>,
) {
    let theme = handle.theme;
    let generation = handle.generation;
    // 右侧分屏的初始宽度取自分割树（v8 M5：宽度由分割树驱动）。
    let side_view_w = tree
        .leaf_size(crate::panels::split::leaf_ids::SIDE_DOCK)
        .map(|s| s.preferred())
        .unwrap_or(size::CONTEXT_W_DEFAULT);
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
            // ===== BASE：主布局 =====
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    position_type: PositionType::Absolute,
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                GlobalZIndex(z::BASE),
                BackgroundColor(theme.bg),
                Themed {
                    role: ThemeRole::Canvas,
                    generation,
                },
                LayerRoot(z::BASE),
                BaseLayer,
            ))
            .with_children(|base| {
                // 顶栏
                base.spawn((
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
                    Themed {
                        role: ThemeRole::Surface,
                        generation,
                    },
                    TopBarMarker,
                ));

                // 主区（图标轨 + 对话 + 分屏；文件面板已抽屉化）
                base.spawn((
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
                        Themed {
                            role: ThemeRole::Surface,
                            generation,
                        },
                        ActivityBarMarker,
                    ));

                    // 对话主区
                    let chat = main
                        .spawn((
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
                            Themed {
                                role: ThemeRole::Canvas,
                                generation,
                            },
                            ChatPanelMarker,
                            crate::resize::SplitLeafMarker {
                                id: crate::panels::split::leaf_ids::CHAT,
                                axis: crate::panels::split::Axis::Horizontal,
                            },
                        ))
                        .id();
                    // v8 M5：对话区注册为 `Slot::Center` 面板（主区默认可见）。
                    register.write(crate::panels::PanelRegister {
                        desc: crate::panels::PanelDescriptor::new(
                            crate::panels::ids::CHAT,
                            crate::panels::Slot::Center,
                            "panel-chat",
                            "chat",
                            0,
                        )
                        .with_default_visible(),
                        view: Some(chat),
                    });

                    // 右拖拽手柄（v8 M5：由 `resize::sync_split_handles` 遍历分割树生成）
                    // 上下文面板（预览/差异/终端；默认展开，宽度走分割树）
                    main.spawn((
                        Node {
                            width: px(side_view_w),
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
                        Themed {
                            role: ThemeRole::Surface,
                            generation,
                        },
                        SideViewMarker,
                        // v8 M5-T3：分屏容器的显隐由派生系统统一写。
                        crate::panels::PanelVisibility::of_slot(crate::panels::Slot::SideDock),
                        // v8 M5-T5：宽度由分割树的 `side-dock` 叶子驱动。
                        crate::resize::SplitLeafMarker {
                            id: crate::panels::split::leaf_ids::SIDE_DOCK,
                            axis: crate::panels::split::Axis::Horizontal,
                        },
                    ));
                });

                // 状态栏
                base.spawn((
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
                    Themed {
                        role: ThemeRole::Surface,
                        generation,
                    },
                    StatusBarMarker,
                ));
            });

            // ===== 其余五个静态层根 =====
            root.spawn((crate::kit::layer_root(z::DOCK), DockLayer));
            root.spawn((crate::kit::layer_root(z::DRAWER), DrawerLayer));
            root.spawn((crate::kit::layer_root(z::DROPDOWN), DropdownLayer));
            root.spawn((crate::kit::layer_root(z::TOOLTIP), TooltipLayer));
            root.spawn((crate::kit::layer_root(z::TOAST), ToastLayer));
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ui::GlobalZIndex;

    fn layout_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(LayoutPlugin);
        for _ in 0..3 {
            app.update();
        }
        app
    }

    /// A1：六个静态层根存在，z 来自 `theme::z`，且层根不带 `Pickable`。
    #[test]
    fn six_layer_roots_have_expected_z_and_no_pickable() {
        let mut app = layout_app();

        let mut query = app.world_mut().query::<(&LayerRoot, &GlobalZIndex)>();
        let mut found: Vec<(i32, i32)> = query
            .iter(app.world())
            .map(|(layer, z_index)| (layer.0, z_index.0))
            .collect();
        found.sort_unstable();

        assert_eq!(
            found,
            vec![
                (z::BASE, z::BASE),
                (z::DOCK, z::DOCK),
                (z::DRAWER, z::DRAWER),
                (z::DROPDOWN, z::DROPDOWN),
                (z::TOOLTIP, z::TOOLTIP),
                (z::TOAST, z::TOAST),
            ]
        );

        let mut with_pickable = app
            .world_mut()
            .query_filtered::<Entity, (With<LayerRoot>, With<bevy::picking::Pickable>)>();
        assert_eq!(
            with_pickable.iter(app.world()).count(),
            0,
            "层根不得带 Pickable（require_markers 下应完全惰性）"
        );
    }

    /// A1：顶栏 / 主区 / 状态栏都在 BASE 层根之下。
    #[test]
    fn main_regions_live_under_base_layer() {
        let mut app = layout_app();

        let markers: [(&str, Entity); 3] = [
            ("topbar", first_with::<TopBarMarker>(&mut app)),
            ("main", first_with::<MainAreaMarker>(&mut app)),
            ("status", first_with::<StatusBarMarker>(&mut app)),
        ];

        for (name, entity) in markers {
            let mut current = entity;
            let mut on_base = false;
            for _ in 0..8 {
                if let Some(layer) = app.world().get::<LayerRoot>(current)
                    && layer.0 == z::BASE
                {
                    on_base = true;
                    break;
                }
                match app.world().get::<ChildOf>(current) {
                    Some(parent) => current = parent.parent(),
                    None => break,
                }
            }
            assert!(on_base, "{name} 的祖先链应包含 BASE 层根");
        }
    }

    fn first_with<T: Component>(app: &mut App) -> Entity {
        let mut query = app.world_mut().query_filtered::<Entity, With<T>>();
        query
            .iter(app.world())
            .next()
            .unwrap_or_else(|| panic!("缺少组件 {}", std::any::type_name::<T>()))
    }
}
