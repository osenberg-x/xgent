use std::path::PathBuf;

use bevy::asset::AssetPlugin;
use bevy::camera::RenderTarget;
use bevy::ecs::message::Messages;
use bevy::input::ButtonInput;
use bevy::input::mouse::{
    AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButton, MouseScrollUnit,
};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use xgent_core::spatial::{
    ClusterId, EdgeKind, SpatialCluster, SpatialEdge, SpatialGraph, SpatialNode, SpatialNodeId,
    SpatialNodeKind, SpatialStatus,
};
use xgent_ui::editor::tabs::OpenFileRequest;
use xgent_ui::layout::CenterHostMarker;
use xgent_ui::layout_mode::LayoutMode;
use xgent_ui::panels::{
    PanelDescriptor, PanelRegister, PanelRegistry, PanelVisibility, PanelsPlugin, Slot, ids,
};
use xgent_ui::space::{
    SpaceCamera, SpaceEdge, SpaceGraph, SpaceHudLabel, SpaceMaterials, SpacePickState, SpacePlugin,
    SpaceQualityBadge, SpaceViewportMarker, SpatialStar, ViewQuality,
};
use xgent_ui::theme::Theme;
use xui::Ellipsis;

fn synthetic_graph(nodes: usize) -> SpatialGraph {
    let clusters = vec![SpatialCluster {
        id: ClusterId(0),
        label: "demo".into(),
        path: None,
        weight: 1.0,
        anchor: Some([0.0, 0.0, 0.0]),
    }];
    let nodes = (0..nodes)
        .map(|index| SpatialNode {
            id: SpatialNodeId(index as u32),
            kind: SpatialNodeKind::File,
            label: format!("file{index}.rs"),
            path: Some(PathBuf::from(format!("src/file{index}.rs"))),
            cluster: ClusterId(0),
            weight: 1.0,
            status: SpatialStatus::Idle,
            anchor: Some([index as f32 * 0.01, 0.0, 0.0]),
        })
        .collect();
    SpatialGraph {
        generation: 1,
        nodes,
        edges: Vec::new(),
        clusters,
        omitted_nodes: 0,
    }
}

fn space_camera_transform(app: &mut App) -> Transform {
    let world = app.world_mut();
    let mut query = world.query_filtered::<&Transform, With<SpaceCamera>>();
    query.single(world).expect("space 相机应存在").clone()
}

#[test]
fn orbit_drag_moves_camera() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .add_plugins(PanelsPlugin)
        .add_plugins(SpacePlugin)
        .add_message::<OpenFileRequest>()
        .insert_resource(ButtonInput::<MouseButton>::default())
        .insert_resource(AccumulatedMouseMotion {
            delta: Vec2::new(10.0, 0.0),
        })
        .insert_resource(AccumulatedMouseScroll {
            unit: MouseScrollUnit::Line,
            delta: Vec2::ZERO,
        });

    app.world_mut().spawn(CenterHostMarker);
    app.insert_resource(SpaceGraph(synthetic_graph(10)));
    app.update();
    app.update();

    {
        let mut registry = app.world_mut().resource_mut::<PanelRegistry>();
        assert!(registry.activate(ids::SPACE));
    }
    let viewport = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<SpaceViewportMarker>>();
        query.single(world).expect("space 视口应存在")
    };
    app.world_mut().entity_mut(viewport).insert(Hovered(true));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    let before = space_camera_transform(&mut app);
    app.update();
    let after = space_camera_transform(&mut app);
    assert_ne!(
        before.translation, after.translation,
        "悬停 + 左键拖拽应改变相机位置"
    );
}

#[test]
fn space_panel_registers_and_syncs_graph() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .add_plugins(PanelsPlugin)
        .add_plugins(SpacePlugin)
        .add_message::<OpenFileRequest>()
        .insert_resource(ButtonInput::<KeyCode>::default());

    app.world_mut().spawn(CenterHostMarker);
    app.insert_resource(SpaceGraph(synthetic_graph(2000)));
    app.update();
    app.update();

    {
        let mut registry = app.world_mut().resource_mut::<PanelRegistry>();
        assert!(
            registry.activate(ids::SPACE),
            "space 面板应已注册到 PanelRegistry"
        );
    }

    let world = app.world_mut();
    let mut star_query = world.query_filtered::<Entity, With<SpatialStar>>();
    assert_eq!(star_query.iter(world).count(), 2000, "2k 图应生成 2k 星体");

    let mut label_query = world.query_filtered::<&Ellipsis, With<SpaceHudLabel>>();
    assert_eq!(label_query.iter(world).count(), 24, "HUD 标签池应为 24");

    let mut camera_query = world.query_filtered::<&RenderTarget, With<SpaceCamera>>();
    let target = camera_query.single(world).expect("space 相机应存在");
    assert!(
        matches!(target, RenderTarget::Image(_)),
        "space 相机应渲染到离屏纹理"
    );

    let mut viewport_query = world.query_filtered::<Entity, With<SpaceViewportMarker>>();
    assert_eq!(
        viewport_query.iter(world).count(),
        1,
        "应有一个显示离屏纹理的 ImageNode"
    );

    let star_material = world.resource::<SpaceMaterials>().star.clone();
    world.resource_mut::<Theme>().accent = Color::srgb(1.0, 0.0, 0.0);
    app.update();
    let materials = app.world().resource::<Assets<StandardMaterial>>();
    let material = materials.get(&star_material).expect("星体材质应存在");
    assert_eq!(
        material.base_color,
        Color::srgb(1.0, 0.0, 0.0),
        "主题变化应更新星体材质"
    );

    app.world_mut().resource_mut::<SpacePickState>().selected = Some(SpatialNodeId(0));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    app.update();
    let messages = app.world().resource::<Messages<OpenFileRequest>>();
    assert!(
        !messages.is_empty(),
        "Enter 应发出 OpenFileRequest 打开选中文件"
    );

    *app.world_mut().resource_mut::<ViewQuality>() = ViewQuality::Reduced;
    app.update();
    let world = app.world_mut();
    let mut reduced_query = world.query_filtered::<Entity, With<SpatialStar>>();
    assert!(
        reduced_query.iter(world).count() <= 800,
        "Reduced 档应把星体压到 800 以内"
    );
    let mut badge_query = world.query_filtered::<&Text, With<SpaceQualityBadge>>();
    let badge = badge_query.single(world).expect("档位角标应存在");
    assert!(badge.0.contains("800"), "降级角标应显示 800 档");

    *app.world_mut().resource_mut::<ViewQuality>() = ViewQuality::Static;
    app.update();
    let world = app.world_mut();
    let mut camera_query = world.query_filtered::<&Camera, With<SpaceCamera>>();
    let camera = camera_query.single(world).expect("space 相机应存在");
    assert!(!camera.is_active, "Static 档应停用 3D 相机渲染");

    *app.world_mut().resource_mut::<ViewQuality>() = ViewQuality::Full;
    app.update();
    let world = app.world_mut();
    let mut full_query = world.query_filtered::<Entity, With<SpatialStar>>();
    assert_eq!(
        full_query.iter(world).count(),
        2000,
        "回到 Full 档应恢复 2k 星体"
    );
    let mut camera_query = world.query_filtered::<&Camera, With<SpaceCamera>>();
    let camera = camera_query.single(world).expect("space 相机应存在");
    assert!(camera.is_active, "Full 档且面板可见时应恢复渲染");

    app.world_mut().resource_mut::<SpaceGraph>().0.omitted_nodes = 12;
    app.update();
    let mut badge_query = app
        .world_mut()
        .query_filtered::<&Text, With<SpaceQualityBadge>>();
    let badge = badge_query.single(app.world()).expect("档位角标应存在");
    assert!(
        badge.0.contains("12"),
        "省略节点数应出现在 HUD：{}",
        badge.0
    );
}

/// 3.5：`Esc` 从编辑器回 space，且相机保持离开前的位置。
#[test]
fn escape_returns_from_editor_to_space_and_keeps_camera() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .add_plugins(PanelsPlugin)
        .add_plugins(xgent_ui::panels::visibility::VisibilityPlugin)
        .add_plugins(SpacePlugin)
        .add_message::<OpenFileRequest>()
        .add_message::<xui::shortcuts::HotkeyTriggered>()
        .add_message::<xgent_agent::AbortMessage>()
        .add_message::<xgent_ui::overlay::DrawerToggle>()
        .add_message::<xgent_ui::editor::tabs::CycleTabRequest>()
        .add_message::<xgent_ui::editor::tabs::CloseTabRequest>()
        .add_message::<xgent_ui::terminal::tabs::SpawnTabRequest>()
        .init_resource::<xui::command_palette::CommandPaletteState>()
        .init_resource::<xui::hotkeys::HotkeyRegistry>()
        .init_resource::<xgent_ui::editor::EditorView>()
        .init_resource::<xgent_ui::editor::tabs::EditorTabs>()
        .init_resource::<xgent_ui::terminal::TerminalTabs>()
        .add_plugins(xgent_ui::shortcuts::ShortcutsPlugin)
        .insert_resource(ButtonInput::<KeyCode>::default());

    let center = app.world_mut().spawn(CenterHostMarker).id();
    // 用最简视图代替编辑器：Esc 的目标是「工作区面板让位给 space」。
    let editor = app
        .world_mut()
        .spawn((
            Node {
                display: Display::None,
                ..default()
            },
            PanelVisibility::of_panel(ids::EDITOR, Slot::Center),
        ))
        .id();
    app.world_mut().entity_mut(center).add_child(editor);
    app.world_mut().write_message(PanelRegister {
        desc: PanelDescriptor::new(ids::EDITOR, Slot::Center, "panel-editor", "file", 0),
        view: Some(editor),
    });
    for _ in 0..3 {
        app.update();
    }

    // 首页 = space；打开编辑器后 space 让位。
    app.world_mut()
        .write_message(xgent_ui::panels::PanelActivate { id: ids::SPACE });
    app.world_mut()
        .write_message(xgent_ui::panels::PanelActivate { id: ids::EDITOR });
    for _ in 0..2 {
        app.update();
    }
    {
        let registry = app.world().resource::<PanelRegistry>();
        assert!(registry.is_visible(ids::EDITOR), "前置：编辑器应已接管主区");
        assert!(!registry.is_visible(ids::SPACE), "前置：space 应已让位");
    }
    assert_eq!(
        app.world().get::<Node>(editor).unwrap().display,
        Display::Flex
    );

    let camera_before = space_camera_transform(&mut app);
    app.world_mut()
        .write_message(xui::shortcuts::HotkeyTriggered {
            id: "chat.abort".into(),
        });
    for _ in 0..2 {
        app.update();
    }

    let registry = app.world().resource::<PanelRegistry>();
    assert!(!registry.is_visible(ids::EDITOR), "Esc 应关闭编辑器");
    assert!(registry.is_visible(ids::SPACE), "Esc 应回到空间首页");
    assert_eq!(
        app.world().get::<Node>(editor).unwrap().display,
        Display::None,
        "编辑器视图应让出主区"
    );
    let camera_after = space_camera_transform(&mut app);
    assert_eq!(
        camera_before.translation, camera_after.translation,
        "返回空间时相机应保持离开前的位置"
    );
}

/// 键盘路径的最小装配（与长测试同源，但只关心按键/消息）。
#[derive(Resource, Default)]
struct Opened(Vec<PathBuf>);

fn record_opened(mut reader: MessageReader<OpenFileRequest>, mut opened: ResMut<Opened>) {
    for event in reader.read() {
        opened.0.push(event.path.clone());
    }
}

fn keyboard_space_app(graph: SpatialGraph) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .add_plugins(PanelsPlugin)
        .add_plugins(xgent_ui::panels::visibility::VisibilityPlugin)
        .add_plugins(SpacePlugin)
        .add_message::<OpenFileRequest>()
        .insert_resource(ButtonInput::<KeyCode>::default())
        .insert_resource(ButtonInput::<MouseButton>::default())
        .init_resource::<Opened>()
        .add_systems(Update, record_opened)
        .insert_resource(SpaceGraph(graph));
    app.world_mut().spawn(CenterHostMarker);
    for _ in 0..3 {
        app.update();
    }
    app.world_mut()
        .resource_mut::<PanelRegistry>()
        .activate(ids::SPACE);
    app.update();
    app
}

fn press_key(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    app.update();
    {
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.release(key);
        input.clear();
    }
    app.update();
}

fn press_mouse(app: &mut App) {
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    {
        let mut input = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        input.release(MouseButton::Left);
        input.clear();
    }
    app.update();
}

/// 键盘选中（[ / ]）→ Enter 打开，不依赖鼠标。
#[test]
fn bracket_keys_select_and_enter_opens_file() {
    let mut app = keyboard_space_app(synthetic_graph(3));

    press_key(&mut app, KeyCode::BracketLeft);
    assert_eq!(
        app.world().resource::<SpacePickState>().selected,
        Some(SpatialNodeId(2)),
        "从无选中状态按 [ 应落到最后一个节点"
    );
    press_key(&mut app, KeyCode::BracketRight);
    assert_eq!(
        app.world().resource::<SpacePickState>().selected,
        Some(SpatialNodeId(0)),
        "] 应从头环绕"
    );
    press_key(&mut app, KeyCode::BracketRight);
    assert_eq!(
        app.world().resource::<SpacePickState>().selected,
        Some(SpatialNodeId(1)),
        "第二次 ] 选中下一个节点"
    );
    press_key(&mut app, KeyCode::Enter);
    let opened = app.world().resource::<Opened>();
    assert_eq!(opened.0.len(), 1, "Enter 应只打开一个文件");
    assert!(
        opened.0[0].ends_with("src/file1.rs") || opened.0[0].ends_with("src\\file1.rs"),
        "应打开当前选中的 file1.rs，实际 {}",
        opened.0[0].display()
    );
}

/// 文本输入焦点存在时，Enter 不得被宇宙抢走；焦点交还后才打开。
#[test]
fn text_focus_blocks_enter_open() {
    let mut app = keyboard_space_app(synthetic_graph(3));
    app.world_mut().resource_mut::<SpacePickState>().selected = Some(SpatialNodeId(0));
    let dummy = app.world_mut().spawn(()).id();
    app.insert_resource(bevy::input_focus::InputFocus::from_entity(dummy));

    press_key(&mut app, KeyCode::Enter);
    assert!(
        app.world().resource::<Opened>().0.is_empty(),
        "输入框有焦点时 Enter 不能打开文件"
    );

    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .clear();
    press_key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.world().resource::<Opened>().0.len(),
        1,
        "焦点交还宇宙后 Enter 才打开文件"
    );
}

/// 点在视口外（Hovered=false）不能清掉输入焦点。
#[test]
fn clicking_outside_viewport_keeps_input_focus() {
    let mut app = keyboard_space_app(synthetic_graph(1));
    let viewport = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<SpaceViewportMarker>>();
        query.single(world).expect("space 视口应存在")
    };
    app.world_mut().entity_mut(viewport).insert(Hovered(false));
    let dummy = app.world_mut().spawn(()).id();
    app.insert_resource(bevy::input_focus::InputFocus::from_entity(dummy));

    press_mouse(&mut app);
    assert_eq!(
        app.world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(dummy),
        "点视口外不应清输入焦点"
    );

    app.world_mut().entity_mut(viewport).insert(Hovered(true));
    press_mouse(&mut app);
    assert!(
        app.world()
            .resource::<bevy::input_focus::InputFocus>()
            .get()
            .is_none(),
        "点视口应把键盘交还宇宙"
    );
}

/// 目录/星团节点不是文件：选中后 Enter 不应把文件夹送进编辑器。
#[test]
fn enter_on_module_does_not_open_editor() {
    let mut graph = synthetic_graph(1);
    graph.nodes[0].kind = SpatialNodeKind::Module;
    graph.nodes[0].path = Some(PathBuf::from("src"));
    let mut app = keyboard_space_app(graph);
    app.world_mut().resource_mut::<SpacePickState>().selected = Some(SpatialNodeId(0));

    press_key(&mut app, KeyCode::Enter);
    assert!(
        app.world().resource::<Opened>().0.is_empty(),
        "Module 节点不应触发 OpenFileRequest"
    );
}

/// 自动退化到 Static 后，重新回到宇宙要恢复 Full，不能跨模式冻结。
#[test]
fn returning_to_universe_resets_static_quality() {
    let mut app = keyboard_space_app(synthetic_graph(1));
    *app.world_mut().resource_mut::<ViewQuality>() = ViewQuality::Static;
    app.insert_resource(LayoutMode::Classic);
    for _ in 0..2 {
        app.update();
    }
    app.insert_resource(LayoutMode::Universe);
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        *app.world().resource::<ViewQuality>(),
        ViewQuality::Full,
        "回到宇宙应恢复完整档位"
    );
}

fn graph_with_edge() -> SpatialGraph {
    let mut graph = synthetic_graph(2);
    graph.edges.push(SpatialEdge {
        from: SpatialNodeId(0),
        to: SpatialNodeId(1),
        kind: EdgeKind::Contains,
        weight: 1.0,
    });
    graph
}

fn edge_count(app: &mut App) -> usize {
    let world = app.world_mut();
    let mut query = world.query_filtered::<Entity, With<SpaceEdge>>();
    query.iter(world).count()
}

/// Reduced 档必须真的关边；回到 Full 再恢复。
#[test]
fn reduced_quality_removes_edges() {
    let mut app = keyboard_space_app(graph_with_edge());
    assert_eq!(edge_count(&mut app), 1, "Full 档应渲染边");

    *app.world_mut().resource_mut::<ViewQuality>() = ViewQuality::Reduced;
    app.update();
    assert_eq!(edge_count(&mut app), 0, "Reduced 档应关闭边");

    *app.world_mut().resource_mut::<ViewQuality>() = ViewQuality::Full;
    app.update();
    assert_eq!(edge_count(&mut app), 1, "回到 Full 应恢复边");
}

/// 数据刷新（重扫替换图）不得移动已经取好景的相机。
#[test]
fn graph_refresh_keeps_camera() {
    let mut app = keyboard_space_app(synthetic_graph(3));
    let before = space_camera_transform(&mut app);
    app.insert_resource(SpaceGraph(synthetic_graph(5)));
    for _ in 0..2 {
        app.update();
    }
    let after = space_camera_transform(&mut app);
    assert_eq!(
        before.translation, after.translation,
        "空间图刷新不应移动相机"
    );
}
