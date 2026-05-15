use bevy::prelude::*;
use xgent_agent::XgentAgentPlugin;
use xgent_mcp::XgentMcpPlugin;
use xgent_provider::XgentProviderPlugin;
use xgent_settings::XgentSettingsPlugin;
use xgent_tools::XgentToolsPlugin;
use xgent_ui::XgentUiPlugins;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "XGent".to_string(),
                resolution: (1280.0, 800.0).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(XgentSettingsPlugin)
        .add_plugins(XgentProviderPlugin)
        .add_plugins(XgentToolsPlugin)
        .add_plugins(XgentMcpPlugin)
        .add_plugins(XgentAgentPlugin)
        .add_plugins(XgentUiPlugins)
        .run();
}
