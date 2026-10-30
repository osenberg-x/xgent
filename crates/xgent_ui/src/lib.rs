//! xgent_ui — XGent 业务 UI 层。
//!
//! Bevy 全栈实现（bevy_ui + 渲染）。组装布局、对话/工具/文件面板、状态栏、
//! 命令面板、设置面板、确认弹窗、快捷键体系。订阅 agent 事件渲染，发送用户输入驱动 agent。
//!
//! 通过 [`xui`] 获取通用组件（虚拟列表、命令面板、输入增强、快捷键、i18n 桥接）。
//! 通过 [`xgent_agent`] 的事件契约与 agent 交互（禁止直接调用 agent 方法）。

// Bevy 系统签名固有的大参数量与复杂 Query/ParamSet 类型，Bevy 生态惯例整体豁免。
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod activity_bar;
pub mod chat_panel;
pub mod command_palette;
pub mod completion;
pub mod completion_ui;
pub mod confirm_dialog;
pub mod context_scope;
pub mod context_set;
pub mod cursor;
pub mod density;
pub mod diff;
pub mod editor;
pub mod file_panel;
pub mod fonts;
pub mod i18n;
pub mod kit;
pub mod layout;
pub mod layout_mode;
pub mod markdown;
pub mod markdown_render;
pub mod modal;
pub mod overlay;
pub mod panels;
pub mod picking;
pub mod resize;
pub mod session_history;
pub mod settings_panel;
pub mod shortcuts;
pub mod space;
pub mod status_bar;
pub mod terminal;
pub mod theme;
pub mod tool_panel;
pub mod top_bar;
pub mod transcript;
pub mod transcript_render;
pub mod transcript_write;
pub mod undo;
pub mod welcome;

use bevy::prelude::*;

/// XGent UI 插件：组装各子插件与全局 UI 状态。
///
/// 依赖 [`xui::XuiPlugin`] 与 [`xgent_agent::XgentAgentPlugin`]，需由调用方
/// （通常是 `xgent_app`）一并添加，并注入 `Localizer`、`AgentBridge` 等资源。
pub struct XgentUiPlugin;

impl Plugin for XgentUiPlugin {
    fn build(&self, app: &mut App) {
        // Bevy 0.20 的 add_plugins 不幂等；多个业务插件会按需自加 PanelsPlugin，
        // 这里先统一兜底，避免后面的元组再次无条件添加导致 panic（M5 勘误 36）。
        if !app.is_plugin_added::<panels::PanelsPlugin>() {
            app.add_plugins(panels::PanelsPlugin);
        }
        if !app.is_plugin_added::<panels::visibility::VisibilityPlugin>() {
            app.add_plugins(panels::visibility::VisibilityPlugin);
        }
        if !app.is_plugin_added::<panels::tabs::TabsPlugin>() {
            app.add_plugins(panels::tabs::TabsPlugin);
        }
        app.add_plugins((
            theme::ThemePlugin,
            density::DensityPlugin,
            layout::LayoutPlugin,
            modal::ModalPlugin,
            overlay::OverlayPlugin,
            picking::PickablePlugin,
            activity_bar::ActivityBarPlugin,
            top_bar::TopBarPlugin,
            chat_panel::ChatPanelPlugin,
            context_scope::ContextScopePlugin,
            welcome::WelcomePlugin,
            file_panel::FilePanelPlugin,
            status_bar::StatusBarPlugin,
            command_palette::CommandPalettePlugin,
            settings_panel::SettingsPanelPlugin,
        ))
        .add_plugins((
            cursor::CursorStylePlugin,
            session_history::SessionHistoryPlugin,
            space::SpacePlugin,
            confirm_dialog::ConfirmDialogPlugin,
            tool_panel::ToolPanelPlugin,
            editor::EditorPlugin,
            terminal::TerminalPlugin,
            resize::ResizePlugin,
            shortcuts::ShortcutsPlugin,
            kit::KitPlugin,
            undo::UndoPlugin,
            transcript_write::TranscriptWritePlugin,
            transcript_render::TranscriptRenderPlugin,
        ))
        .add_plugins((
            completion_ui::CompletionUiPlugin,
            markdown_render::MarkdownRenderPlugin,
        ));
    }
}
