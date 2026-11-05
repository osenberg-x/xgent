//! F-12：状态栏成本文本与点击入口。

use bevy::prelude::*;
use bevy::ui::Pressed;
use xgent_agent::{Conversation, ProviderInfo};
use xgent_settings::{GlobalConfigRes, Localizer};
use xgent_ui::cost::{CostPanelState, CostStats};
use xgent_ui::fonts::UiFonts;
use xgent_ui::layout::StatusBarMarker;
use xgent_ui::status_bar::{CostTextMarker, CostToggleMarker, StatusBarPlugin};
use xgent_ui::theme::Theme;

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(bevy::image::ImagePlugin::default())
        .add_plugins(bevy::window::WindowPlugin::default())
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
        .add_plugins(bevy::camera::visibility::VisibilityPlugin)
        .add_plugins(bevy::mesh::MeshPlugin)
        .init_asset::<bevy::image::TextureAtlasLayout>()
        .init_resource::<Theme>()
        .init_resource::<Localizer>()
        .insert_resource(UiFonts::fallback())
        .insert_resource(GlobalConfigRes::default())
        .insert_resource(Conversation::default())
        .insert_resource(ProviderInfo {
            id: "test".into(),
            model: "m".into(),
            ready: true,
            kind: None,
        })
        .add_plugins(StatusBarPlugin);
    app.world_mut().spawn((StatusBarMarker, Node::default()));
    app.world_mut().spawn(Camera::default());
    app
}

#[test]
fn cost_text_formats_and_toggles_panel() {
    let mut app = test_app();
    app.update();
    app.world_mut().insert_resource(CostStats {
        prompt: 500,
        completion: 500,
        cost: Some(1.234),
        unpriced: false,
    });
    app.update();
    let mut q = app
        .world_mut()
        .query_filtered::<&Text, With<CostTextMarker>>();
    let text = q.single(app.world()).expect("成本文本");
    assert_eq!(text.0, "$1.23");

    let toggle = {
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, With<CostToggleMarker>>();
        q.single(app.world()).expect("成本按钮")
    };
    app.world_mut().entity_mut(toggle).insert(Pressed);
    app.update();
    assert!(app.world().resource::<CostPanelState>().open);
}
