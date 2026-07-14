//! 状态栏：当前 provider/model、会话状态、token 指示。

use bevy::prelude::*;
use xgent_agent::{Conversation, ConversationStatus, DoneMessage, ProviderInfo};

use crate::layout::StatusBarMarker;
use crate::theme::Theme;

/// 状态栏文本节点标记。
#[derive(Component, Default)]
pub struct StatusTextMarker;

/// 累计 token 用量（UI 侧粗略估算）。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    /// 累计 token 数
    pub total: u64,
}

/// 状态栏插件。
pub struct StatusBarPlugin;

impl Plugin for StatusBarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TokenUsage>()
            .add_systems(
                Startup,
                spawn_status_bar.after(crate::layout::spawn_layout),
            )
            .add_systems(Update, (update_status_text, track_token_usage).after(xgent_agent::agent_loop::agent_poll_system));
    }
}

/// 启动时在状态栏内 spawn 文本节点。
fn spawn_status_bar(
    mut commands: Commands,
    q_bar: Query<Entity, With<StatusBarMarker>>,
    theme: Res<Theme>,
) {
    let Ok(bar) = q_bar.single() else {
        return;
    };
    let font = theme.font_size;
    commands.entity(bar).with_children(|p| {
        p.spawn((
            Node { ..default() },
            Text::new(String::new()),
            TextFont {
                font_size: FontSize::Px(font),
                ..default()
            },
            TextColor(theme.text_dim),
            StatusTextMarker,
        ));
    });
}

/// 每帧根据 Conversation / ProviderInfo / TokenUsage 更新状态栏文本。
fn update_status_text(
    conv: Res<Conversation>,
    info: Res<ProviderInfo>,
    tokens: Res<TokenUsage>,
    theme: Res<Theme>,
    mut q: Query<&mut Text, With<StatusTextMarker>>,
) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let provider_label = if info.id.is_empty() {
        "未配置 provider".to_string()
    } else {
        format!("{} / {}", info.id, info.model)
    };
    let status_label = match conv.status {
        ConversationStatus::Idle => "就绪".to_string(),
        ConversationStatus::Thinking => "思考中…".to_string(),
        ConversationStatus::Streaming => "生成中…".to_string(),
        ConversationStatus::ToolRunning => "执行工具…".to_string(),
        ConversationStatus::Confirming => "等待确认".to_string(),
        ConversationStatus::Aborting => "中断中…".to_string(),
        ConversationStatus::Error => "出错".to_string(),
    };
    let token_label = if tokens.total > 0 {
        format!("  ·  ↑ {} tokens", format_tokens(tokens.total))
    } else {
        String::new()
    };
    let _ = theme;
    text.0 = format!("{}  ·  {}{}", provider_label, status_label, token_label);
}

/// 收到 DoneMessage 时递增 token 估算。
fn track_token_usage(
    mut reader: MessageReader<DoneMessage>,
    conv: Res<Conversation>,
    mut tokens: ResMut<TokenUsage>,
) {
    if reader.read().next().is_none() {
        return;
    }
    // 粗略估算：当前助手回复的字数 / 4
    let estimated = (conv.current_assistant_text.len() as u64) / 4;
    tokens.total += estimated.max(1);
}

/// 格式化 token 数（k 单位）。
fn format_tokens(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}
