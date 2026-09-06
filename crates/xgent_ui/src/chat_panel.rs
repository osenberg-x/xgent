//! 对话面板：消息列表（全宽行式布局）+ 浮动输入卡 + 中断。
//!
//! v2 重构：消息不再使用气泡，改为"全宽行 + 头像 + 消息体"布局（参考 Cursor/ChatGPT），
//! 代码块成为对话流中的一级元素。输入区改为浮动卡片风格，带工具栏。
//!
//! 订阅 agent 的 [`DeltaMessage`] 累加到当前助手消息节点；[`DoneMessage`] 时把当前消息
//! 固化为历史消息节点并清空当前。

use bevy::clipboard::Clipboard;
use bevy::input_focus::{AutoFocus, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, LineHeight};

use xgent_agent::{
    CompactedMessage, Conversation, ConversationStatus, DeltaMessage, DoneMessage, ErrorMessage,
    RetryMessage, SessionClearedMessage, SteeringMessage, UserInputMessage,
};
use xui::input::{ChatInput, ChatInputSubmitted};
use xui::scroll_area::{ScrollArea, StickToBottom};

use crate::fonts::ui_text;
use crate::i18n::tr;
use crate::kit::{HoverTint, icon};
use crate::layout::ChatPanelMarker;
use crate::status_bar::TokenUsage;
use crate::theme::{Theme, radius, space, type_scale};
/// 历史消息容器（消息列表，可滚动）。
#[derive(Component, Default)]
pub struct MessageListMarker;

/// 当前正在流式累加的助手消息文本节点。
#[derive(Component, Default)]
pub struct CurrentAssistantText;

/// 回到底部浮钮标记。
#[derive(Component, Default)]
pub struct BackToBottomMarker;

/// agent 历史消息「复制」按钮标记（携带全文，点击写剪贴板）。
#[derive(Component, Default)]
pub struct CopyActionMarker {
    pub text: String,
}

/// qa 快捷 chips（点击填入输入框；key 为 i18n prompt 键）。
#[derive(Component)]
pub struct QaChipMarker {
    pub key: &'static str,
}

/// qa 快捷动作表（标签键, 提示词键）。
const QA_ACTIONS: &[(&str, &str)] = &[
    ("qa-explain", "qa-explain-prompt"),
    ("qa-refactor", "qa-refactor-prompt"),
    ("qa-test", "qa-test-prompt"),
    ("qa-fix", "qa-fix-prompt"),
    ("qa-review", "qa-review-prompt"),
];

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
    /// 回到底部浮钮（M4-T3）。
    pub back_to_bottom: Option<Entity>,
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
                    update_input_focus_ring,
                    update_back_to_bottom,
                    handle_qa_chips,
                    update_msg_actions,
                ),
            )
            .add_systems(
                Update,
                (
                    accumulate_delta,
                    finalize_on_done,
                    on_error,
                    show_compacted_notice,
                    forward_input_submission,
                    spawn_user_message,
                    update_input_border,
                    update_streaming_cursor,
                    update_conversation_info,
                    clear_on_new_session,
                    update_token_hint,
                )
                    .after(xgent_agent::agent_loop::agent_poll_system),
            )
            .add_systems(
                Update,
                show_retry_status
                    .after(xgent_agent::agent_loop::agent_poll_system)
                    .after(finalize_on_done),
            );
    }
}

/// 启动时在对话主区内 spawn 视图标签条 + 消息列表 + 浮动输入卡。
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

    // 视图标签条：对话/编辑器/文件预览 + 右侧会话信息
    let viewtabs = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: px(crate::theme::size::VIEW_TABS_H),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(space::XS),
                padding: UiRect::horizontal(px(space::LG)),
                border: UiRect::bottom(px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme.surface),
            BorderColor::all(theme.line),
        ))
        .with_children(|tabs| {
            // 对话标签（active 态：elevated 底 + 边框）
            tabs.spawn((
                Node {
                    padding: UiRect::all(px(space::XS)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(6.0)),
                    ..default()
                },
                BackgroundColor(theme.elevated),
                BorderColor::all(theme.border),
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
            // 会话信息
            tabs.spawn((
                Text::new(String::new()),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme.text_muted),
                ConversationInfoMarker,
            ));
        })
        .id();

    // 当前正在流式的助手消息节点（限宽 820 居中；v7 正文 text_dim/1.6）
    let current_text = commands
        .spawn((
            Node {
                max_width: px(820.0),
                align_self: AlignSelf::Center,
                flex_direction: FlexDirection::Row,
                column_gap: px(space::MD),
                ..default()
            },
            Text::new(String::new()),
            TextFont {
                font_size: FontSize::Px(type_scale::BODY),
                ..default()
            },
            TextColor(theme.text_dim),
            LineHeight::RelativeToFont(type_scale::line_height::BODY),
            CurrentAssistantText,
        ))
        .id();

    // 消息列表（居中 max-width 容器 + 可滚动）
    let mut scroll_area = ScrollArea::vertical();
    scroll_area.node.padding = UiRect::all(px(space::XXL));
    scroll_area.node.row_gap = px(space::XL);
    let message_list = commands
        .spawn((scroll_area, StickToBottom::default(), MessageListMarker))
        .add_child(current_text)
        .id();

    // 浮动输入卡
    let input_entity = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_height: px(38.0),
                max_height: px(200.0),
                flex_shrink: 0.0,
                padding: UiRect::all(px(space::SM)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(radius::PANEL)),
                ..default()
            },
            BackgroundColor(theme.input_bg),
            BorderColor::all(theme.border),
            TextFont {
                font_size,
                ..default()
            },
            TextColor(theme.text),
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

    // 工具栏（快捷键提示 + 发送按钮区）
    let toolbar = commands
        .spawn((Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            margin: UiRect::top(px(space::SM)),
            padding: UiRect::top(px(space::XS)),
            border: UiRect::top(px(1.0)),
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
                            font_size: FontSize::Px(10.5),
                            ..default()
                        },
                        TextColor(theme.text_muted),
                    ));
                    hint.spawn((
                        Text::new(crate::i18n::tr(&loc, "hint-abort")),
                        TextFont {
                            font_size: FontSize::Px(10.5),
                            ..default()
                        },
                        TextColor(theme.text_muted),
                    ));
                    hint.spawn((
                        Text::new(crate::i18n::tr(&loc, "hint-palette")),
                        TextFont {
                            font_size: FontSize::Px(10.5),
                            ..default()
                        },
                        TextColor(theme.text_muted),
                    ));
                    hint.spawn((
                        Text::new(crate::i18n::tr(&loc, "hint-toggle-sideview")),
                        TextFont {
                            font_size: FontSize::Px(10.5),
                            ..default()
                        },
                        TextColor(theme.text_muted),
                    ));
                    hint.spawn((
                        Text::new(crate::i18n::tr(&loc, "hint-terminal")),
                        TextFont {
                            font_size: FontSize::Px(10.5),
                            ..default()
                        },
                        TextColor(theme.text_muted),
                    ));
                });
            // 右侧 tokenhint + 发送按钮
            meta.spawn((Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(space::SM),
                ..default()
            },))
                .with_children(|right| {
                    right.spawn((
                        Text::new(crate::i18n::tr(&loc, "status-ready")),
                        TextFont {
                            font_size: FontSize::Px(10.5),
                            ..default()
                        },
                        TextColor(theme.text_muted),
                        TokenHintMarker,
                    ));
                });
        })
        .id();

    // qa 快捷 chips 行（v7.1：点击填入输入框，M4-T8）
    let qa_row = commands
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(space::SM),
            margin: UiRect::bottom(px(space::XS)),
            flex_shrink: 0.0,
            ..default()
        },))
        .id();
    for (label_key, prompt_key) in QA_ACTIONS {
        let chip = commands
            .spawn((
                Button,
                Node {
                    padding: UiRect::horizontal(px(space::SM)),
                    margin: UiRect::right(px(space::XS)),
                    border_radius: BorderRadius::MAX,
                    border: UiRect::all(px(1.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::all(theme.border),
                HoverTint::ghost(&theme),
                QaChipMarker { key: prompt_key },
            ))
            .with_children(|c| {
                c.spawn(ui_text(
                    tr(&loc, label_key).to_string(),
                    type_scale::MICRO,
                    510,
                    theme.text_muted,
                    type_scale::line_height::UI,
                ));
            })
            .id();
        commands.entity(qa_row).add_child(chip);
    }

    // inputbar 容器（浮动卡风格）
    let inputbar = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::new(
                    Val::Px(space::SM),
                    Val::Px(space::XXL),
                    Val::Px(space::LG),
                    Val::Px(space::XXL),
                ),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme.surface),
        ))
        .add_child(qa_row)
        .add_child(input_entity)
        .add_child(toolbar)
        .id();

    // 回到底部浮钮（M4-T3；默认隐藏，update_back_to_bottom 控显隐）
    let back_to_bottom = commands
        .spawn((
            Button,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(150.0),
                left: Val::Percent(50.0),
                width: px(36.0),
                height: px(36.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::MAX,
                border: UiRect::all(px(1.0)),
                display: Display::None,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme.elevated),
            BorderColor::all(theme.border),
            Text::new("▼"),
            TextFont {
                font_size: FontSize::Px(type_scale::MICRO),
                ..default()
            },
            TextColor(theme.text_dim),
            BackToBottomMarker,
        ))
        .id();

    commands
        .entity(panel)
        .add_child(viewtabs)
        .add_child(message_list)
        .add_child(inputbar)
        .add_child(back_to_bottom);

    entities.message_list = Some(message_list);
    entities.current_text = Some(current_text);
    entities.input = Some(input_entity);
    entities.back_to_bottom = Some(back_to_bottom);
}

/// 用户提交输入时，在消息列表中 spawn 用户消息（全宽行式布局）。
fn spawn_user_message(
    mut reader: MessageReader<ChatInputSubmitted>,
    entities: Res<ChatPanelEntities>,
    theme: Res<Theme>,
    loc: Res<xgent_settings::Localizer>,
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
        // 在当前助手节点之前插入用户消息（全宽行式：头像 + 消息体）
        commands.entity(list).with_children(|p| {
            p.spawn((Node {
                max_width: px(820.0),
                align_self: AlignSelf::Center,
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                column_gap: px(space::MD),
                ..default()
            },))
                .with_children(|row| {
                    // 头像（icon_bg 底 + "你"）
                    row.spawn((
                        Node {
                            width: px(28.0),
                            height: px(28.0),
                            border_radius: BorderRadius::all(px(6.0)),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(theme.icon_bg),
                        Text::new(crate::i18n::tr(&loc, "role-user")),
                        TextFont {
                            font_size: FontSize::Px(type_scale::CAPTION),
                            weight: FontWeight(590),
                            ..default()
                        },
                        TextColor(theme.text_dim),
                    ));
                    // 消息体（role + content）
                    row.spawn((Node {
                        flex_grow: 1.0,
                        min_width: Val::ZERO,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },))
                        .with_children(|body| {
                            // role 行
                            body.spawn((
                                Text::new(crate::i18n::tr(&loc, "role-user")),
                                TextFont {
                                    font_size: FontSize::Px(12.0),
                                    ..default()
                                },
                                TextColor(theme.text),
                            ));
                            // 正文
                            body.spawn((
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
        commands.entity(list).add_child(current);
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
        if text.0.ends_with('▍') {
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
    loc: Res<xgent_settings::Localizer>,
    icons: Res<crate::kit::IconAssets>,
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
    let content = text.0.trim_end_matches('▍').to_string();
    if content.is_empty() {
        return;
    }
    let font = theme.font_size;
    // 历史副本（限宽 820 居中：头像 + 消息体）
    commands.entity(list).with_children(|p| {
        p.spawn((Node {
            max_width: px(820.0),
            align_self: AlignSelf::Center,
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            column_gap: px(space::MD),
            ..default()
        },))
            .with_children(|row| {
                // 头像（accent 底 + "X"）
                row.spawn((
                    Node {
                        width: px(28.0),
                        height: px(28.0),
                        border_radius: BorderRadius::all(px(6.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(theme.accent),
                    Text::new("X"),
                    TextFont {
                        font_size: FontSize::Px(type_scale::CAPTION),
                        weight: FontWeight(590),
                        ..default()
                    },
                    TextColor(theme.accent_text),
                ));
                // 消息体
                row.spawn((Node {
                    flex_grow: 1.0,
                    min_width: Val::ZERO,
                    flex_direction: FlexDirection::Column,
                    ..default()
                },))
                    .with_children(|body| {
                        body.spawn((
                            Text::new(crate::i18n::tr(&loc, "role-assistant")),
                            TextFont {
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(theme.text),
                        ));
                        body.spawn((
                            Text::new(content.clone()),
                            TextFont {
                                font_size: FontSize::Px(font),
                                ..default()
                            },
                            TextColor(theme.text_dim),
                            LineHeight::RelativeToFont(type_scale::line_height::BODY),
                        ));
                    });
                // 操作栏（右上常显：复制到剪贴板；v7.1 悬停显隐留 span 重构）
                row.spawn((
                    Button,
                    Node {
                        position_type: PositionType::Absolute,
                        top: px(0.0),
                        right: px(0.0),
                        padding: UiRect::all(px(space::XS)),
                        border_radius: BorderRadius::all(px(radius::SMALL)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    HoverTint::ghost(&theme),
                    CopyActionMarker {
                        text: content.clone(),
                    },
                ))
                .with_children(|bar| {
                    bar.spawn(icon(&icons, "copy", 12.0, theme.text_faint));
                });
            });
    });
    commands.entity(current).insert(Text::new(String::new()));
}

/// 出错时把错误信息写到当前助手消息节点。
fn on_error(
    mut reader: MessageReader<ErrorMessage>,
    q: Query<Entity, With<CurrentAssistantText>>,
    mut commands: Commands,
    theme: Res<Theme>,
    loc: Res<xgent_settings::Localizer>,
) {
    let Ok(entity) = q.single() else {
        return;
    };
    for ev in reader.read() {
        let prefix = match ev.kind {
            xgent_core::chat::ErrorKind::NotConfigured => {
                crate::i18n::tr(&loc, "error-not-configured")
            }
            xgent_core::chat::ErrorKind::AuthFailed => crate::i18n::tr(&loc, "error-auth-failed"),
            xgent_core::chat::ErrorKind::Network => crate::i18n::tr(&loc, "error-network"),
            xgent_core::chat::ErrorKind::StreamParse => crate::i18n::tr(&loc, "error-stream-parse"),
            xgent_core::chat::ErrorKind::ProviderError => crate::i18n::tr(&loc, "error-provider"),
        };
        let retry_hint = crate::i18n::tr(&loc, "error-retry-hint");
        commands.entity(entity).insert((
            Text::new(format!("{prefix} {}\n\n{retry_hint}", ev.message)),
            TextColor(theme.accent),
        ));
    }
}
/// 重试时在当前助手消息节点显示「重试中(第 N 次)」与上次失败原因。
fn show_retry_status(
    mut reader: MessageReader<RetryMessage>,
    q: Query<Entity, With<CurrentAssistantText>>,
    mut commands: Commands,
    theme: Res<Theme>,
    loc: Res<xgent_settings::Localizer>,
) {
    let Ok(entity) = q.single() else {
        return;
    };
    for ev in reader.read() {
        let label = if ev.infinite {
            crate::i18n::tr_with(
                &loc,
                "retry-attempt-infinite",
                &[("n", ev.attempt.to_string().into())],
            )
            .to_string()
        } else {
            crate::i18n::tr_with(
                &loc,
                "retry-attempt",
                &[("n", ev.attempt.to_string().into())],
            )
            .to_string()
        };
        let last_error = crate::i18n::tr_with(
            &loc,
            "retry-last-error",
            &[("error", ev.last_error.clone().into())],
        );
        commands.entity(entity).insert((
            Text::new(format!("{label}\n{last_error}")),
            TextColor(theme.text_dim),
        ));
    }
}

/// 压缩触发后在消息列表插入一条 dim 提示。
fn show_compacted_notice(
    mut reader: MessageReader<CompactedMessage>,
    entities: Res<ChatPanelEntities>,
    mut commands: Commands,
    theme: Res<Theme>,
    loc: Res<xgent_settings::Localizer>,
) {
    let Some(list) = entities.message_list else {
        return;
    };
    for ev in reader.read() {
        let before = crate::status_bar::format_tokens(ev.tokens_before.into());
        let after = crate::status_bar::format_tokens(ev.tokens_after.into());
        let notice = crate::i18n::tr_with(
            &loc,
            "compaction-notice",
            &[("before", before.into()), ("after", after.into())],
        );
        commands.entity(list).with_children(|p| {
            p.spawn((Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },))
                .with_children(|row| {
                    row.spawn((
                        Node {
                            padding: UiRect::all(px(space::XS)),
                            border_radius: BorderRadius::all(px(4.0)),
                            ..default()
                        },
                        BackgroundColor(theme.elevated),
                        Text::new(notice.to_string()),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme.text_muted),
                    ));
                });
        });
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
            if let Some(input) = entities.input {
                commands.entity(input).insert(InputBusyMarker {
                    started_at: time.elapsed().as_secs_f64(),
                });
            }
            continue;
        }
        if conv.status == ConversationStatus::Idle || conv.status == ConversationStatus::Error {
            let (text, queries) = crate::editor::at_syntax::parse_at_references(&ev.text);
            user_writer.write(UserInputMessage {
                text,
                editor_queries: queries,
            });
        } else {
            steering_writer.write(SteeringMessage {
                text: ev.text.clone(),
            });
        }
    }
}
/// 更新输入框边框颜色：忙时 accent；空输入发送时红边闪烁。
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
            commands.entity(entity).remove::<InputBusyMarker>();
            border.set_all(theme.border);
        } else {
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
/// 流式光标：会话进行中时，在当前助手消息文本末尾闪烁 `▋`。
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
        if text.0.ends_with('▋') {
            text.0.pop();
        }
        return;
    }
    let show = (time.elapsed().as_secs_f64() % 1.0) < 0.5;
    let has_cursor = text.0.ends_with('▋');
    if show && !has_cursor {
        text.0.push('▋');
    } else if !show && has_cursor {
        text.0.pop();
    }
}
/// 更新会话信息文本（有变更检测，避免每帧遍历消息列表）。
fn update_conversation_info(
    conv: Res<Conversation>,
    tokens: Res<TokenUsage>,
    loc: Res<xgent_settings::Localizer>,
    mut q: Query<&mut Text, With<ConversationInfoMarker>>,
) {
    if !conv.is_changed() && !tokens.is_changed() && !loc.is_changed() {
        return;
    }
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let turns = conv
        .messages
        .iter()
        .filter(|m| matches!(m, xgent_core::chat::AgentMessage::User(_)))
        .count();
    let token_part = if tokens.total > 0 {
        let token_str = crate::status_bar::format_tokens(tokens.total);
        crate::i18n::tr_with(&loc, "conversation-tokens", &[("tokens", token_str.into())])
            .to_string()
    } else {
        String::new()
    };
    let new_text = crate::i18n::tr_with(
        &loc,
        "conversation-info",
        &[
            ("id", conv.id.0.to_string().into()),
            ("turns", turns.to_string().into()),
            ("tokens", token_part.into()),
        ],
    )
    .to_string();
    if text.0 != new_text {
        text.0 = new_text;
    }
}

/// 收到 SessionClearedMessage 时清空消息列表的所有子节点。
fn clear_on_new_session(
    mut reader: MessageReader<SessionClearedMessage>,
    entities: Res<ChatPanelEntities>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    if reader.read().next().is_none() {
        return;
    }
    let Some(list) = entities.message_list else {
        return;
    };
    if let Some(cur) = entities.current_text {
        commands
            .entity(list)
            .detach_children(&[cur])
            .despawn_related::<Children>()
            .add_child(cur);
        commands
            .entity(cur)
            .insert((Text::new(String::new()), TextColor(theme.text)));
    } else {
        commands.entity(list).despawn_related::<Children>();
    }
}

/// 更新输入框右侧 tokenhint 文本（有变更检测）。
fn update_token_hint(
    conv: Res<Conversation>,
    loc: Res<xgent_settings::Localizer>,
    mut q: Query<&mut Text, With<TokenHintMarker>>,
) {
    if !conv.is_changed() && !loc.is_changed() {
        return;
    }
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

/// 输入卡 focus 环（M4-T7）：聚焦时 `Outline` 3px `accent_glow` + 边框 `accent_interactive`。
fn update_input_focus_ring(
    mut commands: Commands,
    focus: Res<InputFocus>,
    theme: Res<Theme>,
    q_input: Query<Entity, With<ChatInputMarker>>,
    q_border: Query<&BorderColor, With<ChatInputMarker>>,
    q_outline: Query<Option<&Outline>, With<ChatInputMarker>>,
) {
    let Ok(entity) = q_input.single() else {
        return;
    };
    let focused = focus.get() == Some(entity);
    let Ok(border) = q_border.get(entity) else {
        return;
    };
    let line = if focused {
        theme.accent_interactive
    } else {
        theme.border
    };
    let want_border = BorderColor::all(line);
    if *border != want_border {
        commands.entity(entity).insert(want_border);
    }
    let want_outline = focused.then_some(Outline {
        width: Val::Px(3.0),
        offset: Val::Px(0.0),
        color: theme.accent_glow,
    });
    let has = matches!(q_outline.get(entity), Ok(Some(_)));
    match (want_outline, has) {
        (Some(o), false) => {
            commands.entity(entity).insert(o);
        }
        (None, true) => {
            commands.entity(entity).remove::<Outline>();
        }
        (Some(o), true) => {
            if let Ok(Some(cur)) = q_outline.get(entity)
                && *cur != o
            {
                commands.entity(entity).insert(o);
            }
        }
        (None, false) => {}
    }
}

/// 回到底部浮钮（M4-T3）：上滚（scroll.y > 1）显现；点击回底。
fn update_back_to_bottom(
    entities: Res<ChatPanelEntities>,
    mut q_scroll: Query<&mut ScrollPosition, With<MessageListMarker>>,
    mut q_node: Query<&mut Node, With<BackToBottomMarker>>,
    q_btn: Query<&Interaction, (With<BackToBottomMarker>, Changed<Interaction>)>,
) {
    let Some(list) = entities.message_list else {
        return;
    };
    let Some(btn) = entities.back_to_bottom else {
        return;
    };
    let Ok(mut scroll) = q_scroll.get_mut(list) else {
        return;
    };
    let show = scroll.y > 1.0;
    if let Ok(mut node) = q_node.get_mut(btn)
        && (node.display == Display::Flex) != show
    {
        node.display = if show { Display::Flex } else { Display::None };
    }
    for interaction in q_btn.iter() {
        if *interaction == Interaction::Pressed {
            scroll.y = 1.0e6; // bevy 滚动系统会钳到有效范围
        }
    }
}

/// qa chips 点击（M4-T8）：清空输入框并填入提示词。
fn handle_qa_chips(
    mut q: Query<(&Interaction, &QaChipMarker), (Changed<Interaction>, With<Button>)>,
    entities: Res<ChatPanelEntities>,
    loc: Res<xgent_settings::Localizer>,
    mut q_input: Query<&mut EditableText, With<ChatInputMarker>>,
) {
    let Some(input) = entities.input else {
        return;
    };
    for (interaction, marker) in q.iter_mut() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if let Ok(mut editable) = q_input.get_mut(input) {
            editable.clear();
            editable.queue_edit(bevy::text::TextEdit::Insert(
                crate::i18n::tr(&loc, marker.key).to_string().into(),
            ));
        }
    }
}

/// agent 历史消息复制钮（M4-T2）：写系统剪贴板。
fn update_msg_actions(
    mut q: Query<(&Interaction, &CopyActionMarker), (Changed<Interaction>, With<Button>)>,
    mut clipboard: ResMut<Clipboard>,
) {
    for (interaction, action) in q.iter_mut() {
        if *interaction == Interaction::Pressed {
            let _ = clipboard.set_text(action.text.clone());
        }
    }
}
