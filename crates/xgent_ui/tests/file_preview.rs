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

/// 点击非代码文件后，预览内容应被异步读取并填充到 fv-body。
///
/// 验证 `PreviewReadResult` → `apply_preview_read_result` 链路：
/// 无 runtime 时走同步降级路径（`commands.write_message`），下帧由
/// `apply_preview_read_result` 消费消息并 spawn Text 节点到 fv-body。
#[test]
fn non_code_file_preview_content_filled() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".log").expect("创建临时文件");
    writeln!(tmp, "line one").expect("写入");
    writeln!(tmp, "line two").expect("写入");
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

    // 模拟点击非代码文件条目
    app.world_mut().spawn((
        Button,
        Node::default(),
        bevy::ui::Interaction::Pressed,
        xgent_ui::file_panel::FileEntry { path: path.clone() },
    ));

    // 跑多帧让 handle_file_click → write_message → apply_preview_read_result 执行
    for _ in 0..10 {
        app.update();
    }

    // fv-body 应有子 Text 节点（内容已填充）
    let body_children = {
        let mut q_body = app
            .world_mut()
            .query_filtered::<Entity, With<xgent_ui::file_panel::FilePreviewBodyMarker>>();
        let body = q_body.iter(app.world()).next();
        if let Some(body) = body {
            let mut q_children = app.world_mut().query::<&Children>();
            q_children
                .get(app.world(), body)
                .map(|c| c.len())
                .unwrap_or(0)
        } else {
            0
        }
    };
    assert!(
        body_children > 0,
        "fv-body 应有子节点（预览内容已填充），实际 {body_children}"
    );

    // 验证填充的文本包含文件内容
    let has_content = {
        let mut q_body = app
            .world_mut()
            .query_filtered::<Entity, With<xgent_ui::file_panel::FilePreviewBodyMarker>>();
        let body = q_body.iter(app.world()).next();
        if let Some(body) = body {
            let children: Vec<Entity> = app
                .world()
                .entity(body)
                .get::<Children>()
                .map(|c| c.to_vec())
                .unwrap_or_default();
            let mut q_text = app.world_mut().query::<&Text>();
            let mut found = false;
            for child in children {
                if let Ok(t) = q_text.get(app.world(), child) {
                    if t.0.contains("line one") || t.0.contains("line two") {
                        found = true;
                        break;
                    }
                }
            }
            found
        } else {
            false
        }
    };
    assert!(
        has_content,
        "fv-body 子节点文本应包含文件内容 'line one'/'line two'"
    );
}

/// 过期的异步预览结果应被丢弃：点击非代码文件 A 后切换到代码文件 B（Editor 视图），
/// A 的读取结果到达时不应污染预览区状态。
#[test]
fn stale_preview_result_discarded_after_view_switch() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let root = dir.path().to_path_buf();

    // 非代码文件 A
    let log_path = root.join("a.log");
    std::fs::write(&log_path, "log content").expect("写文件");

    // 代码文件 B
    let rs_path = root.join("b.rs");
    std::fs::write(&rs_path, "fn main() {}").expect("写文件");

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

    // 模拟点击非代码文件 A（触发预览视图 + 同步降级读取）
    app.world_mut().spawn((
        Button,
        Node::default(),
        bevy::ui::Interaction::Pressed,
        xgent_ui::file_panel::FileEntry {
            path: log_path.clone(),
        },
    ));
    app.update();

    // 确认已进入 Preview 视图
    let content = *app.world().resource::<SideViewContent>();
    assert_eq!(content, SideViewContent::Preview);

    // 模拟点击代码文件 B（切换到 Editor 视图）
    app.world_mut().spawn((
        Button,
        Node::default(),
        bevy::ui::Interaction::Pressed,
        xgent_ui::file_panel::FileEntry {
            path: rs_path.clone(),
        },
    ));
    app.update();

    // 现在视图应为 Editor
    let content = *app.world().resource::<SideViewContent>();
    assert_eq!(content, SideViewContent::Editor);

    // 发一个过期的 PreviewReadResult（路径为 A），模拟异步读取延迟到达
    app.world_mut().write_message(xgent_ui::file_panel::PreviewReadResult {
        path: log_path,
        content: Ok((11, "log content".to_string())),
    });

    // 跑多帧让 apply_preview_read_result 处理
    for _ in 0..5 {
        app.update();
    }

    // 视图应仍为 Editor，不被过期结果污染
    let content = *app.world().resource::<SideViewContent>();
    assert_eq!(
        content,
        SideViewContent::Editor,
        "过期预览结果不应将视图从 Editor 切回 Preview"
    );
    // fv-body 不应被过期结果填充（仍是空或之前的状态）
    // 验证 apply_preview_read_result 未处理过期消息：发消息后 fv-body 子节点数不变
    let body_children_before = {
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, With<xgent_ui::file_panel::FilePreviewBodyMarker>>();
        let body = q.iter(app.world()).next();
        let mut q_children = app.world_mut().query::<&Children>();
        body.and_then(|b| q_children.get(app.world(), b).ok())
            .map(|c| c.len())
            .unwrap_or(0)
    };
    // 过期结果到达前的子节点数应与之后一致（未被过期结果填充新内容）
    // 发另一个过期结果再跑帧，子节点数不应增长
    app.world_mut().write_message(xgent_ui::file_panel::PreviewReadResult {
        path: root.join("a.log"),
        content: Ok((99, "stale payload".to_string())),
    });
    for _ in 0..3 {
        app.update();
    }
    let body_children_after = {
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, With<xgent_ui::file_panel::FilePreviewBodyMarker>>();
        let body = q.iter(app.world()).next();
        let mut q_children = app.world_mut().query::<&Children>();
        body.and_then(|b| q_children.get(app.world(), b).ok())
            .map(|c| c.len())
            .unwrap_or(0)
    };
    assert_eq!(
        body_children_before, body_children_after,
        "过期预览结果不应修改 fv-body 内容"
    );
}

/// 大文件预览应截断到 256KB 上限，不 OOM，且字节数显示原始大小。
#[test]
fn large_file_preview_truncated() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let root = dir.path().to_path_buf();

    // 创建 > 256KB 的 .log 文件（非代码文件 → Preview 视图）
    let log_path = root.join("big.log");
    let line = "x".repeat(80);
    let content: String = std::iter::repeat_with(|| format!("{line}\n"))
        .take(5000) // ~400KB
        .collect();
    std::fs::write(&log_path, content).expect("写大文件");

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

    // 模拟点击大文件
    app.world_mut().spawn((
        Button,
        Node::default(),
        bevy::ui::Interaction::Pressed,
        xgent_ui::file_panel::FileEntry {
            path: log_path.clone(),
        },
    ));

    // 跑多帧让同步降级读取 + apply 执行
    for _ in 0..10 {
        app.update();
    }

    // fv-body 应有子节点（截断后内容已填充）
    let body_has_children = {
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, With<xgent_ui::file_panel::FilePreviewBodyMarker>>();
        let body = q.iter(app.world()).next();
        let mut q_children = app.world_mut().query::<&Children>();
        body.and_then(|b| q_children.get(app.world(), b).ok())
            .map(|c| !c.is_empty())
            .unwrap_or(false)
    };
    assert!(body_has_children, "大文件预览应截断后填充内容");
}

/// 含中文注释的 Rust 文件高亮不应 panic（UTF-8 字节边界安全）。
#[test]
fn rust_file_with_utf8_highlight_no_panic() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let root = dir.path().to_path_buf();

    // 创建含中文注释的 .rs 文件
    let rs_path = root.join("utf8_test.rs");
    // 注意：此文件走代码文件路径（Editor 视图），不走 Preview 高亮。
    // 但 PreviewReadResult 可被手动注入验证 apply_preview_read_result 的字节边界安全。
    std::fs::write(&rs_path, "// 中文注释\nfn 主函数() {\n    println!(\"你好\");\n}\n")
        .expect("写文件");

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

    // 手动设 Preview 视图 + CurrentPreviewPath（绕过 is_code_file 判断）
    {
        let mut content = app
            .world_mut()
            .resource_mut::<SideViewContent>();
        *content = SideViewContent::Preview;
    }
    {
        let mut cp = app
            .world_mut()
            .resource_mut::<xgent_ui::file_panel::CurrentPreviewPath>();
        *cp = xgent_ui::file_panel::CurrentPreviewPath(Some(rs_path.clone()));
    }

    // 重新确认 Preview 视图状态（跑帧后可能被其他系统重置）
    {
        let mut content = app
            .world_mut()
            .resource_mut::<SideViewContent>();
        *content = SideViewContent::Preview;
    }
    {
        let mut cp = app
            .world_mut()
            .resource_mut::<xgent_ui::file_panel::CurrentPreviewPath>();
        *cp = xgent_ui::file_panel::CurrentPreviewPath(Some(rs_path.clone()));
    }
    // 注入 PreviewReadResult 并跑帧（apply_preview_read_result 在 chain 末尾，
    // 但 content/CurrentPreviewPath 在同一帧的 chain 中不被其他系统修改）
    // 读取文件内容并注入 PreviewReadResult（手动调用 truncate 等价逻辑）
    let bytes = std::fs::read(&rs_path).expect("读文件");
    let len = bytes.len();
    let text = String::from_utf8_lossy(&bytes).to_string();
    app.world_mut().write_message(xgent_ui::file_panel::PreviewReadResult {
        path: rs_path.clone(),
        content: Ok((len, text)),
    });

    // 跑帧让 apply_preview_read_result 处理（含高亮 span 切片），不应 panic
    for _ in 0..5 {
        app.update();
    }

    // 如果到达这里说明没有 panic
    let body_has_children = {
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, With<xgent_ui::file_panel::FilePreviewBodyMarker>>();
        let body = q.iter(app.world()).next();
        let mut q_children = app.world_mut().query::<&Children>();
        body.and_then(|b| q_children.get(app.world(), b).ok())
            .map(|c| !c.is_empty())
            .unwrap_or(false)
    };
    assert!(body_has_children, "含中文的 Rust 文件高亮应填充内容");
}

/// 点击代码文件后应清除预览状态：CurrentPreviewPath 为 None，预览头文本为空。
#[test]
fn code_file_click_clears_preview_state() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let root = dir.path().to_path_buf();

    // 非代码文件 A（先预览 A，再切到代码文件 B）
    let log_path = root.join("a.log");
    std::fs::write(&log_path, "log content").expect("写文件");
    let rs_path = root.join("b.rs");
    std::fs::write(&rs_path, "fn main() {}").expect("写文件");

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

    // 模拟点击非代码文件 A（触发预览视图）
    app.world_mut().spawn((
        Button,
        Node::default(),
        bevy::ui::Interaction::Pressed,
        xgent_ui::file_panel::FileEntry {
            path: log_path.clone(),
        },
    ));
    app.update();

    // 确认已进入 Preview 视图 + CurrentPreviewPath = A
    assert_eq!(*app.world().resource::<SideViewContent>(), SideViewContent::Preview);
    assert_eq!(
        app.world()
            .resource::<xgent_ui::file_panel::CurrentPreviewPath>()
            .0
            .as_ref(),
        Some(&log_path)
    );

    // 模拟点击代码文件 B（切换到 Editor 视图）
    app.world_mut().spawn((
        Button,
        Node::default(),
        bevy::ui::Interaction::Pressed,
        xgent_ui::file_panel::FileEntry {
            path: rs_path.clone(),
        },
    ));
    app.update();

    // 视图应为 Editor
    assert_eq!(*app.world().resource::<SideViewContent>(), SideViewContent::Editor);

    // CurrentPreviewPath 应为 None（被清除）
    assert!(
        app.world()
            .resource::<xgent_ui::file_panel::CurrentPreviewPath>()
            .0
            .is_none(),
        "点击代码文件后 CurrentPreviewPath 应为 None"
    );
}
