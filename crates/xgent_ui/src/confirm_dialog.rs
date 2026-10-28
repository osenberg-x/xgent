//! 确认弹窗：订阅 [`ConfirmRequestMessage`]，弹窗展示工具调用与 diff，用户决策发 [`ConfirmDecisionMessage`]。
//!
//! 对齐 ui-prototype.html §4.3 modal 结构：head（确认执行 + ✕）/ body（工具名 + 路径 + diff 区增删色）
//! / foot（拒绝 + 允许按钮）。有 diff（old/new 均有）时展示增删行，否则展示 summary 文本。

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;
use xgent_agent::{ConfirmDecisionMessage, ConfirmRequestMessage};
use xgent_settings::Localizer;
use xgent_tools::confirm::ConfirmDecision;

use crate::fonts::{mono_text_weighted, ui_text};
use crate::i18n::tr;
use crate::theme::{Theme, px, radius, size, space, type_scale};

/// 确认弹窗根节点标记。
#[derive(Component, Default)]
pub struct ConfirmDialogMarker;

/// 确认弹窗插件。
///
/// **组合约束**（R1 记录）：diff 行渲染依赖 `UiFonts`（等宽构造器）——单独
/// 注册本插件且无 `xgent_app` 注入 `UiFonts` 时，收到 `ConfirmRequestMessage`
/// 会因缺资源 panic。真实 app 组合不受影响。
pub struct ConfirmDialogPlugin;

impl Plugin for ConfirmDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                show_on_request,
                hide_on_decision,
                handle_confirm_keyboard,
                close_on_settled,
            )
                .after(xgent_agent::agent_loop::agent_poll_system),
        );
    }
}

use crate::diff::{DiffKind, line_diff};

/// 收到 ConfirmRequestMessage 时弹出确认窗口。
///
/// 弹窗为模态（z 由 `ModalStack` 按栈深派生），spawn 时把 `InputFocus` 移到
/// 弹窗、决策后还原到 chat 输入框：否则焦点滞留在聊天输入框（AutoFocus），
/// handle_confirm_keyboard 的 EditableText 守卫会让 Enter 永远无法确认
/// （Esc 有独立放行路径，见该函数文档）。
fn show_on_request(
    mut commands: Commands,
    mut reader: MessageReader<ConfirmRequestMessage>,
    theme: Res<Theme>,
    fonts: Res<crate::fonts::UiFonts>,
    loc: Res<Localizer>,
    q_dialog: Query<Entity, With<ConfirmDialogMarker>>,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    q_chat_input: Query<Entity, With<crate::chat_panel::ChatInputMarker>>,
    mut modal_push: Option<MessageWriter<crate::modal::ModalPush>>,
    mut modal_pop: Option<MessageWriter<crate::modal::ModalPop>>,
) {
    let Some(req) = reader.read().next() else {
        return;
    };
    let req = &req.0;
    // 已存在弹窗则先移除（MVP 同时只有一个确认请求，重建即可）
    if let Ok(existing) = q_dialog.single() {
        if let Some(modal_pop) = modal_pop.as_mut() {
            modal_pop.write(crate::modal::ModalPop {
                id: Some("confirm"),
            });
        }
        commands.entity(existing).despawn();
    }
    let path = req.input["path"].as_str().unwrap_or(&req.tool_id);
    // 焦点移到弹窗（挂 ConfirmDialogMarker 的 overlay 实体），
    // Enter 守卫（非 EditableText 即放行）随之放行
    let dialog_focus = commands
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
            BackgroundColor(theme.overlay),
            // require_markers 下遮罩必须自带 Pickable，否则点击会穿透到下层。
            bevy::picking::Pickable::default(),
            ConfirmDialogMarker,
        ))
        .id();
    let opener_input = q_chat_input.single().ok();
    if let Some(modal_push) = modal_push.as_mut() {
        modal_push.write(crate::modal::ModalPush {
            id: "confirm",
            entity: dialog_focus,
            dismiss: crate::modal::Dismiss::NotDismissable,
            opener_focus: opener_input,
        });
    }
    focus.set(dialog_focus, bevy::input_focus::FocusCause::Navigated);
    commands.entity(dialog_focus).with_children(|overlay| {
        // modal 容器
        overlay
            .spawn((
                Node {
                    width: px(size::OVERLAY_CONFIRM_W),
                    max_height: Val::Percent(80.0),
                    flex_direction: FlexDirection::Column,
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(radius::PANEL)),
                    ..default()
                },
                BackgroundColor(theme.elevated),
                BorderColor::all(theme.border),
            ))
            .with_children(|modal| {
                // modal-head：icon 块（st_pending_bg/st_pending）+ 确认执行 + ✕
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
                        BackgroundColor(theme.elevated),
                        BorderColor::all(theme.border),
                    ))
                    .with_children(|head| {
                        // 左组：icon 块 + 标题
                        head.spawn((Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: px(space::SM),
                            ..default()
                        },))
                            .with_children(|left| {
                                left.spawn((
                                    Node {
                                        width: px(36.0),
                                        height: px(36.0),
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::Center,
                                        border_radius: BorderRadius::all(px(radius::CTRL)),
                                        flex_shrink: 0.0,
                                        ..default()
                                    },
                                    BackgroundColor(theme.st_pending_bg),
                                    ui_text(
                                        "!",
                                        type_scale::BODY,
                                        590,
                                        theme.st_pending,
                                        type_scale::line_height::UI,
                                    ),
                                ));
                                left.spawn((
                                    Node {
                                        flex_shrink: 1.0,
                                        min_width: Val::ZERO,
                                        overflow: Overflow::clip(),
                                        ..default()
                                    },
                                    // v8 M3-T5：标题过长时省略（审计 C3）。
                                    crate::fonts::ellipsis_text(
                                        tr(&loc, "confirm-title").to_string(),
                                        type_scale::H3,
                                        590,
                                        theme.text,
                                        type_scale::line_height::UI,
                                    ),
                                ));
                            });
                        head.spawn((
                            Button,
                            Hovered::default(),
                            Node {
                                width: px(24.0),
                                height: px(24.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            ui_text(
                                "x",
                                type_scale::BODY,
                                400,
                                theme.text_dim,
                                type_scale::line_height::UI,
                            ),
                            ConfirmDenyMarker,
                        ));
                    });
                // modal-body：工具名 + 路径 + diff
                modal
                    .spawn((Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(space::LG)),
                        row_gap: px(space::SM),
                        ..default()
                    },))
                    .with_children(|body| {
                        // 工具名 + 动作
                        body.spawn(ui_text(
                            format!("{} {}", req.tool_id, tr(&loc, "confirm-will-write")),
                            type_scale::BODY,
                            400,
                            theme.text_dim,
                            type_scale::line_height::UI,
                        ));
                        // 路径（长路径中段省略，保留末两段——v8 M3-T5）
                        body.spawn((
                            Node {
                                flex_shrink: 1.0,
                                min_width: Val::ZERO,
                                overflow: Overflow::clip(),
                                ..default()
                            },
                            crate::fonts::ellipsis_path_text(
                                path,
                                type_scale::BODY_SM,
                                400,
                                theme.text_muted,
                                type_scale::line_height::UI,
                            ),
                        ));
                        // diff 区（若有 old/new）
                        if let (Some(old), Some(new)) = (&req.old_content, &req.new_content) {
                            body.spawn(ui_text(
                                tr(&loc, "confirm-diff-label"),
                                type_scale::CAPTION,
                                400,
                                theme.text_faint,
                                type_scale::line_height::UI,
                            ));
                            let lines = line_diff(old, new);
                            // v8 M3-T2：diff 区改用 ScrollFrame（overflow 由构造器定）。
                            // 文本行是子节点，随内容一起滚动。
                            let mut diff_area = xui::ScrollFrame::vertical();
                            diff_area.content_sized();
                            diff_area.style().max_height = px(220.0);
                            diff_area.style().padding = UiRect::vertical(px(space::SM));
                            diff_area.style().border = UiRect::all(px(1.0));
                            diff_area.style().border_radius = BorderRadius::all(px(radius::CTRL));
                            body.spawn((
                                diff_area,
                                BackgroundColor(theme.code_bg),
                                BorderColor::all(theme.border),
                            ))
                            .with_children(|diff| {
                                for line in &lines {
                                    let (prefix, color, bg) = match line.kind {
                                        DiffKind::Add => ("+ ", theme.st_ok, theme.st_ok_bg),
                                        DiffKind::Del => ("- ", theme.st_fail, theme.st_fail_bg),
                                        DiffKind::Context => ("  ", theme.text_dim, Color::NONE),
                                    };
                                    diff.spawn((
                                        Node {
                                            width: Val::Percent(100.0),
                                            padding: UiRect::horizontal(px(space::MD)),
                                            ..default()
                                        },
                                        BackgroundColor(bg),
                                        mono_text_weighted(
                                            &fonts,
                                            format!("{prefix}{}", line.text),
                                            type_scale::CODE,
                                            400,
                                            color,
                                            type_scale::line_height::CTRL,
                                        ),
                                    ));
                                }
                            });
                        } else {
                            // 无 diff：展示 summary
                            body.spawn(ui_text(
                                req.summary.clone(),
                                type_scale::BODY,
                                400,
                                theme.text,
                                type_scale::line_height::UI,
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
                        BackgroundColor(theme.elevated),
                        BorderColor::all(theme.border),
                    ))
                    .with_children(|foot| {
                        // 拒绝 = ghost（透明底 + border）
                        foot.spawn((
                            Button,
                            Hovered::default(),
                            Node {
                                padding: UiRect {
                                    left: px(space::LG),
                                    right: px(space::LG),
                                    top: px(space::SM + 1.0),
                                    bottom: px(space::SM + 1.0),
                                },
                                border_radius: BorderRadius::all(px(radius::CTRL)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            BorderColor::all(Color::NONE),
                            ui_text(
                                format!("{} (Esc)", tr(&loc, "confirm-deny")),
                                type_scale::BODY_SM,
                                510,
                                theme.text_muted,
                                type_scale::line_height::CTRL,
                            ),
                            ConfirmDenyMarker,
                        ));
                        // 确认 = accent 底白字
                        foot.spawn((
                            Button,
                            Hovered::default(),
                            Node {
                                padding: UiRect {
                                    left: px(space::LG),
                                    right: px(space::LG),
                                    top: px(space::SM + 1.0),
                                    bottom: px(space::SM + 1.0),
                                },
                                border_radius: BorderRadius::all(px(radius::CTRL)),
                                ..default()
                            },
                            BackgroundColor(theme.accent),
                            BorderColor::all(Color::NONE),
                            ui_text(
                                format!("{} (Enter)", tr(&loc, "confirm-allow")),
                                type_scale::BODY_SM,
                                510,
                                theme.accent_text,
                                type_scale::line_height::CTRL,
                            ),
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
    q_allow: Query<(), (With<ConfirmAllowMarker>, Added<Pressed>)>,
    q_deny: Query<(), (With<ConfirmDenyMarker>, Added<Pressed>)>,
    mut commands: Commands,
    mut writer: MessageWriter<ConfirmDecisionMessage>,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    q_chat_input: Query<Entity, With<crate::chat_panel::ChatInputMarker>>,
    mut modal_pop: Option<MessageWriter<crate::modal::ModalPop>>,
) {
    let Ok(dialog) = q_dialog.single() else {
        return;
    };
    let input_entity = q_chat_input.single().ok();
    let mut close = |decision: ConfirmDecision| {
        writer.write(ConfirmDecisionMessage { decision });
        close_dialog(
            &mut commands,
            dialog,
            &mut focus,
            input_entity,
            &mut modal_pop,
        );
    };
    for _ in q_allow.iter() {
        close(ConfirmDecision::Allow);
    }
    for _ in q_deny.iter() {
        close(ConfirmDecision::Deny);
    }
}

/// 关闭弹窗并把焦点还原到 chat 输入框。
///
/// 所有 despawn 弹窗的路径（按钮/键盘）都必须走这里：弹窗 spawn 时焦点
/// 被移到弹窗实体上（见 [`show_on_request`]），不还原的话输入框失焦、
/// 输入框的 Enter 提交与守卫判断都会错乱。
fn close_dialog(
    commands: &mut Commands,
    dialog: Entity,
    focus: &mut bevy::input_focus::InputFocus,
    input_entity: Option<Entity>,
    modal_pop: &mut Option<MessageWriter<crate::modal::ModalPop>>,
) {
    if let Some(modal_pop) = modal_pop.as_mut() {
        modal_pop.write(crate::modal::ModalPop {
            id: Some("confirm"),
        });
    }
    if let Some(input) = input_entity {
        focus.set(input, bevy::input_focus::FocusCause::Navigated);
    } else {
        focus.clear();
    }
    commands.entity(dialog).despawn();
}

/// 请求已终结（确认超时 / 确认被取消）时关闭弹窗并还原焦点。
///
/// 超时后 executor 直接返回错误工具结果，弹窗却不会自己消失：它是模态遮罩，
/// 继续压在界面上会让「允许」按钮接在一个已被丢弃的 receiver 上（点了没反应），
/// 并独占键盘焦点。判据是「本帧有工具结果到达且弹窗仍在」——内层 tool-call
/// 循环串行执行，等待确认期间不会有别的工具结果进来，故结果到达即代表该
/// 请求已终结。用户自己点按钮的路径已在 [`hide_on_decision`] 里关闭弹窗，
/// 那时查询取不到实体，天然不重复。
fn close_on_settled(
    mut commands: Commands,
    mut reader: MessageReader<xgent_agent::ToolResultMessage>,
    q_dialog: Query<Entity, With<ConfirmDialogMarker>>,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    q_chat_input: Query<Entity, With<crate::chat_panel::ChatInputMarker>>,
    mut modal_pop: Option<MessageWriter<crate::modal::ModalPop>>,
) {
    if reader.read().next().is_none() {
        return;
    }
    let Ok(dialog) = q_dialog.single() else {
        return;
    };
    let input_entity = q_chat_input.single().ok();
    close_dialog(
        &mut commands,
        dialog,
        &mut focus,
        input_entity,
        &mut modal_pop,
    );
}

/// 弹窗激活时的键盘决策：Esc→拒绝、Enter→允许。
///
/// 对齐原型 modal-foot 按钮标注 (Esc)/(Enter)。弹窗 overlay 为全局遮罩
/// （`Dismiss::NotDismissable`），激活时独占键盘；`shortcuts` 在模态栈非空时
/// 整体让位，Esc 不会穿透成 `chat.abort`。
///
/// 守卫只约束 Enter（KeyboardInput 是全局消息，弹窗存在时输入框/终端的
/// Enter 也会到达，不拦截会把"回车发消息/提交命令"静默变成"允许工具
/// 调用"，绕过人工确认闸门）：
/// - 焦点在任意 `EditableText` 输入框（chat/palette/settings）→ 跳过；
///   （正常路径弹窗 spawn 时焦点已移到弹窗，见 [`show_on_request`]）
/// - 右侧分屏为终端视图（终端独占键盘、InputFocus 被清空）→ 跳过。
///
/// **Esc 恒放行**：Deny 是安全方向——误拒一次工具调用无害，误放行才
/// 有害，故 Esc 不做焦点守卫（终端里 Esc 退出终端视图的语义由
/// handle_hotkey_triggers 的弹窗存在性检查让位）。
fn handle_confirm_keyboard(
    mut reader: MessageReader<bevy::input::keyboard::KeyboardInput>,
    q_dialog: Query<Entity, With<ConfirmDialogMarker>>,
    q_editable: Query<(), With<bevy::text::EditableText>>,
    registry: Res<crate::panels::PanelRegistry>,
    mut commands: Commands,
    mut writer: MessageWriter<ConfirmDecisionMessage>,
    // InputFocus 的唯一访问（读 get / 写 set 同参数；Res + ResMut 同资源是 B0002 冲突）
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    q_chat_input: Query<Entity, With<crate::chat_panel::ChatInputMarker>>,
    modal: Option<Res<crate::modal::ModalStack>>,
    mut modal_pop: Option<MessageWriter<crate::modal::ModalPop>>,
) {
    let Ok(dialog) = q_dialog.single() else {
        return;
    };
    // 只有确认弹窗在栈顶时才处理键盘（v8 M1：栈是唯一权威）。
    let on_top = modal
        .as_deref()
        .and_then(|stack| stack.top())
        .is_some_and(|layer| layer.id == "confirm");
    if !on_top {
        return;
    }
    let input_entity = q_chat_input.single().ok();
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
                close_dialog(
                    &mut commands,
                    dialog,
                    &mut focus,
                    input_entity,
                    &mut modal_pop,
                );
                return;
            }
            K::Enter => {
                // 焦点在文本输入框：Enter 语义属于输入框，不触发决策
                let in_editable = focus.get().is_some_and(|f| q_editable.contains(f));
                let terminal_active = registry.is_visible(crate::panels::ids::TERMINAL);
                if in_editable || terminal_active {
                    continue;
                }
                writer.write(ConfirmDecisionMessage {
                    decision: ConfirmDecision::Allow,
                });
                close_dialog(
                    &mut commands,
                    dialog,
                    &mut focus,
                    input_entity,
                    &mut modal_pop,
                );
                return;
            }
            _ => {}
        }
    }
}
