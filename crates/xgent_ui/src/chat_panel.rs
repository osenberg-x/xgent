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

use bevy::color::palettes::css;
use bevy::input_focus::AutoFocus;
use bevy::prelude::*;
use bevy::text::EditableText;

use xgent_agent::{
    Conversation, ConversationStatus, DeltaMessage, DoneMessage, ErrorMessage,
    SessionClearedMessage, SteeringMessage, UserInputMessage,
};
use xui::input::{ChatInput, ChatInputSubmitted};
use xui::scroll_area::{ScrollArea, StickToBottom};

use crate::layout::ChatPanelMarker;
use crate::status_bar::TokenUsage;
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

/// 输入框忙时标记（空输入发送时插入，红边闪烁 0.4s 后移除）。
#[derive(Component)]
pub struct InputBusyMarker {
    /// 插入时的 elapsed 秒数
    pub started_at: f64,
}
#[derive(Component, Default)]
pub struct ConversationInfoMarker;

/// 输入框右侧状态文本节点标记（tokenhint，显示就绪/思考中等）。
#[derive(Component, Default)]
pub struct TokenHintMarker;

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
                    update_streaming_cursor,
                    update_conversation_info,
                    clear_on_new_session,
                    update_token_hint,
                )
                    .after(xgent_agent::agent_loop::agent_poll_system),
            );
        // 贴底跟随由 `xui::scroll_area::StickToBottom` 组件 +
        // `ScrollAreaPlugin` 的 `maintain_stick_to_bottom`/`auto_scroll_to_bottom`
        // 系统通用驱动（PostLayout 后），本插件不再自管。
    }
}

/// 启动时在对话主区内 spawn 视图标签条 + 消息列表 + 输入框（含快捷键提示栏）。
fn spawn_chat_panel(
    mut commands: Commands,
    q_panel: Query<Entity, With<ChatPanelMarker>>,
    theme: Res<Theme>,
    loc: Res<xgent_settings::Localizer>,
    mut entities: ResMut<ChatPanelEntities>,
) {
    let Ok(panel) = q_panel.single() else {
        return;
    };
    let font = theme.font_size;
    let font_size = FontSize::Px(font);

    // 视图标签条：💬 对话 + 右侧会话信息
    let viewtabs = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: px(32.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(space::XS),
                padding: UiRect::horizontal(px(space::LG)),
                border: UiRect::bottom(px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme.bar),
            BorderColor::all(theme.border),
        ))
        .with_children(|tabs| {
            // 对话标签（💬 对话，active 态：底部 accent 边框）
            tabs.spawn((
                Node {
                    padding: UiRect::all(px(space::SM)),
                    border: UiRect::bottom(px(2.0)),
                    ..default()
                },
                BorderColor::all(theme.accent),
                Text::new(crate::i18n::tr(&loc, "chat-tab-label").to_string()),
                TextFont {
                    font_size,
                    ..default()
                },
                TextColor(theme.text),
            ));
            // spacer
            tabs.spawn((Node {
                flex_grow: 1.0,
                ..default()
            },));
            // 会话信息（右侧，小字 dim，由 update_conversation_info 系统填充）
            tabs.spawn((
                Text::new(String::new()),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme.text_dim),
                ConversationInfoMarker,
            ));
        })
        .id();

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

    // 消息列表
    let mut scroll_area = ScrollArea::vertical();
    scroll_area.node.padding = UiRect::all(px(space::LG));
    scroll_area.node.row_gap = px(space::LG);
    let message_list = commands
        .spawn((scroll_area, StickToBottom::default(), MessageListMarker))
        .add_child(current_text)
        .id();

    // 输入框容器（inputbar）：input-wrap + input-meta
    let input_entity = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_height: px(60.0),
                max_height: px(200.0),
                flex_shrink: 0.0,
                padding: UiRect::all(px(space::SM)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(6.0)),
                ..default()
            },
            BackgroundColor(theme.panel),
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

    // 快捷键提示栏（input-meta）：左侧 hint，右侧 tokenhint
    let input_meta = commands
        .spawn((Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            margin: UiRect::top(px(space::SM)),
            ..default()
        },))
        .with_children(|meta| {
            // 左侧快捷键提示
            meta.spawn((Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(space::LG),
                align_items: AlignItems::Center,
                ..default()
            },))
                .with_children(|hint| {
                    hint.spawn((
                        Text::new(crate::i18n::tr(&loc, "hint-send")),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme.text_dim),
                    ));
                    hint.spawn((
                        Text::new(crate::i18n::tr(&loc, "hint-abort")),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme.text_dim),
                    ));
                    hint.spawn((
                        Text::new(crate::i18n::tr(&loc, "hint-palette")),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme.text_dim),
                    ));
                    hint.spawn((
                        Text::new(crate::i18n::tr(&loc, "hint-toggle-sideview")),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme.text_dim),
                    ));
                });
            // 右侧 tokenhint（状态文本）
            meta.spawn((
                Text::new(crate::i18n::tr(&loc, "status-ready")),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme.text_dim),
                TokenHintMarker,
            ));
        })
        .id();

    // inputbar 容器：input + input-meta
    let inputbar = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::horizontal(px(space::LG)),
                border: UiRect::top(px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme.bg),
            BorderColor::all(theme.border),
        ))
        .add_child(input_entity)
        .add_child(input_meta)
        .id();

    commands
        .entity(panel)
        .add_child(viewtabs)
        .add_child(message_list)
        .add_child(inputbar);

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
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                        BackgroundColor(theme.bubble_user),
                    ))
                    .with_children(|bubble| {
                        // role 行：头像（你）+ 角色名
                        bubble
                            .spawn((Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: px(6.0),
                                margin: UiRect::bottom(px(space::XS)),
                                ..default()
                            },))
                            .with_children(|role| {
                                // 头像（蓝底圆「你」）
                                role.spawn((
                                    Node {
                                        width: px(18.0),
                                        height: px(18.0),
                                        border_radius: BorderRadius::all(px(9.0)),
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::Center,
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(0.23, 0.35, 0.55, 1.0)),
                                    Text::new("你"),
                                    TextFont {
                                        font_size: FontSize::Px(10.0),
                                        ..default()
                                    },
                                    TextColor(css::WHITE.into()),
                                ));
                                // 角色名
                                role.spawn((
                                    Text::new(crate::i18n::tr(
                                        &xgent_settings::Localizer::default(),
                                        "role-user",
                                    )),
                                    TextFont {
                                        font_size: FontSize::Px(11.0),
                                        ..default()
                                    },
                                    TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                                ));
                            });
                        // 正文
                        bubble.spawn((
                            Text::new(ev.text.clone()),
                            TextFont {
                                font_size: FontSize::Px(font),
                                ..default()
                            },
                            TextColor(theme.text),
                        ));
                    });
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
        // 剥离流式光标（▋）再追加新 delta，避免光标被推到中间
        if text.0.ends_with('▋') {
            text.0.pop();
        }
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
    // 剥离流式光标（▋）—— 光标挂在 CurrentAssistantText 末尾，固化历史副本时去掉
    let content = text.0.trim_end_matches('▋').to_string();
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
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    BackgroundColor(theme.bubble_assistant),
                ))
                .with_children(|bubble| {
                    // role 行：头像（✦）+ 角色名
                    bubble
                        .spawn((Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: px(6.0),
                            margin: UiRect::bottom(px(space::XS)),
                            ..default()
                        },))
                        .with_children(|role| {
                            role.spawn((
                                Node {
                                    width: px(18.0),
                                    height: px(18.0),
                                    border_radius: BorderRadius::all(px(9.0)),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.42, 0.44, 0.72, 1.0)),
                                Text::new("✦"),
                                TextFont {
                                    font_size: FontSize::Px(10.0),
                                    ..default()
                                },
                                TextColor(css::WHITE.into()),
                            ));
                            role.spawn((
                                Text::new(crate::i18n::tr(
                                    &xgent_settings::Localizer::default(),
                                    "role-assistant",
                                )),
                                TextFont {
                                    font_size: FontSize::Px(11.0),
                                    ..default()
                                },
                                TextColor(theme.text_dim),
                            ));
                        });
                    // 正文
                    bubble.spawn((
                        Text::new(content),
                        TextFont {
                            font_size: FontSize::Px(font),
                            ..default()
                        },
                        TextColor(theme.text),
                    ));
                });
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
            Text::new(format!("{prefix}{}\n\n（重新输入可继续对话）", ev.message)),
            TextColor(theme.accent),
        ));
    }
}

pub fn forward_input_submission(
    mut reader: MessageReader<ChatInputSubmitted>,
    mut user_writer: MessageWriter<UserInputMessage>,
    mut steering_writer: MessageWriter<SteeringMessage>,
    entities: Res<ChatPanelEntities>,
    conv: Res<Conversation>,
    time: Res<Time>,
    mut commands: Commands,
) {
    for ev in reader.read() {
        if ev.text.is_empty() {
            // 空输入：插入 InputBusyMarker 触发红边闪烁
            if let Some(input) = entities.input {
                commands.entity(input).insert(InputBusyMarker {
                    started_at: time.elapsed().as_secs_f64(),
                });
            }
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
/// 更新输入框边框颜色：忙时 accent；空输入发送时红边闪烁 0.4s（InputBusyMarker）后移除。
fn update_input_border(
    conv: Res<Conversation>,
    time: Res<Time>,
    theme: Res<Theme>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut BorderColor, Option<&InputBusyMarker>), With<ChatInputBorderMarker>>,
) {
    let Ok((entity, mut border, busy)) = q.single_mut() else {
        return;
    };
    let now = time.elapsed().as_secs_f64();
    if let Some(b) = busy {
        let elapsed = now - b.started_at;
        if elapsed >= 0.4 {
            // 0.4s 后移除 marker
            commands.entity(entity).remove::<InputBusyMarker>();
            border.set_all(theme.border);
        } else {
            // 红边 / 默认 交替（每 0.1s 切换）
            let phase = ((elapsed * 10.0) as usize) % 2;
            border.set_all(if phase == 0 {
                theme.st_fail
            } else {
                theme.border
            });
        }
        return;
    }
    let is_busy =
        conv.status != ConversationStatus::Idle && conv.status != ConversationStatus::Error;
    if is_busy {
        border.set_all(theme.accent);
    } else {
        border.set_all(theme.border);
    }
}
/// 流式光标：会话进行中（Thinking/Streaming/ToolRunning）时，在当前助手
/// 消息文本末尾闪烁 `▋` 字符表示正在生成；空闲时移除光标。
///
/// 对齐 ui-prototype.html `.cursor` —— 光标位于正在生成的助手气泡正文末尾，
/// 而非会话元信息。
/// 闪烁频率 1Hz（500ms 显 / 500ms 隐），用 `Time` 累计秒数取奇偶判定。
fn update_streaming_cursor(
    conv: Res<Conversation>,
    time: Res<Time>,
    mut q: Query<&mut Text, With<CurrentAssistantText>>,
) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let is_busy =
        conv.status != ConversationStatus::Idle && conv.status != ConversationStatus::Error;
    if !is_busy {
        // 空闲：确保无光标（文本末尾无 ▋）
        if text.0.ends_with('▋') {
            text.0.pop();
        }
        return;
    }
    // 忙：每 500ms toggle 末尾 ▋
    let show = (time.elapsed().as_secs_f64() % 1.0) < 0.5;
    let has_cursor = text.0.ends_with('▋');
    if show && !has_cursor {
        text.0.push('▋');
    } else if !show && has_cursor {
        text.0.pop();
    }
}
/// 更新会话信息文本：`会话 #{id} · {N} 轮 · ↑{tokens} tokens`。
///
/// 流式光标（▋）现挂在 `CurrentAssistantText` 末尾（见 `update_streaming_cursor`），
/// 本系统只设会话信息基础文本，不再与光标竞争。
fn update_conversation_info(
    conv: Res<Conversation>,
    tokens: Res<TokenUsage>,
    mut q: Query<&mut Text, With<ConversationInfoMarker>>,
) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let turns = conv
        .messages
        .iter()
        .filter(|m| matches!(m, xgent_core::chat::AgentMessage::User(_)))
        .count();
    let token_part = if tokens.total > 0 {
        format!(
            " · ↑ {} tokens",
            crate::status_bar::format_tokens(tokens.total)
        )
    } else {
        String::new()
    };
    let new_text = format!("会话 #{} · {} 轮{}", conv.id, turns, token_part);
    if text.0 != new_text {
        text.0 = new_text;
    }
}

/// 收到 SessionClearedMessage 时清空消息列表的所有子节点。
///
/// 新建会话后 Conversation 已 reset，UI 消息列表需同步清空。
/// 同时清空当前助手文本节点（防止残留）。
fn clear_on_new_session(
    mut reader: MessageReader<SessionClearedMessage>,
    entities: Res<ChatPanelEntities>,
    mut commands: Commands,
) {
    if reader.read().next().is_none() {
        return;
    }
    // 清空消息列表子节点（历史气泡）
    if let Some(list) = entities.message_list {
        commands.entity(list).despawn_related::<Children>();
    }
    // 清空当前助手文本节点
    if let Some(cur) = entities.current_text {
        commands.entity(cur).insert(Text::new(String::new()));
    }
}

/// 更新输入框右侧 tokenhint 文本：据会话状态显示就绪/思考中/生成中/中断中…等。
///
/// 对齐 ui-prototype.html `setStatus` 的 tokenhint 映射（行 562）。
fn update_token_hint(
    conv: Res<Conversation>,
    loc: Res<xgent_settings::Localizer>,
    mut q: Query<&mut Text, With<TokenHintMarker>>,
) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let label = match conv.status {
        ConversationStatus::Idle => crate::i18n::tr(&loc, "status-ready"),
        ConversationStatus::Thinking => crate::i18n::tr(&loc, "status-thinking"),
        ConversationStatus::Streaming => crate::i18n::tr(&loc, "status-streaming"),
        ConversationStatus::ToolRunning => crate::i18n::tr(&loc, "status-tool-running"),
        ConversationStatus::Confirming => crate::i18n::tr(&loc, "status-confirming"),
        ConversationStatus::Aborting => crate::i18n::tr(&loc, "status-aborting"),
        ConversationStatus::Error => crate::i18n::tr(&loc, "status-error"),
    };
    if text.0 != label {
        text.0 = label;
    }
}
