//! tooltip 定位回归（v8 M1 遗留缺陷）。
//!
//! 缺陷：`position_tooltips` 用 `&GlobalTransform` 取宿主位置，但 bevy 0.20 的 UI
//! 节点带的是 `UiGlobalTransform`（`Node` 的 `#[require]` 里没有 `GlobalTransform`）。
//! 于是每次查询都 `Err` → `continue` → 面板停留在 spawn 时的 `left/top = 0`，
//! 所有 tooltip 都贴在窗口左上角，而不是宿主控件旁边。
//!
//! 实测证据（2026-10-30，真实窗口 + 应用内虚拟指针）：齿轮按钮悬停后
//! `tips=[… left=Px(0.0) top=Px(0.0) …]`，同帧 `btnGT=0/32`（32 个 `Button`
//! 实体里 0 个带 `GlobalTransform`）。

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use xgent_ui::fonts::UiFonts;
use xgent_ui::kit::{KitPlugin, Tooltip, TooltipPanel};
use xgent_ui::theme::{Theme, space, z};

/// 装齐 UI 布局与 kit 系统的 headless 应用（无渲染器，仅布局与命中所需插件）。
fn tip_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(bevy::image::ImagePlugin::default())
        .add_plugins(bevy::window::WindowPlugin {
            primary_window: Some(Window {
                resolution: bevy::window::WindowResolution::new(800, 600),
                ..default()
            }),
            ..default()
        })
        .add_message::<bevy::window::WindowEvent>()
        .add_plugins(bevy::picking::input::PointerInputPlugin)
        .add_plugins(bevy::picking::PickingPlugin)
        .add_plugins(bevy::picking::InteractionPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::input_focus::InputFocusPlugin)
        .add_plugins(bevy::text::TextPlugin)
        .add_plugins(bevy::ui::UiPlugin)
        .add_plugins(bevy::ui_widgets::UiWidgetsPlugins)
        .init_asset::<bevy::image::TextureAtlasLayout>()
        .add_plugins(bevy::camera::visibility::VisibilityPlugin)
        .add_plugins(bevy::mesh::MeshPlugin)
        .init_resource::<Theme>()
        .insert_resource(UiFonts::fallback())
        .add_plugins(KitPlugin);
    app.world_mut().spawn(Camera::default());
    app
}

/// 宿主在窗口坐标系里的矩形（逻辑像素）。
#[derive(Clone, Copy, Debug)]
struct Rect {
    left: f32,
    right: f32,
    top: f32,
}

/// 取某个实体的布局矩形：中心 ± 半尺寸，物理像素换算成逻辑像素。
fn logical_rect(app: &App, entity: Entity) -> Rect {
    let node = app
        .world()
        .get::<ComputedNode>(entity)
        .expect("宿主应有 ComputedNode");
    let transform = app
        .world()
        .get::<bevy::ui::UiGlobalTransform>(entity)
        .expect("宿主应有 UiGlobalTransform");
    let scale = node.inverse_scale_factor().max(0.0001);
    let size = node.size() * scale;
    let center = transform.affine().translation * scale;
    Rect {
        left: center.x - size.x / 2.0,
        right: center.x + size.x / 2.0,
        top: center.y - size.y / 2.0,
    }
}

/// 面板实体（每个宿主最多一个）。
fn panel_of(app: &mut App, host: Entity) -> Option<(Entity, Node, Vec2)> {
    let mut q = app
        .world_mut()
        .query::<(Entity, &TooltipPanel, &Node, &ComputedNode)>();
    q.iter(app.world())
        .filter(|(_, panel, _, _)| panel.host == host)
        .map(|(e, _, node, computed)| (e, node.clone(), computed.size()))
        .next()
}

/// 快进时间并让拾取把 hover 落实：`tooltip_system` 需要累计 500ms 悬停才浮现面板。
fn hover_for(app: &mut App, at: Vec2, frames: usize) {
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(app.world())
        .expect("主窗口");
    app.world_mut()
        .resource_mut::<Messages<bevy::window::WindowEvent>>()
        .write(bevy::window::WindowEvent::CursorMoved(
            bevy::window::CursorMoved {
                window,
                position: at,
                delta: None,
            },
        ));
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_relative_speed(1000.0);
    for _ in 0..frames {
        app.update();
    }
}

/// 悬停在窗口中部的按钮上：tooltip 必须落在宿主右侧，而不是 (0,0)。
#[test]
fn tooltip_panel_lands_beside_its_host() {
    let mut app = tip_app();
    // 与 `build_root_ui` 同款：tooltip 挂在 TOOLTIP 层根下，避免被业务面板裁掉。
    app.world_mut().spawn((
        xgent_ui::kit::layer_root(z::TOOLTIP),
        xgent_ui::layout::TooltipLayer,
    ));

    let host = app
        .world_mut()
        .spawn((
            Button,
            Hovered::default(),
            Tooltip {
                text: "设置".to_string(),
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(300.0),
                top: Val::Px(200.0),
                width: Val::Px(100.0),
                height: Val::Px(40.0),
                ..default()
            },
        ))
        .id();

    // 指针落在宿主中心（350, 220）：走真实 picking，hover 才有意义。
    app.update();
    hover_for(&mut app, Vec2::new(350.0, 220.0), 30);
    assert!(
        app.world().get::<Hovered>(host).is_some_and(|h| h.get()),
        "夹具失效：指针在宿主矩形内但宿主没有 Hovered"
    );

    let Some((_, node, tip_size)) = panel_of(&mut app, host) else {
        panic!("悬停 500ms 后应浮现 tooltip 面板");
    };
    let host_rect = logical_rect(&app, host);
    assert!(
        tip_size.x > 0.0,
        "tooltip 面板应完成布局（宽度为 0 说明没跑布局）"
    );

    let scale = app
        .world()
        .get::<ComputedNode>(host)
        .map(|n| n.inverse_scale_factor().max(0.0001))
        .unwrap_or(1.0);
    let tip_width_logical = tip_size.x * scale;
    let viewport_width = {
        let mut q = app
            .world_mut()
            .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>();
        q.single(app.world()).expect("主窗口").width()
    };
    let expected_left = xgent_ui::kit::tooltip_left(
        host_rect.left,
        host_rect.right,
        tip_width_logical,
        viewport_width,
        space::SM,
    );

    assert_eq!(
        node.top,
        Val::Px(host_rect.top),
        "tooltip 顶边应与宿主顶边对齐"
    );
    assert_eq!(
        node.left,
        Val::Px(expected_left),
        "tooltip 应贴在宿主一侧：左缘空间足够时贴宿主右侧，不足时翻转到左侧。\
         停在 Px(0.0) 说明 position_tooltips 没取到宿主矩形（GlobalTransform 缺陷）"
    );
}

/// 贴右缘的按钮：tooltip 翻转到宿主左侧，但仍然必须跟着宿主，不能回到 (0,0)。
#[test]
fn tooltip_for_edge_button_follows_the_host_not_the_window_corner() {
    let mut app = tip_app();
    app.world_mut().spawn((
        xgent_ui::kit::layer_root(z::TOOLTIP),
        xgent_ui::layout::TooltipLayer,
    ));

    let host = app
        .world_mut()
        .spawn((
            Button,
            Hovered::default(),
            Tooltip {
                text: "很长的提示文案用来触发翻转".to_string(),
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(700.0),
                top: Val::Px(60.0),
                width: Val::Px(80.0),
                height: Val::Px(30.0),
                ..default()
            },
        ))
        .id();

    app.update();
    hover_for(&mut app, Vec2::new(740.0, 75.0), 30);
    assert!(
        app.world().get::<Hovered>(host).is_some_and(|h| h.get()),
        "夹具失效：指针在右缘宿主矩形内但宿主没有 Hovered"
    );

    let Some((_, node, tip_size)) = panel_of(&mut app, host) else {
        panic!("悬停 500ms 后应浮现 tooltip 面板");
    };
    let host_rect = logical_rect(&app, host);
    let scale = app
        .world()
        .get::<ComputedNode>(host)
        .map(|n| n.inverse_scale_factor().max(0.0001))
        .unwrap_or(1.0);
    let tip_width_logical = tip_size.x * scale;
    assert!(
        tip_width_logical < host_rect.left,
        "夹具要求提示宽度小于宿主左缘空间，实际 tip={tip_width_logical} host_left={}",
        host_rect.left
    );

    assert_eq!(
        node.top,
        Val::Px(host_rect.top),
        "右缘按钮的 tooltip 顶边同样要跟住宿主"
    );
    assert_eq!(
        node.left,
        Val::Px(host_rect.left - space::SM - tip_width_logical),
        "右缘按钮的 tooltip 必须翻转到宿主左侧"
    );
}

/// 同类缺陷的护栏：0.20 的 UI 节点上没有 `GlobalTransform`（`Node` 的 `#[require]`
/// 只带 `UiTransform`/`UiGlobalTransform`），对 UI 节点写 `&GlobalTransform` 的查询
/// 会静默 `Err`——本缺陷就是这样，且一条日志都没有。
#[test]
fn ui_code_does_not_query_global_transform() {
    let kit_src = include_str!("../src/kit.rs");
    assert!(
        !kit_src.contains("&GlobalTransform"),
        "kit.rs 不得对 UI 节点查询 &GlobalTransform，应使用 UiGlobalTransform"
    );
    assert!(
        kit_src.contains("&UiGlobalTransform"),
        "tooltip 定位必须从 UiGlobalTransform 取宿主矩形"
    );
}
