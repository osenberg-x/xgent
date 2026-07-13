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

use crate::i18n::tr;

/// 快捷键插件。
pub struct ShortcutsPlugin;

impl Plugin for ShortcutsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_xgent_hotkeys)
            .add_systems(Update, handle_hotkey_triggers);
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
}

/// 订阅 HotkeyTriggered，据 id 执行业务。
fn handle_hotkey_triggers(
    mut reader: MessageReader<HotkeyTriggered>,
    mut palette: ResMut<CommandPaletteState>,
    mut abort_writer: MessageWriter<AbortMessage>,
) {
    for ev in reader.read() {
        match ev.id.as_str() {
            "palette.open" | "palette.open_files" => palette.open(),
            "chat.abort" => {
                abort_writer.write(AbortMessage);
            }
            "settings.open" => palette.open(),
            _ => {}
        }
    }
}
