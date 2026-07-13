//! 设置面板：provider 配置、语言切换、主题（MVP 仅占位骨架）。
//!
//! MVP 不做完整设置 UI；语言切换已通过命令面板的 `lang.switch.*` 命令实现。
//! provider 配置写操作经 daemon config.write，留待后续接入 IPC client 后完善。

use bevy::prelude::*;

/// 设置面板插件（MVP 空实现，保留扩展点）。
pub struct SettingsPanelPlugin;

impl Plugin for SettingsPanelPlugin {
    fn build(&self, _app: &mut App) {
        // TODO: spawn 设置面板节点，绑定 provider/语言/主题配置
    }
}
