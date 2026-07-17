//! 对话面板：消息列表（流式渲染）+ 输入框 + 中断。
//!
//! 订阅 agent 的 [`DeltaMessage`] 累加到当前助手消息节点；[`DoneMessage`] 时把当前消息
//! 固化为历史消息节点并清空当前；输入框发送语义由 [`xui::ChatInput`] 处理，
//! 提交时转发为 [`UserInputMessage`]。
//!
//! 用户消息右对齐（`bubble_user`），助手消息左对齐（`bubble_assistant`）。
//! 消息列表自动滚动到底部。
//!
//! 消息列表 MVP 用简单列容器 + 每条消息一个文本节点；大列表虚拟化留待后续接入 xui::VirtualList。

use bevy::input_focus::AutoFocus;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::ScrollPosition;

use xgent_agent::{
    Conversation, ConversationStatus, DeltaMessage, DoneMessage, ErrorMessage, SteeringMessage,
    UserInputMessage,
};
use xui::input::{ChatInput, ChatInputSubmitted};
use xui::mouse_wheel_scroll::MouseWheelScrolled;

use crate::layout::ChatPanelMarker;
use crate::theme::{Theme, space};

/// 历史消息容器（消息列表，可滚动）。
#[derive(Component, Default)]
pub struct MessageListMarker;

/// 当前正在流式累加的助手消息文本节点。
#[derive(Component, Default)]
pub struct CurrentAssistantText;

/// 对话输入框实体标记。
#[derive(Component, Default)]
pub struct ChatInputMarker;

/// 输入框边框标记（用于忙时变色）。
#[derive(Component, Default)]
pub struct ChatInputBorderMarker;

/// 对话面板关键实体句柄（启动时填充）。
#[derive(Resource, Default)]
pub struct ChatPanelEntities {
    pub message_list: Option<Entity>,
    pub current_text: Option<Entity>,
    pub input: Option<Entity>,
}

/// 消息列表"贴底跟随"状态：`true` 时随内容增长滚到底，用户上滚后置 `false`、
/// 重新滚回底部时恢复 `true`。避免流式累加期间抢夺用户正在阅读的历史位置。
#[derive(Resource, Default, Clone, Copy)]
pub struct MessageListStickBottom(pub bool);

/// 对话面板插件。
pub struct ChatPanelPlugin;

impl Plugin for ChatPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatPanelEntities>()
            // 初始即贴底：首屏与首轮对话自动跟到底，用户上滚后改由
            // maintain_stick_bottom 维护此标志。
            .insert_resource(MessageListStickBottom(true))
            .add_systems(Startup, spawn_chat_panel.after(crate::layout::spawn_layout))
            .add_systems(
                Update,
                (
                    accumulate_delta,
                    finalize_on_done,
                    on_error,
                    forward_input_submission,
                    spawn_user_message,
                    update_input_border,
                )
                    .after(xgent_agent::agent_loop::agent_poll_system),
            )
            // 自动滚动与贴底状态维护放在布局之后（PostLayout），此时
            // content_size 为本帧最新值。顺序：先 maintain 再 auto_scroll。
            //
            // maintain 只在本帧有滚轮事件时（MouseWheelScrolled 为真）刷新贴底
            // 标志——读用户滚轮提交后的 ScrollPosition 判断是否离开底部；无滚轮
            // 时保持原值，避免内容增长导致 max_offset 变大时误判离开底部。
            // auto_scroll 随后只在 stick 为真时推到底，不会覆盖用户手动滚动。
            .add_systems(
                PostUpdate,
                (maintain_stick_bottom, auto_scroll_to_bottom)
                    .after(bevy::ui::UiSystems::PostLayout)
                    .chain(),
            );
    }
}

/// 启动时在对话主区内 spawn 消息列表 + 输入框。
fn spawn_chat_panel(
    mut commands: Commands,
    q_panel: Query<Entity, With<ChatPanelMarker>>,
    theme: Res<Theme>,
    mut entities: ResMut<ChatPanelEntities>,
) {
    let Ok(panel) = q_panel.single() else {
        return;
    };
    let font = theme.font_size;
    let font_size = FontSize::Px(font);

    let current_text = commands
        .spawn((
            Node {
                max_width: Val::Percent(80.0),
                padding: UiRect::all(px(space::SM)),
                border_radius: BorderRadius::all(px(6.0)),
                ..default()
            },
            BackgroundColor(theme.bubble_assistant),
            Text::new(String::new()),
            TextFont {
                font_size,
                ..default()
            },
            TextColor(theme.text),
            CurrentAssistantText,
        ))
        .id();

    let message_list = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                // overflow 关键：ScrollPosition 只在 Scroll 轴生效（见
                // ui_node.rs ScrollPosition 文档）。clip 仅裁剪渲染、不影响布局，内容
                // 仍会撑大容器；Hidden 才"影响布局再裁剪"。故：
                //   y: Scroll  → 让 ScrollPosition 生效，纵向滚动
                //   x: Hidden  → 影响布局+裁剪，防宽内容撑破挤占文件面板
                min_height: Val::ZERO,
                min_width: Val::ZERO,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                flex_basis: Val::ZERO,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(space::SM)),
                overflow: Overflow {
                    x: OverflowAxis::Hidden,
                    y: OverflowAxis::Scroll,
                },
                row_gap: px(space::SM),
                ..default()
            },
            ScrollPosition::default(),
            MessageListMarker,
        ))
        .add_child(current_text)
        .id();

    let input_entity = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_height: px(60.0),
                max_height: px(200.0),
                flex_shrink: 0.0,
                padding: UiRect::all(px(space::SM)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(theme.bg),
            BorderColor::all(theme.border),
            TextFont {
                font_size,
                ..default()
            },
            TextColor(theme.text_dim),
            bevy::text::TextCursorStyle::default(),
            EditableText {
                allow_newlines: true,
                ..default()
            },
            ChatInput::multiline(),
            AutoFocus,
            ChatInputMarker,
            ChatInputBorderMarker,
        ))
        .id();

    commands
        .entity(panel)
        .add_child(message_list)
        .add_child(input_entity);

    entities.message_list = Some(message_list);
    entities.current_text = Some(current_text);
    entities.input = Some(input_entity);
}

/// 用户提交输入时，在消息列表中 spawn 用户消息气泡（右对齐）。
fn spawn_user_message(
    mut reader: MessageReader<ChatInputSubmitted>,
    entities: Res<ChatPanelEntities>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Some(list) = entities.message_list else {
        return;
    };
    let Some(current) = entities.current_text else {
        return;
    };
    let font = theme.font_size;
    for ev in reader.read() {
        if ev.text.is_empty() {
            continue;
        }
        // 在当前助手节点之前插入用户消息气泡
        commands.entity(list).with_children(|p| {
            // 右对齐行容器
            p.spawn((Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            },))
                .with_children(|row| {
                    row.spawn((
                        Node {
                            max_width: Val::Percent(80.0),
                            padding: UiRect::all(px(space::SM)),
                            border_radius: BorderRadius::all(px(6.0)),
                            ..default()
                        },
                        BackgroundColor(theme.bubble_user),
                        Text::new(ev.text.clone()),
                        TextFont {
                            font_size: FontSize::Px(font),
                            ..default()
                        },
                        TextColor(theme.text),
                    ));
                });
        });
        // 把当前助手节点移到列表末尾（在用户消息之后）
        commands.entity(list).add_child(current);
        // 清空当前助手节点（清除上一轮的错误文本或残留内容）
        commands.entity(current).insert(Text::new(String::new()));
    }
}

/// 订阅 DeltaMessage，累加到当前助手消息节点。
fn accumulate_delta(
    mut reader: MessageReader<DeltaMessage>,
    mut q: Query<&mut Text, With<CurrentAssistantText>>,
) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    for ev in reader.read() {
        text.0.push_str(&ev.text);
    }
}

/// Done 时把当前助手消息固化为历史副本节点，并清空当前节点。
fn finalize_on_done(
    mut reader: MessageReader<DoneMessage>,
    entities: Res<ChatPanelEntities>,
    q: Query<&Text, With<CurrentAssistantText>>,
    mut commands: Commands,
    theme: Res<Theme>,
) {
    let Some(current) = entities.current_text else {
        return;
    };
    let Some(list) = entities.message_list else {
        return;
    };
    if reader.read().next().is_none() {
        return;
    }
    let Ok(text) = q.get(current) else {
        return;
    };
    let content = text.0.clone();
    if content.is_empty() {
        return;
    }
    let font = theme.font_size;
    // 在消息列表插入历史副本（左对齐行容器 + 助手气泡）
    commands.entity(list).with_children(|p| {
        p.spawn((Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::FlexStart,
            ..default()
        },))
            .with_children(|row| {
                row.spawn((
                    Node {
                        max_width: Val::Percent(80.0),
                        padding: UiRect::all(px(space::SM)),
                        border_radius: BorderRadius::all(px(6.0)),
                        ..default()
                    },
                    BackgroundColor(theme.bubble_assistant),
                    Text::new(content),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text),
                ));
            });
    });
    // 清空当前节点
    commands.entity(current).insert(Text::new(String::new()));
}

/// 出错时把错误信息写到当前助手消息节点。
fn on_error(
    mut reader: MessageReader<ErrorMessage>,
    q: Query<Entity, With<CurrentAssistantText>>,
    mut commands: Commands,
    theme: Res<Theme>,
) {
    let Ok(entity) = q.single() else {
        return;
    };
    for ev in reader.read() {
        let prefix = match ev.kind {
            xgent_core::chat::ErrorKind::NotConfigured => "⚠ [未配置] ",
            xgent_core::chat::ErrorKind::AuthFailed => "⚠ [鉴权失败] ",
            xgent_core::chat::ErrorKind::Network => "⚠ [网络] ",
            xgent_core::chat::ErrorKind::StreamParse => "⚠ [解析] ",
            xgent_core::chat::ErrorKind::ProviderError => "⚠ ",
        };
        commands.entity(entity).insert((
            Text::new(format!("{prefix}{}", ev.message)),
            TextColor(theme.accent),
        ));
    }
}

/// 订阅 xui 的 ChatInputSubmitted，转发为 agent 的 UserInputMessage 或 SteeringMessage。
///
/// Idle/Error 时发 UserInputMessage（新对话）；
/// Streaming/ToolRunning/Confirming/Thinking 时发 SteeringMessage（注入到当前对话，MVP 不中断工具）。
pub fn forward_input_submission(
    mut reader: MessageReader<ChatInputSubmitted>,
    mut user_writer: MessageWriter<UserInputMessage>,
    mut steering_writer: MessageWriter<SteeringMessage>,
    conv: Res<Conversation>,
) {
    for ev in reader.read() {
        if ev.text.is_empty() {
            continue;
        }
        if conv.status == ConversationStatus::Idle || conv.status == ConversationStatus::Error {
            user_writer.write(UserInputMessage {
                text: ev.text.clone(),
            });
        } else {
            // agent 执行中：发 Steering，注入到当前对话
            steering_writer.write(SteeringMessage {
                text: ev.text.clone(),
            });
        }
    }
}

/// 维护 [`MessageListStickBottom`]：用户上滚离开底部时置 `false`，重新滚回底部
/// 附近时恢复 `true`。在 `PostUpdate` 的 `PostLayout` 之后、且在
/// [`auto_scroll_to_bottom`] **之前**运行（见插件 `.chain()`），读到的是用户
/// 滚轮在 `Update` 阶段提交后的 `ScrollPosition`——尚未被 `auto_scroll` 推回。
///
/// **仅在本帧有滚轮事件时刷新**（通过 [`MouseWheelScrolled`] 标志判断），无滚轮
/// 时保持 `stick` 原值不动。这样内容增长导致 `max_offset` 变大时不会误判为
/// "用户离开底部"——`auto_scroll` 仍能正常把列表推到新底部。
///
/// 判定阈值 `STICK_THRESHOLD`：`scroll.y >= max_offset - threshold` 即视为贴底。
/// `max_offset` 取本帧布局后的 `content_size`/`size`。
fn maintain_stick_bottom(
    mut stick: ResMut<MessageListStickBottom>,
    mut scrolled: ResMut<MouseWheelScrolled>,
    q_scroll: Query<(&ScrollPosition, &ComputedNode), With<MessageListMarker>>,
) {
    // 无滚轮事件 → 不动 stick，避免内容增长误判离开底部。
    if !scrolled.0 {
        return;
    }
    // 消费标志，下帧自动归 false（send_scroll_events 只在滚轮时置真）。
    scrolled.0 = false;

    let Ok((scroll, node)) = q_scroll.single() else {
        return;
    };
    let scale = node.inverse_scale_factor;
    let max_offset = ((node.content_size.y - node.size.y) * scale).max(0.0);
    const STICK_THRESHOLD: f32 = 32.0;
    let at_bottom = scroll.0.y >= max_offset - STICK_THRESHOLD || max_offset == 0.0;
    // 贴底 → 置真；离开 → 置假。只在状态变化时写，避免无谓变更检测。
    if stick.0 != at_bottom {
        stick.0 = at_bottom;
    }
}

/// 消息列表"贴底跟随"：当 [`MessageListStickBottom`] 为真时，把 `ScrollPosition.y`
/// 推到最大偏移；用户上滚后标志为假，则保留其手动滚动位置，不抢夺阅读位置。
///
/// 滚动位置单位为逻辑像素，`ComputedNode` 的 `size`/`content_size` 为物理像素，
/// 须乘 `inverse_scale_factor` 转换。直接读列表容器自身的 `content_size`（由
/// `ui_layout_system` 测量得到），无需手算子节点高度累加——后者会漏掉 padding、
/// gap 的布局结果与缩放，导致 clamp 后 `scroll_position` 停在 0、内容从底部
/// 被裁剪而不可见（详见 Bevy `examples/ui/scroll_and_overflow/scroll.rs` 的惯用法）。
fn auto_scroll_to_bottom(
    stick: Res<MessageListStickBottom>,
    mut q_scroll: Query<(&mut ScrollPosition, &ComputedNode), With<MessageListMarker>>,
) {
    let Ok((mut scroll, node)) = q_scroll.single_mut() else {
        return;
    };
    // 物理像素 → 逻辑像素
    let scale = node.inverse_scale_factor;
    let content_height = node.content_size.y * scale;
    let viewport_height = node.size.y * scale;
    // 内容超过视口时滚到底部；不足时回 0（避免残留偏移）。
    let max_offset = (content_height - viewport_height).max(0.0);
    if stick.0 {
        scroll.0.y = max_offset;
    }
}

/// 根据 Conversation 状态更新输入框边框颜色（忙时变色）。
fn update_input_border(
    conv: Res<Conversation>,
    theme: Res<Theme>,
    mut q: Query<&mut BorderColor, With<ChatInputBorderMarker>>,
) {
    let Ok(mut border) = q.single_mut() else {
        return;
    };
    let is_busy =
        conv.status != ConversationStatus::Idle && conv.status != ConversationStatus::Error;
    if is_busy {
        border.set_all(theme.accent);
    } else {
        border.set_all(theme.border);
    }
}
