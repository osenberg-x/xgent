//! 会话历史面板：列出历史会话，点击恢复。
//!
//! 作为 overlay 弹窗（类似设置面板），从 sessions 目录读取 JSONL 摘要。
//! 订阅 `SessionListMessage` 获取会话列表，发 `RestoreSessionMessage` 恢复。

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;

use xgent_agent::{ListSessionsMessage, RestoreSessionMessage, SessionListMessage, SessionSummary};
use xgent_settings::Localizer;

use crate::fonts::ui_text;
use crate::i18n::{tr, tr_with};
use crate::theme::{Theme, space, type_scale};

/// 抽屉/浮层 z 基准：历史抽屉占用 40，文件抽屉（M5-T6）同层共享。
///
/// 文件抽屉=40 遮罩 /41 面板；确认弹窗=50（最高，见 confirm_dialog）。
/// 会话历史面板状态（open/close）。
#[derive(Resource, Default)]
pub struct SessionHistoryState {
    pub open: bool,
}

/// 面板根节点标记。
#[derive(Component, Default)]
pub struct SessionHistoryOverlayMarker;

/// 单条会话项标记（携带会话 id）。
#[derive(Component)]
pub struct SessionItemMarker {
    pub session_id: String,
}

/// 恢复按钮标记（携带会话 id）。
#[derive(Component)]
pub struct SessionRestoreButtonMarker {
    pub session_id: String,
}

/// 关闭按钮标记。
#[derive(Component, Default)]
pub struct SessionHistoryCloseMarker;

/// 会话列表容器标记（虚拟列表）。
#[derive(Component, Default)]
pub struct SessionListMarker;

/// 会话列表占位块标记（撑出列表总高度）。
#[derive(Component, Default)]
pub struct SessionListSpacerMarker;

/// 会话列表空态文案标记。
#[derive(Component, Default)]
pub struct SessionListEmptyMarker;

/// 会话列表行标记（虚拟列表的行，携带数据下标）。
#[derive(Component)]
pub struct SessionListRow {
    /// 对应 `CachedSessionList.sessions` 的下标
    pub index: usize,
}

/// 会话行高（逻辑像素）。固定行高是虚拟化定位的前提。
const SESSION_ROW_H: f32 = 48.0;

/// 缓存的会话列表（用于渲染）。
#[derive(Resource, Default)]
pub struct CachedSessionList {
    pub sessions: Vec<SessionSummary>,
}

pub struct SessionHistoryPlugin;

impl Plugin for SessionHistoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SessionHistoryState>()
            .init_resource::<CachedSessionList>();
        // 面板注册表（幂等：宿主可能已装配）
        if !app.is_plugin_added::<crate::panels::PanelsPlugin>() {
            app.add_plugins(crate::panels::PanelsPlugin);
        }
        if !app.is_plugin_added::<crate::panels::visibility::VisibilityPlugin>() {
            app.add_plugins(crate::panels::visibility::VisibilityPlugin);
        }
        app.add_systems(
            Startup,
            spawn_session_history.after(crate::layout::spawn_layout),
        )
        .add_systems(Update, session_history_overlay_systems)
        .add_systems(
            Update,
            handle_session_list_results.after(xgent_agent::agent_loop::agent_poll_system),
        )
        .add_systems(Update, sync_session_list_rows)
        .add_systems(
            Update,
            sync_session_history_open.in_set(crate::panels::PanelSet::Visibility),
        );
    }
}

/// 虚拟列表同步：按可见区间增删会话行（v8 M3-T6）。
///
/// 可见区间由 `xui::VirtualList` 每帧算出；本系统把区间外的行 despawn、把缺失
/// 的行 spawn。会话数据来自 [`CachedSessionList`]。
///
/// v8 M5：面板常驻后，**空态文案**与**占位块**也由本系统维护（不再随开关
/// 重建整个面板）。
fn sync_session_list_rows(
    cached: Res<CachedSessionList>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    mut q_list: Query<(Entity, &mut xui::VirtualList), With<SessionListMarker>>,
    q_rows: Query<(Entity, &SessionListRow), With<SessionListRow>>,
    q_empty: Query<Entity, With<SessionListEmptyMarker>>,
    mut q_spacer: Query<(Entity, &mut Node), With<SessionListSpacerMarker>>,
    mut commands: Commands,
) {
    let Ok((list_entity, mut list)) = q_list.single_mut() else {
        return;
    };
    let total = cached.sessions.len();
    if list.item_count != total {
        list.item_count = total;
    }

    // 空态：列表为空时显示文案，非空时移除。
    let empty_present = !q_empty.is_empty();
    if total == 0 {
        if !empty_present {
            commands.entity(list_entity).with_children(|list| {
                list.spawn((
                    Node {
                        padding: UiRect::all(Val::Px(space::XL)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    ui_text(
                        tr(&loc, "history-empty"),
                        type_scale::BODY_SM,
                        400,
                        theme.text_muted,
                        type_scale::line_height::UI,
                    ),
                    SessionListEmptyMarker,
                ));
            });
        }
    } else {
        for entity in q_empty.iter() {
            commands.entity(entity).despawn();
        }
    }

    // 占位块：撑出列表总高度（否则滚动条只反映已渲染的行）
    let spacer_h = px(total as f32 * SESSION_ROW_H);
    if total == 0 {
        for (entity, _) in q_spacer.iter() {
            commands.entity(entity).despawn();
        }
    } else if let Ok((entity, mut node)) = q_spacer.single_mut() {
        if node.height != spacer_h {
            node.height = spacer_h;
        }
        let _ = entity;
    } else {
        commands.entity(list_entity).with_children(|list| {
            list.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: spacer_h,
                    ..default()
                },
                SessionListSpacerMarker,
            ));
        });
    }

    let first = list.first_visible;
    let last = (first + list.visible_count).min(total);

    for (entity, row) in q_rows.iter() {
        if row.index < first || row.index >= last {
            commands.entity(entity).despawn();
        }
    }
    let present: std::collections::HashSet<usize> = q_rows
        .iter()
        .map(|(_, row)| row.index)
        .filter(|idx| *idx >= first && *idx < last)
        .collect();
    for index in first..last {
        if present.contains(&index) {
            continue;
        }
        let Some(session) = cached.sessions.get(index) else {
            continue;
        };
        commands.entity(list_entity).with_children(|p| {
            spawn_session_item(p, session, index, &theme, &loc);
        });
    }
}

/// 派生视图：会话历史面板开关（由注册表派生）。
pub fn sync_session_history_open(
    registry: Res<crate::panels::PanelRegistry>,
    layout: Res<crate::panels::SlotLayout>,
    mut history: ResMut<SessionHistoryState>,
) {
    let want = layout.is_panel_visible(&registry, crate::panels::ids::SESSION_HISTORY);
    if history.open != want {
        history.open = want;
    }
}

fn session_history_overlay_systems(
    registry: Res<crate::panels::PanelRegistry>,
    q_overlay_pressed: Query<(), (With<SessionHistoryOverlayMarker>, Added<Pressed>)>,
    q_restore: Query<&SessionRestoreButtonMarker, Added<Pressed>>,
    q_close: Query<(), (With<SessionHistoryCloseMarker>, Added<Pressed>)>,
    mut restore_writer: MessageWriter<RestoreSessionMessage>,
    mut deactivate: MessageWriter<crate::panels::PanelDeactivate>,
    mut list_writer: MessageWriter<ListSessionsMessage>,
) {
    // v8 M5：历史面板是 `Slot::Overlay` 的**注册面板**，显隐由注册表裁决
    // （与文件抽屉同属单例槽位，二者不再争层级）。
    let visible = registry.is_visible(crate::panels::ids::SESSION_HISTORY);

    // 打开时拉取会话列表
    if visible && registry.is_changed() {
        list_writer.write(ListSessionsMessage);
    }

    // 遮罩点击关闭（点在面板外 = overlay 根节点收到按下；仅 Added 帧检查）
    for _ in q_overlay_pressed.iter() {
        deactivate.write(crate::panels::PanelDeactivate {
            id: crate::panels::ids::SESSION_HISTORY,
        });
    }

    // 处理关闭按钮
    for _ in q_close.iter() {
        deactivate.write(crate::panels::PanelDeactivate {
            id: crate::panels::ids::SESSION_HISTORY,
        });
    }

    // 处理恢复按钮点击
    for marker in q_restore.iter() {
        restore_writer.write(RestoreSessionMessage {
            session_id: marker.session_id.clone(),
        });
        deactivate.write(crate::panels::PanelDeactivate {
            id: crate::panels::ids::SESSION_HISTORY,
        });
    }
}

/// 启动时 spawn 会话历史面板（遮罩 + 左抽屉），并注册为 `Slot::Overlay` 面板。
///
/// v8 M5：面板**常驻**（不再按开关 spawn/despawn），显隐由
/// `derive_panel_visibility` 依据注册表派生；与文件抽屉同属 `Slot::Overlay`
/// 单例槽位，二者不再争层级。
fn spawn_session_history(
    mut commands: Commands,
    q_root: Query<Entity, With<crate::layout::UiRoot>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    mut register: MessageWriter<crate::panels::PanelRegister>,
) {
    let Ok(root) = q_root.single() else {
        return;
    };
    // 遮罩（覆盖全窗，点击关闭）
    let overlay = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::None,
                ..default()
            },
            BackgroundColor(theme.overlay),
            crate::kit::drawer_z(),
            Button,
            Hovered::default(),
            SessionHistoryOverlayMarker,
            crate::panels::PanelVisibility::of_panel(
                crate::panels::ids::SESSION_HISTORY,
                crate::panels::Slot::Overlay,
            ),
        ))
        .id();
    commands.entity(root).add_child(overlay);

    // 左抽屉面板（复用 drawer 结构：surface 底 + 右边框）
    let panel = commands
        .spawn((
            Node {
                width: Val::Px(crate::theme::size::DRAWER_W_PREFERRED),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::right(Val::Px(1.0)),
                // v8 M3-T2：抽屉外壳只做硬裁（不是滚动容器）；
                // 真正滚动的是下面的会话列表。
                overflow: Overflow::clip(),
                display: Display::None,
                ..default()
            },
            BackgroundColor(theme.surface),
            BorderColor::all(theme.line),
            // 0.20 指针事件会冒泡到 Button 祖先：面板消费 PointerPress，
            // 点面板不会误触遮罩关闭（等价旧 FocusPolicy::Block）
            crate::kit::block_press_bubbling(),
            // 同上：面板需 Pickable 才能接住点击，避免穿透到遮罩。
            bevy::picking::Pickable::default(),
            crate::panels::PanelVisibility::of_panel(
                crate::panels::ids::SESSION_HISTORY,
                crate::panels::Slot::Overlay,
            ),
        ))
        .id();
    commands.entity(overlay).add_child(panel);

    commands.entity(panel).with_children(|panel| {
        // 标题栏
        panel
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::all(Val::Px(space::MD)),
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(theme.border),
            ))
            .with_children(|head| {
                head.spawn(ui_text(
                    tr(&loc, "history-title"),
                    type_scale::H3,
                    590,
                    theme.text,
                    type_scale::line_height::UI,
                ));
                head.spawn((
                    Button,
                    Hovered::default(),
                    Node {
                        width: Val::Px(crate::theme::size::ICON_SM),
                        height: Val::Px(crate::theme::size::ICON_SM),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    ui_text(
                        tr(&loc, "history-close"),
                        type_scale::BODY,
                        400,
                        theme.text_dim,
                        type_scale::line_height::UI,
                    ),
                    SessionHistoryCloseMarker,
                ));
            });

        // 会话列表
        // v8 M3-T2：改用 ScrollFrame（overflow 由构造器定，滚轮真的能滚）。
        // v8 M3-T6：虚拟化——只渲染可见行，占位块撑出总高度。
        // v8 M5：空态文案由 `sync_session_list_rows` 按数据维护
        // （面板常驻，不再随开关重建）。
        panel.spawn((
            xui::ScrollFrame::vertical(),
            xui::VirtualList::new(0, SESSION_ROW_H),
            SessionListMarker,
        ));
    });

    // v8 M5：注册为 `Slot::Overlay` 面板（与文件抽屉同属单例槽位）。
    register.write(crate::panels::PanelRegister {
        desc: crate::panels::PanelDescriptor::new(
            crate::panels::ids::SESSION_HISTORY,
            crate::panels::Slot::Overlay,
            "panel-session-history",
            "clock",
            1,
        ),
        view: Some(panel),
    });
}

/// 生成单个会话条目。
fn spawn_session_item(
    parent: &mut ChildSpawnerCommands,
    session: &SessionSummary,
    index: usize,
    theme: &Theme,
    loc: &Localizer,
) {
    let title = session
        .title
        .clone()
        .unwrap_or_else(|| format!("#{}", session.id));
    let date = format_timestamp(session.timestamp);
    let msg_count = tr_with(
        loc,
        "history-message-count",
        &[("count", session.message_count.to_string())],
    )
    .to_string();

    parent
        .spawn((
            Node {
                // v8 M3-T6：绝对定位到行位（虚拟列表只渲染可见行）
                position_type: PositionType::Absolute,
                top: px(index as f32 * SESSION_ROW_H),
                left: px(0.0),
                width: Val::Percent(100.0),
                height: px(SESSION_ROW_H),
                padding: UiRect::all(Val::Px(space::SM)),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(theme.line),
            SessionItemMarker {
                session_id: session.id.clone(),
            },
            SessionListRow { index },
        ))
        .with_children(|row| {
            // 左侧：标题 + 元信息
            row.spawn((Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            },))
                .with_children(|info| {
                    info.spawn(ui_text(
                        title,
                        type_scale::BODY_SM,
                        510,
                        theme.text,
                        type_scale::line_height::UI,
                    ));
                    info.spawn(ui_text(
                        format!("{date} · {msg_count}"),
                        type_scale::MICRO,
                        400,
                        theme.text_faint,
                        type_scale::line_height::UI,
                    ));
                });
            // 右侧：恢复按钮
            row.spawn((
                Button,
                Hovered::default(),
                Node {
                    padding: UiRect::horizontal(Val::Px(space::SM)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(px(crate::theme::radius::SMALL)),
                    ..default()
                },
                BackgroundColor(theme.elevated),
                BorderColor::all(theme.border),
                ui_text(
                    tr(loc, "history-restore"),
                    type_scale::CAPTION,
                    510,
                    theme.text,
                    type_scale::line_height::UI,
                ),
                SessionRestoreButtonMarker {
                    session_id: session.id.clone(),
                },
            ));
        });
}

/// 格式化时间戳为简短日期。
fn format_timestamp(ms: u64) -> String {
    if ms == 0 {
        return String::new();
    }
    let secs = ms / 1000;
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    if days > 0 {
        format!("{days}d {hours}h")
    } else {
        let mins = (secs % 3600) / 60;
        format!("{hours}h {mins}m")
    }
}

/// 订阅 SessionListMessage，更新缓存。
fn handle_session_list_results(
    mut reader: MessageReader<SessionListMessage>,
    mut cached: ResMut<CachedSessionList>,
) {
    for ev in reader.read() {
        cached.sessions = ev.sessions.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::{PanelDescriptor, PanelRegistry, Slot, ids};

    /// v8 M5 回归：历史面板是 `Slot::Overlay` 的**注册面板**，与文件抽屉同属
    /// 单例槽位——二者同一时刻至多一个可见（不再争层级）。
    #[test]
    fn history_and_file_drawer_share_one_exclusive_slot() {
        let mut app = App::new();
        app.add_plugins(bevy::app::ScheduleRunnerPlugin::default())
            .init_resource::<Theme>()
            .init_resource::<Localizer>()
            .init_resource::<CachedSessionList>()
            .init_resource::<SessionHistoryState>()
            .add_message::<ListSessionsMessage>()
            .add_message::<RestoreSessionMessage>()
            .add_plugins(crate::panels::PanelsPlugin);
        {
            let mut registry = app.world_mut().resource_mut::<PanelRegistry>();
            registry.register(
                PanelDescriptor::new(ids::FILE_DRAWER, Slot::Overlay, "t", "folder", 0),
                None,
            );
            registry.register(
                PanelDescriptor::new(ids::SESSION_HISTORY, Slot::Overlay, "t", "clock", 1),
                None,
            );
        }

        // 打开历史面板
        app.world_mut().write_message(crate::panels::PanelActivate {
            id: ids::SESSION_HISTORY,
        });
        app.update();
        {
            let registry = app.world().resource::<PanelRegistry>();
            assert!(registry.is_visible(ids::SESSION_HISTORY));
            assert!(!registry.is_visible(ids::FILE_DRAWER));
        }

        // 再打开文件抽屉 → 历史面板自动失活（单例槽位）
        app.world_mut().write_message(crate::panels::PanelActivate {
            id: ids::FILE_DRAWER,
        });
        app.update();
        {
            let registry = app.world().resource::<PanelRegistry>();
            assert!(registry.is_visible(ids::FILE_DRAWER));
            assert!(!registry.is_visible(ids::SESSION_HISTORY));
        }
    }
}
