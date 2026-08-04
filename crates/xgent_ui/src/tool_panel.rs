//! 工具调用时间线：内联在对话流中，展示工具名/参数/状态/结果。
//!
//! v2 重构：从独立卡片改为"嵌入式时间线"节点，左侧带图标节点 +
//! 连接线，体现 agent 执行流程序列。

use bevy::prelude::*;
use bevy::ui::ScrollPosition;

use xgent_agent::{ToolCallMessage, ToolResultMessage};
use xgent_settings::Localizer;

use crate::chat_panel::MessageListMarker;
use crate::i18n::tr;
use crate::theme::{Theme, space};

/// 工具调用时间线节点标记。
#[derive(Component, Default)]
pub struct ToolCardMarker {
    /// 工具调用 id
    pub tool_call_id: String,
    /// 工具 id（展示用）
    pub tool_id: String,
    /// 是否展开结果详情
    pub expanded: bool,
}

/// 工具卡片状态文本节点标记。
#[derive(Component, Default)]
pub struct ToolStatusLabelMarker;

/// 工具卡片结果文本节点标记。
#[derive(Component, Default)]
pub struct ToolResultTextMarker;

/// 工具卡片状态点（dot）标记。
#[derive(Component, Default)]
pub struct ToolStatusDotMarker;

/// 工具卡片 head（点击 toggle 展开/折叠）。
#[derive(Component, Default)]
pub struct ToolCardHeadMarker;

/// 工具卡片折叠行标记。
#[derive(Component, Default)]
pub struct ToolFoldMarker;

/// 工具面板插件。
pub struct ToolPanelPlugin;
impl Plugin for ToolPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                spawn_tool_card,
                update_tool_result,
                handle_tool_card_click,
                apply_tool_card_visibility,
            )
                .after(xgent_agent::agent_loop::agent_poll_system),
        );
    }
}

/// 订阅 ToolCallMessage，在消息列表中 spawn 时间线节点。
fn spawn_tool_card(
    mut reader: MessageReader<ToolCallMessage>,
    q_list: Query<Entity, With<MessageListMarker>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    mut commands: Commands,
) {
    let Ok(list) = q_list.single() else {
        return;
    };
    let font = theme.font_size;
    for ev in reader.read() {
        let summary = format_tool_summary(&ev.tool_id, &ev.input);
        commands.entity(list).with_children(|p| {
            // 时间线行：图标节点 + 卡片体
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    column_gap: px(space::MD),
                    ..default()
                },
            ))
            .with_children(|tl| {
                // 时间线图标节点（左侧，带连接线效果）
                tl.spawn((
                    Node {
                        width: px(28.0),
                        height: px(28.0),
                        border_radius: BorderRadius::all(px(6.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(1.0)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(theme.panel),
                    BorderColor::all(theme.border),
                    Text::new("🔧"),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                ));
                // 卡片体
                tl.spawn((
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::ZERO,
                        padding: UiRect::all(px(space::SM)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(8.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(space::XS),
                        ..default()
                    },
                    BackgroundColor(theme.panel),
                    BorderColor::all(theme.border),
                    ToolCardMarker {
                        tool_call_id: ev.tool_call_id.clone(),
                        tool_id: ev.tool_id.clone(),
                        expanded: false,
                    },
                ))
                .with_children(|card| {
                    // head：工具名 + 参数摘要 + 状态药丸
                    card.spawn((
                        Button,
                        Node {
                            width: Val::Percent(100.0),
                            flex_direction: FlexDirection::Row,
                            column_gap: px(space::SM),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        ToolCardHeadMarker,
                    ))
                    .with_children(|header| {
                        // 工具名
                        header.spawn((
                            Text::new(ev.tool_id.clone()),
                            TextFont {
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(theme.text),
                        ));
                        // 参数摘要
                        header.spawn((
                            Node {
                                flex_grow: 1.0,
                                ..default()
                            },
                            Text::new(summary),
                            TextFont {
                                font_size: FontSize::Px(11.5),
                                ..default()
                            },
                            TextColor(theme.text_dim),
                        ));
                        // 状态药丸（elevated 底 + dot + 标签）
                        header.spawn((
                            Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: px(space::XS),
                                padding: UiRect::horizontal(px(space::SM)),
                                border_radius: BorderRadius::all(px(16.0)),
                                ..default()
                            },
                            BackgroundColor(theme.elevated),
                        ))
                        .with_children(|pill| {
                            pill.spawn((
                                Node {
                                    width: px(6.0),
                                    height: px(6.0),
                                    border_radius: BorderRadius::all(px(3.0)),
                                    ..default()
                                },
                                BackgroundColor(theme.st_running),
                                ToolStatusDotMarker,
                            ));
                            pill.spawn((
                                Text::new(tr(&loc, "tool-running")),
                                TextFont {
                                    font_size: FontSize::Px(11.0),
                                    ..default()
                                },
                                TextColor(theme.text_dim),
                                ToolStatusLabelMarker,
                            ));
                        });
                    });
                    // 结果区域（初始隐藏）
                    card.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            overflow: Overflow::clip_y(),
                            max_height: Val::Px(0.0),
                            ..default()
                        },
                        ScrollPosition::default(),
                        Text::new(String::new()),
                        TextFont {
                            font_size: FontSize::Px(font - 1.5),
                            ..default()
                        },
                        TextColor(theme.text_dim),
                        ToolResultTextMarker,
                    ));
                    // fold 行
                    card.spawn((
                        Button,
                        Node {
                            width: Val::Percent(100.0),
                            padding: UiRect::all(px(space::XS)),
                            border: UiRect::top(px(1.0)),
                            ..default()
                        },
                        BorderColor::all(theme.line),
                        Text::new(String::new()),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme.text_muted),
                        ToolFoldMarker,
                    ));
                });
            });
        });
    }
}

/// 订阅 ToolResultMessage，更新对应卡片。
fn update_tool_result(
    mut reader: MessageReader<ToolResultMessage>,
    mut q_cards: Query<(&mut ToolCardMarker, &Children), With<ToolCardMarker>>,
    mut params: ParamSet<(
        Query<(&mut Text, &mut TextColor), With<ToolStatusLabelMarker>>,
        Query<(&mut Text, &mut Node), With<ToolResultTextMarker>>,
        Query<&mut BackgroundColor, With<ToolStatusDotMarker>>,
        Query<&mut Text, With<ToolFoldMarker>>,
    )>,
    loc: Res<Localizer>,
    theme: Res<Theme>,
) {
    for ev in reader.read() {
        for (mut card, children) in q_cards.iter_mut() {
            if card.tool_call_id != ev.tool_call_id {
                continue;
            }
            let (status_label, status_color, dot_color) = if ev.denied {
                (
                    tr(&loc, "tool-denied"),
                    theme.st_deny,
                    BackgroundColor(theme.st_deny),
                )
            } else if ev.is_error {
                (
                    tr(&loc, "tool-failed"),
                    theme.st_fail,
                    BackgroundColor(theme.st_fail),
                )
            } else {
                (
                    tr(&loc, "tool-done"),
                    theme.st_ok,
                    BackgroundColor(theme.st_ok),
                )
            };
            let line_count = ev.output.lines().count();
            let fold_text = crate::i18n::tr_with(
                &loc,
                "tool-fold-result",
                &[("lines", line_count.to_string().into())],
            );
            card.expanded = true;
            {
                let mut q_status = params.p0();
                for child in children.iter() {
                    if let Ok((mut text, mut color)) = q_status.get_mut(child) {
                        text.0 = status_label.clone();
                        color.0 = status_color;
                    }
                }
            }
            {
                let mut q_result = params.p1();
                for child in children.iter() {
                    if let Ok((mut text, mut node)) = q_result.get_mut(child) {
                        text.0 = ev.output.clone();
                        node.max_height = Val::Px(200.0);
                    }
                }
            }
            {
                let mut q_dot = params.p2();
                for child in children.iter() {
                    if let Ok(mut bg) = q_dot.get_mut(child) {
                        *bg = dot_color;
                    }
                }
            }
            {
                let mut q_fold = params.p3();
                for child in children.iter() {
                    if let Ok(mut text) = q_fold.get_mut(child) {
                        text.0 = fold_text.clone();
                    }
                }
            }
            break;
        }
    }
}

/// 格式化工具调用的参数摘要。
fn format_tool_summary(tool_id: &str, input: &serde_json::Value) -> String {
    match tool_id {
        "read_file" | "ReadFile" => {
            if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
                return path.to_string();
            }
        }
        "write_file" | "WriteFile" => {
            if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
                return path.to_string();
            }
        }
        "search_files" | "SearchFiles" => {
            if let Some(pattern) = input.get("pattern").and_then(|v| v.as_str()) {
                return format!("\"{}\"", pattern);
            }
        }
        "run_command" | "RunCommand" => {
            if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
                return cmd.to_string();
            }
        }
        _ => {}
    }
    let s = input.to_string();
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > 50 {
        let truncated: String = chars[..47].iter().collect();
        format!("{truncated}…")
    } else {
        s
    }
}
/// 处理工具卡片 head / fold 点击：toggle `expanded`。
fn handle_tool_card_click(
    q_head: Query<(&Interaction, &ChildOf), (With<ToolCardHeadMarker>, Changed<Interaction>)>,
    q_fold: Query<(&Interaction, &ChildOf), (With<ToolFoldMarker>, Changed<Interaction>)>,
    mut q_cards: Query<&mut ToolCardMarker>,
    q_children: Query<&Children>,
) {
    for (interaction, parent) in q_head.iter().chain(q_fold.iter()) {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if let Ok(card_children) = q_children.get(parent.0) {
            if let Ok(mut card) = q_cards.get_mut(parent.0) {
                card.expanded = !card.expanded;
            }
            let _ = card_children;
        }
    }
}

/// 据 `ToolCardMarker.expanded` 切换结果区显隐 + fold 文本。
///
/// 优化：仅处理 `expanded` 状态变化的卡片（Changed<ToolCardMarker>）。
fn apply_tool_card_visibility(
    q_cards: Query<(&ToolCardMarker, &Children), Changed<ToolCardMarker>>,
    mut q_result: Query<&mut Node, With<ToolResultTextMarker>>,
    mut q_fold: Query<&mut Text, With<ToolFoldMarker>>,
    loc: Res<Localizer>,
) {
    for (card, children) in q_cards.iter() {
        for child in children.iter() {
            if let Ok(mut node) = q_result.get_mut(child) {
                node.max_height = if card.expanded {
                    Val::Px(200.0)
                } else {
                    Val::Px(0.0)
                };
            }
            if let Ok(mut text) = q_fold.get_mut(child) {
                if !text.0.is_empty() {
                    let prefix = if card.expanded { "▾" } else { "▸" };
                    let label_key = if card.expanded {
                        "tool-fold-result"
                    } else {
                        "tool-unfold-result"
                    };
                    let lines = text
                        .0
                        .split_whitespace()
                        .find(|s| s.parse::<usize>().is_ok())
                        .unwrap_or("0");
                    let new_text = crate::i18n::tr_with(
                        &loc,
                        label_key,
                        &[("lines", lines.to_string().into())],
                    );
                    text.0 = format!("{prefix} {new_text}");
                }
            }
        }
    }
}
