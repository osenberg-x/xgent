//! 工具调用卡片：内联在对话流中，展示工具名/参数/状态/结果。
//!
//! 订阅 [`ToolCallMessage`] 在消息列表中 spawn 卡片；
//! 订阅 [`ToolResultMessage`] 更新卡片状态与结果。
//! 折叠态只显示摘要，点击展开看详情。

use bevy::prelude::*;
use bevy::ui::ScrollPosition;

use xgent_agent::{ToolCallMessage, ToolResultMessage};
use xgent_settings::Localizer;

use crate::chat_panel::MessageListMarker;
use crate::i18n::tr;
use crate::theme::{Theme, space};

/// 工具调用卡片标记。
#[derive(Component, Default)]
pub struct ToolCardMarker {
    /// 工具 id（用于匹配 result）
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

/// 工具面板插件。
pub struct ToolPanelPlugin;

impl Plugin for ToolPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_tool_card, update_tool_result).after(xgent_agent::agent_loop::agent_poll_system));
    }
}

/// 订阅 ToolCallMessage，在消息列表中 spawn 工具调用卡片。
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
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::all(px(space::SM)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(4.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(space::XS),
                    ..default()
                },
                BackgroundColor(theme.panel),
                BorderColor::all(theme.border),
                ToolCardMarker {
                    tool_id: ev.tool_id.clone(),
                    expanded: false,
                },
            ))
            .with_children(|card| {
                // 工具名 + 参数摘要 + 状态标识
                card.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        column_gap: px(space::SM),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                ))
                .with_children(|header| {
                    // 工具图标
                    header.spawn((
                        Text::new("🔧"),
                        TextFont {
                            font_size: FontSize::Px(font),
                            ..default()
                        },
                        TextColor(theme.text),
                    ));
                    // 工具名
                    header.spawn((
                        Text::new(ev.tool_id.clone()),
                        TextFont {
                            font_size: FontSize::Px(font),
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
                            font_size: FontSize::Px(font),
                            ..default()
                        },
                        TextColor(theme.text_dim),
                    ));
                    // 状态标识（初始为"执行中"）
                    header.spawn((
                        Text::new(tr(&loc, "tool-running")),
                        TextFont {
                            font_size: FontSize::Px(font),
                            ..default()
                        },
                        TextColor(theme.accent),
                        ToolStatusLabelMarker,
                    ));
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
                        font_size: FontSize::Px(font - 2.0),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                    ToolResultTextMarker,
                ));
            });
        });
    }
}

/// 订阅 ToolResultMessage，更新对应卡片的状态与结果。
fn update_tool_result(
    mut reader: MessageReader<ToolResultMessage>,
    mut q_cards: Query<(&mut ToolCardMarker, &Children), With<ToolCardMarker>>,
    mut params: ParamSet<(
        Query<(&mut Text, &mut TextColor), With<ToolStatusLabelMarker>>,
        Query<(&mut Text, &mut Node), With<ToolResultTextMarker>>,
    )>,
    loc: Res<Localizer>,
) {
    for ev in reader.read() {
        for (mut card, children) in q_cards.iter_mut() {
            if card.tool_id != ev.tool_id {
                continue;
            }
            let status_label = if ev.success {
                tr(&loc, "tool-done")
            } else {
                tr(&loc, "tool-failed")
            };
            let status_color = if ev.success {
                Color::srgba(0.3, 0.8, 0.4, 1.0)
            } else {
                Color::srgba(0.9, 0.3, 0.3, 1.0)
            };
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
                        card.expanded = !card.expanded;
                        if card.expanded {
                            node.max_height = Val::Px(200.0);
                        } else {
                            node.max_height = Val::Px(0.0);
                        }
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
    // 回退：取 JSON 的前 50 字符
    let s = input.to_string();
    if s.len() > 50 {
        format!("{}…", &s[..47])
    } else {
        s
    }
}
