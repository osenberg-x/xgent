//! v8 M3-T4/T5 省略原语回归（审计 C3 / E6）。
//!
//! 缺陷 C3：宽度不足时文本直接溢出或换行，用户分不清「被裁」与「就这么长」。
//! 修法是自建省略号原语（bevy 0.20 无 `TextTruncate`），并在 6 处截断点接入。
//!
//! 这里用**真实字体**（随仓库分发的 Inter Variable）跑测量——没有字体的
//! headless 环境量不出宽度，省略逻辑会退化成「不截断」。

use bevy::prelude::*;
use bevy::text::TextLayout;
use xui::ellipsis::Ellipsis;

/// 装一个能跑文本测量的 App（真实默认字体 + 省略插件）。
fn ellipsis_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(bevy::image::ImagePlugin::default())
        .add_plugins(bevy::window::WindowPlugin::default())
        .add_plugins(bevy::picking::input::PointerInputPlugin)
        .add_plugins(bevy::picking::PickingPlugin)
        .add_plugins(bevy::picking::InteractionPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::input_focus::InputFocusPlugin)
        .add_plugins(bevy::input_focus::InputDispatchPlugin)
        .add_plugins(bevy::text::TextPlugin)
        .add_plugins(bevy::ui::UiPlugin)
        .add_plugins(bevy::ui_widgets::UiWidgetsPlugins)
        .init_asset::<bevy::image::TextureAtlasLayout>()
        .add_plugins(bevy::camera::visibility::VisibilityPlugin)
        .add_plugins(bevy::mesh::MeshPlugin)
        .add_plugins(xui::ellipsis::EllipsisPlugin);
    app.world_mut().spawn((Camera2d, IsDefaultUiCamera));
    // 与 xgent_app::startup::load_fonts 同款：把 Inter 插成默认字体
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../xgent_app/assets/fonts/Inter-Variable.ttf"
    );
    let data = std::fs::read(path).expect("Inter Variable 字体应随仓库分发");
    app.world_mut()
        .resource_mut::<Assets<Font>>()
        .insert(AssetId::default(), Font::from_bytes(data));
    app
}

/// 造一个固定宽度的省略文本节点。
fn spawn_ellipsis(app: &mut App, width: f32, source: &str) -> Entity {
    app.world_mut()
        .spawn((
            Node {
                width: Val::Px(width),
                height: Val::Px(20.0),
                overflow: Overflow::clip(),
                ..default()
            },
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..default()
            },
            Text::new(""),
            Ellipsis::new(source),
        ))
        .id()
}

/// 跑若干帧（省略在布局之后才生效）。
fn run(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

/// C3 回归：节点宽度不足时文本被省略成 `…`。
#[test]
fn ellipsis_truncates_when_node_is_narrow() {
    let mut app = ellipsis_app();
    let source = "这是一个非常非常长的文件名，需要被省略掉才会不出现在栏外";
    let e = spawn_ellipsis(&mut app, 120.0, source);
    run(&mut app, 5);

    let text = app.world().get::<Text>(e).expect("应有 Text").0.clone();
    assert!(text.ends_with('…'), "宽度不足应出现省略号（实测 {text:?}）");
    assert!(
        text.chars().count() < source.chars().count(),
        "截断后应比原文短（实测 {text:?}）"
    );
    assert!(
        source.starts_with(text.trim_end_matches('…')),
        "省略后应保留原文前缀（实测 {text:?}）"
    );
}

/// C3 回归：宽度够时**不**截断。
#[test]
fn ellipsis_keeps_full_text_when_it_fits() {
    let mut app = ellipsis_app();
    let source = "短名";
    let e = spawn_ellipsis(&mut app, 400.0, source);
    run(&mut app, 5);

    let text = app.world().get::<Text>(e).expect("应有 Text").0.clone();
    assert_eq!(text, source, "宽度足够时不应截断");
}

/// C3 回归：宽度变化时重新截断（缓存不返回过期结果）。
#[test]
fn ellipsis_follows_width_changes() {
    let mut app = ellipsis_app();
    let source = "这是一个非常非常长的文件名，需要被省略掉才会不出现在栏外";
    let e = spawn_ellipsis(&mut app, 900.0, source);
    run(&mut app, 5);
    let wide = app.world().get::<Text>(e).expect("应有 Text").0.clone();
    assert_eq!(wide, source, "宽节点不应截断");

    // 收窄节点 → 应重新截断
    app.world_mut()
        .entity_mut(e)
        .get_mut::<Node>()
        .expect("应有 Node")
        .width = Val::Px(100.0);
    run(&mut app, 5);
    let narrow = app.world().get::<Text>(e).expect("应有 Text").0.clone();
    assert!(
        narrow.ends_with('…') && narrow.chars().count() < wide.chars().count(),
        "收窄后应重新截断（实测 {narrow:?}）"
    );
}

/// 性能：宽度不变时不应重复测量（缓存命中）。
#[test]
fn ellipsis_cache_avoids_repeated_measurement() {
    let mut app = ellipsis_app();
    let e = spawn_ellipsis(&mut app, 120.0, "一个相当长的文本内容需要被省略处理掉才对");
    run(&mut app, 5);

    let cache = app
        .world()
        .resource::<xui::ellipsis::EllipsisCache>()
        .clone();
    let hits_before = cache.hits;
    let misses_before = cache.misses;
    assert!(misses_before >= 1, "首帧应发生一次测量");

    run(&mut app, 5);
    let cache = app
        .world()
        .resource::<xui::ellipsis::EllipsisCache>()
        .clone();
    assert_eq!(
        cache.misses, misses_before,
        "宽度不变时不应重复测量（misses {} → {}）",
        misses_before, cache.misses
    );
    assert!(
        cache.hits > hits_before,
        "缓存应持续命中（hits {} → {}）",
        hits_before,
        cache.hits
    );
    let _ = e;
}

/// 中段省略：路径保留首段与末两段。
#[test]
fn mid_ellipsis_keeps_last_two_segments() {
    let mut app = ellipsis_app();
    let source = "C:/very/deep/nested/directory/structure/file.rs";
    let e = app
        .world_mut()
        .spawn((
            Node {
                width: Val::Px(300.0),
                height: Val::Px(20.0),
                overflow: Overflow::clip(),
                ..default()
            },
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..default()
            },
            Text::new(""),
            xui::Ellipsis::mid(source),
        ))
        .id();
    run(&mut app, 5);

    let text = app.world().get::<Text>(e).expect("应有 Text").0.clone();
    let codes: Vec<u32> = text.chars().map(|c| c as u32).collect();
    assert!(text.contains('…'), "应出现省略号（codes {codes:?}）");
    assert!(
        text.ends_with("file.rs"),
        "应保留末段文件名（codes {codes:?}）"
    );
    assert!(
        text.contains("structure"),
        "应保留倒数第二段（codes {codes:?}）"
    );
}

/// M3-T5 验收：6 处截断点都接了省略原语。
#[test]
fn six_truncation_points_use_ellipsis() {
    let sources = [
        ("file_panel.rs", include_str!("../src/file_panel.rs")),
        ("editor/mod.rs", include_str!("../src/editor/mod.rs")),
        ("status_bar.rs", include_str!("../src/status_bar.rs")),
        (
            "command_palette.rs",
            include_str!("../src/command_palette.rs"),
        ),
        ("tool_panel.rs", include_str!("../src/tool_panel.rs")),
        (
            "confirm_dialog.rs",
            include_str!("../src/confirm_dialog.rs"),
        ),
    ];
    let consumers = sources
        .iter()
        .filter(|(_, src)| src.contains("ellipsis_text(") || src.contains("ellipsis_path_text("))
        .count();
    assert!(consumers >= 6, "省略消费者应 ≥ 6 处（实测 {consumers}）");
}
