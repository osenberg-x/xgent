//! Transcript 渲染侧（M6.5 T1/T2/T5）：按块序列差量渲染消息、工具卡与通知。
//!
//! P5：消息列表接 `xui::VirtualList`（不等高）。渲染窗口 = 可见区间 ∪ 当前轮；
//! 滚出窗口的块被 despawn，用上下占位块撑住总高度。

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::ScrollPosition;
use bevy::ui_widgets::Button;
use xgent_agent::{Conversation, ConversationStatus};
use xgent_settings::Localizer;
use xui::virtual_list::{
    VirtualList, VirtualListItem, build_measured_offsets, compute_visible_range_measured,
};

use crate::chat_panel::{CopyActionMarker, MessageListMarker};
use crate::fonts::{UiFonts, ui_text};
use crate::i18n::{tr, tr_with};
use crate::kit::{HoverTint, IconAssets};
use crate::markdown::parse_markdown;
use crate::markdown_render::spawn_markdown;
use crate::theme::{Theme, size, space, type_scale};
use crate::tool_panel::spawn_tool_card_entity;
use crate::transcript::{BlockRef, NoticeKind, Transcript};

/// 流式光标字符。
const STREAM_CURSOR: char = '▋';

/// 渲染块实体上的顺序键。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TranscriptBlockKey {
    /// Transcript 代次（clear/restore 后旧实体作废）。
    pub generation: u64,
    /// 轮次。
    pub turn_index: u64,
    /// 块下标。
    pub block_index: usize,
}

/// 虚拟列表顶部 / 底部占位块。
#[derive(Component, Debug, Clone, Copy)]
pub struct TranscriptSpacer {
    /// 是否为顶部占位。
    pub top: bool,
}

/// 助手消息行的子实体定位。
#[derive(Component, Debug, Clone, Copy)]
struct AssistantParts {
    body: Entity,
    text: Option<Entity>,
    copy: Entity,
}

/// 助手正文文本节点标记。
#[derive(Component, Debug, Clone, Copy)]
struct AssistantTextBody;

/// 助手消息已渲染 Markdown 的标记。
#[derive(Component, Debug, Clone, Copy)]
struct AssistantMarkdownDone;

/// 当前虚拟窗口（first 变化才重建，避免流式每帧重建）。
#[derive(Resource, Default)]
struct RenderWindow {
    generation: u64,
    first: usize,
}

/// Transcript 渲染插件。
pub struct TranscriptRenderPlugin;

impl Plugin for TranscriptRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderWindow>().add_systems(
            Update,
            render_transcript.after(crate::transcript_write::TranscriptWriteSet),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_transcript(
    transcript: Res<Transcript>,
    conv: Res<Conversation>,
    time: Res<Time>,
    mut commands: Commands,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    fonts: Res<UiFonts>,
    icons: Res<IconAssets>,
    mut window: ResMut<RenderWindow>,
    mut q_list: Query<
        (
            Entity,
            &mut VirtualList,
            Option<&ScrollPosition>,
            Option<&ComputedNode>,
        ),
        With<MessageListMarker>,
    >,
    q_children: Query<&Children>,
    q_nodes: Query<(
        Entity,
        Option<&TranscriptBlockKey>,
        Option<&TranscriptSpacer>,
    )>,
    q_assistant: Query<(&AssistantParts, Option<&AssistantMarkdownDone>)>,
    mut q_text_copy: ParamSet<(Query<&mut Text>, Query<&mut CopyActionMarker>)>,
) {
    let Ok((list, mut vlist, scroll, node)) = q_list.single_mut() else {
        return;
    };
    let generation = transcript.generation();
    let busy = conv.status != ConversationStatus::Idle && conv.status != ConversationStatus::Error;
    let show_cursor = busy && (time.elapsed().as_secs_f64() % 1.0) < 0.5;

    // 虚拟项序号 → 块下标（ToolResult 没有独立视觉，不占虚拟项）。
    let render_items: Vec<usize> = transcript
        .blocks()
        .iter()
        .enumerate()
        .filter(|(_, block)| !matches!(block, BlockRef::ToolResult { .. }))
        .map(|(index, _)| index)
        .collect();
    vlist.item_count = render_items.len();

    let viewport_h = node.map(|node| node.size().y).unwrap_or(0.0);
    let scroll_y = scroll.map(|position| position.0.y).unwrap_or(0.0);
    let offsets = build_measured_offsets(render_items.len(), vlist.item_height, &vlist.measured);
    let (mut first, count) = if viewport_h > 0.0 {
        compute_visible_range_measured(&offsets, viewport_h, scroll_y, 4)
    } else {
        (0, render_items.len())
    };
    let active_start = transcript
        .current_turn()
        .and_then(|turn| {
            render_items
                .iter()
                .position(|&block| transcript.blocks()[block].turn_index() == turn)
        })
        .unwrap_or(render_items.len());
    first = first.min(active_start);

    let mut desired: Vec<usize> = (first..(first + count).min(render_items.len())).collect();
    desired.extend(active_start..render_items.len());
    desired.sort_unstable();
    desired.dedup();
    let desired_blocks: Vec<usize> = desired.iter().map(|&item| render_items[item]).collect();
    let desired_set: std::collections::HashSet<usize> = desired_blocks.iter().copied().collect();

    let rebuild = window.generation != generation || window.first != first;
    if rebuild {
        for (entity, key, spacer) in q_nodes.iter() {
            if key.is_some() || spacer.is_some() {
                commands.entity(entity).despawn();
            }
        }
        window.generation = generation;
        window.first = first;
    }

    // 现有块：despawn 出窗的；更新窗内的助手正文。
    let mut existing = std::collections::HashSet::new();
    if !rebuild {
        for (entity, key, _) in q_nodes.iter() {
            let Some(key) = key else {
                continue;
            };
            if key.generation != generation || !desired_set.contains(&key.block_index) {
                commands.entity(entity).despawn();
            } else {
                existing.insert(key.block_index);
            }
        }
    }

    for &block_index in &desired_blocks {
        let BlockRef::AssistantMessage {
            blocks, finalized, ..
        } = &transcript.blocks()[block_index]
        else {
            continue;
        };
        if !existing.contains(&block_index) {
            continue;
        }
        let Some(row) = q_nodes
            .iter()
            .find(|(_, key, _)| {
                key.is_some_and(|key| {
                    key.generation == generation && key.block_index == block_index
                })
            })
            .map(|(entity, _, _)| entity)
        else {
            continue;
        };
        let Ok((parts, markdown_done)) = q_assistant.get(row) else {
            continue;
        };
        let content = assistant_text(blocks);
        if let Ok(mut copy) = q_text_copy.p1().get_mut(parts.copy) {
            copy.text = content.clone();
        }
        if *finalized {
            if markdown_done.is_none() {
                if let Some(text) = parts.text {
                    commands.entity(text).despawn();
                }
                let markdown = spawn_markdown(
                    &mut commands,
                    &parse_markdown(&content),
                    &theme,
                    &fonts,
                    &loc,
                );
                commands.entity(parts.body).add_child(markdown);
                commands.entity(row).insert(AssistantMarkdownDone);
            }
        } else if let Some(text_entity) = parts.text
            && let Ok(mut text) = q_text_copy.p0().get_mut(text_entity)
        {
            let mut shown = content;
            if show_cursor {
                shown.push(STREAM_CURSOR);
            }
            if text.0 != shown {
                text.0 = shown;
            }
        }
    }

    // 新块：只渲染窗口内的。
    let mut new_entities = Vec::new();
    for &block_index in &desired_blocks {
        if existing.contains(&block_index) {
            continue;
        }
        let Some(entity) = spawn_block(
            &transcript,
            block_index,
            &mut commands,
            &theme,
            &fonts,
            &icons,
            &loc,
        ) else {
            continue;
        };
        commands.entity(entity).insert((
            TranscriptBlockKey {
                generation,
                turn_index: transcript.blocks()[block_index].turn_index(),
                block_index,
            },
            VirtualListItem {
                index: render_items
                    .iter()
                    .position(|&item| item == block_index)
                    .unwrap_or(0),
            },
        ));
        new_entities.push(entity);
    }

    if rebuild {
        let mut children = Vec::new();
        if first > 0 {
            let height = offsets.get(first).copied().unwrap_or(0.0);
            children.push(spawn_spacer(&mut commands, height, true));
        }
        children.extend(new_entities);
        let last_v = desired.last().map(|last| last + 1).unwrap_or(0);
        let bottom_offset = offsets.get(last_v).copied().unwrap_or(0.0);
        let bottom_height = (offsets.last().copied().unwrap_or(0.0) - bottom_offset).max(0.0);
        children.push(spawn_spacer(&mut commands, bottom_height, false));
        commands.entity(list).add_children(&children);
    } else if !new_entities.is_empty() {
        // 追加：插到底部占位块之前，保持顺序。
        let insert_at = q_children.get(list).ok().and_then(|children| {
            q_nodes
                .iter()
                .find(|(_, _, spacer)| spacer.is_some_and(|spacer| !spacer.top))
                .and_then(|(spacer, _, _)| children.iter().position(|child| child == spacer))
        });
        match insert_at {
            Some(index) => {
                commands.entity(list).insert_children(index, &new_entities);
            }
            None => {
                commands.entity(list).add_children(&new_entities);
            }
        }
    }
}

fn spawn_spacer(commands: &mut Commands, height: f32, top: bool) -> Entity {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(height.max(0.0)),
                flex_shrink: 0.0,
                ..default()
            },
            TranscriptSpacer { top },
        ))
        .id()
}

fn spawn_block(
    transcript: &Transcript,
    block_index: usize,
    commands: &mut Commands,
    theme: &Theme,
    fonts: &UiFonts,
    icons: &IconAssets,
    loc: &Localizer,
) -> Option<Entity> {
    let block = transcript.blocks().get(block_index)?;
    Some(match block {
        BlockRef::UserMessage { content, .. } => spawn_user_block(commands, content, theme, loc),
        BlockRef::AssistantMessage {
            blocks, finalized, ..
        } => spawn_assistant_row(
            commands,
            &assistant_text(blocks),
            *finalized,
            theme,
            fonts,
            loc,
        ),
        BlockRef::ToolCall {
            tool_call_id,
            tool_name,
            arguments,
            status,
            ..
        } => {
            let result = transcript.blocks().iter().find_map(|other| match other {
                BlockRef::ToolResult {
                    tool_call_id: id,
                    content,
                    is_error,
                    ..
                } if id == tool_call_id => Some((content.as_str(), *is_error)),
                _ => None,
            });
            spawn_tool_card_entity(
                commands,
                tool_call_id,
                tool_name,
                &serde_json::from_str(arguments).unwrap_or(serde_json::Value::Null),
                theme,
                fonts,
                icons,
                loc,
                *status,
                result.map(|(content, _)| content),
                result.map(|(_, is_error)| is_error).unwrap_or(false),
            )
        }
        BlockRef::CompressionNotice {
            tokens_before,
            tokens_after,
            ..
        } => {
            let before = crate::status_bar::format_tokens((*tokens_before).into());
            let after = crate::status_bar::format_tokens((*tokens_after).into());
            let text = tr_with(
                loc,
                "compaction-notice",
                &[("before", before), ("after", after)],
            );
            spawn_notice(commands, &text, NoticeKind::Info, theme)
        }
        BlockRef::Notice { text, kind, .. } => spawn_notice(commands, text, *kind, theme),
        BlockRef::ToolResult { .. } => return None,
    })
}

fn assistant_text(blocks: &[crate::transcript::ContentBlock]) -> String {
    blocks
        .iter()
        .map(|block| match block {
            crate::transcript::ContentBlock::Text(text) => text.clone(),
            crate::transcript::ContentBlock::Code { code, .. } => code.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn spawn_user_block(commands: &mut Commands, text: &str, theme: &Theme, loc: &Localizer) -> Entity {
    commands
        .spawn((Node {
            max_width: Val::Px(size::MSG_MAX_W),
            align_self: AlignSelf::Center,
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(space::MD),
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                Node {
                    width: Val::Px(28.0),
                    height: Val::Px(28.0),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(theme.icon_bg),
                ui_text(
                    tr(loc, "role-user"),
                    type_scale::BODY_SM,
                    590,
                    theme.text_dim,
                    type_scale::line_height::UI,
                ),
            ));
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::ZERO,
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|body| {
                body.spawn(ui_text(
                    tr(loc, "role-user"),
                    type_scale::CAPTION,
                    400,
                    theme.text,
                    type_scale::line_height::UI,
                ));
                body.spawn(ui_text(
                    text.to_string(),
                    theme.font_size,
                    400,
                    theme.text,
                    type_scale::line_height::for_text(text),
                ));
            });
        })
        .id()
}

fn spawn_assistant_row(
    commands: &mut Commands,
    content: &str,
    finalized: bool,
    theme: &Theme,
    fonts: &UiFonts,
    loc: &Localizer,
) -> Entity {
    let mut body_entity = Entity::PLACEHOLDER;
    let mut text_entity = None;
    let mut copy_entity = Entity::PLACEHOLDER;
    let row = commands
        .spawn((Node {
            max_width: Val::Px(size::MSG_MAX_W),
            align_self: AlignSelf::Center,
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(space::MD),
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                Node {
                    width: Val::Px(28.0),
                    height: Val::Px(28.0),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(theme.accent),
                ui_text(
                    "X",
                    type_scale::CAPTION,
                    590,
                    theme.accent_text,
                    type_scale::line_height::UI,
                ),
            ));
            let body = row
                .spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::ZERO,
                    flex_direction: FlexDirection::Column,
                    ..default()
                })
                .with_children(|body| {
                    body.spawn(ui_text(
                        tr(loc, "role-assistant"),
                        type_scale::BODY_SM,
                        510,
                        theme.text,
                        type_scale::line_height::UI,
                    ));
                    if !finalized {
                        let text = body
                            .spawn((
                                ui_text(
                                    content.to_string(),
                                    theme.font_size,
                                    400,
                                    theme.text_dim,
                                    type_scale::line_height::for_text(content),
                                ),
                                AssistantTextBody,
                            ))
                            .id();
                        text_entity = Some(text);
                    }
                })
                .id();
            body_entity = body;
            copy_entity = row
                .spawn((
                    Button,
                    Hovered::default(),
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(0.0),
                        right: Val::Px(0.0),
                        padding: UiRect::all(Val::Px(space::XS)),
                        border_radius: BorderRadius::all(Val::Px(crate::theme::radius::SMALL)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    HoverTint::ghost(theme),
                    CopyActionMarker {
                        text: content.to_string(),
                    },
                ))
                .id();
        })
        .id();
    if finalized {
        let markdown = spawn_markdown(commands, &parse_markdown(content), theme, fonts, loc);
        commands.entity(body_entity).add_child(markdown);
        commands.entity(row).insert(AssistantMarkdownDone);
    }
    commands.entity(row).insert(AssistantParts {
        body: body_entity,
        text: text_entity,
        copy: copy_entity,
    });
    row
}

fn spawn_notice(commands: &mut Commands, text: &str, kind: NoticeKind, theme: &Theme) -> Entity {
    let color = match kind {
        NoticeKind::Info => theme.text_muted,
        NoticeKind::Error => theme.accent,
        NoticeKind::Retry => theme.text_dim,
    };
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    padding: UiRect::all(Val::Px(space::XS)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(theme.elevated),
                ui_text(
                    text,
                    type_scale::MICRO,
                    400,
                    color,
                    type_scale::line_height::UI,
                ),
            ));
        })
        .id()
}
