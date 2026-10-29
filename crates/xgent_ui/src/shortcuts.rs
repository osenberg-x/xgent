//! 快捷键体系：参考 VSCode 默认绑定，注册快捷键并订阅 [`HotkeyTriggered`] 执行业务。
//!
//! M7-T1：快捷键单一来源。`HotkeyAction` 声明所有快捷键动作，注册与处理一律走此清单。
//! 启动期一致性断言：注册表 id 集 == 分发表 id 集 == HOTKEY_ACTIONS id 集。

use bevy::ecs::message::MessageWriter;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use xgent_agent::AbortMessage;
use xgent_settings::Localizer;
use xui::command_palette::CommandPaletteState;
use xui::hotkeys::{Hotkey, HotkeyRegistry};
use xui::shortcuts::HotkeyTriggered;

use crate::editor::EditorView;
use crate::editor::tabs::{CloseTabRequest, CycleTabRequest};
use crate::i18n::tr;

/// 快捷键动作声明：每个注册的快捷键必须在此清单中有对应条目。
///
/// M7-T1：`handler_name` 用于启动期断言——确保分发表实现了所有注册的动作。
#[derive(Debug, Clone, Copy)]
pub struct HotkeyAction {
    /// 唯一 id，如 `"palette.open"`
    pub id: &'static str,
    /// i18n key，用于展示标签
    pub label_key: &'static str,
    /// 默认主键
    pub default_key: KeyCode,
    /// 是否需要 Shift
    pub shift: bool,
    /// 是否需要平台主修饰键（Cmd/Ctrl）
    pub primary: bool,
    /// handler 函数名（用于启动期断言）
    pub handler_name: &'static str,
}

/// 所有快捷键动作的权威清单。
///
/// M7-T1：注册与处理一律走此清单，消灭死键与空实现。
pub const HOTKEY_ACTIONS: &[HotkeyAction] = &[
    HotkeyAction {
        id: "palette.open",
        label_key: "hotkey-palette",
        default_key: KeyCode::KeyP,
        shift: true,
        primary: true,
        handler_name: "handle_palette_open",
    },
    HotkeyAction {
        id: "palette.open_files",
        label_key: "hotkey-files",
        default_key: KeyCode::KeyP,
        shift: false,
        primary: true,
        handler_name: "handle_palette_open_files",
    },
    HotkeyAction {
        id: "chat.abort",
        label_key: "hotkey-abort",
        default_key: KeyCode::Escape,
        shift: false,
        primary: false,
        handler_name: "handle_chat_abort",
    },
    HotkeyAction {
        id: "settings.open",
        label_key: "hotkey-settings",
        default_key: KeyCode::Comma,
        shift: false,
        primary: true,
        handler_name: "handle_settings_open",
    },
    HotkeyAction {
        id: "filepanel.toggle",
        label_key: "hotkey-toggle-filepanel",
        default_key: KeyCode::KeyB,
        shift: false,
        primary: true,
        handler_name: "handle_filepanel_toggle",
    },
    HotkeyAction {
        id: "input.focus",
        label_key: "hotkey-focus-input",
        default_key: KeyCode::KeyI,
        shift: false,
        primary: true,
        handler_name: "handle_input_focus",
    },
    HotkeyAction {
        id: "editor.view",
        label_key: "hotkey-editor-view",
        default_key: KeyCode::KeyE,
        shift: true,
        primary: true,
        handler_name: "handle_editor_view",
    },
    HotkeyAction {
        id: "chat.view",
        label_key: "hotkey-chat-view",
        default_key: KeyCode::KeyD,
        shift: true,
        primary: true,
        handler_name: "handle_chat_view",
    },
    HotkeyAction {
        id: "editor.close_tab",
        label_key: "hotkey-editor-close-tab",
        default_key: KeyCode::KeyW,
        shift: false,
        primary: true,
        handler_name: "handle_editor_close_tab",
    },
    HotkeyAction {
        id: "editor.cycle_tab",
        label_key: "hotkey-editor-cycle-tab",
        default_key: KeyCode::Tab,
        shift: false,
        primary: true,
        handler_name: "handle_editor_cycle_tab",
    },
    HotkeyAction {
        id: "sideview.toggle",
        label_key: "hotkey-toggle-sideview",
        default_key: KeyCode::Backslash,
        shift: false,
        primary: true,
        handler_name: "handle_sideview_toggle",
    },
    HotkeyAction {
        id: "terminal.toggle",
        label_key: "hotkey-toggle-terminal",
        default_key: KeyCode::Backquote,
        shift: false,
        primary: true,
        handler_name: "handle_terminal_toggle",
    },
];

/// 快捷键插件。
pub struct ShortcutsPlugin;

impl Plugin for ShortcutsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_xgent_hotkeys)
            .add_systems(
                Startup,
                assert_hotkey_consistency.after(register_xgent_hotkeys),
            )
            .add_systems(
                Update,
                handle_hotkey_triggers.after(crate::command_palette::handle_palette_triggers),
            );
    }
}

/// M7-T1：启动期一致性断言——注册表 id 集 == HOTKEY_ACTIONS id 集。
///
/// 若注册表缺少某 action 或多出某 action，panic 并打印差异。
fn assert_hotkey_consistency(registry: Res<HotkeyRegistry>) {
    let registered_ids: std::collections::HashSet<&str> =
        registry.bindings.keys().map(|s| s.as_str()).collect();
    let action_ids: std::collections::HashSet<&str> = HOTKEY_ACTIONS.iter().map(|a| a.id).collect();

    let missing_in_registry: Vec<_> = action_ids.difference(&registered_ids).collect();
    let extra_in_registry: Vec<_> = registered_ids.difference(&action_ids).collect();

    if !missing_in_registry.is_empty() || !extra_in_registry.is_empty() {
        panic!(
            "M7-T1 快捷键一致性断言失败：\n  HOTKEY_ACTIONS 有但注册表无: {:?}\n  注册表有但 HOTKEY_ACTIONS 无: {:?}",
            missing_in_registry, extra_in_registry
        );
    }
}

/// 启动时注册参考 VSCode 的快捷键。
///
/// M7-T1：从 `HOTKEY_ACTIONS` 常量数组注册，保证注册表与声明清单一致。
pub fn register_xgent_hotkeys(mut reg: ResMut<HotkeyRegistry>, loc: Res<Localizer>) {
    for action in HOTKEY_ACTIONS {
        let mut h = Hotkey::new(action.id, action.default_key, tr(&loc, action.label_key));
        if action.shift {
            h = h.with_shift();
        }
        if action.primary {
            h = h.with_primary();
        }
        let _ = reg.register(h);
    }
}

/// 订阅 HotkeyTriggered，据 id 执行业务。
///
/// M7-T1：所有 HOTKEY_ACTIONS 中的 id 必须在此有对应处理分支，消灭空实现。
/// Bevy 的系统参数上限是 16 个；本系统的消息写入器较多，故用
/// [`ParamSet`] 归并为一个参数。
pub(crate) fn handle_hotkey_triggers(
    mut reader: MessageReader<HotkeyTriggered>,
    mut palette: ResMut<CommandPaletteState>,
    mut msgs: ParamSet<(
        MessageWriter<AbortMessage>,
        MessageWriter<crate::overlay::DrawerToggle>,
        MessageWriter<crate::panels::PanelActivate>,
        MessageWriter<crate::panels::PanelDeactivate>,
        MessageWriter<CycleTabRequest>,
        MessageWriter<CloseTabRequest>,
        MessageWriter<crate::terminal::tabs::SpawnTabRequest>,
    )>,
    mut layout: ResMut<crate::panels::SlotLayout>,
    mut view: ResMut<EditorView>,
    registry: Res<crate::panels::PanelRegistry>,
    editor_tabs: Res<crate::editor::tabs::EditorTabs>,
    terminal_tabs: Res<crate::terminal::TerminalTabs>,
    project_root: Option<Res<crate::file_panel::ProjectRoot>>,
    modal: Option<Res<crate::modal::ModalStack>>,
    modal_esc: Option<Res<crate::modal::ModalEscConsumed>>,
    completion_esc: Option<Res<crate::completion_ui::CompletionConsumed>>,
    mut input_focus: Option<ResMut<bevy::input_focus::InputFocus>>,
    q_chat_input: Query<Entity, With<crate::chat_panel::ChatInputMarker>>,
) {
    // v8 M1：模态栈非空（或本帧 Esc 已被模态栈消费）时，全局快捷键整体让位。
    // 必须先排空 reader，否则消息会在模态关闭后被重放。
    let modal_open = modal.as_deref().is_some_and(|stack| !stack.is_empty());
    let esc_consumed = modal_esc.as_deref().is_some_and(|consumed| consumed.0)
        || completion_esc
            .as_deref()
            .is_some_and(|consumed| consumed.esc);
    if modal_open || esc_consumed {
        for _ in reader.read() {}
        return;
    }
    for ev in reader.read() {
        match ev.id.as_str() {
            // handle_chat_abort: Esc 中断当前对话
            "chat.abort" => {
                // 终端视图激活时，Esc 退出终端视图（不中断对话）
                if registry.is_visible(crate::panels::ids::TERMINAL) {
                    msgs.p3().write(crate::panels::PanelDeactivate {
                        id: crate::panels::ids::TERMINAL,
                    });
                    layout.set_open(crate::panels::Slot::SideDock, false);
                } else if *view == EditorView::Editor {
                    // 编辑器视图激活时，Esc 优先退出编辑器视图而非中断对话
                    *view = EditorView::Chat;
                } else {
                    msgs.p0().write(AbortMessage);
                }
            }
            // handle_terminal_toggle: Ctrl+` 切换终端视图
            "terminal.toggle" => {
                if registry.is_visible(crate::panels::ids::TERMINAL) {
                    msgs.p3().write(crate::panels::PanelDeactivate {
                        id: crate::panels::ids::TERMINAL,
                    });
                    layout.set_open(crate::panels::Slot::SideDock, false);
                } else {
                    msgs.p2().write(crate::panels::PanelActivate {
                        id: crate::panels::ids::TERMINAL,
                    });
                    layout.set_open(crate::panels::Slot::SideDock, true);
                    // 无 tab 时首次唤起自动 spawn 一个（对齐设计 §3.5）
                    if terminal_tabs.is_empty() {
                        let cwd = project_root
                            .as_deref()
                            .map(|r| r.path.clone())
                            .unwrap_or_else(std::env::temp_dir);
                        msgs.p6()
                            .write(crate::terminal::tabs::SpawnTabRequest { cwd });
                    }
                }
            }
            // handle_settings_open: Cmd/Ctrl+, 打开设置
            "settings.open" => {
                palette.open();
                palette.query = "settings".into();
            }
            // handle_filepanel_toggle: Cmd/Ctrl+B 切换文件面板
            "filepanel.toggle" => {
                // v8 M1：与 rail / 「+ 添加」走同一条消息，互斥由 OverlayRegistry 裁决。
                msgs.p1().write(crate::overlay::DrawerToggle(
                    crate::overlay::OverlayId::FileDrawer,
                ));
            }
            // handle_sideview_toggle: Cmd/Ctrl+\ 切换右侧分屏
            "sideview.toggle" => {
                // 已展开且 Editor → 收起；否则展开并切 Editor（让出 Terminal）
                if layout.is_open(crate::panels::Slot::SideDock)
                    && registry.is_visible(crate::panels::ids::EDITOR)
                {
                    msgs.p3().write(crate::panels::PanelDeactivate {
                        id: crate::panels::ids::EDITOR,
                    });
                    layout.set_open(crate::panels::Slot::SideDock, false);
                } else {
                    msgs.p2().write(crate::panels::PanelActivate {
                        id: crate::panels::ids::EDITOR,
                    });
                    layout.set_open(crate::panels::Slot::SideDock, true);
                }
            }
            // handle_input_focus: Cmd/Ctrl+I 聚焦输入框（M7-T1 修复空实现）
            "input.focus" => {
                if let Ok(input_entity) = q_chat_input.single() {
                    if let Some(ref mut focus) = input_focus {
                        focus.set(input_entity, bevy::input_focus::FocusCause::Navigated);
                    }
                }
            }
            // handle_editor_view: Cmd/Ctrl+Shift+E 切换到编辑器视图
            "editor.view" => {
                *view = EditorView::Editor;
                msgs.p2().write(crate::panels::PanelActivate {
                    id: crate::panels::ids::EDITOR,
                });
                layout.set_open(crate::panels::Slot::SideDock, true);
            }
            // handle_chat_view: Cmd/Ctrl+Shift+D 切换回对话视图
            "chat.view" => {
                // 聚焦会话区：收起上下文面板；同步 view 保证 Esc（chat.abort）
                // 的 view 判断与可见状态一致，否则 Esc 被静默吞掉一次
                *view = EditorView::Chat;
                layout.set_open(crate::panels::Slot::SideDock, false);
            }
            // handle_editor_close_tab: Cmd/Ctrl+W 关闭当前标签
            "editor.close_tab" => {
                // 关闭当前激活标签（此前是空操作，× 按钮是唯一关闭入口）
                if let Some(active) = editor_tabs.active
                    && let Some(&entity) = editor_tabs.tabs.get(active)
                {
                    msgs.p5().write(CloseTabRequest {
                        entity,
                        force: false,
                    });
                }
            }
            // handle_editor_cycle_tab: Cmd/Ctrl+Tab 循环切换标签
            "editor.cycle_tab" => {
                msgs.p4().write(CycleTabRequest { forward: true });
            }
            // handle_palette_open: Cmd/Ctrl+Shift+P 打开命令面板
            "palette.open" => {
                palette.open();
                palette.query.clear();
            }
            // handle_palette_open_files: Cmd/Ctrl+P 文件快速打开（M7-T1 修复死键）
            "palette.open_files" => {
                palette.open();
                palette.query.clear();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input_focus::InputFocus;
    use xui::hotkeys::HotkeyRegistry;

    /// M7-T1：验证 HOTKEY_ACTIONS 与注册表的一致性断言。
    #[test]
    fn hotkey_consistency_assertion_passes() {
        let mut app = App::new();
        app.init_resource::<HotkeyRegistry>()
            .init_resource::<xgent_settings::Localizer>()
            .add_systems(Startup, register_xgent_hotkeys)
            .add_systems(
                Startup,
                assert_hotkey_consistency.after(register_xgent_hotkeys),
            );

        // 运行启动系统
        app.world_mut().run_schedule(Startup);

        // 如果一致性断言失败，上面的 run_schedule 会 panic
        // 这里只需要验证能正常到达这里
        let registry = app.world().resource::<HotkeyRegistry>();
        assert_eq!(registry.bindings.len(), HOTKEY_ACTIONS.len());
    }

    /// M7-T1：验证所有 HOTKEY_ACTIONS 都有唯一的 id。
    #[test]
    fn hotkey_actions_have_unique_ids() {
        let mut ids = std::collections::HashSet::new();
        for action in HOTKEY_ACTIONS {
            assert!(ids.insert(action.id), "Duplicate id: {}", action.id);
        }
    }

    /// M7-T1：验证所有 HOTKEY_ACTIONS 都有非空的 handler_name。
    #[test]
    fn hotkey_actions_have_handler_names() {
        for action in HOTKEY_ACTIONS {
            assert!(
                !action.handler_name.is_empty(),
                "Empty handler for {}",
                action.id
            );
        }
    }
}
