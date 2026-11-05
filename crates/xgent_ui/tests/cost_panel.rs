//! F-12 成本面板 headless 回归：开关、当前会话文本、关闭按钮。

use bevy::prelude::*;
use bevy::ui::Pressed;
use xgent_agent::Conversation;
use xgent_settings::{GlobalConfigRes, Localizer};
use xgent_ui::cost::{CostHistoryState, CostPanelState, CostStats, HistoryUsage, ModelUsage};
use xgent_ui::cost_panel::{CostCurrentText, CostPanelClose, CostPanelRoot};
use xgent_ui::cost_panel::{CostHistoryText, CostModelsText};
use xgent_ui::fonts::UiFonts;
use xgent_ui::layout::UiRoot;
use xgent_ui::theme::Theme;

fn test_app() -> App {
    let dir = std::mem::ManuallyDrop::new(tempfile::tempdir().expect("临时目录"));
    unsafe { std::env::set_var("XGENT_AGENT_DIR", dir.path()) };
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
        .add_plugins(bevy::text::TextPlugin)
        .add_plugins(bevy::ui::UiPlugin)
        .add_plugins(bevy::ui_widgets::UiWidgetsPlugins)
        .add_plugins(bevy::camera::visibility::VisibilityPlugin)
        .add_plugins(bevy::mesh::MeshPlugin)
        .init_asset::<bevy::image::TextureAtlasLayout>()
        .init_resource::<Theme>()
        .init_resource::<Localizer>()
        .init_resource::<CostStats>()
        .insert_resource(GlobalConfigRes::default())
        .insert_resource(Conversation::default())
        .insert_resource(UiFonts::fallback())
        .add_plugins(xgent_ui::modal::ModalPlugin)
        .add_plugins(xgent_ui::cost::CostPlugin)
        .add_plugins(xgent_ui::cost_panel::CostPanelPlugin);
    app.world_mut().spawn((UiRoot, Node::default()));
    app.world_mut().spawn(Camera::default());
    app
}

fn panel_visibility(app: &mut App) -> Visibility {
    let mut q = app
        .world_mut()
        .query_filtered::<&Visibility, With<CostPanelRoot>>();
    *q.single(app.world()).expect("成本面板根")
}

#[test]
fn panel_opens_and_closes() {
    let mut app = test_app();
    app.update();
    assert_eq!(panel_visibility(&mut app), Visibility::Hidden);

    app.world_mut().resource_mut::<CostPanelState>().open = true;
    app.update();
    app.update();
    assert_eq!(panel_visibility(&mut app), Visibility::Visible);

    let mut q = app
        .world_mut()
        .query_filtered::<&Text, With<CostCurrentText>>();
    let text = q.single(app.world()).expect("当前会话文本");
    assert!(text.0.contains("当前会话"), "实际: {}", text.0);

    let close = {
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, With<CostPanelClose>>();
        q.single(app.world()).expect("关闭按钮")
    };
    app.world_mut().entity_mut(close).insert(Pressed);
    app.update();
    app.update();
    assert!(!app.world().resource::<CostPanelState>().open);
    assert_eq!(panel_visibility(&mut app), Visibility::Hidden);
}

#[test]
fn panel_renders_history_and_models() {
    let mut app = test_app();
    app.update();
    app.world_mut().resource_mut::<CostPanelState>().open = true;
    for _ in 0..100 {
        app.update();
        if !app.world().resource::<CostHistoryState>().loading {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let mut history = HistoryUsage::default();
    history.sessions = 2;
    history.prompt = 1_000_000;
    history.completion = 500_000;
    history.cost = Some(3.0);
    history.models.insert(
        "mini".to_string(),
        ModelUsage {
            model: "mini".to_string(),
            prompt: 1_000_000,
            completion: 500_000,
            cost: Some(3.0),
        },
    );
    {
        let mut state = app.world_mut().resource_mut::<CostHistoryState>();
        state.loading = false;
        state.error = None;
        state.result = Some(history);
    }
    app.update();
    let mut q = app
        .world_mut()
        .query_filtered::<&Text, With<CostHistoryText>>();
    let text = q.single(app.world()).expect("历史文本");
    assert!(text.0.contains('2'), "实际: {}", text.0);
    let mut q = app
        .world_mut()
        .query_filtered::<&Text, With<CostModelsText>>();
    let models = q.single(app.world()).expect("模型文本");
    assert!(models.0.contains("mini"), "实际: {}", models.0);
}
