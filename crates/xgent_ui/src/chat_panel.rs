//! 对话面板：消息列表（流式渲染）+ 输入框 + 中断。
//!
//! 订阅 agent 的 [`DeltaMessage`] 累加到当前助手消息节点；[`DoneMessage`] 时把当前消息
//! 固化为历史消息节点并清空当前；输入框发送语义由 [`xui::ChatInput`] 处理，
//! 提交时转发为 [`UserInputMessage`]。
//!
//! 用户消息右对齐（`bubble_user`），助手消息左对齐（`bubble_assistant`）。
//! 消息列表自动滚动到底部。
//!
//! 消息列表 MVP 用简单列容器 + 每条消息一个文本节点；大列表虚拟化留待后续接入 xui::VirtualList。

use bevy::input_focus::AutoFocus;
use bevy::prelude::*;
use bevy::text::EditableText;

use xgent_agent::{
    Conversation, ConversationStatus, DeltaMessage, DoneMessage, ErrorMessage, SteeringMessage,
    UserInputMessage,
};
use xui::input::{ChatInput, ChatInputSubmitted};
use xui::scroll_area::{ScrollArea, StickToBottom};

use crate::layout::ChatPanelMarker;
use crate::theme::{Theme, space};
/// 历史消息容器（消息列表，可滚动）。
#[derive(Component, Default)]
pub struct MessageListMarker;

/// 当前正在流式累加的助手消息文本节点。
#[derive(Component, Default)]
pub struct CurrentAssistantText;

/// 对话输入框实体标记。
#[derive(Component, Default)]
pub struct ChatInputMarker;

/// 输入框边框标记（用于忙时变色）。
#[derive(Component, Default)]
pub struct ChatInputBorderMarker;

/// 对话面板关键实体句柄（启动时填充）。
#[derive(Resource, Default)]
pub struct ChatPanelEntities {
    pub message_list: Option<Entity>,
    pub current_text: Option<Entity>,
    pub input: Option<Entity>,
}

/// 对话面板插件。
pub struct ChatPanelPlugin;

impl Plugin for ChatPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatPanelEntities>()
            .add_systems(Startup, spawn_chat_panel.after(crate::layout::spawn_layout))
            .add_systems(
                Update,
                (
                    accumulate_delta,
                    finalize_on_done,
                    on_error,
                    forward_input_submission,
                    spawn_user_message,
                    update_input_border,
                )
                    .after(xgent_agent::agent_loop::agent_poll_system),
            );
        // 贴底跟随由 `xui::scroll_area::StickToBottom` 组件 +
        // `ScrollAreaPlugin` 的 `maintain_stick_to_bottom`/`auto_scroll_to_bottom`
        // 系统通用驱动（PostLayout 后），本插件不再自管。
    }
}

/// 启动时在对话主区内 spawn 消息列表 + 输入框。
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
                max_width: Val::Percent(80.0),
                padding: UiRect::all(px(space::SM)),
                border_radius: BorderRadius::all(px(6.0)),
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

    // 消息列表：用通用 `ScrollArea` 提供防撑破 + 滚动契约，
    // `StickToBottom` 驱动流式累加期间的贴底跟随。padding/row_gap 为本列表
    // 业务样式，在 `ScrollArea::vertical()` 的 Node 上直接补设。
    let mut scroll_area = ScrollArea::vertical();
    scroll_area.node.padding = UiRect::all(px(space::SM));
    scroll_area.node.row_gap = px(space::SM);
    let message_list = commands
        .spawn((scroll_area, StickToBottom::default(), MessageListMarker))
        .add_child(current_text)
        .id();

    let input_entity = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_height: px(60.0),
                max_height: px(200.0),
                flex_shrink: 0.0,
                padding: UiRect::all(px(space::SM)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(theme.bg),
            BorderColor::all(theme.border),
            TextFont {
                font_size,
                ..default()
            },
            TextColor(theme.text_dim),
            bevy::text::TextCursorStyle::default(),
            EditableText {
                allow_newlines: true,
                ..default()
            },
            ChatInput::multiline(),
            AutoFocus,
            ChatInputMarker,
            ChatInputBorderMarker,
        ))
        .id();

    commands
        .entity(panel)
        .add_child(message_list)
        .add_child(input_entity);

    entities.message_list = Some(message_list);
    entities.current_text = Some(current_text);
    entities.input = Some(input_entity);
}

/// 用户提交输入时，在消息列表中 spawn 用户消息气泡（右对齐）。
fn spawn_user_message(
    mut reader: MessageReader<ChatInputSubmitted>,
    entities: Res<ChatPanelEntities>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Some(list) = entities.message_list else {
        return;
    };
    let Some(current) = entities.current_text else {
        return;
    };
    let font = theme.font_size;
    for ev in reader.read() {
        if ev.text.is_empty() {
            continue;
        }
        // 在当前助手节点之前插入用户消息气泡
        commands.entity(list).with_children(|p| {
            // 右对齐行容器
            p.spawn((Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            },))
                .with_children(|row| {
                    row.spawn((
                        Node {
                            max_width: Val::Percent(80.0),
                            padding: UiRect::all(px(space::SM)),
                            border_radius: BorderRadius::all(px(6.0)),
                            ..default()
                        },
                        BackgroundColor(theme.bubble_user),
                        Text::new(ev.text.clone()),
                        TextFont {
                            font_size: FontSize::Px(font),
                            ..default()
                        },
                        TextColor(theme.text),
                    ));
                });
        });
        // 把当前助手节点移到列表末尾（在用户消息之后）
        commands.entity(list).add_child(current);
        // 清空当前助手节点（清除上一轮的错误文本或残留内容）
        commands.entity(current).insert(Text::new(String::new()));
    }
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
    // 在消息列表插入历史副本（左对齐行容器 + 助手气泡）
    commands.entity(list).with_children(|p| {
        p.spawn((Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::FlexStart,
            ..default()
        },))
            .with_children(|row| {
                row.spawn((
                    Node {
                        max_width: Val::Percent(80.0),
                        padding: UiRect::all(px(space::SM)),
                        border_radius: BorderRadius::all(px(6.0)),
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
        let prefix = match ev.kind {
            xgent_core::chat::ErrorKind::NotConfigured => "⚠ [未配置] ",
            xgent_core::chat::ErrorKind::AuthFailed => "⚠ [鉴权失败] ",
            xgent_core::chat::ErrorKind::Network => "⚠ [网络] ",
            xgent_core::chat::ErrorKind::StreamParse => "⚠ [解析] ",
            xgent_core::chat::ErrorKind::ProviderError => "⚠ ",
        };
        commands.entity(entity).insert((
            Text::new(format!("{prefix}{}", ev.message)),
            TextColor(theme.accent),
        ));
    }
}

/// 订阅 xui 的 ChatInputSubmitted，转发为 agent 的 UserInputMessage 或 SteeringMessage。
///
/// Idle/Error 时发 UserInputMessage（新对话）；
/// Streaming/ToolRunning/Confirming/Thinking 时发 SteeringMessage（注入到当前对话，MVP 不中断工具）。
pub fn forward_input_submission(
    mut reader: MessageReader<ChatInputSubmitted>,
    mut user_writer: MessageWriter<UserInputMessage>,
    mut steering_writer: MessageWriter<SteeringMessage>,
    conv: Res<Conversation>,
) {
    for ev in reader.read() {
        if ev.text.is_empty() {
            continue;
        }
        if conv.status == ConversationStatus::Idle || conv.status == ConversationStatus::Error {
            user_writer.write(UserInputMessage {
                text: ev.text.clone(),
            });
        } else {
            // agent 执行中：发 Steering，注入到当前对话
            steering_writer.write(SteeringMessage {
                text: ev.text.clone(),
            });
        }
    }
}

/// 根据 Conversation 状态更新输入框边框颜色（忙时变色）。
fn update_input_border(
    conv: Res<Conversation>,
    theme: Res<Theme>,
    mut q: Query<&mut BorderColor, With<ChatInputBorderMarker>>,
) {
    let Ok(mut border) = q.single_mut() else {
        return;
    };
    let is_busy =
        conv.status != ConversationStatus::Idle && conv.status != ConversationStatus::Error;
    if is_busy {
        border.set_all(theme.accent);
    } else {
        border.set_all(theme.border);
    }
}
