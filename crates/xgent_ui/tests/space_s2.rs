//! S2 交互联动：Tab 遍历、搜索飞行、2D↔3D 同步、Esc 优先级、反馈层。

use std::path::PathBuf;

use bevy::asset::AssetPlugin;
use bevy::input::ButtonInput;
use bevy::prelude::*;
use bevy::text::EditableText;
use xgent_core::spatial::{
    ClusterId, SpatialCluster, SpatialGraph, SpatialNode, SpatialNodeId, SpatialNodeKind,
    SpatialStatus,
};
use xgent_ui::editor::buffer::{BufferState, EditorBuffer};
use xgent_ui::editor::tabs::{EditorTabs, OpenFileRequest};
use xgent_ui::layout::CenterHostMarker;
use xgent_ui::panels::{PanelRegistry, PanelsPlugin, Slot, SlotLayout, ids};
use xgent_ui::space::s2::{SpaceDetailCardMarker, SpaceDetailText, SpaceSearchInputMarker};
use xgent_ui::space::view::OrbitCamera;
use xgent_ui::space::{SpaceGraph, SpacePickState, SpacePlugin, SpaceSearchState};

#[derive(Resource, Default)]
struct Opened(Vec<PathBuf>);

fn record_opened(mut reader: MessageReader<OpenFileRequest>, mut opened: ResMut<Opened>) {
    for event in reader.read() {
        opened.0.push(event.path.clone());
    }
}

fn graph() -> SpatialGraph {
    let clusters = vec![SpatialCluster {
        id: ClusterId(0),
        label: "demo".into(),
        path: None,
        weight: 1.0,
        anchor: Some([0.0, 0.0, 0.0]),
    }];
    let nodes = (0..3)
        .map(|index| SpatialNode {
            id: SpatialNodeId(index),
            kind: SpatialNodeKind::File,
            label: format!("file{index}.rs"),
            path: Some(PathBuf::from(format!("src/file{index}.rs"))),
            cluster: ClusterId(0),
            weight: 1.0,
            status: SpatialStatus::Idle,
            anchor: Some([10.0 + index as f32 * 10.0, 0.0, 0.0]),
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

fn s2_app() -> App {
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
        .init_resource::<EditorTabs>()
        .init_resource::<Opened>()
        .add_systems(Update, record_opened)
        .insert_resource(SpaceGraph(graph()));
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
    let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    input.press(key);
    drop(input);
    app.update();
    let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    input.release(key);
    input.clear();
    app.update();
}

fn press_tab(app: &mut App, reverse: bool) {
    let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    if reverse {
        input.press(KeyCode::ShiftLeft);
    }
    input.press(KeyCode::Tab);
    drop(input);
    app.update();
    let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    input.release(KeyCode::Tab);
    input.release(KeyCode::ShiftLeft);
    input.clear();
    app.update();
}

fn set_search_text(app: &mut App, text: &str) {
    let input = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<SpaceSearchInputMarker>>();
        query.single(world).expect("搜索输入框应存在")
    };
    *app.world_mut()
        .get_mut::<EditableText>(input)
        .expect("搜索框应有 EditableText") = EditableText::new(text.to_string());
    app.update();
}

fn orbit(app: &mut App) -> OrbitCamera {
    let world = app.world_mut();
    let mut query = world.query_filtered::<&OrbitCamera, With<xgent_ui::space::SpaceCamera>>();
    *query.single(world).expect("space 相机应存在")
}

fn selected(app: &mut App) -> Option<SpatialNodeId> {
    app.world().resource::<SpacePickState>().selected
}

#[test]
fn tab_walks_nodes_without_rotating_camera() {
    let mut app = s2_app();
    let before = orbit(&mut app);

    press_tab(&mut app, false);
    assert_eq!(
        selected(&mut app),
        Some(SpatialNodeId(0)),
        "Tab 从第一个节点开始"
    );
    let after_first = orbit(&mut app);
    assert_eq!(after_first.yaw, before.yaw, "Tab 不得改变 yaw");
    assert_eq!(after_first.pitch, before.pitch, "Tab 不得改变 pitch");
    assert_eq!(after_first.dist, before.dist, "Tab 不得改变 dist");
    assert_ne!(after_first.target, before.target, "相机 target 应跟随选中");

    press_tab(&mut app, false);
    assert_eq!(
        selected(&mut app),
        Some(SpatialNodeId(1)),
        "第二次 Tab 前进"
    );
    press_tab(&mut app, true);
    assert_eq!(selected(&mut app), Some(SpatialNodeId(0)), "Shift+Tab 反向");
    press_tab(&mut app, true);
    assert_eq!(
        selected(&mut app),
        Some(SpatialNodeId(2)),
        "反向应从头部回绕"
    );
}

#[test]
fn text_focus_and_search_block_tab() {
    let mut app = s2_app();
    let dummy = app
        .world_mut()
        .spawn(EditableText::new("typing".to_string()))
        .id();
    app.insert_resource(bevy::input_focus::InputFocus::from_entity(dummy));
    press_tab(&mut app, false);
    assert_eq!(selected(&mut app), None, "文本焦点时 Tab 不选星");

    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .clear();
    press_key(&mut app, KeyCode::Slash);
    assert!(app.world().resource::<SpaceSearchState>().open);
    press_tab(&mut app, false);
    assert_eq!(selected(&mut app), None, "搜索打开时 Tab 不选星");
}

#[test]
fn search_flies_then_second_enter_opens() {
    let mut app = s2_app();
    let before = orbit(&mut app);
    press_key(&mut app, KeyCode::Slash);
    assert!(app.world().resource::<SpaceSearchState>().open);
    set_search_text(&mut app, "file1");
    assert_eq!(
        app.world().resource::<SpaceSearchState>().results,
        vec![SpatialNodeId(1)],
        "搜索只匹配 file1"
    );

    press_key(&mut app, KeyCode::Enter);
    assert!(
        !app.world().resource::<SpaceSearchState>().open,
        "Enter 后搜索应关闭"
    );
    assert_eq!(selected(&mut app), Some(SpatialNodeId(1)));
    assert!(
        app.world().resource::<Opened>().0.is_empty(),
        "搜索 Enter 不得直接打开编辑器"
    );
    let after = orbit(&mut app);
    assert_eq!(
        after.target,
        Vec3::new(20.0, 0.0, 0.0),
        "相机应定位到目标星"
    );
    assert_eq!(after.yaw, before.yaw);
    assert_eq!(after.pitch, before.pitch);

    press_key(&mut app, KeyCode::Enter);
    let opened = app.world().resource::<Opened>();
    assert_eq!(opened.0.len(), 1, "第二次 Enter 应打开文件");
    assert!(opened.0[0].ends_with("src/file1.rs"));
}

#[test]
fn search_esc_closes_search_and_keeps_selection() {
    let mut app = s2_app();
    app.world_mut().resource_mut::<SpacePickState>().selected = Some(SpatialNodeId(0));
    press_key(&mut app, KeyCode::Slash);
    set_search_text(&mut app, "file");
    press_key(&mut app, KeyCode::Escape);
    assert!(!app.world().resource::<SpaceSearchState>().open);
    assert_eq!(selected(&mut app), Some(SpatialNodeId(0)), "关搜索不清选中");
}

#[test]
fn escape_clears_selection_before_chat_abort() {
    let mut app = s2_app();
    app.world_mut().resource_mut::<SpacePickState>().selected = Some(SpatialNodeId(1));
    press_key(&mut app, KeyCode::Escape);
    assert_eq!(selected(&mut app), None, "Esc 先清 3D 选中");
}

#[test]
fn editor_active_file_syncs_selection_without_camera_move() {
    let mut app = s2_app();
    let buffer = app
        .world_mut()
        .spawn(EditorBuffer {
            path: PathBuf::from("src/file1.rs"),
            state: BufferState::Clean,
            disk_content: String::new(),
        })
        .id();
    {
        let mut tabs = app.world_mut().resource_mut::<EditorTabs>();
        tabs.tabs.push(buffer);
        tabs.active = Some(0);
    }
    app.update();
    assert_eq!(selected(&mut app), Some(SpatialNodeId(1)));
    let before = orbit(&mut app);
    assert_eq!(
        before.target,
        Vec3::new(20.0, 0.0, 0.0),
        "编辑器同步不移动相机"
    );

    let missing = app
        .world_mut()
        .spawn(EditorBuffer {
            path: PathBuf::from("src/not-in-graph.rs"),
            state: BufferState::Clean,
            disk_content: String::new(),
        })
        .id();
    {
        let mut tabs = app.world_mut().resource_mut::<EditorTabs>();
        tabs.tabs.push(missing);
        tabs.active = Some(1);
    }
    app.update();
    assert_eq!(selected(&mut app), None, "图中不存在的文件清除选中");
}

#[test]
fn detail_card_follows_selection_and_tooltip_delays() {
    let mut app = s2_app();
    app.world_mut().resource_mut::<SpacePickState>().selected = Some(SpatialNodeId(2));
    app.update();
    let (visibility, text) = {
        let world = app.world_mut();
        let mut card = world.query_filtered::<(&Visibility,), With<SpaceDetailCardMarker>>();
        let visibility = *card.single(world).expect("详情卡应存在").0;
        let mut texts = world.query_filtered::<&Text, With<SpaceDetailText>>();
        let text = texts.single(world).expect("详情文本应存在").0.clone();
        (visibility, text)
    };
    assert_eq!(visibility, Visibility::Visible);
    assert!(text.contains("file2.rs") && text.contains("src/file2.rs"));
}

#[test]
fn hidden_space_ignores_s2_keys() {
    let mut app = s2_app();
    app.world_mut()
        .resource_mut::<SlotLayout>()
        .set_open(Slot::Center, false);
    app.update();
    press_key(&mut app, KeyCode::Slash);
    press_tab(&mut app, false);
    assert!(!app.world().resource::<SpaceSearchState>().open);
    assert_eq!(selected(&mut app), None);
}

#[test]
fn search_with_no_match_keeps_enter_inert_and_esc_closes() {
    let mut app = s2_app();
    press_key(&mut app, KeyCode::Slash);
    set_search_text(&mut app, "zzzz-no-such-file");
    assert!(
        app.world()
            .resource::<SpaceSearchState>()
            .results
            .is_empty(),
        "无匹配时结果列表应为空"
    );
    press_key(&mut app, KeyCode::Enter);
    assert!(
        app.world().resource::<SpaceSearchState>().open,
        "无结果时 Enter 不应关闭搜索"
    );
    assert_eq!(selected(&mut app), None, "无结果时 Enter 不选中");
    press_key(&mut app, KeyCode::Escape);
    assert!(!app.world().resource::<SpaceSearchState>().open);
}

/// 搜索框不能被 ChatInput 的 Enter 提交观察者清空（真实输入栈回归）。
#[test]
fn search_input_is_not_a_chat_input() {
    let mut app = s2_app();
    press_key(&mut app, KeyCode::Slash);
    app.update();
    let input = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<SpaceSearchInputMarker>>();
        query.single(world).expect("搜索输入框应存在")
    };
    assert!(
        app.world().get::<xui::input::ChatInput>(input).is_none(),
        "搜索框不得挂 ChatInput：Enter 提交观察者会先清空查询"
    );
}

/// 装齐真实输入/焦点/分发插件后，Enter 仍按当前查询选中 file1。
#[test]
fn search_enter_with_real_input_stack_selects_current_result() {
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput};

    let mut app = s2_app();
    app.add_plugins((
        bevy::input::InputPlugin,
        bevy::input_focus::InputFocusPlugin,
        bevy::input_focus::InputDispatchPlugin,
        xui::input::InputEnhancePlugin,
    ));

    let send = |app: &mut App, key: KeyCode, logical: Key, text: Option<&str>| {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(KeyboardInput {
                key_code: key,
                logical_key: logical.clone(),
                state,
                text: text.map(|value| value.into()),
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        }
    };

    send(
        &mut app,
        KeyCode::Slash,
        Key::Character("/".into()),
        Some("/"),
    );
    assert!(app.world().resource::<SpaceSearchState>().open);
    set_search_text(&mut app, "file1");
    let input = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<SpaceSearchInputMarker>>();
        query.single(world).expect("搜索输入框应存在")
    };
    app.insert_resource(bevy::input_focus::InputFocus::from_entity(input));
    send(&mut app, KeyCode::Enter, Key::Enter, None);

    assert_eq!(
        selected(&mut app),
        Some(SpatialNodeId(1)),
        "真实输入栈下 Enter 应选中当前查询结果 file1，而不是首项"
    );
    assert!(
        app.world().resource::<Opened>().0.is_empty(),
        "搜索 Enter 不得直接打开编辑器"
    );
}
