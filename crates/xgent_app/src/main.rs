use bevy::{prelude::*, window::WindowResolution};

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
        .run();
}
