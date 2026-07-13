//! 命令面板集成：注册 XGent 命令，订阅 [`PaletteTriggered`] 执行业务。
//!
//! MVP 命令集合：新建会话、切换语言、打开设置。文件快速打开留待 file_panel 完善后接入。
//! 面板打开/关闭由快捷键 `Cmd+Shift+P` 触发（见 [`crate::shortcuts`]）。

use bevy::prelude::*;
use xgent_settings::Localizer;
use xui::command_palette::{
    CommandKind, CommandPaletteState, CommandRegistry, PaletteCommand, PaletteTriggered,
};
use xui::i18n_bridge::Strings;

use crate::i18n::tr;

/// 命令面板插件。
pub struct CommandPalettePlugin;

impl Plugin for CommandPalettePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_xgent_commands)
            .add_systems(Update, handle_palette_triggers);
    }
}

/// 启动时注册 XGent 命令。
pub fn register_xgent_commands(mut registry: ResMut<CommandRegistry>, loc: Res<Localizer>) {
    // 新建会话
    registry.register(PaletteCommand {
        id: "session.new".into(),
        label: tr(&loc, "cmd-session-new"),
        kind: CommandKind::Action,
    });
    // 切换为英文
    registry.register(PaletteCommand {
        id: "lang.switch.en".into(),
        label: tr(&loc, "cmd-lang-en"),
        kind: CommandKind::Action,
    });
    // 切换为中文
    registry.register(PaletteCommand {
        id: "lang.switch.zh".into(),
        label: tr(&loc, "cmd-lang-zh"),
        kind: CommandKind::Action,
    });
    // 打开设置
    registry.register(PaletteCommand {
        id: "settings.open".into(),
        label: tr(&loc, "cmd-settings-open"),
        kind: CommandKind::Action,
    });
}

/// 订阅 PaletteTriggered，据命令 id 执行业务（MVP 仅打印日志 + 关闭面板）。
fn handle_palette_triggers(
    mut reader: MessageReader<PaletteTriggered>,
    mut state: ResMut<CommandPaletteState>,
    mut loc: ResMut<Localizer>,
) {
    for ev in reader.read() {
        match ev.command_id.as_str() {
            "lang.switch.en" => loc.switch("en-US"),
            "lang.switch.zh" => loc.switch("zh-CN"),
            "session.new" => { /* TODO: 重置会话 */ }
            "settings.open" => { /* TODO: 打开设置面板 */ }
            _ => {}
        }
        state.close();
    }
}

/// 用注入的 Strings 刷新命令标签（语言切换后由调用方触发）。
pub fn relabel_commands(mut registry: ResMut<CommandRegistry>, strings: Res<Strings>) {
    for cmd in registry.commands.iter_mut() {
        cmd.label = xui::i18n_bridge::tr(&strings, &format!("cmd-{}", cmd.id.replace('.', "-")));
    }
}
