//! 欢迎空态（v7）：会话为空时覆盖对话区——品牌块 / 标题 / 快捷卡 / 最近会话
//! （方案 §8.4）。快捷卡与 qa chips 共用 `QaChipMarker` 点击填入通路；
//! 最近会话数据复用 `ListSessionsMessage`/`SessionListMessage` 流。

use bevy::prelude::*;
use bevy::text::LetterSpacing;
use xgent_agent::{
    Conversation, ListSessionsMessage, RestoreSessionMessage, SessionListMessage, SessionSummary,
};
use xgent_settings::Localizer;

use crate::chat_panel::QaChipMarker;
use crate::fonts::ui_text;
use crate::i18n::tr;
use crate::kit::{HoverTint, IconAssets, icon};
use crate::layout::ChatPanelMarker;
use crate::theme::{Theme, space, type_scale};

/// 欢迎覆盖层标记。
#[derive(Component, Default)]
pub struct WelcomeMarker;

/// 最近会话列表容器标记。
#[derive(Component, Default)]
pub struct WelcomeRecentMarker;

/// 最近会话条目标记（携带会话 id）。
#[derive(Component, Default)]
pub struct RecentItemMarker {
    pub session_id: String,
}

/// 最近会话数据缓存（`SessionListMessage` 回填）。
#[derive(Resource, Debug, Clone, Default)]
pub struct WelcomeSessions(pub Vec<SessionSummary>);

/// 欢迎空态插件。
pub struct WelcomePlugin;

impl Plugin for WelcomePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WelcomeSessions>()
            .add_systems(
                Startup,
                spawn_welcome
                    .after(crate::layout::spawn_layout)
                    .after(crate::chat_panel::spawn_chat_panel),
            )
            .add_systems(
                Update,
                (
                    toggle_welcome,
                    read_session_list,
                    rebuild_recent_sessions,
                    handle_recent_click,
                ),
            );
    }
}

/// 启动时 spawn 欢迎覆盖层（默认隐藏，`toggle_welcome` 控显隐）。
fn spawn_welcome(
    mut commands: Commands,
    q_panel: Query<Entity, With<ChatPanelMarker>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    icons: Res<IconAssets>,
) {
    let Ok(panel) = q_panel.single() else {
        return;
    };

    let welcome = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(space::SM),
                padding: UiRect::horizontal(px(space::XXXL)),
                display: Display::None,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme.bg),
            WelcomeMarker,
        ))
        .with_children(|w| {
            // 品牌块
            w.spawn((
                Node {
                    width: px(64.0),
                    height: px(64.0),
                    border_radius: BorderRadius::all(px(12.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    margin: UiRect::bottom(px(space::SM)),
                    ..default()
                },
                BackgroundColor(theme.accent),
                ui_text(
                    "X",
                    28.0,
                    590,
                    theme.accent_text,
                    type_scale::line_height::UI,
                ),
            ));
            // 标题 / 副标题
            w.spawn((
                ui_text(
                    tr(&loc, "welcome-title").to_string(),
                    type_scale::DISPLAY,
                    590,
                    theme.text,
                    type_scale::line_height::TIGHT,
                ),
                LetterSpacing::Px(-0.29),
            ));
            w.spawn((ui_text(
                tr(&loc, "welcome-sub").to_string(),
                type_scale::BODY,
                400,
                theme.text_muted,
                type_scale::line_height::BODY,
            ),));

            // 快捷卡 ×3（点击经 QaChipMarker 填入输入框）
            let cards = [
                (
                    "info",
                    theme.st_info_bg,
                    theme.st_info,
                    "welcome-card-explain",
                    "welcome-card-explain-desc",
                    "qa-explain-prompt",
                ),
                (
                    "edit",
                    theme.accent_bg,
                    theme.accent_interactive,
                    "welcome-card-refactor",
                    "welcome-card-refactor-desc",
                    "qa-refactor-prompt",
                ),
                (
                    "check",
                    theme.st_ok_bg,
                    theme.st_ok,
                    "welcome-card-test",
                    "welcome-card-test-desc",
                    "qa-test-prompt",
                ),
            ];
            w.spawn((Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                justify_content: JustifyContent::Center,
                column_gap: px(space::MD),
                row_gap: px(space::SM),
                margin: UiRect::top(px(space::MD)),
                max_width: px(660.0),
                ..default()
            },))
                .with_children(|row| {
                    for (icon_name, tint_bg, tint_fg, title_key, desc_key, prompt_key) in cards {
                        row.spawn((
                            Button,
                            Node {
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Start,
                                row_gap: px(space::XS),
                                padding: UiRect::all(px(space::MD)),
                                width: px(200.0),
                                border_radius: BorderRadius::all(px(8.0)),
                                border: UiRect::all(px(1.0)),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            BackgroundColor(theme.subtle),
                            BorderColor::all(theme.border),
                            HoverTint::standard(&theme),
                            QaChipMarker { key: prompt_key },
                        ))
                        .with_children(|card| {
                            card.spawn((
                                Node {
                                    width: px(32.0),
                                    height: px(32.0),
                                    border_radius: BorderRadius::all(px(6.0)),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    margin: UiRect::bottom(px(space::XS)),
                                    ..default()
                                },
                                BackgroundColor(tint_bg),
                            ))
                            .with_children(|blk| {
                                blk.spawn(icon(&icons, icon_name, 16.0, tint_fg));
                            });
                            card.spawn((ui_text(
                                tr(&loc, title_key).to_string(),
                                type_scale::SMALL,
                                590,
                                theme.text,
                                type_scale::line_height::UI,
                            ),));
                            card.spawn((ui_text(
                                tr(&loc, desc_key).to_string(),
                                type_scale::MICRO,
                                400,
                                theme.text_muted,
                                type_scale::line_height::UI,
                            ),));
                        });
                    }
                });

            // 最近会话
            w.spawn((
                ui_text(
                    tr(&loc, "welcome-recent").to_string(),
                    type_scale::TINY,
                    510,
                    theme.text_muted,
                    type_scale::line_height::UI,
                ),
                LetterSpacing::Px(0.5),
            ));
            w.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(space::XS),
                    width: px(560.0),
                    margin: UiRect::top(px(space::XS)),
                    ..default()
                },
                WelcomeRecentMarker,
            ));
        })
        .id();
    commands.entity(panel).add_child(welcome);
}

/// 会话为空 → 显示并触发一次会话列表拉取；非空 → 隐藏。
fn toggle_welcome(
    conv: Res<Conversation>,
    mut q: Query<&mut Node, With<WelcomeMarker>>,
    mut list_writer: MessageWriter<ListSessionsMessage>,
    mut was_visible: Local<bool>,
) {
    let empty = conv.messages.is_empty();
    if empty == *was_visible {
        return;
    }
    for mut node in q.iter_mut() {
        node.display = if empty { Display::Flex } else { Display::None };
    }
    if empty {
        // 进入空态：拉取最近会话列表
        list_writer.write(ListSessionsMessage);
    }
    *was_visible = empty;
}

/// `SessionListMessage` 回填缓存。
fn read_session_list(
    mut reader: MessageReader<SessionListMessage>,
    mut sessions: ResMut<WelcomeSessions>,
) {
    for ev in reader.read() {
        sessions.0 = ev.sessions.clone();
    }
}

/// 最近会话数据变化时重建列表（≤3 条；点击恢复）。
fn rebuild_recent_sessions(
    mut commands: Commands,
    sessions: Res<WelcomeSessions>,
    theme: Res<Theme>,
    mut q: Query<(Entity, &Children), With<WelcomeRecentMarker>>,
    items: Query<(), With<RecentItemMarker>>,
) {
    if !sessions.is_changed() {
        return;
    }
    let Ok((container, children)) = q.single_mut() else {
        return;
    };
    for child in children.iter() {
        if items.contains(child) {
            commands.entity(child).despawn();
        }
    }
    for s in sessions.0.iter().take(3) {
        let title = s.title.clone().unwrap_or_else(|| s.id.clone());
        commands.entity(container).with_children(|col| {
            col.spawn((
                Button,
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(space::SM),
                    padding: UiRect::all(px(space::XS)),
                    border_radius: BorderRadius::all(px(6.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::all(Color::NONE),
                HoverTint::standard(&theme),
                RecentItemMarker {
                    session_id: s.id.clone(),
                },
            ))
            .with_children(|row| {
                row.spawn((ui_text(
                    title,
                    type_scale::BODY_SM,
                    510,
                    theme.text_dim,
                    type_scale::line_height::UI,
                ),));
                row.spawn((ui_text(
                    s.id.clone(),
                    type_scale::MICRO,
                    400,
                    theme.text_faint,
                    type_scale::line_height::UI,
                ),));
            });
        });
    }
}

/// 点击最近会话 → 发恢复请求。
fn handle_recent_click(
    mut q: Query<(&Interaction, &RecentItemMarker), (Changed<Interaction>, With<Button>)>,
    mut restore: MessageWriter<RestoreSessionMessage>,
) {
    for (interaction, item) in q.iter() {
        if *interaction == Interaction::Pressed {
            restore.write(RestoreSessionMessage {
                session_id: item.session_id.clone(),
            });
        }
    }
}
