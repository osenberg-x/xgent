//! xgent_ui 集成测试。

#![cfg(test)]

use bevy::prelude::*;
use xgent_settings::Localizer;

use xgent_ui::layout::LayoutPlugin;
use xgent_ui::layout::{ChatPanelMarker, MainAreaMarker, StatusBarMarker, TopBarMarker, UiRoot};

/// 测试布局：启动后各区域 marker 节点存在。
#[test]
fn layout_spawns_regions() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Localizer>()
        .add_plugins(LayoutPlugin);

    // 跑若干帧让 Startup 执行
    for _ in 0..3 {
        app.update();
    }

    fn has<T: Component>(app: &mut App) -> bool {
        let mut q = app.world_mut().query_filtered::<Entity, With<T>>();
        q.iter(app.world()).next().is_some()
    }
    assert!(has::<UiRoot>(&mut app), "UiRoot 应存在");
    assert!(has::<TopBarMarker>(&mut app), "TopBar 应存在");
    assert!(has::<MainAreaMarker>(&mut app), "MainArea 应存在");
    assert!(has::<ChatPanelMarker>(&mut app), "ChatPanel 应存在");
    assert!(has::<StatusBarMarker>(&mut app), "StatusBar 应存在");
}

/// 测试布局 + 文件抽屉插件：抽屉面板节点存在（初始隐藏）。
#[test]
fn file_drawer_panel_spawns_with_plugin() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<bevy::image::Image>()
        .init_asset::<bevy::text::Font>()
        .init_resource::<Localizer>()
        .insert_resource(xgent_ui::fonts::UiFonts {
            ui: Handle::default(),
            mono: Handle::default(),
        })
        .add_plugins(LayoutPlugin)
        .add_plugins(xgent_ui::file_panel::FilePanelPlugin)
        .add_plugins(xgent_ui::kit::KitPlugin)
        .add_message::<xgent_ui::editor::conflict::FileChangedEvent>()
        .add_message::<xgent_ui::editor::tabs::OpenFileRequest>();

    for _ in 0..3 {
        app.update();
    }

    let mut q = app
        .world_mut()
        .query_filtered::<&Node, With<xgent_ui::layout::FilePanelMarker>>();
    let node = q
        .iter(app.world())
        .next()
        .expect("抽屉面板节点应存在（初始隐藏也应有节点）");
    assert_eq!(
        node.display,
        bevy::ui::Display::None,
        "抽屉初始应隐藏（FileDrawerOpen 默认 false）"
    );
}
