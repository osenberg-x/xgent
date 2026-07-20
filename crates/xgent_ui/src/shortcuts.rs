//! 快捷键体系：参考 VSCode 默认绑定，注册快捷键并订阅 [`HotkeyTriggered`] 执行业务。
//!
//! 平台修饰键与冲突检测由 [`xui`] 处理。

use bevy::ecs::message::MessageWriter;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use xgent_agent::AbortMessage;
use xgent_settings::Localizer;
use xui::command_palette::CommandPaletteState;
use xui::hotkeys::{Hotkey, HotkeyRegistry};
use xui::shortcuts::HotkeyTriggered;

use crate::editor::EditorView;
use crate::editor::tabs::CycleTabRequest;
use crate::i18n::tr;
use crate::layout::FilePanelCollapsed;

/// 快捷键插件。
pub struct ShortcutsPlugin;

impl Plugin for ShortcutsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_xgent_hotkeys)
            .add_systems(
                Update,
                handle_hotkey_triggers.after(crate::command_palette::handle_palette_triggers),
            );
    }
}

/// 启动时注册参考 VSCode 的快捷键。
pub fn register_xgent_hotkeys(mut reg: ResMut<HotkeyRegistry>, loc: Res<Localizer>) {
    // Cmd/Ctrl+Shift+P：打开命令面板（动作）
    let _ = reg.register(
        Hotkey::new("palette.open", KeyCode::KeyP, tr(&loc, "hotkey-palette"))
            .with_primary()
            .with_shift(),
    );
    // Cmd/Ctrl+P：文件快速打开（MVP 复用命令面板）
    let _ = reg.register(
        Hotkey::new(
            "palette.open_files",
            KeyCode::KeyP,
            tr(&loc, "hotkey-files"),
        )
        .with_primary(),
    );
    // Esc：中断当前对话
    let _ = reg.register(Hotkey::new(
        "chat.abort",
        KeyCode::Escape,
        tr(&loc, "hotkey-abort"),
    ));
    // Cmd/Ctrl+,：打开设置（MVP 复用命令面板）
    let _ = reg.register(
        Hotkey::new("settings.open", KeyCode::Comma, tr(&loc, "hotkey-settings")).with_primary(),
    );
    // Cmd/Ctrl+B：切换文件面板
    let _ = reg.register(
        Hotkey::new(
            "filepanel.toggle",
            KeyCode::KeyB,
            tr(&loc, "hotkey-toggle-filepanel"),
        )
        .with_primary(),
    );
    // Cmd/Ctrl+I：聚焦输入框
    let _ = reg.register(
        Hotkey::new("input.focus", KeyCode::KeyI, tr(&loc, "hotkey-focus-input")).with_primary(),
    );
    // Cmd/Ctrl+Shift+E：切换到编辑器视图
    let _ = reg.register(
        Hotkey::new("editor.view", KeyCode::KeyE, tr(&loc, "hotkey-editor-view"))
            .with_primary()
            .with_shift(),
    );
    // Cmd/Ctrl+Shift+D：切换回对话视图
    let _ = reg.register(
        Hotkey::new("chat.view", KeyCode::KeyD, tr(&loc, "hotkey-chat-view"))
            .with_primary()
            .with_shift(),
    );
    // Cmd/Ctrl+W：关闭当前标签
    let _ = reg.register(
        Hotkey::new(
            "editor.close_tab",
            KeyCode::KeyW,
            tr(&loc, "hotkey-editor-close-tab"),
        )
        .with_primary(),
    );
    // Cmd/Ctrl+Tab：循环切换标签
    let _ = reg.register(
        Hotkey::new(
            "editor.cycle_tab",
            KeyCode::Tab,
            tr(&loc, "hotkey-editor-cycle-tab"),
        )
        .with_primary(),
    );
    // Cmd/Ctrl+\：切换右侧分屏（对话/编辑器分屏）
    let _ = reg.register(
        Hotkey::new(
            "sideview.toggle",
            KeyCode::Backslash,
            tr(&loc, "hotkey-toggle-sideview"),
        )
        .with_primary(),
    );
}

/// 订阅 HotkeyTriggered，据 id 执行业务。
pub(crate) fn handle_hotkey_triggers(
    mut reader: MessageReader<HotkeyTriggered>,
    mut palette: ResMut<CommandPaletteState>,
    mut abort_writer: MessageWriter<AbortMessage>,
    mut file_panel: ResMut<FilePanelCollapsed>,
    mut side_view: ResMut<crate::layout::SideViewCollapsed>,
    mut view: ResMut<EditorView>,
    mut cycle_writer: MessageWriter<CycleTabRequest>,
) {
    for ev in reader.read() {
        match ev.id.as_str() {
            "palette.open" | "palette.open_files" => palette.open(),
            "chat.abort" => {
                // 编辑器视图激活时，Esc 优先退出编辑器视图而非中断对话
                if *view == EditorView::Editor {
                    *view = EditorView::Chat;
                } else {
                    abort_writer.write(AbortMessage);
                }
            }
            "settings.open" => palette.open(),
            "filepanel.toggle" => {
                file_panel.0 = !file_panel.0;
            }
            "sideview.toggle" => {
                side_view.0 = !side_view.0;
            }
            "input.focus" => {}
            "editor.view" => {
                *view = EditorView::Editor;
            }
            "chat.view" => {
                *view = EditorView::Chat;
            }
            "editor.close_tab" => {
                // 关闭当前标签：MVP 留给 UI 按钮处理
            }
            "editor.cycle_tab" => {
                cycle_writer.write(CycleTabRequest { forward: true });
            }
            _ => {}
        }
    }
}
