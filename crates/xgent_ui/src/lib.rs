//! xgent_ui — XGent 业务 UI 层。
//!
//! Bevy 全栈实现（bevy_ui + 渲染）。组装布局、对话/工具/文件面板、状态栏、
//! 命令面板、设置面板、确认弹窗、快捷键体系。订阅 agent 事件渲染，发送用户输入驱动 agent。
//!
//! 通过 [`xui`] 获取通用组件（虚拟列表、命令面板、输入增强、快捷键、i18n 桥接）。
//! 通过 [`xgent_agent`] 的事件契约与 agent 交互（禁止直接调用 agent 方法）。

pub mod chat_panel;
pub mod command_palette;
pub mod confirm_dialog;
pub mod file_panel;
pub mod i18n;
pub mod layout;
pub mod settings_panel;
pub mod shortcuts;
pub mod status_bar;
pub mod theme;

use bevy::prelude::*;

/// XGent UI 插件：组装各子插件与全局 UI 状态。
///
/// 依赖 [`xui::XuiPlugin`] 与 [`xgent_agent::XgentAgentPlugin`]，需由调用方
/// （通常是 `xgent_app`）一并添加，并注入 `Localizer`、`AgentBridge` 等资源。
pub struct XgentUiPlugin;

impl Plugin for XgentUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            layout::LayoutPlugin,
            chat_panel::ChatPanelPlugin,
            file_panel::FilePanelPlugin,
            status_bar::StatusBarPlugin,
            command_palette::CommandPalettePlugin,
            settings_panel::SettingsPanelPlugin,
            confirm_dialog::ConfirmDialogPlugin,
            shortcuts::ShortcutsPlugin,
        ));
    }
}
