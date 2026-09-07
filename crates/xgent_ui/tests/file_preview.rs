//! 文件打开/预览页/抽屉回归测试（M5-T6 抽屉化后）。
//!
//! 历史：复现「点击打开文件后编辑器视图闪一下就消失」回归。根因是终端模块
//! `handle_close_tab_requests`（`terminal/tabs.rs`）在关闭 tab 的 for 循环**之外**
//! 每帧检查 `tabs.is_empty()` 并把 `SideViewContent` 重置为 `None`。修复后视图稳定。
//!
//! M5-T6 行为变更：文件面板抽屉化，点击文件条目统一发 `OpenFileRequest`
//! 走上下文面板预览页（编辑器 buffer）加载，原内嵌预览区已取消。
//! 本文件验证：打开文件 → 预览页稳定显示；抽屉开关链路；非代码文件也走编辑器。

use std::io::Write;

use bevy::prelude::*;
use bevy::ui::Display;
use xgent_settings::Localizer;
use xgent_ui::editor::{EditorPlugin, EditorViewMarker, SideViewContent};
use xgent_ui::file_panel::FilePanelPlugin;
use xgent_ui::layout::{FileDrawerOpen, LayoutPlugin};
use xgent_ui::resize::ResizePlugin;
use xui::i18n_bridge::Strings;
use xui_i18n::StringSource;

/// 空 StringSource（测试不关心 i18n 文案）。
struct NoopStrings;
impl StringSource for NoopStrings {
    fn get(&self, key: &str, _args: &[(&str, String)]) -> String {
        key.to_string()
    }
    fn current_lang(&self) -> &str {
        "zh-CN"
    }
}

/// 测试插件集：接近真实 app 的最小组合（含 TerminalPlugin）。
fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<bevy::image::Image>()
        .init_asset::<bevy::text::Font>()
        .insert_resource(xgent_ui::fonts::UiFonts {
            ui: Handle::default(),
            mono: Handle::default(),
        })
        .add_plugins((
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            xui::XuiPlugin,
            LayoutPlugin,
            ResizePlugin,
            EditorPlugin,
            FilePanelPlugin,
            xgent_ui::terminal::TerminalPlugin,
            xgent_ui::kit::KitPlugin,
        ))
        .insert_resource(Strings(Box::new(NoopStrings)))
        .init_resource::<Localizer>();
    for _ in 0..3 {
        app.update();
    }
    app
}

/// 读 `EditorViewMarker`（预览页主体）的 display。
fn editor_view_display(app: &mut App) -> Display {
    let mut q = app
        .world_mut()
        .query_filtered::<&Node, With<EditorViewMarker>>();
    q.iter(app.world())
        .next()
        .map(|n| n.display)
        .unwrap_or(Display::None)
}

/// 模拟点击文件抽屉条目（spawn 带按下态 Interaction 的 FileEntry）。
fn click_file(app: &mut App, path: &std::path::Path) {
    app.world_mut().spawn((
        Button,
        Node::default(),
        bevy::ui::Interaction::Pressed,
        xgent_ui::file_panel::FileEntry {
            path: path.to_path_buf(),
        },
    ));
}

/// 打开代码文件后，预览页应稳定显示，不会闪一下消失。
#[test]
fn open_code_file_keeps_editor_view_visible() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").expect("创建临时文件");
    writeln!(tmp, "fn main() {{}}").expect("写入临时文件");
    tmp.flush().expect("flush");
    let path = tmp.path().to_path_buf();

    let mut app = test_app();
    click_file(&mut app, &path);

    // 跑多帧，断言预览页 display 始终为 Flex（稳定可见）
    for frame in 1..=10 {
        app.update();
        let display = editor_view_display(&mut app);
        let content = *app.world().resource::<SideViewContent>();
        assert_eq!(
            content,
            SideViewContent::Editor,
            "帧 {frame}: SideViewContent 应保持 Editor，实际 {content:?}"
        );
        assert_eq!(
            display,
            Display::Flex,
            "帧 {frame}: EditorViewMarker display 应为 Flex（闪一下消失回归），实际 {display:?}"
        );
    }
}

/// 点击非代码文件后，预览页同样稳定显示（v7：非代码文件也走编辑器 buffer）。
#[test]
fn click_non_code_file_keeps_preview_page_visible() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".log").expect("创建临时文件");
    writeln!(tmp, "hello preview").expect("写入临时文件");
    tmp.flush().expect("flush");
    let path = tmp.path().to_path_buf();

    let mut app = test_app();
    click_file(&mut app, &path);

    for frame in 1..=10 {
        app.update();
        let display = editor_view_display(&mut app);
        let content = *app.world().resource::<SideViewContent>();
        assert_eq!(
            content,
            SideViewContent::Editor,
            "帧 {frame}: 非代码文件也应切到预览页（Editor），实际 {content:?}"
        );
        assert_eq!(display, Display::Flex, "帧 {frame}: 预览页应稳定显示");
    }
}

/// 文件抽屉开关链路：FileDrawerOpen 切换时抽屉与遮罩显隐联动。
#[test]
fn file_drawer_visibility_toggles() {
    let mut app = test_app();

    // 初始关闭：抽屉与遮罩均隐藏
    {
        let (panel, overlay) = drawer_displays(&mut app);
        assert_eq!(panel, Display::None, "抽屉初始应隐藏");
        assert_eq!(overlay, Display::None, "遮罩初始应隐藏");
    }

    // 打开抽屉
    app.world_mut().resource_mut::<FileDrawerOpen>().0 = true;
    app.update();
    {
        let (panel, overlay) = drawer_displays(&mut app);
        assert_eq!(panel, Display::Flex, "抽屉打开后应显示");
        assert_eq!(overlay, Display::Flex, "遮罩随抽屉显示");
    }

    // 关闭抽屉
    app.world_mut().resource_mut::<FileDrawerOpen>().0 = false;
    app.update();
    {
        let (panel, overlay) = drawer_displays(&mut app);
        assert_eq!(panel, Display::None, "抽屉关闭后应隐藏");
        assert_eq!(overlay, Display::None, "遮罩随抽屉隐藏");
    }
}

/// 读取抽屉面板与遮罩的 display（不存在时返回 None 态）。
fn drawer_displays(app: &mut App) -> (Display, Display) {
    let mut q_panel = app
        .world_mut()
        .query_filtered::<&Node, With<xgent_ui::layout::FilePanelMarker>>();
    let panel = q_panel
        .iter(app.world())
        .next()
        .map(|n| n.display)
        .unwrap_or(Display::None);
    let mut q_overlay = app
        .world_mut()
        .query_filtered::<&Node, With<xgent_ui::file_panel::FileDrawerOverlayMarker>>();
    let overlay = q_overlay
        .iter(app.world())
        .next()
        .map(|n| n.display)
        .unwrap_or(Display::None);
    (panel, overlay)
}

/// 点击文件条目应自动关闭抽屉（浏览→点文件→预览页打开全链路）。
#[test]
fn clicking_file_closes_drawer_and_opens_preview() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").expect("创建临时文件");
    writeln!(tmp, "fn main() {{}}").expect("写入临时文件");
    tmp.flush().expect("flush");
    let path = tmp.path().to_path_buf();

    let mut app = test_app();
    // 先打开抽屉
    app.world_mut().resource_mut::<FileDrawerOpen>().0 = true;
    app.update();

    click_file(&mut app, &path);
    app.update();

    // 抽屉应关闭 + 预览页应打开
    let drawer_open = app.world().resource::<FileDrawerOpen>().0;
    assert!(!drawer_open, "点击文件后抽屉应自动关闭");
    assert_eq!(
        *app.world().resource::<SideViewContent>(),
        SideViewContent::Editor,
        "点击文件后应进入预览页"
    );
}

/// 大文件也走编辑器链路：点击后进入预览页且不 panic（截断逻辑属编辑器）。
#[test]
fn large_file_click_enters_preview_page() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let log_path = dir.path().join("big.log");
    let line = "x".repeat(80);
    let content: String = std::iter::repeat_with(|| format!("{line}\n"))
        .take(5000) // ~400KB
        .collect();
    std::fs::write(&log_path, content).expect("写大文件");

    let mut app = test_app();
    click_file(&mut app, &log_path);
    for _ in 0..10 {
        app.update();
    }

    assert_eq!(
        *app.world().resource::<SideViewContent>(),
        SideViewContent::Editor,
        "大文件点击后应进入预览页"
    );
    assert_eq!(editor_view_display(&mut app), Display::Flex);
}
