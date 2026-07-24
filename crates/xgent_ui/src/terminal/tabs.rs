//! 终端多 tab 管理：spawn/close/switch/clear + tab 条 UI。
//!
//! 详见 `doc/design/terminal-design.md` §3.2、§3.5、§4.1。
//!
//! spawn 流程（[`handle_spawn_tab_requests`]）：
//! 1. spawn `TerminalTab` 实体（pty_id 占位 0，status=Running）+ `RenderHistory`
//! 2. 调 [`crate::terminal::io::spawn_pty_session`]：backend.spawn + drain task，
//!    pty_id 经 bridge channel 回填（[`handle_pty_events`] 写 `TerminalSpawned`）
//!
//! close 流程（[`handle_close_tab_requests`]）：backend.kill + despawn 实体 +
//! 更新 tabs；最后一个 tab 关 → SideViewContent=None + 收起。

use std::path::PathBuf;

use bevy::prelude::*;

use crate::editor::SideViewContent;
use crate::terminal::io::{PtyBridge, default_spawn_request, spawn_pty_session};
use crate::terminal::{
    TerminalIoRuntime, TerminalTab, TerminalTabCloseMarker, TerminalTabMarker, TerminalTabStatus,
    TerminalTabs,
};
use crate::theme::{Theme, px, space};

/// 新建 tab 请求（由 ＋ 按钮 / Ctrl+` 首次唤起 / 顶栏 🖥 触发）。
#[derive(Message, Debug, Clone)]
pub struct SpawnTabRequest {
    /// 初始 cwd。
    pub cwd: PathBuf,
}

/// 关闭 tab 请求。
#[derive(Message, Debug, Clone)]
pub struct CloseTabRequest {
    pub tab: Entity,
}

/// 切换 tab 请求（点击 tab 条）。
#[derive(Message, Debug, Clone)]
pub struct SwitchTabRequest {
    pub tab: Entity,
}

/// 清屏请求（清空当前 tab 的 RenderHistory）。
#[derive(Message, Debug, Clone)]
pub struct ClearTabRequest {
    pub tab: Entity,
}

/// 处理新建 tab 请求：spawn 实体 + 起 PTY spawn/drain task。
pub fn handle_spawn_tab_requests(
    mut reader: MessageReader<SpawnTabRequest>,
    mut tabs: ResMut<TerminalTabs>,
    rt: Res<TerminalIoRuntime>,
    bridge: Res<PtyBridge>,
    mut commands: Commands,
    mut content: ResMut<SideViewContent>,
) {
    let (Some(handle), Some(backend)) = (rt.handle.as_ref(), rt.backend.as_ref()) else {
        tracing::warn!("终端后端未注入，无法 spawn tab");
        return;
    };
    for req in reader.read() {
        tabs.next_seq = tabs.next_seq.saturating_add(1);
        let seq = tabs.next_seq;
        // 单一 shell 来源：从 SpawnRequest 取，避免 statusbar 标签与实际 spawn 不一致
        let spawn_req = default_spawn_request(req.cwd.clone());
        let shell = spawn_req.shell;
        let tab_entity = commands
            .spawn((
                TerminalTab {
                    pty_id: None, // 由 bridge 回填（Spawned 事件）
                    title: format!("shell #{seq}"),
                    status: TerminalTabStatus::Created,
                    shell,
                    cwd: req.cwd.clone(),
                    exit_code: None,
                },
                crate::terminal::output::RenderHistory::default(),
            ))
            .id();
        tabs.open(tab_entity);

        spawn_pty_session(
            handle,
            backend.clone(),
            tab_entity,
            spawn_req,
            bridge.sender(),
        );

        // 切到终端视图 + 展开分屏
        *content = SideViewContent::Terminal;
    }
}

/// 处理关闭 tab 请求：backend.kill + despawn + 更新 tabs。
pub fn handle_close_tab_requests(
    mut reader: MessageReader<CloseTabRequest>,
    mut tabs: ResMut<TerminalTabs>,
    rt: Res<TerminalIoRuntime>,
    q_tabs: Query<&TerminalTab>,
    mut content: ResMut<SideViewContent>,
    mut commands: Commands,
) {
    let (handle_opt, backend_opt) = (rt.handle.as_ref(), rt.backend.as_ref());
    for req in reader.read() {
        if let (Some(handle), Some(backend)) = (handle_opt, backend_opt) {
            if let Ok(tab) = q_tabs.get(req.tab) {
                if let Some(pty_id) = tab.pty_id {
                    let backend = backend.clone();
                    handle.spawn(async move {
                        let _ = backend.kill(pty_id).await;
                    });
                }
            }
        }
        if let Some((entity, _)) = tabs.close(req.tab) {
            commands.entity(entity).despawn();
        }
    }
    if tabs.is_empty() {
        *content = SideViewContent::None;
    }
}

/// 处理切换 tab 请求。
pub fn handle_switch_tab_requests(
    mut reader: MessageReader<SwitchTabRequest>,
    mut tabs: ResMut<TerminalTabs>,
) {
    for req in reader.read() {
        if let Some(idx) = tabs.tabs.iter().position(|&e| e == req.tab) {
            tabs.active = Some(idx);
        }
    }
}

/// 处理清屏请求：清空当前 tab 的 RenderHistory。
pub fn handle_clear_tab_requests(
    mut reader: MessageReader<ClearTabRequest>,
    mut q: Query<&mut crate::terminal::output::RenderHistory>,
) {
    for req in reader.read() {
        if let Ok(mut hist) = q.get_mut(req.tab) {
            hist.clear();
        }
    }
}

/// 据 [`TerminalTabs`] 重建 tab 条 UI。
pub fn rebuild_terminal_tabs(
    tabs: Res<TerminalTabs>,
    q_tabs: Query<&TerminalTab>,
    q_bar: Query<Entity, With<crate::terminal::TerminalTabBarMarker>>,
    q_existing: Query<Entity, With<TerminalTabMarker>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    if !tabs.is_changed() && !tabs.is_added() {
        return;
    }
    let Ok(bar) = q_bar.single() else {
        return;
    };
    for entity in q_existing.iter() {
        commands.entity(entity).despawn();
    }
    let font = theme.font_size;
    let active_idx = tabs.active;
    for (i, &tab_entity) in tabs.tabs.iter().enumerate() {
        let Ok(tab) = q_tabs.get(tab_entity) else {
            continue;
        };
        let is_active = Some(i) == active_idx;
        let dot_color = match tab.status {
            TerminalTabStatus::Created => theme.st_pending,
            TerminalTabStatus::Running => theme.st_ok,
            TerminalTabStatus::Exited => theme.text_dim,
        };
        let bg = if is_active { theme.panel } else { theme.bar };
        let border_color = if is_active {
            theme.accent
        } else {
            theme.border
        };
        let title = tab.title.clone();
        commands.entity(bar).with_children(|bar| {
            bar.spawn((
                Button,
                Node {
                    padding: UiRect::horizontal(px(space::SM)),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(space::XS),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(4.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(bg),
                BorderColor::all(border_color),
                TerminalTabMarker { tab: tab_entity },
            ))
            .with_children(|item| {
                item.spawn((
                    Node {
                        width: px(6.0),
                        height: px(6.0),
                        border_radius: BorderRadius::all(px(3.0)),
                        ..default()
                    },
                    BackgroundColor(dot_color),
                ));
                item.spawn((
                    Text::new(title),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text),
                ));
                item.spawn((
                    Button,
                    Node {
                        width: px(16.0),
                        height: px(16.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    Text::new("x"),
                    TextFont {
                        font_size: FontSize::Px(font - 2.0),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                    TerminalTabCloseMarker,
                ));
            });
        });
    }
}

/// 处理 tab 项点击：切换激活；处理关闭×点击：发 CloseTabRequest。
pub fn handle_terminal_tab_click(
    q_tabs: Query<(&TerminalTabMarker, &Interaction), Changed<Interaction>>,
    q_close: Query<
        (&TerminalTabMarker, &Interaction, &ChildOf),
        (With<TerminalTabCloseMarker>, Changed<Interaction>),
    >,
    mut writer: MessageWriter<SwitchTabRequest>,
    mut close_writer: MessageWriter<CloseTabRequest>,
) {
    for (marker, interaction) in q_tabs.iter() {
        if *interaction == Interaction::Pressed {
            writer.write(SwitchTabRequest { tab: marker.tab });
        }
    }
    for (marker, interaction, _child) in q_close.iter() {
        if *interaction == Interaction::Pressed {
            close_writer.write(CloseTabRequest { tab: marker.tab });
        }
    }
}
