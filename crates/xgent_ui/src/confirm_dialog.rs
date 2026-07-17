//! 确认弹窗：订阅 [`ConfirmRequestMessage`]，弹窗展示工具调用，用户决策发 [`ConfirmDecisionMessage`]。
//!
//! MVP 用一个 overlay 节点 + 允许/拒绝两个按钮。决策经 [`ConfirmDecisionMessage`] 回 agent。

use bevy::prelude::*;
use xgent_agent::{ConfirmDecisionMessage, ConfirmRequestMessage};
use xgent_settings::Localizer;
use xgent_tools::confirm::ConfirmDecision;

use crate::i18n::tr;
use crate::theme::{Theme, space};

/// 确认弹窗根节点标记。
#[derive(Component, Default)]
pub struct ConfirmDialogMarker;

/// 确认弹窗文本节点标记。
#[derive(Component, Default)]
pub struct ConfirmDialogTextLabel;

/// 确认弹窗插件。
pub struct ConfirmDialogPlugin;

impl Plugin for ConfirmDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (show_on_request, hide_on_decision).after(xgent_agent::agent_loop::agent_poll_system),
        );
    }
}

/// 收到 ConfirmRequestMessage 时弹出确认窗口。
fn show_on_request(
    mut commands: Commands,
    mut reader: MessageReader<ConfirmRequestMessage>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    q_dialog: Query<Entity, With<ConfirmDialogMarker>>,
    q_label: Query<Entity, With<ConfirmDialogTextLabel>>,
) {
    let req = match reader.read().next() {
        Some(ev) => ev,
        None => return,
    };
    // 若已存在弹窗则更新文本，否则新建
    let text = if req.0.summary.is_empty() {
        crate::i18n::tr_with(
            &loc,
            "confirm-write-file",
            &[("path", req.0.tool_id.clone())],
        )
    } else {
        req.0.summary.clone()
    };

    if let Ok(existing) = q_dialog.single() {
        if let Ok(label) = q_label.single() {
            commands.entity(label).insert(Text::new(text));
        }
        let _ = existing;
        return;
    }

    let font = theme.font_size;
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(0.0),
                left: px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
            ConfirmDialogMarker,
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    padding: UiRect::all(px(space::LG)),
                    border: UiRect::all(px(1.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(space::MD),
                    min_width: px(320.0),
                    ..default()
                },
                BackgroundColor(theme.panel),
                BorderColor::all(theme.border),
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new(text),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text),
                    ConfirmDialogTextLabel,
                ));
                card.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(space::MD),
                    ..default()
                },))
                    .with_children(|btns| {
                        btns.spawn((
                            Button,
                            Node {
                                padding: UiRect::all(px(space::SM)),
                                ..default()
                            },
                            BackgroundColor(theme.accent),
                            Text::new(tr(&loc, "confirm-allow")),
                            TextFont {
                                font_size: FontSize::Px(font),
                                ..default()
                            },
                            TextColor(theme.text),
                            ConfirmAllowMarker,
                        ));
                        btns.spawn((
                            Button,
                            Node {
                                padding: UiRect::all(px(space::SM)),
                                ..default()
                            },
                            BackgroundColor(theme.bar),
                            Text::new(tr(&loc, "confirm-deny")),
                            TextFont {
                                font_size: FontSize::Px(font),
                                ..default()
                            },
                            TextColor(theme.text),
                            ConfirmDenyMarker,
                        ));
                    });
            });
        });
}

/// 决策按钮标记。
#[derive(Component, Default)]
pub struct ConfirmAllowMarker;
#[derive(Component, Default)]
pub struct ConfirmDenyMarker;

/// 用户点决策按钮时发 ConfirmDecisionMessage 并关闭弹窗。
fn hide_on_decision(
    q_dialog: Query<Entity, With<ConfirmDialogMarker>>,
    q_allow: Query<&Interaction, (With<ConfirmAllowMarker>, Changed<Interaction>)>,
    q_deny: Query<&Interaction, (With<ConfirmDenyMarker>, Changed<Interaction>)>,
    mut commands: Commands,
    mut writer: MessageWriter<ConfirmDecisionMessage>,
) {
    let Ok(dialog) = q_dialog.single() else {
        return;
    };
    let mut close = |decision: ConfirmDecision| {
        writer.write(ConfirmDecisionMessage { decision });
        commands.entity(dialog).despawn();
    };
    for i in q_allow.iter() {
        if *i == Interaction::Pressed {
            close(ConfirmDecision::Allow);
        }
    }
    for i in q_deny.iter() {
        if *i == Interaction::Pressed {
            close(ConfirmDecision::Deny);
        }
    }
}
