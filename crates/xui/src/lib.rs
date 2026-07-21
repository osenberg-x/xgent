//! `xui` — 面向 Bevy 的轻量通用 UI 组件库。
//!
//! 纯依赖 `bevy` + `xui_i18n`，**不依赖任何 `xgent_*` crate**，可独立发布被其他 Bevy 项目复用。
//!
//! 封装策略：官方已覆盖的基础 widget（button/checkbox/slider/dialog/menu/popover、text_input IME）
//! 直接用官方；`xui` 只补官方未覆盖或需增强的部分：虚拟列表、命令面板、输入增强、快捷键体系、i18n 桥接。
//!
//! 业务解耦：命令面板、快捷键只发触发事件，不执行业务逻辑，调用方订阅事件实现。

pub mod command_palette;
pub mod hotkeys;
pub mod i18n_bridge;
pub mod input;
pub mod mouse_wheel_scroll;
pub mod scroll_area;
pub mod scrollbar;
pub mod shortcuts;
pub mod text_editor;
pub mod virtual_list;
use bevy::prelude::*;
pub use command_palette::{
    CommandKind, CommandPalettePlugin, CommandPaletteState, CommandRegistry, PaletteCommand,
    PaletteTriggered,
};
pub use hotkeys::{Hotkey, HotkeyConflict, HotkeyRegistry, Mod, platform_primary_mod};
pub use i18n_bridge::{Strings, tr, tr_with};
pub use input::{ChatInput, ChatInputSubmitted, InputEnhancePlugin, SendModifier};
pub use scroll_area::{ScrollArea, ScrollAreaMarker, ScrollAreaPlugin, StickToBottom};
pub use scrollbar::{Scrollbar, ScrollbarPlugin, ScrollbarThumb, ScrollbarTrack};
pub use shortcuts::{HotkeyTriggered, ShortcutsPlugin};
pub use text_editor::{
    EditorDirtyChanged, EditorSaveRequested, HighlightCache, Language, Rope, TextEditor,
    TextEditorPlugin, TextEditorUpdateSet,
};
pub use text_editor::highlight::{highlight, span_color_for, HighlightSpan, SpanKind};
pub use virtual_list::{VirtualItemBuilder, VirtualList, VirtualListPlugin};

/// `xui` 插件：注册所有子组件插件与快捷键注册表。
///
/// `Strings`（i18n 桥接 Resource）由宿主应用注入，本插件不创建。
pub struct XuiPlugin;

impl Plugin for XuiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            VirtualListPlugin,
            CommandPalettePlugin,
            InputEnhancePlugin,
            ShortcutsPlugin,
            text_editor::TextEditorPlugin,
            mouse_wheel_scroll::MouseWheelScrollPlugin,
            scroll_area::ScrollAreaPlugin,
            scrollbar::ScrollbarPlugin,
        ))
        .init_resource::<HotkeyRegistry>();
    }
}
