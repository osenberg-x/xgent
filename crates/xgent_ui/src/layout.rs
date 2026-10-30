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

/// 对话面板（MVP-2 起位于 SideDock）。
#[derive(Component, Default)]
pub struct ChatPanelMarker;

/// 主区 Center 面板容器（3D / 编辑器 / 预览 / 差异 / 终端）。
#[derive(Component, Default)]
pub struct CenterHostMarker;

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
        // 布局模式（宇宙 / 经典 2D 回退）也属于布局层：切换时搬视图与槽位。
        if !app.is_plugin_added::<crate::layout_mode::LayoutModePlugin>() {
            app.add_plugins(crate::layout_mode::LayoutModePlugin);
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
    // `UiPickingSettings.require_markers = true`（xgent_app 启动时设定）会让 UI 拾取
    // 后端**跳过没有 `UiPickingCamera` 的相机**（见 bevy_ui::picking_backend::ui_picking
    // 的 camera_query 过滤），后果是全局零命中、所有鼠标交互失效。因此这个标记必须
    // 与相机一起 spawn，不能只给节点加 `Pickable`。
    commands.spawn((Camera2d, bevy::ui::picking_backend::UiPickingCamera));

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

                    // 主区 CenterHost（3D / 编辑器 / 预览 / 差异 / 终端）
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
                        Themed {
                            role: ThemeRole::Canvas,
                            generation,
                        },
                        CenterHostMarker,
                        crate::resize::SplitLeafMarker {
                            id: crate::panels::split::leaf_ids::CHAT,
                            axis: crate::panels::split::Axis::Horizontal,
                        },
                    ));

                    // 右侧栏：对话常驻（SideDock）。编辑器/终端/差异在 CenterHost。
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
                        // 右侧栏只在槽内有可见面板时出现：宇宙里是常驻对话，
                        // 经典回退里是编辑器/终端（都没开时整块收起，不留空条）。
                        crate::panels::PanelVisibility::of_slot_panels(
                            crate::panels::Slot::SideDock,
                        ),
                        crate::resize::SplitLeafMarker {
                            id: crate::panels::split::leaf_ids::SIDE_DOCK,
                            axis: crate::panels::split::Axis::Horizontal,
                        },
                    ))
                    .with_children(|side| {
                        let chat = side
                            .spawn((
                                Node {
                                    display: Display::Flex,
                                    width: Val::Percent(100.0),
                                    flex_direction: FlexDirection::Column,
                                    overflow: Overflow::clip(),
                                    ..xui::region_fill()
                                },
                                BackgroundColor(theme.bg),
                                Themed {
                                    role: ThemeRole::Canvas,
                                    generation,
                                },
                                ChatPanelMarker,
                                crate::panels::PanelVisibility::of_panel(
                                    crate::panels::ids::CHAT,
                                    crate::panels::Slot::SideDock,
                                ),
                            ))
                            .id();
                        // 对话迁到 SideDock；sticky 表示常驻，页签不提供关闭。
                        register.write(crate::panels::PanelRegister {
                            desc: crate::panels::PanelDescriptor::new(
                                crate::panels::ids::CHAT,
                                crate::panels::Slot::SideDock,
                                "panel-chat",
                                "chat",
                                0,
                            )
                            .with_default_visible()
                            .sticky(),
                            view: Some(chat),
                        });
                    });
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

    /// A1 的另一半：`require_markers = true` 时 UI 拾取后端**只认带 `UiPickingCamera`
    /// 的相机**（`bevy_ui::picking_backend::ui_picking` 的 camera_query 过滤）。相机缺
    /// 这个标记 = 全局零命中 = 所有鼠标交互静默失效，而节点侧的 `Pickable` 再多也照样
    /// 全绿——headless 测试看不见这件事，只能锁组件契约。
    #[test]
    fn layout_camera_carries_ui_picking_camera() {
        let mut app = layout_app();
        let mut cameras = app.world_mut().query::<(
            Entity,
            &Camera,
            Has<bevy::ui::picking_backend::UiPickingCamera>,
        )>();
        let found: Vec<bool> = cameras
            .iter(app.world())
            .map(|(_, _, can_pick)| can_pick)
            .collect();
        assert_eq!(found.len(), 1, "布局应恰好 spawn 一个 UI 相机：{found:?}");
        assert!(
            found[0],
            "UI 相机必须带 UiPickingCamera：require_markers 下缺它会导致零命中、点击全废"
        );
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
