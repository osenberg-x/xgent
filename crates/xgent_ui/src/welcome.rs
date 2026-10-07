//! 欢迎空态（v7）：会话为空时覆盖**消息流区域**——品牌块 / 标题 / 副标题 /
//! 最近会话（方案 §8.4）。挂在 `MessageListMarker` 上而非对话主区，输入卡与
//! 上下文条始终可见（原型 `.welcome` 是 `.conversation` 的子节点）。
//! 提示词入口只保留输入框上方的 qa chips 行一处（空态不再重复摆快捷卡）；
//! 最近会话数据复用 `ListSessionsMessage`/`SessionListMessage` 流。

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::LetterSpacing;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;
use xgent_agent::{
    Conversation, ListSessionsMessage, RestoreSessionMessage, SessionListMessage, SessionSummary,
};
use xgent_settings::Localizer;

use crate::chat_panel::MessageListMarker;
use crate::fonts::ui_text;
use crate::i18n::tr;
use crate::kit::HoverTint;
use crate::theme::{Theme, space, type_scale};

/// 欢迎覆盖层标记。
#[derive(Component, Default)]
pub struct WelcomeMarker;

/// 最近会话列表容器标记。
#[derive(Component, Default)]
pub struct WelcomeRecentMarker;

/// 最近会话小标题标记（无历史会话时整行隐藏，避免空态出现孤立标题）。
#[derive(Component, Default)]
pub struct WelcomeRecentHeadMarker;

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
///
/// 父节点是消息流（`MessageListMarker`）而非对话主区：覆盖层为绝对定位铺满，
/// 挂主区会一并盖住下方的输入卡，空态时输入框就看不见了。
fn spawn_welcome(
    mut commands: Commands,
    q_list: Query<Entity, With<MessageListMarker>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
) {
    let Ok(list) = q_list.single() else {
        return;
    };

    let welcome = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                // 顶部对齐而非居中：下限窗口（1024x640）下消息流盒高仅约 340px，
                // 字号放大或最近会话满 3 条时内容会高于盒高，居中会让品牌块溢出
                // 顶部被裁；顶对齐只损失底部。原型 `.welcome` 同为顶对齐。
                justify_content: JustifyContent::FlexStart,
                row_gap: px(space::SM),
                padding: UiRect::all(px(space::XXXL)),
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
                Node {
                    margin: UiRect::top(px(space::LG)),
                    ..default()
                },
                WelcomeRecentHeadMarker,
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
    commands.entity(list).add_child(welcome);
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

/// 最近会话数据变化时重建列表（≤3 条；点击恢复）。无会话时连小标题一起收起。
fn rebuild_recent_sessions(
    mut commands: Commands,
    sessions: Res<WelcomeSessions>,
    theme: Res<Theme>,
    mut q: Query<(Entity, &Children), With<WelcomeRecentMarker>>,
    items: Query<(), With<RecentItemMarker>>,
    mut q_head: Query<&mut Node, With<WelcomeRecentHeadMarker>>,
) {
    if !sessions.is_changed() {
        return;
    }
    // 空列表时隐藏孤立的「最近会话」标题
    if let Ok(mut head) = q_head.single_mut() {
        head.display = if sessions.0.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
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
                Hovered::default(),
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
    q: Query<&RecentItemMarker, (Added<Pressed>, With<Button>)>,
    mut restore: MessageWriter<RestoreSessionMessage>,
) {
    for item in q.iter() {
        restore.write(RestoreSessionMessage {
            session_id: item.session_id.clone(),
        });
    }
}
