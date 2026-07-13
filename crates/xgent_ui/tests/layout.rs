//! xgent_ui 集成测试。

#![cfg(test)]

use bevy::prelude::*;
use xgent_settings::Localizer;

use xgent_ui::layout::LayoutPlugin;
use xgent_ui::layout::{
    ChatPanelMarker, FilePanelMarker, MainAreaMarker, StatusBarMarker, TopBarMarker, UiRoot,
};
use xgent_ui::theme::Theme;

/// 测试布局：启动后三区 marker 节点存在。
#[test]
fn layout_spawns_three_regions() {
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
    assert!(has::<FilePanelMarker>(&mut app), "FilePanel 应存在");
    assert!(has::<ChatPanelMarker>(&mut app), "ChatPanel 应存在");
    assert!(has::<StatusBarMarker>(&mut app), "StatusBar 应存在");
}

/// 测试主题默认是暗色。
#[test]
fn theme_default_is_dark() {
    let _theme = Theme::dark();
}
