//! 命令面板集成：注册 XGent 命令，渲染 overlay 面板，键盘导航。
//!
//! 命令面板 UI 为 overlay：居中顶部、宽 500px、半透明遮罩。
//! 键盘：↑↓ 导航、Enter 执行、Esc 关闭。
//! 面板打开/关闭由快捷键 `Cmd+Shift+P` 触发（见 [`crate::shortcuts`]）。

use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::AutoFocus;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::ScrollPosition;

use xgent_agent::NewSessionMessage;
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
                    handle_palette_click,
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

/// 仅在 `filtered` 内容变化时重建命令项 entity；`selected` 变化只更新视觉。
///
/// 关键：不能每帧无条件 despawn+重建，否则 `Interaction::Pressed` 来不及被
/// `handle_palette_click` 观察到就被新 entity 覆盖（新 entity 的 Interaction 为 None）。
/// 用 `Local` 缓存上次 filtered，内容相同时保持 entity 稳定。
fn rebuild_list(
    state: Res<CommandPaletteState>,
    registry: Res<CommandRegistry>,
    theme: Res<Theme>,
    q_list: Query<Entity, With<PaletteListMarker>>,
    q_items_entity: Query<Entity, With<PaletteItemMarker>>,
    mut q_items_visual: Query<
        (&PaletteItemMarker, &mut BackgroundColor, &mut TextColor),
        With<Button>,
    >,
    mut commands: Commands,
    mut last_filtered: Local<Vec<usize>>,
) {
    let Ok(list) = q_list.single() else {
        // overlay 未渲染（面板关闭）：清空缓存，下次打开时强制重建 item，
        // 避免 last_filtered 仍记旧内容导致 rebuild_list 跳过重建（面板空白）。
        last_filtered.clear();
        return;
    };

    // 仅当 filtered 内容变化时才重建 entity，保持 Interaction 组件稳定。
    // 若每帧无条件 despawn+重建，鼠标按下那帧的 Interaction::Pressed 会被
    // 新 entity（Interaction::None）覆盖，handle_palette_click 永远观察不到 Pressed。
    if *last_filtered != state.filtered {
        for entity in q_items_entity.iter() {
            commands.entity(entity).despawn();
        }
        last_filtered.clone_from(&state.filtered);
        let font = theme.font_size;
        commands.entity(list).with_children(|p| {
            for &idx in state.filtered.iter().take(20) {
                let Some(cmd) = registry.commands.get(idx) else {
                    continue;
                };
                p.spawn((
                    // 用 Button（自带 Interaction）使鼠标点击可被检测，
                    // 对齐 top_bar/file_panel 的点击处理模式。
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::horizontal(px(space::SM)),
                        ..default()
                    },
                    Interaction::default(),
                    BackgroundColor::default(),
                    Text::new(format!("{} {}", kind_icon(cmd.kind), cmd.label.clone())),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                    PaletteItemMarker { index: idx },
                ));
            }
        });
    }

    // 选中态视觉更新（不重建 entity，避免破坏 Interaction 状态）
    let selected_idx = state
        .filtered
        .get(state.selected)
        .copied()
        .unwrap_or(usize::MAX);
    for (marker, mut bg, mut color) in q_items_visual.iter_mut() {
        let is_selected = marker.index == selected_idx;
        bg.0 = if is_selected {
            Color::srgba(0.36, 0.62, 0.92, 0.3)
        } else {
            Color::NONE
        };
        color.0 = if is_selected {
            theme.text
        } else {
            theme.text_dim
        };
    }
}

/// 处理命令项鼠标点击：Pressed 时定位 filtered 中的位置、设 selected、触发命令。
///
/// 与键盘 Enter 走同一条 `trigger_selected` 路径，保持行为一致。
fn handle_palette_click(
    q_items: Query<(&Interaction, &PaletteItemMarker), Changed<Interaction>>,
    mut state: ResMut<CommandPaletteState>,
    registry: Res<CommandRegistry>,
    mut writer: MessageWriter<PaletteTriggered>,
) {
    if !state.open {
        return;
    }
    for (interaction, marker) in q_items.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        // 把命令在 registry 中的索引转成 filtered 列表里的位置
        if let Some(pos) = state.filtered.iter().position(|&i| i == marker.index) {
            state.selected = pos;
            trigger_selected(&state, &registry, &mut writer);
        }
    }
}

/// 订阅 PaletteTriggered，据命令 id 执行业务。
pub(crate) fn handle_palette_triggers(
    mut reader: MessageReader<PaletteTriggered>,
    mut state: ResMut<CommandPaletteState>,
    mut loc: ResMut<Localizer>,
    mut settings_state: ResMut<crate::settings_panel::SettingsPanelState>,
    mut new_session: MessageWriter<NewSessionMessage>,
) {
    for ev in reader.read() {
        match ev.command_id.as_str() {
            "lang.switch.en" => loc.switch("en-US"),
            "lang.switch.zh" => loc.switch("zh-CN"),
            "session.new" => {
                // 新建会话：发 NewSessionMessage，agent_poll_system 处理 reset
                new_session.write(NewSessionMessage);
            }
            "settings.open" => {
                settings_state.open = true;
            }
            _ => {}
        }
        state.close();
    }
}
/// 据 `CommandKind` 返回 emoji 图标。
fn kind_icon(kind: xui::command_palette::CommandKind) -> &'static str {
    use xui::command_palette::CommandKind;
    match kind {
        CommandKind::File => "📁",
        CommandKind::Action => "⚙",
    }
}
