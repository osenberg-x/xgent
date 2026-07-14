//! 命令面板集成：注册 XGent 命令，渲染 overlay 面板，键盘导航。
//!
//! 命令面板 UI 为 overlay：居中顶部、宽 500px、半透明遮罩。
//! 键盘：↑↓ 导航、Enter 执行、Esc 关闭。
//! 面板打开/关闭由快捷键 `Cmd+Shift+P` 触发（见 [`crate::shortcuts`]）。

use bevy::input::keyboard::KeyCode;
use bevy::input::ButtonInput;
use bevy::input_focus::AutoFocus;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::ScrollPosition;

use xgent_settings::Localizer;
use xui::command_palette::{
    CommandKind, CommandPaletteState, CommandRegistry, PaletteCommand, PaletteTriggered,
    trigger_selected,
};
use xui::input::ChatInput;

use crate::i18n::tr;
use crate::theme::{Theme, space};

/// 命令面板 overlay 根节点标记。
#[derive(Component, Default)]
pub struct CommandPaletteOverlayMarker;

/// 命令面板输入框标记。
#[derive(Component, Default)]
pub struct PaletteInputMarker;

/// 命令列表容器标记。
#[derive(Component, Default)]
pub struct PaletteListMarker;

/// 单条命令项标记（携带在 `CommandRegistry` 中的索引）。
#[derive(Component, Default)]
pub struct PaletteItemMarker {
    pub index: usize,
}

/// 命令面板插件。
pub struct CommandPalettePlugin;

impl Plugin for CommandPalettePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_xgent_commands)
            .add_systems(
                Update,
                (
                    toggle_palette_visibility,
                    handle_palette_keyboard,
                    sync_input_to_query,
                    rebuild_list,
                    handle_palette_triggers,
                )
                    .chain()
                    .after(xgent_agent::agent_loop::agent_poll_system)
                    .after(xui::command_palette::filter_commands),
            );
    }
}

/// 启动时注册 XGent 命令。
pub fn register_xgent_commands(mut registry: ResMut<CommandRegistry>, loc: Res<Localizer>) {
    registry.register(PaletteCommand {
        id: "session.new".into(),
        label: tr(&loc, "cmd-session-new"),
        kind: CommandKind::Action,
    });
    registry.register(PaletteCommand {
        id: "lang.switch.en".into(),
        label: tr(&loc, "cmd-lang-en"),
        kind: CommandKind::Action,
    });
    registry.register(PaletteCommand {
        id: "lang.switch.zh".into(),
        label: tr(&loc, "cmd-lang-zh"),
        kind: CommandKind::Action,
    });
    registry.register(PaletteCommand {
        id: "settings.open".into(),
        label: tr(&loc, "cmd-settings-open"),
        kind: CommandKind::Action,
    });
}

/// 根据 CommandPaletteState.open 切换面板的 spawn/despawn。
fn toggle_palette_visibility(
    state: Res<CommandPaletteState>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    mut commands: Commands,
    q_overlay: Query<Entity, With<CommandPaletteOverlayMarker>>,
) {
    if state.is_changed() && !state.is_added() {
        // open 变化时 spawn/despawn
    }
    if state.open && q_overlay.is_empty() {
        spawn_palette_overlay(&mut commands, &theme, &loc);
    } else if !state.open {
        for entity in q_overlay.iter() {
            commands.entity(entity).despawn();
        }
    }
}

/// spawn 命令面板 overlay。
fn spawn_palette_overlay(commands: &mut Commands, theme: &Theme, _loc: &Localizer) {
    let font = theme.font_size;
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(0.0),
                left: px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                padding: UiRect::top(px(space::XL)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4)),
            CommandPaletteOverlayMarker,
        ))
        .with_children(|overlay| {
            // 面板容器
            overlay
                .spawn((
                    Node {
                        width: px(500.0),
                        max_height: px(400.0),
                        flex_direction: FlexDirection::Column,
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(8.0)),
                        overflow: Overflow::clip_y(),
                        ..default()
                    },
                    BackgroundColor(theme.panel),
                    BorderColor::all(theme.border),
                ))
                .with_children(|panel| {
                    // 输入框
                    panel.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            padding: UiRect::all(px(space::SM)),
                            ..default()
                        },
                        Text::new(String::new()),
                        TextFont {
                            font_size: FontSize::Px(font),
                            ..default()
                        },
                        TextColor(theme.text),
                        EditableText {
                            allow_newlines: false,
                            ..default()
                        },
                        ChatInput::single_line(),
                        AutoFocus,
                        PaletteInputMarker,
                    ));
                    // 命令列表
                    panel.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            overflow: Overflow::clip_y(),
                            ..default()
                        },
                        ScrollPosition::default(),
                        PaletteListMarker,
                    ));
                });
        });
}

fn handle_palette_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<CommandPaletteState>,
    registry: Res<CommandRegistry>,
    mut writer: MessageWriter<PaletteTriggered>,
) {
    if !state.open {
        return;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        state.select_next();
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        state.select_prev();
    }
    if keys.just_pressed(KeyCode::Enter) {
        trigger_selected(&state, &registry, &mut writer);
    }
    if keys.just_pressed(KeyCode::Escape) {
        state.close();
    }
}

/// 将输入框文本同步到 state.query。
fn sync_input_to_query(
    mut state: ResMut<CommandPaletteState>,
    q_input: Query<&Text, With<PaletteInputMarker>>,
) {
    let Ok(text) = q_input.single() else {
        return;
    };
    if text.0 != state.query {
        state.query = text.0.clone();
    }
}

/// 每帧根据 filtered 列表重建命令项。
fn rebuild_list(
    state: Res<CommandPaletteState>,
    registry: Res<CommandRegistry>,
    theme: Res<Theme>,
    q_list: Query<Entity, With<PaletteListMarker>>,
    q_items: Query<Entity, With<PaletteItemMarker>>,
    mut commands: Commands,
) {
    let Ok(list) = q_list.single() else {
        return;
    };
    // 清除旧项
    for item in q_items.iter() {
        commands.entity(item).despawn();
    }
    let font = theme.font_size;
    // 重建
    commands.entity(list).with_children(|p| {
        for &idx in state.filtered.iter().take(20) {
            let Some(cmd) = registry.commands.get(idx) else {
                continue;
            };
            let is_selected = idx == state.filtered.get(state.selected).copied().unwrap_or(usize::MAX);
            let bg = if is_selected {
                BackgroundColor(Color::srgba(0.36, 0.62, 0.92, 0.3))
            } else {
                BackgroundColor::default()
            };
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::horizontal(px(space::SM)),
                    ..default()
                },
                bg,
                Text::new(cmd.label.clone()),
                TextFont {
                    font_size: FontSize::Px(font),
                    ..default()
                },
                TextColor(if is_selected { theme.text } else { theme.text_dim }),
                PaletteItemMarker { index: idx },
            ));
        }
    });
}

/// 订阅 PaletteTriggered，据命令 id 执行业务。
pub(crate) fn handle_palette_triggers(
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
