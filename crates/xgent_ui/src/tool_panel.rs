//! 工具调用卡片：内联在对话流中，展示工具名/参数/状态/结果。
//!
//! 订阅 [`ToolCallMessage`] 在消息列表中 spawn 卡片；
//! 订阅 [`ToolResultMessage`] 更新卡片状态与结果。
//! 折叠态只显示摘要，点击展开看详情。

use bevy::prelude::*;
use bevy::ui::ScrollPosition;

use xgent_agent::{ConfirmRequestMessage, ToolCallMessage, ToolResultMessage};
use xgent_settings::Localizer;

use crate::chat_panel::MessageListMarker;
use crate::i18n::tr;
use crate::theme::{Theme, space};

/// 工具调用卡片标记。
#[derive(Component, Default)]
pub struct ToolCardMarker {
    /// LLM 返回的 tool_call_id（唯一，用于匹配 result/confirm）
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

/// 工具卡片折叠行标记（▾ 结果：N 行，点击 toggle 展开）。
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
                update_tool_pending,
                update_tool_result,
                handle_tool_card_click,
                apply_tool_card_visibility,
            )
                .chain()
                .after(xgent_agent::agent_loop::agent_poll_system),
        );
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
                ToolCardMarker {
                    tool_call_id: ev.tool_call_id.clone(),
                    tool_id: ev.tool_id.clone(),
                    expanded: false,
                },
            ))
            .with_children(|card| {
                // head：图标 + 工具名 + 参数摘要 + 状态点 + 状态标签（点击 toggle 展开）
                card.spawn((
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        column_gap: px(space::SM),
                        align_items: AlignItems::Center,
                        padding: UiRect::all(px(space::SM)),
                        ..default()
                    },
                    BackgroundColor(theme.bar),
                    ToolCardHeadMarker,
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
                    // 状态点（dot，初始 running 色）
                    header.spawn((
                        Node {
                            width: px(8.0),
                            height: px(8.0),
                            border_radius: BorderRadius::all(px(4.0)),
                            ..default()
                        },
                        BackgroundColor(theme.st_running),
                        ToolStatusDotMarker,
                    ));
                    // 状态标签（初始"执行中"）
                    header.spawn((
                        Text::new(tr(&loc, "tool-running")),
                        TextFont {
                            font_size: FontSize::Px(font),
                            ..default()
                        },
                        TextColor(theme.text_dim),
                        ToolStatusLabelMarker,
                    ));
                });
                // 结果区域（初始隐藏，max_height=0）
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
                // fold 行（▾ 结果：N 行，初始隐藏，结果到达显示）
                card.spawn((
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::all(px(space::SM)),
                        border: UiRect::top(px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme.bar),
                    BorderColor::all(theme.border),
                    Text::new(String::new()),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                    ToolFoldMarker,
                ));
            });
        });
    }
}

/// 订阅 ToolResultMessage，更新对应卡片：状态点/标签/结果/fold 行。
///
/// 结果到达时设 `expanded=true`（默认展开，不 toggle），显示结果区，
/// 填充 fold 行「▾ 结果：N 行」，状态点变 ok/fail/deny 色。
///
/// 三态：`denied` → 已拒绝（⊘ 灰色，不展开结果）；`is_error` → 失败（✗ 红色）；
/// 否则 → 完成（✓ 绿色）。对齐 ui-design.md §4.2。
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
) {
    for ev in reader.read() {
        for (mut card, children) in q_cards.iter_mut() {
            if card.tool_call_id != ev.tool_call_id {
                continue;
            }
            // 三态判定：denied 优先于 is_error
            let (status_label, status_color, dot_color) = if ev.denied {
                (
                    tr(&loc, "tool-denied"),
                    theme_st_deny(),
                    BackgroundColor(theme_st_deny()),
                )
            } else if ev.is_error {
                (
                    tr(&loc, "tool-failed"),
                    Color::srgba(0.9, 0.3, 0.3, 1.0),
                    BackgroundColor(theme_st_fail()),
                )
            } else {
                (
                    tr(&loc, "tool-done"),
                    Color::srgba(0.3, 0.8, 0.4, 1.0),
                    BackgroundColor(theme_st_ok()),
                )
            };
            let line_count = ev.output.lines().count();
            // denied 态不展开结果区（输出是固定文案，无查看价值）
            let fold_text = if ev.denied {
                String::new()
            } else {
                format!("▾ 结果：{} 行 · 点击折叠", line_count)
            };
            card.expanded = !ev.denied;
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
                        // denied 态隐藏结果区
                        node.max_height = if ev.denied { Val::Px(0.0) } else { Val::Px(200.0) };
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

/// 订阅 ConfirmRequestMessage，把对应卡片切到「待确认」态（⏸ 黄色）。
///
/// 事件顺序：ToolCall（spawn 卡片，running）→ ConfirmRequest（切 pending）
/// → ToolResult（切 done/failed/denied）。对齐 ui-prototype.html §4.2。
fn update_tool_pending(
    mut reader: MessageReader<ConfirmRequestMessage>,
    mut q_cards: Query<(&mut ToolCardMarker, &Children), With<ToolCardMarker>>,
    mut params: ParamSet<(
        Query<(&mut Text, &mut TextColor), With<ToolStatusLabelMarker>>,
        Query<&mut BackgroundColor, With<ToolStatusDotMarker>>,
    )>,
    loc: Res<Localizer>,
) {
    for ev in reader.read() {
        for (card, children) in q_cards.iter_mut() {
            // ConfirmRequest 无 tool_call_id，按 tool_id 匹配最近一张同 tool_id 的卡片
            if card.tool_id != ev.0.tool_id {
                continue;
            }
            let label = tr(&loc, "tool-pending");
            let color = theme_st_pending();
            {
                let mut q_status = params.p0();
                for child in children.iter() {
                    if let Ok((mut text, mut tc)) = q_status.get_mut(child) {
                        text.0 = label.clone();
                        tc.0 = color;
                    }
                }
            }
            {
                let mut q_dot = params.p1();
                for child in children.iter() {
                    if let Ok(mut bg) = q_dot.get_mut(child) {
                        *bg = BackgroundColor(theme_st_pending());
                    }
                }
            }
            // pending 匹配首张即可（MVP 工具串行执行，同时只有一张 pending）
            break;
        }
    }
}

/// 便捷：从全局 Theme 取 fail 色（避免 update_tool_result 加 Theme 参数致 query 冲突）。
fn theme_st_fail() -> Color {
    Color::srgba(0.88, 0.34, 0.34, 1.0)
}
/// 便捷：从全局 Theme 取 ok 色。
fn theme_st_ok() -> Color {
    Color::srgba(0.31, 0.78, 0.47, 1.0)
}
/// 便捷：取 deny 色（灰色，对齐 theme.st_deny）。
fn theme_st_deny() -> Color {
    Color::srgba(0.53, 0.53, 0.53, 1.0)
}
/// 便捷：取 pending 色（黄色，对齐 theme.st_pending）。
fn theme_st_pending() -> Color {
    Color::srgba(0.88, 0.70, 0.25, 1.0)
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
/// 处理工具卡片 head / fold 点击：toggle `expanded`。
///
/// head 与 fold 都可点击 toggle；`apply_tool_card_visibility` 据 expanded 应用显隐。
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
        // head/fold 的父节点是卡片本体
        if let Ok(card_children) = q_children.get(parent.0) {
            // 卡片本体即 parent.0，直接查 ToolCardMarker
            if let Ok(mut card) = q_cards.get_mut(parent.0) {
                card.expanded = !card.expanded;
            }
            let _ = card_children;
        }
    }
}

/// 据 `ToolCardMarker.expanded` 切换结果区显隐 + fold 文本。
fn apply_tool_card_visibility(
    q_cards: Query<(&ToolCardMarker, &Children)>,
    mut q_result: Query<&mut Node, With<ToolResultTextMarker>>,
    mut q_fold: Query<&mut Text, With<ToolFoldMarker>>,
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
                    // 结果已到达（fold 文本非空），据 expanded 切文案
                    let expanded_text = text.0.starts_with("▾");
                    if card.expanded && !expanded_text {
                        text.0 = text.0.replacen("▸", "▾", 1);
                        text.0 = text.0.replacen("展开", "折叠", 1);
                    } else if !card.expanded && expanded_text {
                        text.0 = text.0.replacen("▾", "▸", 1);
                        text.0 = text.0.replacen("折叠", "展开", 1);
                    }
                }
            }
        }
    }
}
