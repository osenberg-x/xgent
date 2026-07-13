//! 对话面板：消息列表（流式渲染）+ 输入框 + 中断。
//!
//! 订阅 agent 的 [`DeltaMessage`] 累加到当前助手消息节点；[`DoneMessage`] 时把当前消息
//! 固化为历史消息节点并清空当前；输入框发送语义由 [`xui::ChatInput`] 处理，
//! 提交时转发为 [`UserInputMessage`]。
//!
//! 消息列表 MVP 用简单列容器 + 每条消息一个文本节点；大列表虚拟化留待后续接入 xui::VirtualList。

use bevy::input_focus::AutoFocus;
use bevy::prelude::*;
use bevy::text::EditableText;

use xgent_agent::{Conversation, DeltaMessage, DoneMessage, ErrorMessage, UserInputMessage};
use xui::input::{ChatInput, ChatInputSubmitted};

use crate::layout::ChatPanelMarker;
use crate::theme::{Theme, space};

/// 历史消息容器（消息列表）。
#[derive(Component, Default)]
pub struct MessageListMarker;

/// 当前正在流式累加的助手消息文本节点。
#[derive(Component, Default)]
pub struct CurrentAssistantText;

/// 对话输入框实体标记。
#[derive(Component, Default)]
pub struct ChatInputMarker;

/// 对话面板关键实体句柄（启动时填充）。
#[derive(Resource, Default)]
pub struct ChatPanelEntities {
    pub message_list: Option<Entity>,
    pub current_text: Option<Entity>,
}

/// 对话面板插件。
pub struct ChatPanelPlugin;

impl Plugin for ChatPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatPanelEntities>()
            .add_systems(Startup, spawn_chat_panel)
            .add_systems(
                Update,
                (
                    accumulate_delta,
                    finalize_on_done,
                    on_error,
                    forward_input_submission,
                ),
            );
    }
}

/// 启动时在对话侧栏内 spawn 消息列表 + 输入框。
fn spawn_chat_panel(
    mut commands: Commands,
    q_panel: Query<Entity, With<ChatPanelMarker>>,
    theme: Res<Theme>,
    mut entities: ResMut<ChatPanelEntities>,
) {
    let Ok(panel) = q_panel.single() else {
        return;
    };
    let font = theme.font_size;
    let font_size = FontSize::Px(font);
    let current_text = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(px(space::SM)),
                ..default()
            },
            BackgroundColor(theme.bubble_assistant),
            Text::new(String::new()),
            TextFont {
                font_size,
                ..default()
            },
            TextColor(theme.text),
            CurrentAssistantText,
        ))
        .id();

    let message_list = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(space::SM)),
                overflow: Overflow::clip_y(),
                row_gap: px(space::SM),
                ..default()
            },
            MessageListMarker,
        ))
        .add_child(current_text)
        .id();

    let input_entity = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_height: px(40.0),
                padding: UiRect::all(px(space::SM)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(theme.panel),
            BorderColor::all(theme.border),
            TextFont {
                font_size,
                ..default()
            },
            TextColor(theme.text_dim),
            EditableText {
                // 多行输入，placeholder 由初始空值 + text_dim 颜色呈现
                allow_newlines: true,
                ..default()
            },
            ChatInput::multiline(),
            AutoFocus,
            ChatInputMarker,
        ))
        .id();

    commands
        .entity(panel)
        .add_child(message_list)
        .add_child(input_entity);

    entities.message_list = Some(message_list);
    entities.current_text = Some(current_text);
}

/// 订阅 DeltaMessage，累加到当前助手消息节点。
fn accumulate_delta(
    mut reader: MessageReader<DeltaMessage>,
    mut q: Query<&mut Text, With<CurrentAssistantText>>,
) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    for ev in reader.read() {
        text.0.push_str(&ev.text);
    }
}

/// Done 时把当前助手消息固化为历史副本节点，并清空当前节点。
fn finalize_on_done(
    mut reader: MessageReader<DoneMessage>,
    entities: Res<ChatPanelEntities>,
    q: Query<&Text, With<CurrentAssistantText>>,
    mut commands: Commands,
    theme: Res<Theme>,
) {
    let Some(current) = entities.current_text else {
        return;
    };
    let Some(list) = entities.message_list else {
        return;
    };
    if reader.read().next().is_none() {
        return;
    }
    let Ok(text) = q.get(current) else {
        return;
    };
    let content = text.0.clone();
    if content.is_empty() {
        return;
    }
    let font = theme.font_size;
    // 在消息列表插入历史副本
    commands.entity(list).with_children(|p| {
        p.spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(px(space::SM)),
                ..default()
            },
            BackgroundColor(theme.bubble_assistant),
            Text::new(content),
            TextFont {
                font_size: FontSize::Px(font),
                ..default()
            },
            TextColor(theme.text),
        ));
    });
    // 清空当前节点
    commands.entity(current).insert(Text::new(String::new()));
}

/// 出错时把错误信息写到当前助手消息节点。
fn on_error(
    mut reader: MessageReader<ErrorMessage>,
    q: Query<Entity, With<CurrentAssistantText>>,
    mut commands: Commands,
    theme: Res<Theme>,
) {
    let Ok(entity) = q.single() else {
        return;
    };
    for ev in reader.read() {
        commands
            .entity(entity)
            .insert((Text::new(format!("⚠ {}", ev.0)), TextColor(theme.accent)));
    }
}

/// 订阅 xui 的 ChatInputSubmitted，转发为 agent 的 UserInputMessage。
pub fn forward_input_submission(
    mut reader: MessageReader<ChatInputSubmitted>,
    mut writer: MessageWriter<UserInputMessage>,
    _conv: Res<Conversation>,
) {
    for ev in reader.read() {
        writer.write(UserInputMessage {
            text: ev.text.clone(),
        });
    }
}
