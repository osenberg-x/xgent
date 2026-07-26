//! 文件预览/编辑器视图回归测试。
//!
//! 复现「点击打开文件后预览区/编辑器视图闪一下就消失」回归。根因是终端模块
//! `handle_close_tab_requests`（`terminal/tabs.rs`）在关闭 tab 的 for 循环**之外**
//! 每帧检查 `tabs.is_empty()` 并把 `SideViewContent` 重置为 `None`。因终端默认无
//! tab，该系统每帧覆盖编辑器/预览视图设置，导致点击文件后视图闪一下即被重置。
//! 修复：把 `is_empty` 检查移入关闭循环内，仅在真正关闭一个 tab 后触发。
//!
//! 本测试用接近真实 app 的插件集（含 TerminalPlugin）无头验证：打开文件后
//! `EditorViewMarker`/`FilePreviewMarker` 的 `display` 跨多帧稳定为 Flex。

use std::io::Write;

use bevy::prelude::*;
use bevy::ui::Display;
use xgent_settings::Localizer;
use xgent_ui::editor::tabs::OpenFileRequest;
use xgent_ui::editor::{EditorPlugin, EditorViewMarker, SideViewContent};
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

/// 打开代码文件后，编辑器视图（含文件标签头）应稳定显示，不会闪一下消失。
#[test]
fn open_code_file_keeps_editor_view_visible() {
    // 临时 .rs 文件（代码文件 → Editor 视图）
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").expect("创建临时文件");
    writeln!(tmp, "fn main() {{}}").expect("写入临时文件");
    tmp.flush().expect("flush");
    let path = tmp.path().to_path_buf();

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins((
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            xui::XuiPlugin,
            LayoutPlugin,
            ResizePlugin,
            EditorPlugin,
            FilePanelPlugin,
            xgent_ui::terminal::TerminalPlugin,
        ))
        .insert_resource(Strings(Box::new(NoopStrings)))
        .init_resource::<Localizer>();
    // Theme / SideViewCollapsed 由 LayoutPlugin；EditorTabs/EditorView/SideViewContent
    // /EditorIoRuntime/EditorStateSnapshot 及各 message 由 EditorPlugin。

    // 跑 Startup
    for _ in 0..3 {
        app.update();
    }

    // 直接发 OpenFileRequest（等价于点击代码文件后 handle_file_click 写入的 message）
    app.world_mut().write_message(OpenFileRequest {
        path: path.clone(),
        line: None,
    });

    // 跑多帧，断言编辑器视图 display 始终为 Flex（稳定可见）
    for frame in 1..=10 {
        app.update();
        let editor_display = {
            let mut q = app
                .world_mut()
                .query_filtered::<&Node, With<EditorViewMarker>>();
            q.iter(app.world())
                .next()
                .map(|n| n.display)
                .unwrap_or(Display::None)
        };
        let content = *app.world().resource::<SideViewContent>();
        assert_eq!(
            content,
            SideViewContent::Editor,
            "帧 {frame}: SideViewContent 应保持 Editor，实际 {content:?}"
        );
        assert_eq!(
            editor_display,
            Display::Flex,
            "帧 {frame}: EditorViewMarker display 应为 Flex（闪一下消失回归），实际 {editor_display:?}"
        );
    }
}

/// 点击非代码文件后，文件预览区（含 📄 路径标签头）应稳定显示，不会闪一下消失。
#[test]
fn click_non_code_file_keeps_preview_visible() {
    // 临时 .log 文件（非代码文件 → Preview 视图）
    let mut tmp = tempfile::NamedTempFile::with_suffix(".log").expect("创建临时文件");
    writeln!(tmp, "hello preview").expect("写入临时文件");
    tmp.flush().expect("flush");
    let path = tmp.path().to_path_buf();

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins((
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            xui::XuiPlugin,
            LayoutPlugin,
            ResizePlugin,
            EditorPlugin,
            FilePanelPlugin,
            xgent_ui::terminal::TerminalPlugin,
        ))
        .insert_resource(Strings(Box::new(NoopStrings)))
        .init_resource::<Localizer>();

    for _ in 0..3 {
        app.update();
    }

    // 模拟点击文件条目：spawn FileEntry Button，Interaction 设为 Pressed
    app.world_mut().spawn((
        Button,
        Node::default(),
        bevy::ui::Interaction::Pressed,
        xgent_ui::file_panel::FileEntry { path: path.clone() },
    ));

    // 跑多帧，断言预览容器 display 始终为 Flex
    for frame in 1..=10 {
        app.update();
        let preview_display = {
            let mut q = app
                .world_mut()
                .query_filtered::<&Node, With<xgent_ui::file_panel::FilePreviewMarker>>();
            q.iter(app.world())
                .next()
                .map(|n| n.display)
                .unwrap_or(Display::None)
        };
        let content = *app.world().resource::<SideViewContent>();
        assert_eq!(
            content,
            SideViewContent::Preview,
            "帧 {frame}: SideViewContent 应保持 Preview，实际 {content:?}"
        );
        assert_eq!(
            preview_display,
            Display::Flex,
            "帧 {frame}: FilePreviewMarker display 应为 Flex（闪一下消失回归），实际 {preview_display:?}"
        );
    }
}
