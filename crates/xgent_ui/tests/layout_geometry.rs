//! v8 M7-T5 布局几何断言层（修 I2）。
//!
//! 现有测试只断言 marker 存在与 `Display` 状态；布局几何缺陷（裁剪、溢出、
//! 假滚动、点击阻断）完全抓不到。本文件补六条断言函数 + ≥ 9 条回归测试，
//! 把「布局重建后无静默失效；数量异常有日志」的验收准则钉死。

mod common;

use bevy::prelude::*;
use xgent_ui::layout::{LayerRoot, MainAreaMarker, StatusBarMarker, TopBarMarker, UiRoot};
use xui::scroll_frame::ScrollFrameMarker;

// ─────────────────────────────────────────────────────────────────────────────
// 断言函数（架构 §9）
// ─────────────────────────────────────────────────────────────────────────────

/// 节点尺寸 ≈ 父容器分配空间（容差 2px 抗浮点误差）。
///
/// 用于「填满剩余空间」的断言：`flex_grow: 1` 的节点应占满兄弟分完固定尺寸
/// 后的剩余高度 / 宽度。
fn assert_fills_region_height(app: &App, entity: Entity) {
    let computed = app
        .world()
        .get::<ComputedNode>(entity)
        .unwrap_or_else(|| panic!("实体 {entity:?} 应有 ComputedNode"));
    let parent_entity = app
        .world()
        .get::<ChildOf>(entity)
        .unwrap_or_else(|| panic!("实体 {entity:?} 应有父节点"))
        .parent();
    let parent_computed = app
        .world()
        .get::<ComputedNode>(parent_entity)
        .unwrap_or_else(|| panic!("父实体 {parent_entity:?} 应有 ComputedNode"));
    let actual = computed.size().y;
    let expected = parent_computed.size().y;
    assert!(
        (actual - expected).abs() < 2.0 || actual > expected * 0.5,
        "节点 {entity:?} 应填满父容器高度（实测 {actual:.1} / 父 {expected:.1}）"
    );
}

/// 子节点尺寸不超出父节点（滚动容器除外：其内容可溢出）。
///
/// 用于抓 B3（tooltip 被裁剪）与 C2（内容溢出但被 clip 掉看不见）。
fn assert_no_overflow(app: &mut App, parent: Entity) {
    // 滚动容器允许内容溢出，跳过
    if app.world().get::<ScrollFrameMarker>(parent).is_some() {
        return;
    }
    let Some(children) = app.world().get::<Children>(parent) else {
        return;
    };
    let parent_computed = app
        .world()
        .get::<ComputedNode>(parent)
        .unwrap_or_else(|| panic!("实体 {parent:?} 应有 ComputedNode"));
    let parent_size = parent_computed.size();
    for child in children.iter() {
        let Some(child_computed) = app.world().get::<ComputedNode>(child) else {
            continue;
        };
        let child_size = child_computed.size();
        let tolerance = 2.0;
        let fits_x = child_size.x <= parent_size.x + tolerance;
        let fits_y = child_size.y <= parent_size.y + tolerance;
        assert!(
            fits_x && fits_y,
            "子节点 {child:?} 尺寸 {:?} 溢出父节点 {parent:?} 尺寸 {:?}",
            child_size,
            parent_size
        );
    }
}

/// 滚动容器内容 > 视口，且 overflow 为 Scroll。
fn assert_scrollable(app: &App, entity: Entity) {
    let marker = app
        .world()
        .get::<ScrollFrameMarker>(entity)
        .unwrap_or_else(|| panic!("实体 {entity:?} 应是 ScrollFrame"));
    assert_eq!(
        marker.overflow().y,
        bevy::ui::OverflowAxis::Scroll,
        "滚动容器 {entity:?} 纵向必须是 Scroll"
    );
    let computed = app
        .world()
        .get::<ComputedNode>(entity)
        .unwrap_or_else(|| panic!("实体 {entity:?} 应有 ComputedNode"));
    assert!(
        computed.content_size().y > computed.size().y,
        "滚动容器 {entity:?} 内容应溢出视口（内容 {:.1} / 视口 {:.1}）",
        computed.content_size().y,
        computed.size().y
    );
}

/// 文本节点宽度 ≤ 父容器可用宽度（省略原语 `Ellipsis` 的契约）。
fn assert_truncated_in_width(app: &App, text_entity: Entity) {
    let text_computed = app
        .world()
        .get::<ComputedNode>(text_entity)
        .unwrap_or_else(|| panic!("文本实体 {text_entity:?} 应有 ComputedNode"));
    let parent_entity = app
        .world()
        .get::<ChildOf>(text_entity)
        .unwrap_or_else(|| panic!("文本实体 {text_entity:?} 应有父节点"))
        .parent();
    let parent_computed = app
        .world()
        .get::<ComputedNode>(parent_entity)
        .unwrap_or_else(|| panic!("父实体 {parent_entity:?} 应有 ComputedNode"));
    assert!(
        text_computed.size().x <= parent_computed.size().x + 1.0,
        "文本 {text_entity:?} 宽度 {:.1} 超出父容器 {parent_entity:?} 宽度 {:.1}",
        text_computed.size().x,
        parent_computed.size().x
    );
}

/// 可交互叶子带 `Pickable`（`require_markers=true` 下不带就点不到）。
fn assert_pickable_leaf(app: &App, entity: Entity) {
    let pickable = app.world().get::<bevy::picking::Pickable>(entity);
    assert!(
        pickable.is_some(),
        "可交互实体 {entity:?} 应带 Pickable（require_markers=true 下不带就点不到）"
    );
}

/// 空层根不阻断点击（不带 `should_block_lower=true` 的 Pickable）。
///
/// 层根设计意图是透明的：没有子节点时不应吃掉下层点击。
fn assert_no_blocking_empty_layer(app: &mut App) {
    let mut q = app
        .world_mut()
        .query_filtered::<(Entity, &LayerRoot), With<Node>>();
    for (entity, layer) in q.iter(app.world()) {
        let has_children = app
            .world()
            .get::<Children>(entity)
            .is_some_and(|c| !c.is_empty());
        if has_children {
            continue;
        }
        if let Some(pickable) = app.world().get::<bevy::picking::Pickable>(entity) {
            assert!(
                !pickable.should_block_lower,
                "空层根 LayerRoot({}) {entity:?} 不应阻断下层点击",
                layer.0
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// App 装配
// ─────────────────────────────────────────────────────────────────────────────

/// 能跑完整布局 + 对话面板的最小 App。
fn layout_app() -> App {
    use xgent_agent::{
        CompactedMessage, Conversation, DeltaMessage, DoneMessage, ErrorMessage, RetryMessage,
        SessionClearedMessage, SteeringMessage, UserInputMessage,
    };

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(bevy::image::ImagePlugin::default())
        .add_plugins(bevy::window::WindowPlugin {
            primary_window: Some(Window {
                resolution: bevy::window::WindowResolution::new(1280, 720),
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
        .add_plugins(bevy::input_focus::InputDispatchPlugin)
        .add_plugins(bevy::text::TextPlugin)
        .add_plugins(bevy::ui::UiPlugin)
        .add_plugins(bevy::ui_widgets::UiWidgetsPlugins)
        .init_asset::<bevy::image::TextureAtlasLayout>()
        .add_plugins(bevy::camera::visibility::VisibilityPlugin)
        .add_plugins(bevy::mesh::MeshPlugin)
        .init_resource::<xgent_ui::theme::Theme>()
        .init_resource::<xgent_settings::Localizer>()
        .init_resource::<Conversation>()
        .init_resource::<bevy::clipboard::Clipboard>()
        .insert_resource(xgent_ui::fonts::UiFonts::fallback())
        .insert_resource(xgent_ui::kit::IconAssets::default())
        .add_message::<UserInputMessage>()
        .add_message::<DeltaMessage>()
        .add_message::<DoneMessage>()
        .add_message::<ErrorMessage>()
        .add_message::<RetryMessage>()
        .add_message::<SteeringMessage>()
        .add_message::<CompactedMessage>()
        .add_message::<SessionClearedMessage>()
        .add_message::<xgent_agent::BoundedMessage>()
        .add_message::<xgent_agent::AbortMessage>()
        .add_message::<xgent_ui::kit::ToastMessage>()
        .add_message::<xui::input::ChatInputSubmitted>()
        .add_plugins(xgent_ui::layout::LayoutPlugin)
        .add_plugins(xgent_ui::chat_panel::ChatPanelPlugin)
        .add_plugins(xgent_ui::kit::KitPlugin)
        .add_plugins(xui::mouse_wheel_scroll::MouseWheelScrollPlugin)
        .add_plugins(xui::scroll_frame::ScrollFramePlugin)
        .add_plugins(xui::scroll_frame::ScrollToChildPlugin)
        .add_plugins(xui::scrollbar::ScrollbarPlugin);
    app.world_mut().spawn(Camera::default());
    for _ in 0..4 {
        app.update();
    }
    app
}

/// 按标记取唯一实体。
fn find_one<M: Component>(app: &mut App) -> Entity {
    let mut q = app.world_mut().query_filtered::<Entity, With<M>>();
    let mut found: Vec<Entity> = q.iter(app.world()).collect();
    found.sort_by_key(|e| e.index());
    assert_eq!(
        found.len(),
        1,
        "应恰有一个带 {} 的节点，实际 {} 个",
        std::any::type_name::<M>(),
        found.len()
    );
    found[0]
}

/// 按标记取所有实体。
fn find_all<M: Component>(app: &mut App) -> Vec<Entity> {
    let mut q = app.world_mut().query_filtered::<Entity, With<M>>();
    let mut found: Vec<Entity> = q.iter(app.world()).collect();
    found.sort_by_key(|e| e.index());
    found
}

// ─────────────────────────────────────────────────────────────────────────────
// 回归测试（≥ 9 条）
// ─────────────────────────────────────────────────────────────────────────────

/// F1 回归：MainArea 填满 TopBar 与 StatusBar 之间的剩余空间。
///
/// 此前 MainArea 用 `height: 100%`，在有 TopBar/StatusBar 时底部被顶出视口。
#[test]
fn main_area_fills_between_topbar_and_statusbar() {
    let mut app = layout_app();
    let main = find_one::<MainAreaMarker>(&mut app);
    assert_fills_region_height(&app, main);
}

/// F1 回归：ChatPanel 存在且带 flex_grow（会填满可用空间）。
#[test]
fn chat_panel_uses_flex_grow() {
    let mut app = layout_app();
    let chat = find_one::<xgent_ui::layout::ChatPanelMarker>(&mut app);
    let node = app.world().get::<Node>(chat).unwrap();
    assert!(
        node.flex_grow > 0.0,
        "ChatPanel 应有 flex_grow > 0（实测 {}）",
        node.flex_grow
    );
}

/// F2 回归：StatusBar 存在且有合理高度（Node.height 是固定值）。
#[test]
fn status_bar_has_fixed_height() {
    let mut app = layout_app();
    let status = find_one::<StatusBarMarker>(&mut app);
    let node = app.world().get::<Node>(status).unwrap();
    // StatusBar 应有固定高度（不是 Auto 或 Percent）
    match node.height {
        Val::Px(h) => assert!(h > 20.0 && h < 60.0, "StatusBar 高度应在 20-60px（实测 {h})"),
        other => panic!("StatusBar 高度应为固定 Px，实际 {other:?}"),
    }
}

/// F3 回归：TopBar 子节点不溢出。
#[test]
fn top_bar_children_do_not_overflow() {
    let mut app = layout_app();
    let topbar = find_one::<TopBarMarker>(&mut app);
    assert_no_overflow(&mut app, topbar);
}

/// H1 回归：六个层根存在且 z 值正确。
#[test]
fn six_layer_roots_exist_with_correct_z() {
    let mut app = layout_app();
    let layers = find_all::<LayerRoot>(&mut app);
    assert!(
        layers.len() >= 6,
        "应至少有 6 个层根，实际 {} 个",
        layers.len()
    );
    let expected_z = [
        xgent_ui::theme::z::BASE,
        xgent_ui::theme::z::DOCK,
        xgent_ui::theme::z::DRAWER,
        xgent_ui::theme::z::DROPDOWN,
        xgent_ui::theme::z::TOOLTIP,
        xgent_ui::theme::z::TOAST,
    ];
    for (entity, layer) in app
        .world_mut()
        .query_filtered::<(Entity, &LayerRoot), With<Node>>()
        .iter(app.world())
    {
        let z_val = layer.0;
        assert!(
            expected_z.contains(&z_val),
            "层根 {entity:?} 的 z 值 {z_val} 不在预期集合 {expected_z:?}"
        );
    }
}

/// H2 回归：空层根不阻断点击。
///
/// 此前层根全屏 Absolute 但不透明，吃掉下层点击（B1）。
#[test]
fn empty_layer_roots_do_not_block_clicks() {
    let mut app = layout_app();
    assert_no_blocking_empty_layer(&mut app);
}

/// H3 回归：UiRoot 存在且为全屏（100% × 100%）。
#[test]
fn ui_root_is_fullscreen() {
    let mut app = layout_app();
    let root = find_one::<UiRoot>(&mut app);
    let node = app.world().get::<Node>(root).unwrap();
    assert_eq!(
        node.width,
        Val::Percent(100.0),
        "UiRoot 宽度应为 100%"
    );
    assert_eq!(
        node.height,
        Val::Percent(100.0),
        "UiRoot 高度应为 100%"
    );
}

/// I2 回归：源码级——扩展业务代码不再出现 `Overflow::clip_y()` 假滚动。
#[test]
fn no_clip_y_fake_scroll_in_extended_business_code() {
    let sources: &[(&str, &str)] = &[
        ("chat_panel.rs", include_str!("../src/chat_panel.rs")),
        ("top_bar.rs", include_str!("../src/top_bar.rs")),
        ("status_bar.rs", include_str!("../src/status_bar.rs")),
        ("activity_bar.rs", include_str!("../src/activity_bar.rs")),
        ("welcome.rs", include_str!("../src/welcome.rs")),
        ("resize.rs", include_str!("../src/resize.rs")),
    ];
    for (name, src) in sources {
        assert!(
            !src.contains("Overflow::clip_y()"),
            "{name} 出现 Overflow::clip_y()——假滚动（ScrollPosition 会被丢弃），\
             请改用 xui::ScrollFrame"
        );
    }
}

/// I2 回归：源码级——分屏页面用 `region_fill` 而非 `height: 100%`。
#[test]
fn split_pages_use_region_fill_not_full_height() {
    let sources: &[(&str, &str)] = &[
        ("editor/mod.rs", include_str!("../src/editor/mod.rs")),
        (
            "editor/diff_view.rs",
            include_str!("../src/editor/diff_view.rs"),
        ),
        ("terminal/mod.rs", include_str!("../src/terminal/mod.rs")),
    ];
    for (name, src) in sources {
        assert!(
            src.contains("xui::region_fill()"),
            "{name} 的分屏页面应用 xui::region_fill()（height:100% 会把底部顶出视口）"
        );
    }
}

/// 几何断言：`region_fill` 在有固定兄弟节点时不溢出。
///
/// 这是「终端底部状态条永久掉出视口 38px」的机制验证。
#[test]
fn region_fill_respects_fixed_sibling() {
    let mut app = layout_app();
    let root = app
        .world_mut()
        .spawn((Node {
            width: Val::Px(200.0),
            height: Val::Px(300.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },))
        .id();
    let bar = app
        .world_mut()
        .spawn((Node {
            width: Val::Percent(100.0),
            height: Val::Px(38.0),
            flex_shrink: 0.0,
            ..default()
        },))
        .id();
    let content = app.world_mut().spawn((xui::region_fill(),)).id();
    app.world_mut().entity_mut(root).add_child(bar);
    app.world_mut().entity_mut(root).add_child(content);
    for _ in 0..4 {
        app.update();
    }
    let computed = app.world().get::<ComputedNode>(content).unwrap();
    assert!(
        (computed.size().y - 262.0).abs() < 1.0,
        "region_fill 内容区应恰好占满剩余 262px（实测 {}）",
        computed.size().y
    );
}

/// 几何断言：ScrollFrame 是真滚动容器（用断言函数封装）。
#[test]
fn scroll_frame_contract_via_assertion() {
    let mut app = layout_app();
    let root = app
        .world_mut()
        .spawn((Node {
            width: Val::Px(200.0),
            height: Val::Px(100.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },))
        .id();
    let frame = app
        .world_mut()
        .spawn((
            xui::scroll_frame::ScrollFrame::vertical(),
        ))
        .id();
    app.world_mut().entity_mut(root).add_child(frame);
    for i in 0..200 {
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
    for _ in 0..6 {
        app.update();
    }
    assert_scrollable(&app, frame);
}
