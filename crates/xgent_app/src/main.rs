use bevy::{prelude::*, window::WindowResolution};
use xgent_settings::{ProviderSettings, XgentSettingsPlugin};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "XGent".to_string(),
                resolution: WindowResolution::new(1080, 920),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(XgentSettingsPlugin)
        .add_systems(Update, |settings: Res<ProviderSettings>| {
            info!("default_provider: {}", settings.default_provider);
        })
        .run();
}
