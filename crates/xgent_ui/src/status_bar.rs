//! 状态栏（v7）：分段时间线——daemon/提供方 · tokens · 成本 | spacer | 陪伴 ·
//! 编码 · 会话（#id · N 轮）。段间以右侧细线分隔；仅陪伴开关可点击（本期）。
//!
//! 会话状态文本已移除（顶栏 agent pill 承担，方案 §8.9）；状态点忙时脉冲。

use bevy::prelude::*;
use xgent_agent::{Conversation, ConversationStatus, DoneMessage, ProviderInfo};
use xgent_core::chat::AgentMessage;

use crate::fonts::ui_text;
use crate::layout::StatusBarMarker;
use crate::theme::{Theme, space, type_scale};

/// 状态点（小圆点）标记。
#[derive(Component, Default)]
pub struct StatusDotMarker;

/// provider/model 文本节点标记。
#[derive(Component, Default)]
pub struct ProviderTextMarker;

/// token 用量文本节点标记。
#[derive(Component, Default)]
pub struct TokenTextMarker;

/// 会话段文本标记（#id · N 轮）。
#[derive(Component, Default)]
pub struct SessionTextMarker;

/// 陪伴开关按钮标记。
#[derive(Component, Default)]
pub struct CompanionToggleMarker;

/// 陪伴开关文本标记（★ 陪伴已开启/关闭）。
#[derive(Component, Default)]
pub struct CompanionTextMarker;

/// 陪伴开关状态（宠物本体为 P1；本期为状态栏点亮态占位）。
#[derive(Resource, Debug, Clone, Copy)]
pub struct CompanionOn(pub bool);

impl Default for CompanionOn {
    fn default() -> Self {
        Self(true)
    }
}

/// 累计 token 用量（UI 侧粗略估算）。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    pub total: u64,
}

/// 状态栏插件。
pub struct StatusBarPlugin;

impl Plugin for StatusBarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TokenUsage>()
            .init_resource::<CompanionOn>()
            .add_systems(
                Startup,
                // 显式排在布局生成之后：Startup 系统并行无序，不排序则可能查不到
                // StatusBarMarker 容器导致状态栏永久空白（Startup 只跑一次无重试）。
                spawn_status_bar.after(crate::layout::spawn_layout),
            )
            .add_systems(
                Update,
                (
                    update_status_segments,
                    update_session_text,
                    update_companion_text,
                    toggle_companion,
                    update_status_dot,
                    track_token_usage,
                ),
            );
    }
}

/// 状态栏分段节点（右侧细线分隔；高度 20px 垂直居中于 32px 栏）。
fn segment_node() -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: px(space::XS),
        padding: UiRect::horizontal(px(space::SM)),
        height: px(20.0),
        border: UiRect::right(px(1.0)),
        ..default()
    }
}

/// 启动时在状态栏内 spawn 各分段。
fn spawn_status_bar(
    mut commands: Commands,
    q_bar: Query<Entity, With<StatusBarMarker>>,
    theme: Res<Theme>,
    loc: Res<xgent_settings::Localizer>,
) {
    let Ok(bar) = q_bar.single() else {
        return;
    };
    let micro = type_scale::MICRO;
    let dim = theme.text_muted;

    commands.entity(bar).with_children(|p| {
        // daemon/提供方段：状态点 + provider 文本
        p.spawn((segment_node(), BorderColor::all(theme.line)))
            .with_children(|seg| {
                seg.spawn((
                    Node {
                        width: px(6.0),
                        height: px(6.0),
                        border_radius: BorderRadius::all(px(3.0)),
                        ..default()
                    },
                    BackgroundColor(theme.st_ok),
                    StatusDotMarker,
                ));
                seg.spawn((
                    ui_text(String::new(), micro, 400, dim, type_scale::line_height::UI),
                    ProviderTextMarker,
                ));
            });
        // token 段
        p.spawn((segment_node(), BorderColor::all(theme.line)))
            .with_children(|seg| {
                seg.spawn((
                    ui_text(String::new(), micro, 400, dim, type_scale::line_height::UI),
                    TokenTextMarker,
                ));
            });
        // 成本段（占位，OQ-10 成本统计细化后接入）
        p.spawn((segment_node(), BorderColor::all(theme.line)))
            .with_children(|seg| {
                seg.spawn(ui_text(
                    "$0.00",
                    micro,
                    400,
                    dim,
                    type_scale::line_height::UI,
                ));
            });
        // spacer
        p.spawn((Node {
            flex_grow: 1.0,
            ..default()
        },));
        // 陪伴开关（本期唯一可点项）
        p.spawn((
            Button,
            segment_node(),
            BorderColor::all(theme.line),
            CompanionToggleMarker,
        ))
        .with_children(|seg| {
            seg.spawn((
                ui_text(
                    String::new(),
                    micro,
                    400,
                    theme.warm,
                    type_scale::line_height::UI,
                ),
                CompanionTextMarker,
            ));
        });
        // 编码段（保留）
        p.spawn((segment_node(), BorderColor::all(theme.line)))
            .with_children(|seg| {
                seg.spawn(ui_text(
                    crate::i18n::tr(&loc, "status-encoding").to_string(),
                    micro,
                    400,
                    dim,
                    type_scale::line_height::UI,
                ));
            });
        // 会话段：#id · N 轮（末段无分隔线）
        p.spawn((
            ui_text(String::new(), micro, 400, dim, type_scale::line_height::UI),
            SessionTextMarker,
        ));
    });
}

/// 每帧更新 provider/token 文本（有变更检测，避免每帧分配 i18n 字符串）。
fn update_status_segments(
    info: Res<ProviderInfo>,
    tokens: Res<TokenUsage>,
    loc: Res<xgent_settings::Localizer>,
    mut q: ParamSet<(
        Query<&mut Text, With<ProviderTextMarker>>,
        Query<&mut Text, With<TokenTextMarker>>,
    )>,
) {
    if !info.is_changed() && !loc.is_changed() {
        // provider 段无变化
    } else if let Ok(mut text) = q.p0().single_mut() {
        let label = if info.id.is_empty() {
            crate::i18n::tr(&loc, "status-provider-not-configured").to_string()
        } else {
            format!("{} / {}", info.id, info.model)
        };
        if text.0 != label {
            text.0 = label;
        }
    }
    if tokens.is_changed() {
        if let Ok(mut text) = q.p1().single_mut() {
            let label = if tokens.total > 0 {
                format!("↑ {} tokens", format_tokens(tokens.total))
            } else {
                String::new()
            };
            if text.0 != label {
                text.0 = label;
            }
        }
    }
}

/// 更新会话段：#id · N 轮（轮数 = 用户消息数；承接原 ConversationInfo，方案 §8.4）。
fn update_session_text(
    conv: Res<Conversation>,
    loc: Res<xgent_settings::Localizer>,
    mut q: Query<&mut Text, With<SessionTextMarker>>,
) {
    if !conv.is_changed() && !loc.is_changed() {
        return;
    }
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let rounds = conv
        .messages
        .iter()
        .filter(|m| matches!(m, AgentMessage::User(_)))
        .count();
    let label = crate::i18n::tr_with(
        &loc,
        "status-session",
        &[
            ("id", conv.id.to_string().into()),
            ("rounds", rounds.to_string().into()),
        ],
    );
    if text.0 != label {
        text.0 = label.to_string();
    }
}

/// 更新陪伴开关文本与颜色（开启=warm 星标，关闭=muted）。
fn update_companion_text(
    on: Res<CompanionOn>,
    loc: Res<xgent_settings::Localizer>,
    theme: Res<Theme>,
    mut q: Query<(&mut Text, &mut TextColor), With<CompanionTextMarker>>,
) {
    if !on.is_changed() && !loc.is_changed() && !theme.is_changed() {
        return;
    }
    let Ok((mut text, mut color)) = q.single_mut() else {
        return;
    };
    let key = if on.0 {
        "status-companion-on"
    } else {
        "status-companion-off"
    };
    let label = crate::i18n::tr(&loc, key).to_string();
    if text.0 != label {
        text.0 = label;
    }
    let want = if on.0 { theme.warm } else { theme.text_muted };
    if color.0 != want {
        color.0 = want;
    }
}

/// 点击陪伴段：切换开关（本期唯一可点项，方案 §8.9）。
fn toggle_companion(
    q: Query<&Interaction, (With<CompanionToggleMarker>, Changed<Interaction>)>,
    mut on: ResMut<CompanionOn>,
) {
    for interaction in q.iter() {
        if *interaction == Interaction::Pressed {
            on.0 = !on.0;
        }
    }
}

/// 状态点：忙时 running 色 + 脉冲，空闲 ok 色，错误 fail 色。
fn update_status_dot(
    conv: Res<Conversation>,
    time: Res<Time>,
    theme: Res<Theme>,
    mut q: Query<&mut BackgroundColor, With<StatusDotMarker>>,
) {
    let Ok(mut bg) = q.single_mut() else {
        return;
    };
    let is_busy =
        conv.status != ConversationStatus::Idle && conv.status != ConversationStatus::Error;
    let is_error = conv.status == ConversationStatus::Error;
    let base = if is_error {
        theme.st_fail
    } else if is_busy {
        theme.st_running
    } else {
        theme.st_ok
    };
    let alpha = if is_busy {
        0.4 + 0.6 * (0.5 + 0.5 * (time.elapsed().as_secs_f64() * std::f64::consts::TAU / 1.4).sin())
    } else {
        1.0
    } as f32;
    let srgba = base.to_srgba();
    let want = BackgroundColor(Color::srgba(srgba.red, srgba.green, srgba.blue, alpha));
    if *bg != want {
        *bg = want;
    }
}

/// 收到 DoneMessage 时累加真实 token 用量。
fn track_token_usage(mut reader: MessageReader<DoneMessage>, mut tokens: ResMut<TokenUsage>) {
    for ev in reader.read() {
        if let Some(u) = &ev.usage {
            tokens.total += u.prompt as u64 + u.completion as u64;
        }
    }
}

/// 格式化 token 数（k 单位）。
pub fn format_tokens(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}
