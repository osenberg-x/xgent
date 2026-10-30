use bevy::prelude::*;
use xgent_settings::Localizer;
use xgent_ui::layout::{CenterHostMarker, ChatPanelMarker, LayoutPlugin, SideViewMarker};
use xgent_ui::panels::visibility::VisibilityPlugin;
use xgent_ui::panels::{
    PanelActivate, PanelDescriptor, PanelRegister, PanelRegistry, PanelVisibility, Slot,
    SlotLayout, ids,
};

fn find_one<M: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    let mut query = world.query_filtered::<Entity, With<M>>();
    let mut found: Vec<Entity> = query.iter(world).collect();
    assert_eq!(found.len(), 1, "应恰有一个 {}", std::any::type_name::<M>());
    found.remove(0)
}

#[test]
fn center_panel_activates_while_chat_stays_in_side_dock() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Localizer>()
        .add_plugins(LayoutPlugin);
    if !app.is_plugin_added::<VisibilityPlugin>() {
        app.add_plugins(VisibilityPlugin);
    }
    for _ in 0..3 {
        app.update();
    }

    let chat = find_one::<ChatPanelMarker>(&mut app);
    let side = find_one::<SideViewMarker>(&mut app);
    let center = find_one::<CenterHostMarker>(&mut app);
    let children = app
        .world()
        .get::<Children>(side)
        .expect("SideView 应有子节点");
    assert!(
        children.iter().any(|child| child == chat),
        "对话应在 SideDock 容器内，而不是 CenterHost"
    );
    assert_eq!(
        app.world().get::<Node>(chat).unwrap().display,
        Display::Flex,
        "对话侧栏默认可见"
    );

    // 注册一个 Center 面板（模拟编辑器/终端）并激活。
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
    app.update();
    app.world_mut()
        .write_message(PanelActivate { id: ids::EDITOR });
    app.update();

    assert!(
        app.world().resource::<SlotLayout>().is_open(Slot::SideDock),
        "激活 Center 面板不得收起对话侧栏"
    );
    assert!(
        app.world()
            .resource::<PanelRegistry>()
            .is_visible(ids::CHAT),
        "对话面板应保持可见"
    );
    assert_eq!(
        app.world().get::<Node>(editor).unwrap().display,
        Display::Flex,
        "编辑器应接管 Center"
    );
    assert_eq!(
        app.world().get::<Node>(chat).unwrap().display,
        Display::Flex,
        "对话仍在 SideDock 可见"
    );
}
