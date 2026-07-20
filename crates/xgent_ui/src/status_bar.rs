//! 状态栏：分段布局（status-dot + provider/model · 会话状态 · token · spacer · 编码语言）。
//!
//! 状态点（小圆点）忙时脉冲；空闲时 ok 色。对齐 ui-prototype.html #statusbar。

use bevy::prelude::*;
use xgent_agent::{Conversation, ConversationStatus, DoneMessage, ProviderInfo};

use crate::layout::StatusBarMarker;
use crate::theme::{Theme, space};

/// 状态点（小圆点）标记。
#[derive(Component, Default)]
pub struct StatusDotMarker;

/// provider/model 文本节点标记。
#[derive(Component, Default)]
pub struct ProviderTextMarker;

/// 会话状态文本节点标记。
#[derive(Component, Default)]
pub struct ConvStatusMarker;

/// token 用量文本节点标记。
#[derive(Component, Default)]
pub struct TokenTextMarker;

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
            .add_systems(Startup, spawn_status_bar)
            .add_systems(
                Update,
                (
                    update_status_segments,
                    update_status_dot,
                    track_token_usage,
                ),
            );
    }
}

/// 启动时在状态栏内 spawn 分段：status-dot + provider · 状态 · token · spacer · 编码。
fn spawn_status_bar(
    mut commands: Commands,
    q_bar: Query<Entity, With<StatusBarMarker>>,
    theme: Res<Theme>,
) {
    let Ok(bar) = q_bar.single() else {
        return;
    };
    let font = theme.font_size;
    let font_size = FontSize::Px(font);
    let dim = theme.text_dim;
    commands.entity(bar).with_children(|p| {
        // status-dot（小圆点，7px）
        p.spawn((
            Node {
                width: px(7.0),
                height: px(7.0),
                border_radius: BorderRadius::all(px(3.5)),
                margin: UiRect::right(px(space::SM)),
                ..default()
            },
            BackgroundColor(theme.st_ok),
            StatusDotMarker,
        ));
        // provider/model 文本
        p.spawn((
            Text::new(String::new()),
            TextFont { font_size, ..default() },
            TextColor(dim),
            ProviderTextMarker,
        ));
        // 分隔 ·
        p.spawn((
            Text::new("·"),
            TextFont { font_size, ..default() },
            TextColor(dim),
        ));
        // 会话状态文本
        p.spawn((
            Text::new(String::new()),
            TextFont { font_size, ..default() },
            TextColor(dim),
            ConvStatusMarker,
        ));
        // 分隔 ·
        p.spawn((
            Text::new("·"),
            TextFont { font_size, ..default() },
            TextColor(dim),
        ));
        // token 文本
        p.spawn((
            Text::new(String::new()),
            TextFont { font_size, ..default() },
            TextColor(dim),
            TokenTextMarker,
        ));
        // spacer
        p.spawn((Node { flex_grow: 1.0, ..default() },));
        // 编码/语言段（右侧）
        p.spawn((
            Text::new("UTF-8 · LF · Rust"),
            TextFont { font_size, ..default() },
            TextColor(dim),
        ));
    });
}

/// 每帧更新各分段文本（provider / 状态 / token）。
fn update_status_segments(
    conv: Res<Conversation>,
    info: Res<ProviderInfo>,
    tokens: Res<TokenUsage>,
    mut q: ParamSet<(
        Query<&mut Text, With<ProviderTextMarker>>,
        Query<&mut Text, With<ConvStatusMarker>>,
        Query<&mut Text, With<TokenTextMarker>>,
    )>,
) {
    if let Ok(mut text) = q.p0().single_mut() {
        let label = if info.id.is_empty() {
            "未配置 provider".to_string()
        } else {
            format!("{} / {}", info.id, info.model)
        };
        if text.0 != label {
            text.0 = label;
        }
    }
    if let Ok(mut text) = q.p1().single_mut() {
        let label = match conv.status {
            ConversationStatus::Idle => "就绪",
            ConversationStatus::Thinking => "思考中…",
            ConversationStatus::Streaming => "生成中…",
            ConversationStatus::ToolRunning => "执行工具…",
            ConversationStatus::Confirming => "等待确认",
            ConversationStatus::Aborting => "中断中…",
            ConversationStatus::Error => "出错",
        };
        if text.0 != label {
            text.0 = label.to_string();
        }
    }
    if let Ok(mut text) = q.p2().single_mut() {
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

/// 状态点：忙时 running 色 + 脉冲（opacity 正弦），空闲 ok 色。
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

/// 收到 DoneMessage 时递增 token 估算。
fn track_token_usage(
    mut reader: MessageReader<DoneMessage>,
    conv: Res<Conversation>,
    mut tokens: ResMut<TokenUsage>,
) {
    for ev in reader.read() {
        // 粗略估算：当前助手文本字符数 / 4
        let chars = conv.current_assistant_text.chars().count() as u64;
        tokens.total += chars / 4;
        let _ = ev;
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

/// 便捷：f32 → Val::Px
fn px(v: f32) -> Val {
    Val::Px(v)
}
