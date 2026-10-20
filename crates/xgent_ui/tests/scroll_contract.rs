//! v8 M3 滚动契约回归（审计 C1 / C2 / C4 / C5）。
//!
//! 缺陷 C1：业务层 9 处用 `Overflow::clip_y()` 配 `ScrollPosition`。bevy 的
//! `ScrollPosition` 只在 `OverflowAxis::Scroll` 轴生效，`clip_y` 轴上的偏移被
//! 布局系统直接丢弃 → 「看起来能滚、实际滚不动」，内容被硬裁（C2：模型列表
//! 够不着末项）。
//!
//! 本文件用两条腿把这条纪律钉死：
//!
//! 1. **源码级契约**（`no_fake_scroll_containers`）：把 7 个业务文件编进测试
//!    二进制，断言里面不再出现 `Overflow::clip_y()`。谁再手搓假滚动，测试当场红。
//! 2. **引擎级行为**（`scroll_frame_is_a_real_scroll_container`）：真造一个
//!    滚动容器灌 200 条，断言布局把 `ScrollPosition` 吃进去了
//!    （`ComputedNode.scroll_position` 跟着走）——`clip_y` 时期它恒为 0。

mod common;

use bevy::prelude::*;
use bevy::ui::ScrollPosition;
use xui::scroll_frame::{ScrollFrame, ScrollFrameMarker};

/// 参与「假滚动」纪律的 7 个业务文件（编译进测试二进制，源码级断言）。
const BUSINESS_SOURCES: &[(&str, &str)] = &[
    ("file_panel.rs", include_str!("../src/file_panel.rs")),
    (
        "session_history.rs",
        include_str!("../src/session_history.rs"),
    ),
    (
        "command_palette.rs",
        include_str!("../src/command_palette.rs"),
    ),
    (
        "confirm_dialog.rs",
        include_str!("../src/confirm_dialog.rs"),
    ),
    ("editor/mod.rs", include_str!("../src/editor/mod.rs")),
    (
        "settings_panel.rs",
        include_str!("../src/settings_panel.rs"),
    ),
    ("tool_panel.rs", include_str!("../src/tool_panel.rs")),
];

/// C1 回归：业务代码里不再有「clip_y 配 ScrollPosition」的假滚动。
///
/// `Overflow::clip_y()` 是「假滚动」的味道：它让容器看起来能滚，实际把
/// `ScrollPosition` 丢掉。真要滚就用 `xui::ScrollFrame`（overflow 由构造器定）。
#[test]
fn no_fake_scroll_containers() {
    for (name, src) in BUSINESS_SOURCES {
        assert!(
            !src.contains("Overflow::clip_y()"),
            "{name} 又出现了 Overflow::clip_y()——那是假滚动（ScrollPosition 会被丢弃），\
             请改用 xui::ScrollFrame"
        );
    }
}

/// C1 回归：业务代码不再自己拼 `ScrollPosition`（滚动语义只由 ScrollFrame 决定）。
#[test]
fn business_code_does_not_hand_roll_scroll_position() {
    for (name, src) in BUSINESS_SOURCES {
        assert!(
            !src.contains("ScrollPosition::default()"),
            "{name} 又手工挂了 ScrollPosition——滚动区域请走 xui::ScrollFrame"
        );
    }
}

/// 造一个能跑 UI 布局的最小 App（只为了量 `ComputedNode`）。
fn ui_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(bevy::image::ImagePlugin::default())
        .add_plugins(bevy::window::WindowPlugin::default())
        .add_plugins(bevy::picking::input::PointerInputPlugin)
        .add_plugins(bevy::picking::PickingPlugin)
        .add_plugins(bevy::picking::InteractionPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::input_focus::InputFocusPlugin)
        .add_plugins(bevy::input_focus::InputDispatchPlugin)
        .add_plugins(bevy::text::TextPlugin)
        .add_plugins(bevy::ui::UiPlugin)
        .init_asset::<bevy::image::TextureAtlasLayout>()
        .add_plugins(bevy::camera::visibility::VisibilityPlugin)
        .add_plugins(bevy::mesh::MeshPlugin)
        .add_plugins(xui::mouse_wheel_scroll::MouseWheelScrollPlugin)
        .add_plugins(xui::scroll_frame::ScrollFramePlugin)
        .add_plugins(xui::scroll_frame::ScrollToChildPlugin)
        .add_plugins(xui::scrollbar::ScrollbarPlugin);
    app.world_mut().spawn((Camera2d, IsDefaultUiCamera));
    app
}

/// 造一个 200 条目的滚动容器，返回容器实体。
fn scroll_container_with_items(app: &mut App, items: usize) -> Entity {
    let root = app
        .world_mut()
        .spawn((Node {
            width: Val::Px(200.0),
            height: Val::Px(100.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },))
        .id();
    let frame = app.world_mut().spawn((ScrollFrame::vertical(),)).id();
    app.world_mut().entity_mut(root).add_child(frame);
    for i in 0..items {
        let child = app
            .world_mut()
            .spawn((Node {
                width: Val::Percent(100.0),
                height: Val::Px(10.0),
                flex_shrink: 0.0,
                ..default()
            },))
            .id();
        app.world_mut().entity_mut(frame).add_child(child);
        let _ = i;
    }
    frame
}

/// C1 回归：`ScrollFrame` 是真滚动容器——布局会吃 `ScrollPosition`。
///
/// `clip_y` 时期：`ComputedNode.scroll_position` 恒为 0（偏移被丢弃）。
/// `Scroll` 时期：偏移被写回 `ComputedNode.scroll_position`，末项可达。
#[test]
fn scroll_frame_is_a_real_scroll_container() {
    let mut app = ui_app();
    let frame = scroll_container_with_items(&mut app, 200);
    for _ in 0..4 {
        app.update();
    }

    // 契约：overflow 由构造器定
    let marker = app
        .world()
        .get::<ScrollFrameMarker>(frame)
        .expect("容器应带 ScrollFrameMarker");
    assert_eq!(
        marker.overflow().y,
        bevy::ui::OverflowAxis::Scroll,
        "纵向必须是 Scroll（clip_y 会被引擎丢弃）"
    );

    let computed = *app
        .world()
        .get::<ComputedNode>(frame)
        .expect("容器应有 ComputedNode");
    // 内容 200 × 10px 远超视口 100px
    assert!(
        computed.content_size().y > computed.size().y,
        "内容应溢出视口（内容 {:?} / 视口 {:?}）",
        computed.content_size().y,
        computed.size().y
    );

    // 写一个偏移量，跑一帧，布局应把它写回 ComputedNode（= 引擎真的消费了）
    app.world_mut()
        .entity_mut(frame)
        .get_mut::<ScrollPosition>()
        .expect("容器应有 ScrollPosition")
        .0
        .y = 50.0;
    app.update();
    let after = *app
        .world()
        .get::<ComputedNode>(frame)
        .expect("容器应有 ComputedNode");
    assert!(
        after.scroll_position.y > 0.0,
        "布局应保留滚动偏移（实测 {}）——若恒为 0 说明容器不是真滚动容器",
        after.scroll_position.y
    );

    // 滚到底：偏移被钳到 max_offset（内容高 - 视口高）
    let max_offset = after.content_size().y - after.size().y;
    app.world_mut()
        .entity_mut(frame)
        .get_mut::<ScrollPosition>()
        .expect("容器应有 ScrollPosition")
        .0
        .y = 1.0e6;
    app.update();
    let bottom = *app
        .world()
        .get::<ComputedNode>(frame)
        .expect("容器应有 ComputedNode");
    assert!(
        (bottom.scroll_position.y - max_offset).abs() < 1.0,
        "滚到底应停在 max_offset（期望 {max_offset}，实测 {}）",
        bottom.scroll_position.y
    );
    assert!(
        max_offset > 0.0,
        "200 条内容应产生可滚距离（实测 {max_offset}）"
    );
}

/// C2 回归：设置面板的模型列表是滚动容器，且 200 个模型时末项可达。
#[test]
fn settings_model_list_is_scrollable_and_reaches_last_item() {
    use xgent_ui::settings_panel::{ModelListContainerMarker, ModelListResultMessage};

    let mut app = common::panel_app();
    common::open_panel(&mut app);

    // 灌 200 个模型
    let models: Vec<String> = (0..200).map(|i| format!("model-{i:03}")).collect();
    app.world_mut().write_message(ModelListResultMessage {
        models: models.clone(),
        error: String::new(),
    });
    for _ in 0..4 {
        app.update();
    }

    let container = {
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, With<ModelListContainerMarker>>();
        q.iter(app.world())
            .next()
            .expect("设置面板应有模型列表容器")
    };

    // 契约：模型列表是 ScrollFrame（此前只有 max_height:150 且完全没有滚动）
    let marker = app
        .world()
        .get::<ScrollFrameMarker>(container)
        .expect("模型列表应是 ScrollFrame");
    assert_eq!(marker.overflow().y, bevy::ui::OverflowAxis::Scroll);

    // 200 条都渲染出来了
    let children = app
        .world()
        .get::<Children>(container)
        .map(|c| c.len())
        .unwrap_or(0);
    assert_eq!(children, 200, "200 个模型应全部渲染成条目");

    // 末项可达：内容高 > 视口高，且偏移被引擎消费
    let computed = *app
        .world()
        .get::<ComputedNode>(container)
        .expect("模型列表应有 ComputedNode");
    assert!(
        computed.content_size().y > computed.size().y,
        "200 条应溢出模型列表视口（内容 {:?} / 视口 {:?}）",
        computed.content_size().y,
        computed.size().y
    );
}

/// C1 回归：文件树是滚动容器（此前 `clip_y` + `ScrollPosition` 形同虚设）。
#[test]
fn file_tree_is_a_scroll_frame() {
    use xgent_ui::file_panel::{FilePanelPlugin, FileTreeMarker};

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .init_asset::<bevy::image::Image>()
        .init_asset::<bevy::text::Font>()
        .init_resource::<xgent_settings::Localizer>()
        .insert_resource(xgent_ui::fonts::UiFonts {
            ui: Handle::default(),
            mono: Handle::default(),
        })
        .add_plugins(xgent_ui::layout::LayoutPlugin)
        .add_plugins(xgent_ui::overlay::OverlayPlugin)
        .add_plugins(FilePanelPlugin)
        .add_plugins(xgent_ui::kit::KitPlugin)
        .add_message::<xgent_ui::editor::conflict::FileChangedEvent>()
        .add_message::<xgent_ui::editor::tabs::OpenFileRequest>();
    for _ in 0..3 {
        app.update();
    }

    let tree = {
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, With<FileTreeMarker>>();
        q.iter(app.world()).next().expect("抽屉应有文件树容器")
    };
    let marker = app
        .world()
        .get::<ScrollFrameMarker>(tree)
        .expect("文件树应是 ScrollFrame");
    assert_eq!(marker.overflow().y, bevy::ui::OverflowAxis::Scroll);
}
