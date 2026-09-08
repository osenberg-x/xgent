//! toast 生命周期回归（M6-T1）。
//!
//! 验证：写 `ToastMessage` → 浮层 spawn；TTL 2.2s 后自动 despawn。

use bevy::prelude::*;
use xgent_settings::Localizer;
use xgent_ui::kit::{KitPlugin, ToastMarker, ToastMessage};
use xui::i18n_bridge::Strings;
use xui_i18n::StringSource;

struct NoopStrings;
impl StringSource for NoopStrings {
    fn get(&self, key: &str, _args: &[(&str, String)]) -> String {
        key.to_string()
    }
    fn current_lang(&self) -> &str {
        "zh-CN"
    }
}

#[test]
fn toast_lifecycle() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<bevy::image::Image>()
        .init_asset::<bevy::text::Font>()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(100),
        ))
        .insert_resource(Strings(Box::new(NoopStrings)))
        .init_resource::<Localizer>()
        .insert_resource(xgent_ui::fonts::UiFonts {
            ui: Handle::default(),
            mono: Handle::default(),
        })
        .add_plugins(xgent_ui::layout::LayoutPlugin)
        .add_plugins(KitPlugin);

    for _ in 0..3 {
        app.update();
    }

    // 发 toast
    app.world_mut().write_message(ToastMessage {
        text: "测试提示".into(),
    });
    app.update();

    fn count(app: &mut App) -> usize {
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, With<ToastMarker>>();
        q.iter(app.world()).count()
    }
    // toast 已 spawn
    assert_eq!(count(&mut app), 1, "toast 应在场");

    // 每帧推进 100ms：10 帧 = 1 秒，toast 仍应在（TTL 2.2s）
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(count(&mut app), 1, "1 秒后 toast 仍应在场（TTL 2.2s）");

    // R4：替换式刷新契约——已有 toast 在场时新消息替换文案并重置计时。
    // 此时旧 toast 已存活 1 秒；若重置语义丢失（继续按旧值倒计时），
    // 再 1.3 秒即超 2.2s 消失；正确重置后 1.3s < 2.2s 应仍在场。
    app.world_mut().write_message(ToastMessage {
        text: "替换后的文案".into(),
    });
    app.update();
    {
        let mut q = app
            .world_mut()
            .query_filtered::<&Text, With<xgent_ui::kit::ToastTextMarker>>();
        let t = q.iter(app.world()).next().expect("toast 文本节点应存在");
        assert_eq!(t.0, "替换后的文案", "在场 toast 应被新消息替换文案");
    }
    for _ in 0..13 {
        app.update();
    }
    assert_eq!(
        count(&mut app),
        1,
        "替换后计时应重置：1.3 秒后 toast 仍应在场"
    );

    // 30 帧 = 3 秒（自重置点）> 2.2s，应已 despawn
    for _ in 0..30 {
        app.update();
    }
    assert_eq!(count(&mut app), 0, "2.2 秒后 toast 应消失");
}
