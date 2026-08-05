//! 会话历史面板：列出历史会话，点击恢复。
//!
//! 作为 overlay 弹窗（类似设置面板），从 sessions 目录读取 JSONL 摘要。
//! 订阅 `SessionListMessage` 获取会话列表，发 `RestoreSessionMessage` 恢复。

use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::ui::ScrollPosition;

use xgent_agent::{
    ListSessionsMessage, RestoreSessionMessage, SessionListMessage, SessionRestoredMessage,
    SessionSummary,
};
use xgent_settings::Localizer;

use crate::i18n::{tr, tr_with};
use crate::theme::{Theme, space};

/// 会话历史面板状态（open/close）。
#[derive(Resource, Default)]
pub struct SessionHistoryState {
    pub open: bool,
}

/// 面板根节点标记。
#[derive(Component, Default)]
pub struct SessionHistoryOverlayMarker;

/// 单条会话项标记（携带会话 id）。
#[derive(Component)]
pub struct SessionItemMarker {
    pub session_id: String,
}

/// 恢复按钮标记（携带会话 id）。
#[derive(Component)]
pub struct SessionRestoreButtonMarker {
    pub session_id: String,
}

/// 关闭按钮标记。
#[derive(Component, Default)]
pub struct SessionHistoryCloseMarker;

/// 缓存的会话列表（用于渲染）。
#[derive(Resource, Default)]
pub struct CachedSessionList {
    pub sessions: Vec<SessionSummary>,
}

pub struct SessionHistoryPlugin;

impl Plugin for SessionHistoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SessionHistoryState>()
            .init_resource::<CachedSessionList>()
            .add_systems(Update, session_history_overlay_systems)
            .add_systems(
                Update,
                handle_session_list_results.after(xgent_agent::agent_loop::agent_poll_system),
            )
            .add_systems(
                Update,
                handle_restore_results.after(xgent_agent::agent_loop::agent_poll_system),
            );
    }
}

#[allow(clippy::too_many_arguments)]
fn session_history_overlay_systems(
    theme: Res<Theme>,
    loc: Res<Localizer>,
    cached: Res<CachedSessionList>,
    mut commands: Commands,
    q_overlay: Query<Entity, With<SessionHistoryOverlayMarker>>,
    q_restore: Query<(&Interaction, &SessionRestoreButtonMarker), Changed<Interaction>>,
    q_close: Query<&Interaction, (With<SessionHistoryCloseMarker>, Changed<Interaction>)>,
    mut restore_writer: MessageWriter<RestoreSessionMessage>,
    mut history_state: ResMut<SessionHistoryState>,
    mut list_writer: MessageWriter<ListSessionsMessage>,
) {
    // 打开时 spawn overlay（若不存在）
    if history_state.open && q_overlay.is_empty() {
        list_writer.write(ListSessionsMessage);
        spawn_overlay(&mut commands, &theme, &loc, &cached);
    }

    // 关闭时 despawn overlay
    if !history_state.open && !q_overlay.is_empty() {
        for entity in q_overlay.iter() {
            commands.entity(entity).despawn();
        }
    }

    // 处理关闭按钮
    for i in q_close.iter() {
        if *i == Interaction::Pressed {
            history_state.open = false;
        }
    }

    // 处理恢复按钮点击
    for (interaction, marker) in q_restore.iter() {
        if *interaction == Interaction::Pressed {
            restore_writer.write(RestoreSessionMessage {
                session_id: marker.session_id.clone(),
            });
            history_state.open = false;
        }
    }
}

/// 生成会话历史 overlay 面板。
fn spawn_overlay(
    commands: &mut Commands,
    theme: &Theme,
    loc: &Localizer,
    cached: &CachedSessionList,
) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(theme.overlay),
            GlobalZIndex(40),
            SessionHistoryOverlayMarker,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: Val::Px(480.0),
                        max_height: Val::Px(520.0),
                        flex_direction: FlexDirection::Column,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(8.0)),
                        overflow: Overflow::clip_y(),
                        ..default()
                    },
                    BackgroundColor(theme.panel),
                    BorderColor::all(theme.border),
                ))
                .with_children(|panel| {
                    // 标题栏
                    panel
                        .spawn((
                            Node {
                                width: Val::Percent(100.0),
                                padding: UiRect::all(Val::Px(space::SM)),
                                flex_direction: FlexDirection::Row,
                                justify_content: JustifyContent::SpaceBetween,
                                align_items: AlignItems::Center,
                                border: UiRect::bottom(Val::Px(1.0)),
                                ..default()
                            },
                            BorderColor::all(theme.border),
                        ))
                        .with_children(|head| {
                            head.spawn((
                                Text::new(tr(loc, "history-title").to_string()),
                                TextFont {
                                    font_size: FontSize::Px(14.0),
                                    ..default()
                                },
                                TextColor(theme.text),
                            ));
                            head.spawn((
                                Button,
                                Node {
                                    width: Val::Px(24.0),
                                    height: Val::Px(24.0),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    ..default()
                                },
                                Text::new(tr(loc, "history-close").to_string()),
                                TextFont {
                                    font_size: FontSize::Px(14.0),
                                    ..default()
                                },
                                TextColor(theme.text_dim),
                                SessionHistoryCloseMarker,
                            ));
                        });

                    // 会话列表
                    panel
                        .spawn((
                            Node {
                                width: Val::Percent(100.0),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                overflow: Overflow::clip_y(),
                                ..default()
                            },
                            ScrollPosition::default(),
                        ))
                        .with_children(|list| {
                            if cached.sessions.is_empty() {
                                list.spawn((
                                    Node {
                                        padding: UiRect::all(Val::Px(space::XL)),
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        ..default()
                                    },
                                    Text::new(tr(loc, "history-empty").to_string()),
                                    TextFont {
                                        font_size: FontSize::Px(13.0),
                                        ..default()
                                    },
                                    TextColor(theme.text_muted),
                                ));
                            } else {
                                for s in &cached.sessions {
                                    spawn_session_item(list, s, theme, loc);
                                }
                            }
                        });
                });
        });
}

/// 生成单个会话条目。
fn spawn_session_item(
    parent: &mut ChildSpawnerCommands,
    session: &SessionSummary,
    theme: &Theme,
    loc: &Localizer,
) {
    let title = session
        .title
        .clone()
        .unwrap_or_else(|| format!("#{}", session.id));
    let date = format_timestamp(session.timestamp);
    let msg_count = tr_with(
        loc,
        "history-message-count",
        &[("count", session.message_count.to_string().into())],
    )
    .to_string();

    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(space::SM)),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(theme.line),
            SessionItemMarker {
                session_id: session.id.clone(),
            },
        ))
        .with_children(|row| {
            // 左侧：标题 + 元信息
            row.spawn((
                Node {
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
            ))
            .with_children(|info| {
                info.spawn((
                    Text::new(title.clone()),
                    TextFont {
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(theme.text),
                ));
                info.spawn((
                    Text::new(format!("{date} · {msg_count}")),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme.text_muted),
                ));
            });
            // 右侧：恢复按钮
            row.spawn((
                Button,
                Node {
                    padding: UiRect::horizontal(Val::Px(space::SM)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(theme.elevated),
                BorderColor::all(theme.border),
                Text::new(tr(loc, "history-restore").to_string()),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(theme.text),
                SessionRestoreButtonMarker {
                    session_id: session.id.clone(),
                },
            ));
        });
}

/// 格式化时间戳为简短日期。
fn format_timestamp(ms: u64) -> String {
    if ms == 0 {
        return String::new();
    }
    let secs = ms / 1000;
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    if days > 0 {
        format!("{days}d {hours}h")
    } else {
        let mins = (secs % 3600) / 60;
        format!("{hours}h {mins}m")
    }
}

/// 订阅 SessionListMessage，更新缓存并重建面板。
fn handle_session_list_results(
    mut reader: MessageReader<SessionListMessage>,
    mut cached: ResMut<CachedSessionList>,
    state: Res<SessionHistoryState>,
    mut commands: Commands,
    q_overlay: Query<Entity, With<SessionHistoryOverlayMarker>>,
) {
    for ev in reader.read() {
        cached.sessions = ev.sessions.clone();
    }
    // 如果面板打开且缓存更新了，despawn 旧面板（下帧自动重建）
    if state.open && !q_overlay.is_empty() && cached.is_changed() {
        for entity in q_overlay.iter() {
            commands.entity(entity).despawn();
        }
    }
}

/// 订阅 SessionRestoredMessage，通知 UI 重建消息列表。
fn handle_restore_results(
    mut reader: MessageReader<SessionRestoredMessage>,
    entities: Res<crate::chat_panel::ChatPanelEntities>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    mut commands: Commands,
) {
    let Some(list) = entities.message_list else {
        return;
    };
    let Some(cur) = entities.current_text else {
        return;
    };
    for ev in reader.read() {
        // 清空消息列表的子节点（保留 current_text）
        commands
            .entity(list)
            .detach_children(&[cur])
            .despawn_related::<Children>()
            .add_child(cur);
        commands
            .entity(cur)
            .insert((Text::new(String::new()), TextColor(theme.text)));

        let font = theme.font_size;
        for msg in &ev.messages {
            use xgent_core::chat::AgentMessage;
            match msg {
                AgentMessage::User(um) => {
                    let text = extract_text(&um.content);
                    if text.is_empty() {
                        continue;
                    }
                    spawn_history_message_row(&mut commands, list, &theme, &loc, &text, true, font);
                }
                AgentMessage::Assistant(am) => {
                    let text = extract_text(&am.content);
                    if text.is_empty() {
                        continue;
                    }
                    spawn_history_message_row(
                        &mut commands,
                        list,
                        &theme,
                        &loc,
                        &text,
                        false,
                        font,
                    );
                }
                AgentMessage::ToolResult(tr_msg) => {
                    let label = format!("[{}] {}", tr_msg.tool_name, tr_msg.content);
                    spawn_history_message_row(&mut commands, list, &theme, &loc, &label, false, font);
                }
                AgentMessage::Notification(n) => {
                    spawn_history_message_row(&mut commands, list, &theme, &loc, &n.text, false, font);
                }
            }
        }
    }
}

/// 从 ContentBlock 列表中提取纯文本。
fn extract_text(blocks: &[xgent_core::chat::ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            xgent_core::chat::ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 生成一条历史消息行（用户/助手统一布局）。
fn spawn_history_message_row(
    commands: &mut Commands,
    list: Entity,
    theme: &Theme,
    loc: &Localizer,
    text: &str,
    is_user: bool,
    font: f32,
) {
    let avatar_text: String;
    let role_label: String;
    let avatar_bg: Color;
    let text_color: Color;

    if is_user {
        avatar_text = tr(loc, "role-user").to_string();
        role_label = tr(loc, "role-user").to_string();
        avatar_bg = theme.elevated;
        text_color = theme.text;
    } else {
        avatar_text = "✦".to_string();
        role_label = tr(loc, "role-assistant").to_string();
        avatar_bg = theme.accent;
        text_color = theme.text_dim;
    }

    commands.entity(list).with_children(|p| {
        p.spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(space::MD),
                ..default()
            },
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    width: Val::Px(28.0),
                    height: Val::Px(28.0),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(avatar_bg),
                Text::new(avatar_text),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(theme.text),
            ));
            row.spawn((
                Node {
                    flex_grow: 1.0,
                    min_width: Val::ZERO,
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
            ))
            .with_children(|body| {
                body.spawn((
                    Text::new(role_label),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(theme.text),
                ));
                body.spawn((
                    Text::new(text.to_string()),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(text_color),
                ));
            });
        });
    });
}
