//! 文件打开/预览页/抽屉回归测试（M5-T6 抽屉化后）。
//!
//! 历史：复现「点击打开文件后编辑器视图闪一下就消失」回归。根因是终端模块
//! `handle_close_tab_requests`（`terminal/tabs.rs`）在关闭 tab 的 for 循环**之外**
//! 每帧检查 `tabs.is_empty()` 并把分屏内容重置为空。修复后视图稳定。
//!
//! M5-T6 行为变更：文件面板抽屉化，点击文件条目统一发 `OpenFileRequest`
//! 走上下文面板预览页（编辑器 buffer）加载，原内嵌预览区已取消。
//! 本文件验证：打开文件 → 预览页稳定显示；抽屉开关链路；非代码文件也走编辑器。

use std::io::Write;

use bevy::prelude::*;
use bevy::ui::Display;
use xgent_settings::Localizer;
use xgent_ui::editor::{EditorPlugin, EditorViewMarker};
use xgent_ui::file_panel::FilePanelPlugin;
use xgent_ui::layout::LayoutPlugin;
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
        .init_asset::<bevy::image::TextureAtlasLayout>()
        .insert_resource(xgent_ui::fonts::UiFonts::fallback())
        .add_plugins((
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            xui::XuiPlugin,
            LayoutPlugin,
            ResizePlugin,
            EditorPlugin,
            FilePanelPlugin,
            xgent_ui::overlay::OverlayPlugin,
            xgent_ui::terminal::TerminalPlugin,
            xgent_ui::kit::KitPlugin,
        ))
        .insert_resource(Strings(Box::new(NoopStrings)))
        .init_resource::<Localizer>();
    // 相机：UI 布局需要渲染目标尺寸（无相机时视口高 0，抽屉高度为 0，
    // 虚拟列表因视口高 0 而不渲染任何行）
    app.world_mut().spawn(Camera::default());
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

/// editor 面板当前是否可见（v8 M5：分屏内容由面板注册表裁决）。
fn editor_panel_visible(app: &mut App) -> bool {
    app.world()
        .resource::<xgent_ui::panels::PanelRegistry>()
        .is_visible(xgent_ui::panels::ids::EDITOR)
}

/// 模拟点击文件抽屉条目（spawn 带按下态 `Pressed` 的 FileEntry）。
fn click_file(app: &mut App, path: &std::path::Path) {
    app.world_mut().spawn((
        bevy::ui_widgets::Button,
        bevy::picking::hover::Hovered::default(),
        Node::default(),
        bevy::ui::Pressed,
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

    // 跑多帧：预览页一旦可见就不得再消失（闪一下消失回归）
    // v8 M5：分屏内容经「点击 → OpenFileRequest → PanelActivate → 注册表」
    // 多一跳，可见性会晚 1~2 帧出现——断言的是「可见后不再消失」。
    let mut became_visible = false;
    for frame in 1..=10 {
        app.update();
        let visible = editor_panel_visible(&mut app);
        if visible {
            became_visible = true;
            assert_eq!(
                editor_view_display(&mut app),
                Display::Flex,
                "帧 {frame}: EditorViewMarker display 应为 Flex"
            );
        } else {
            assert!(
                !became_visible,
                "帧 {frame}: 预览页可见后又消失（闪一下消失回归）"
            );
        }
    }
    assert!(became_visible, "预览页应在一段时间内变为可见");
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

    let mut became_visible = false;
    for frame in 1..=10 {
        app.update();
        let visible = editor_panel_visible(&mut app);
        if visible {
            became_visible = true;
            assert_eq!(
                editor_view_display(&mut app),
                Display::Flex,
                "帧 {frame}: 预览页应稳定显示"
            );
        } else {
            assert!(!became_visible, "帧 {frame}: 预览页可见后又消失");
        }
    }
    assert!(
        became_visible,
        "非代码文件也应切到预览页（editor 面板可见）"
    );
}

/// 行号列与文件实际行数一致（回归：尾部换行曾产生幽灵行号）。
#[test]
fn line_numbers_match_file_line_count() {
    // 单行 + 尾部换行 → 只显示行号 1
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").expect("创建临时文件");
    writeln!(tmp, "fn main() {{}}").expect("写入临时文件");
    tmp.flush().expect("flush");
    let path = tmp.path().to_path_buf();

    let mut app = test_app();
    click_file(&mut app, &path);
    for _ in 0..10 {
        app.update();
    }

    let mut q = app
        .world_mut()
        .query_filtered::<&Text, With<xui::LineNumbersMarker>>();
    let texts: Vec<String> = q.iter(app.world()).map(|t| t.0.clone()).collect();
    assert_eq!(
        texts,
        vec!["   1".to_string()],
        "单行文件（带尾部换行）行号列应只显示行号 1"
    );
}

/// 多行文件（带尾部换行）行号数与实际行数一致。
#[test]
fn line_numbers_match_multiline_file_line_count() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").expect("创建临时文件");
    writeln!(tmp, "fn a() {{}}\nfn b() {{}}\nfn c() {{}}").expect("写入临时文件");
    tmp.flush().expect("flush");
    let path = tmp.path().to_path_buf();

    let mut app = test_app();
    click_file(&mut app, &path);
    for _ in 0..10 {
        app.update();
    }

    let mut q = app
        .world_mut()
        .query_filtered::<&Text, With<xui::LineNumbersMarker>>();
    let texts: Vec<String> = q.iter(app.world()).map(|t| t.0.clone()).collect();
    assert_eq!(
        texts,
        vec!["   1\n   2\n   3".to_string()],
        "三行文件行号列应显示 1..3（尾部幽灵行不计数）"
    );
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
    // v8 M1-T4：抽屉开关统一走 DrawerToggle 消息（OverlayRegistry 是唯一权威），
    // 直接改 Resource 会与注册表失配（点击时的 toggle 会当成「打开」）。
    app.world_mut()
        .write_message(xgent_ui::overlay::DrawerToggle(
            xgent_ui::overlay::OverlayId::FileDrawer,
        ));
    // 两帧：overlay 的 toggle 系统（改 Resource）与文件面板的显隐系统（写
    // Node.display）之间没有显式排序，第二帧保证显示层跟上
    app.update();
    app.update();
    {
        let (panel, overlay) = drawer_displays(&mut app);
        assert_eq!(panel, Display::Flex, "抽屉打开后应显示");
        assert_eq!(overlay, Display::Flex, "遮罩随抽屉显示");
    }

    // 关闭抽屉（v8 M5：抽屉是注册面板，开关经面板消息）
    app.world_mut()
        .write_message(xgent_ui::panels::PanelDeactivate {
            id: xgent_ui::panels::ids::FILE_DRAWER,
        });
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
    // v8 M1-T4：抽屉开关统一走 DrawerToggle 消息（OverlayRegistry 是唯一权威），
    // 直接改 Resource 会与注册表失配（点击时的 toggle 会当成「打开」）。
    app.world_mut()
        .write_message(xgent_ui::overlay::DrawerToggle(
            xgent_ui::overlay::OverlayId::FileDrawer,
        ));
    app.update();

    click_file(&mut app, &path);
    // 三帧：handle_file_click（写 DrawerToggle）→ overlay 的 toggle（改
    // Resource）→ handle_drawer_visibility（写 Node.display）之间无显式排序，
    // 三帧保证链路走完
    app.update();
    app.update();
    app.update();

    // R4：显示层断言（Resource 已关但 display 未跟上时该断言失败）
    let (panel, overlay) = drawer_displays(&mut app);
    assert_eq!(panel, Display::None, "点击文件后抽屉面板应隐藏");
    assert_eq!(overlay, Display::None, "点击文件后抽屉遮罩应隐藏");
    assert!(editor_panel_visible(&mut app), "点击文件后应进入预览页");
}

/// 大文件端到端加载：点击后内容应异步读入 buffer（消费者可观察 rope 行数）。
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

    assert!(editor_panel_visible(&mut app), "大文件点击后应进入预览页");
    assert_eq!(editor_view_display(&mut app), Display::Flex);

    // R4：内容加载断言——rope 行数 = 5000 数据行（+ 末行换行产生的空行）
    use xgent_ui::editor::buffer::EditorBuffer;
    let mut q = app.world_mut().query_filtered::<&EditorBuffer, ()>();
    let buf = q
        .iter(app.world())
        .next()
        .expect("大文件应产生 buffer 实体");
    assert_eq!(
        buf.disk_content.lines().count(),
        5000,
        "大文件内容应完整加载（无静默截断）"
    );
}
