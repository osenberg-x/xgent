//! 确认弹窗：订阅 [`ConfirmRequestMessage`]，弹窗展示工具调用与 diff，用户决策发 [`ConfirmDecisionMessage`]。
//!
//! 对齐 ui-prototype.html §4.3 modal 结构：head（确认执行 + ✕）/ body（工具名 + 路径 + diff 区增删色）
//! / foot（拒绝 + 允许按钮）。有 diff（old/new 均有）时展示增删行，否则展示 summary 文本。

use bevy::prelude::*;
use xgent_agent::{ConfirmDecisionMessage, ConfirmRequestMessage};
use xgent_settings::Localizer;
use xgent_tools::confirm::ConfirmDecision;

use crate::i18n::tr;
use crate::theme::{Theme, space};

/// 确认弹窗根节点标记。
#[derive(Component, Default)]
pub struct ConfirmDialogMarker;

/// 确认弹窗插件。
pub struct ConfirmDialogPlugin;

impl Plugin for ConfirmDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (show_on_request, hide_on_decision, handle_confirm_keyboard)
                .after(xgent_agent::agent_loop::agent_poll_system),
        );
    }
}

/// 便捷：f32 → Val::Px
fn px(v: f32) -> Val {
    Val::Px(v)
}

/// diff 行的类型（增/删/上下文）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum DiffKind {
    Add,
    Del,
    Context,
}

/// 一行 diff（kind + 文本）。
struct DiffLine {
    kind: DiffKind,
    text: String,
}

/// 简单行级 diff：求公共前缀与后缀，中间旧行标 Del、新行标 Add。
///
/// 无需外部依赖，MVP 足够。复杂 diff（跨行移动）留待 P1。
fn line_diff(old: &str, new: &str) -> Vec<DiffLine> {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    // 公共前缀
    let mut prefix = 0;
    while prefix < old_lines.len() && prefix < new_lines.len() && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }
    // 公共后缀
    let mut suffix = 0;
    while suffix < old_lines.len() - prefix
        && suffix < new_lines.len() - prefix
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let mut out = Vec::new();
    // 前缀上下文
    for i in 0..prefix {
        out.push(DiffLine { kind: DiffKind::Context, text: old_lines[i].into() });
    }
    // 中间：先删后增
    for i in prefix..old_lines.len() - suffix {
        out.push(DiffLine { kind: DiffKind::Del, text: old_lines[i].into() });
    }
    for i in prefix..new_lines.len() - suffix {
        out.push(DiffLine { kind: DiffKind::Add, text: new_lines[i].into() });
    }
    // 后缀上下文
    for i in old_lines.len() - suffix..old_lines.len() {
        out.push(DiffLine { kind: DiffKind::Context, text: old_lines[i].into() });
    }
    out
}

/// 收到 ConfirmRequestMessage 时弹出确认窗口。
fn show_on_request(
    mut commands: Commands,
    mut reader: MessageReader<ConfirmRequestMessage>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    q_dialog: Query<Entity, With<ConfirmDialogMarker>>,
) {
    let Some(req) = reader.read().next() else {
        return;
    };
    let req = &req.0;
    // 已存在弹窗则先移除（MVP 同时只有一个确认请求，重建即可）
    if let Ok(existing) = q_dialog.single() {
        commands.entity(existing).despawn();
    }
    let font = theme.font_size;
    let mono = font - 1.5;
    let path = req.input["path"].as_str().unwrap_or(&req.tool_id);

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
                // z_index: ZIndex::Arbitrary(50),  // 用 GlobalZIndex 组件替代
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            GlobalZIndex(50),
            ConfirmDialogMarker,
        ))
        .with_children(|overlay| {
            // modal 容器
            overlay
                .spawn((
                    Node {
                        width: px(560.0),
                        max_height: Val::Percent(80.0),
                        flex_direction: FlexDirection::Column,
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(8.0)),
                        ..default()
                    },
                    BackgroundColor(theme.panel),
                    BorderColor::all(theme.border),
                ))
                .with_children(|modal| {
                    // modal-head：确认执行 + ✕
                    modal
                        .spawn((
                            Node {
                                width: Val::Percent(100.0),
                                flex_direction: FlexDirection::Row,
                                justify_content: JustifyContent::SpaceBetween,
                                align_items: AlignItems::Center,
                                padding: UiRect::all(px(space::MD)),
                                border: UiRect::bottom(px(1.0)),
                                ..default()
                            },
                            BackgroundColor(theme.bar),
                            BorderColor::all(theme.border),
                        ))
                        .with_children(|head| {
                            head.spawn((
                                Text::new(tr(&loc, "confirm-title")),
                                TextFont { font_size: FontSize::Px(font + 1.0), ..default() },
                                TextColor(theme.text),
                            ));
                            head.spawn((
                                Button,
                                Node {
                                    width: px(24.0),
                                    height: px(24.0),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    ..default()
                                },
                                Text::new("x"),
                                TextFont { font_size: FontSize::Px(font), ..default() },
                                TextColor(theme.text_dim),
                                ConfirmDenyMarker,
                            ));
                        });
                    // modal-body：工具名 + 路径 + diff
                    modal
                        .spawn((
                            Node {
                                width: Val::Percent(100.0),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(space::LG)),
                                row_gap: px(space::SM),
                                ..default()
                            },
                        ))
                        .with_children(|body| {
                            // 工具名 + 描述
                            body.spawn((
                                Text::new(format!(
                                    "{} {} {}",
                                    req.tool_id,
                                    tr(&loc, "confirm-will-write"),
                                    path
                                )),
                                TextFont { font_size: FontSize::Px(font), ..default() },
                                TextColor(theme.text_dim),
                            ));
                            // diff 区（若有 old/new）
                            if let (Some(old), Some(new)) = (&req.old_content, &req.new_content) {
                                body.spawn((
                                    Text::new(tr(&loc, "confirm-diff-label")),
                                    TextFont { font_size: FontSize::Px(font - 2.0), ..default() },
                                    TextColor(theme.text_dim),
                                ));
                                let lines = line_diff(old, new);
                                body.spawn((
                                    Node {
                                        width: Val::Percent(100.0),
                                        max_height: px(220.0),
                                        flex_direction: FlexDirection::Column,
                                        overflow: Overflow::clip_y(),
                                        padding: UiRect::vertical(px(space::SM)),
                                        border: UiRect::all(px(1.0)),
                                        border_radius: BorderRadius::all(px(4.0)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(0.07, 0.08, 0.10, 1.0)),
                                    BorderColor::all(theme.border),
                                    ScrollPosition::default(),
                                ))
                                .with_children(|diff| {
                                    for line in &lines {
                                        let (prefix, color) = match line.kind {
                                            DiffKind::Add => ("+ ", theme.st_ok),
                                            DiffKind::Del => ("- ", theme.st_fail),
                                            DiffKind::Context => ("  ", theme.text_dim),
                                        };
                                        diff.spawn((
                                            Node {
                                                width: Val::Percent(100.0),
                                                padding: UiRect::horizontal(px(space::MD)),
                                                ..default()
                                            },
                                            Text::new(format!("{prefix}{}", line.text)),
                                            TextFont {
                                                font_size: FontSize::Px(mono),
                                                ..default()
                                            },
                                            TextColor(color),
                                        ));
                                    }
                                });
                            } else {
                                // 无 diff：展示 summary
                                body.spawn((
                                    Text::new(req.summary.clone()),
                                    TextFont { font_size: FontSize::Px(font), ..default() },
                                    TextColor(theme.text),
                                ));
                            }
                        });
                    // modal-foot：拒绝 + 允许按钮
                    modal
                        .spawn((
                            Node {
                                width: Val::Percent(100.0),
                                flex_direction: FlexDirection::Row,
                                justify_content: JustifyContent::FlexEnd,
                                column_gap: px(space::SM),
                                padding: UiRect::all(px(space::MD)),
                                border: UiRect::top(px(1.0)),
                                ..default()
                            },
                            BackgroundColor(theme.bar),
                            BorderColor::all(theme.border),
                        ))
                        .with_children(|foot| {
                            foot.spawn((
                                Button,
                                Node {
                                    padding: UiRect::all(px(space::SM)),
                                    border: UiRect::all(px(1.0)),
                                    border_radius: BorderRadius::all(px(4.0)),
                                    ..default()
                                },
                                BackgroundColor(theme.st_fail),
                                BorderColor::all(theme.st_fail),
                                Text::new(format!("{} (Esc)", tr(&loc, "confirm-deny"))),
                                TextFont { font_size: FontSize::Px(font), ..default() },
                                TextColor(Color::WHITE),
                                ConfirmDenyMarker,
                            ));
                            foot.spawn((
                                Button,
                                Node {
                                    padding: UiRect::all(px(space::SM)),
                                    border: UiRect::all(px(1.0)),
                                    border_radius: BorderRadius::all(px(4.0)),
                                    ..default()
                                },
                                BackgroundColor(theme.accent),
                                BorderColor::all(theme.accent),
                                Text::new(format!("{} (Enter)", tr(&loc, "confirm-allow"))),
                                TextFont { font_size: FontSize::Px(font), ..default() },
                                TextColor(Color::WHITE),
                                ConfirmAllowMarker,
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

/// 用户点决策按钮（或 head ✕）时发 ConfirmDecisionMessage 并关闭弹窗。
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

/// 弹窗激活时的键盘决策：Esc→拒绝、Enter→允许。
///
/// 对齐原型 modal-foot 按钮标注 (Esc)/(Enter)。弹窗 overlay 为全局遮罩
/// （GlobalZIndex 50），激活时独占键盘。Esc 的 chat.abort 冲突由
/// [`shortcuts::handle_hotkey_triggers`] 查弹窗存在性跳过解决。
fn handle_confirm_keyboard(
    mut reader: MessageReader<bevy::input::keyboard::KeyboardInput>,
    q_dialog: Query<Entity, With<ConfirmDialogMarker>>,
    mut commands: Commands,
    mut writer: MessageWriter<ConfirmDecisionMessage>,
) {
    if q_dialog.single().is_err() {
        return;
    }
    for ev in reader.read() {
        if ev.state != bevy::input::ButtonState::Pressed {
            continue;
        }
        use bevy::input::keyboard::KeyCode as K;
        match ev.key_code {
            K::Escape => {
                writer.write(ConfirmDecisionMessage {
                    decision: ConfirmDecision::Deny,
                });
                // despawn 由 hide_on_decision 负责，但键盘路径无 dialog 查询
                // 此处也需 despawn——hide_on_decision 只在按钮 Changed 时触发
                if let Ok(dialog) = q_dialog.single() {
                    commands.entity(dialog).despawn();
                }
                return;
            }
            K::Enter => {
                writer.write(ConfirmDecisionMessage {
                    decision: ConfirmDecision::Allow,
                });
                if let Ok(dialog) = q_dialog.single() {
                    commands.entity(dialog).despawn();
                }
                return;
            }
            _ => {}
        }
    }
}
