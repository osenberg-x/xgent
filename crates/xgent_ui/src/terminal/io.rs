//! PTY IO 桥接（ECS Messages ↔ async [`TerminalBackend`]）。
//!
//! 详见 `doc/design/terminal-design.md` §3.2-§3.4、ADR-0011。
//!
//! 数据流（UI → backend）：
//! ```text
//! TerminalInput / TerminalResize Message
//!   → handle_terminal_input / handle_terminal_resize
//!   → spawn tokio task 调 backend.write/resize（fire-and-forget，错误记 warn）
//! ```
//!
//! 数据流（backend → UI）：
//! ```text
//! backend.spawn 传入 output_tx（tokio mpsc）
//!   → spawn drain task：循环 recv().await → 推 PTY 事件进 crossbeam bridge tx
//!   → handle_pty_events 每帧 try_recv bridge rx → 发 TerminalOutputChunk/Exited Message
//!   → output 模块消费 Message 累积进 RenderHistory
//! ```
//!
//! bridge channel 用 `crossbeam-channel`（`Receiver` 是 `Sync`，可放 `Resource`；
//! `try_recv` 取 `&self`，ECS 系统无需 `ResMut`）。

use bevy::prelude::*;

use crate::terminal::{TerminalIoRuntime, TerminalTab, TerminalTabStatus};
use xgent_terminal::TerminalBackend;

/// 桥接事件（PTY 事件 + 关联的 tab 实体）。
#[derive(Debug, Clone)]
pub enum PtyBridgeEvent {
    /// PTY spawn 成功：回填 pty_id。
    Spawned {
        tab: Entity,
        pty_id: xgent_terminal::TerminalId,
    },
    /// PTY 输出 / 退出数据流。
    Pty {
        tab: Entity,
        event: xgent_terminal::TerminalEvent,
    },
}

/// 桥接 channel Resource：tokio drain task → ECS 系统。
///
/// 由 [`TerminalIoRuntime`] 持有 `crossbeam` sender + 一个独立的 [`PtyBridge`]
/// Resource 持有 receiver。`spawn` 时把 sender clone 给 drain task。
#[derive(Resource, Clone)]
pub struct PtyBridge {
    tx: crossbeam_channel::Sender<PtyBridgeEvent>,
}

/// bridge receiver（独占 Resource，非 `ResMut` 因 `crossbeam` rx 是 `Sync`）。
#[derive(Resource)]
pub struct PtyBridgeRx {
    rx: crossbeam_channel::Receiver<PtyBridgeEvent>,
}

impl PtyBridge {
    /// 建桥：返回 (tx Resource, rx Resource)。
    pub fn pair() -> (Self, PtyBridgeRx) {
        let (tx, rx) = crossbeam_channel::unbounded::<PtyBridgeEvent>();
        (PtyBridge { tx }, PtyBridgeRx { rx })
    }

    /// sender 句柄（clone 给 drain task）。
    pub fn sender(&self) -> crossbeam_channel::Sender<PtyBridgeEvent> {
        self.tx.clone()
    }
}

impl PtyBridgeRx {
    /// 非阻塞 drain 全部事件。
    pub fn drain(&self) -> Vec<PtyBridgeEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            out.push(ev);
        }
        out
    }
}

/// 终端输入请求（UI → backend）：整行 + `\n` 或单控制字节（Ctrl+C/D）。
#[derive(Message, Debug, Clone)]
pub struct TerminalInput {
    /// 对应的 tab 实体。
    pub tab: Entity,
    /// 字节内容。
    pub bytes: Vec<u8>,
}

/// 终端 resize 请求（UI → backend）。
#[derive(Message, Debug, Clone)]
pub struct TerminalResize {
    pub tab: Entity,
    pub cols: u16,
    pub rows: u16,
}

/// PTY spawn 完成（backend → UI）：回填真实 pty_id。
#[derive(Message, Debug, Clone)]
pub struct TerminalSpawned {
    pub tab: Entity,
    pub pty_id: xgent_terminal::TerminalId,
}

/// PTY 输出数据块（backend → UI，高频）。
#[derive(Message, Debug, Clone)]
pub struct TerminalOutputChunk {
    pub tab: Entity,
    pub bytes: Vec<u8>,
}

/// PTY 退出（backend → UI）。
#[derive(Message, Debug, Clone)]
pub struct TerminalExited {
    pub tab: Entity,
    pub exit_code: Option<i32>,
}

/// 处理 [`TerminalInput`]：spawn tokio task 调 backend.write。
pub fn handle_terminal_input(
    mut reader: MessageReader<TerminalInput>,
    rt: Res<TerminalIoRuntime>,
    q_tabs: Query<&TerminalTab>,
) {
    let (Some(handle), Some(backend)) = (rt.handle.as_ref(), rt.backend.as_ref()) else {
        return;
    };
    for req in reader.read() {
        let Ok(tab) = q_tabs.get(req.tab) else {
            continue;
        };
        if tab.status == TerminalTabStatus::Exited {
            continue;
        }
        let Some(pty_id) = tab.pty_id else {
            // Created 态：PTY 尚未就绪，丢弃输入
            continue;
        };
        let bytes = req.bytes.clone();
        let backend = backend.clone();
        handle.spawn(async move {
            if let Err(e) = backend.write(pty_id, bytes).await {
                tracing::warn!("终端 write 失败: {e}");
            }
        });
    }
}

/// 处理 [`TerminalResize`]：spawn tokio task 调 backend.resize。
pub fn handle_terminal_resize(
    mut reader: MessageReader<TerminalResize>,
    rt: Res<TerminalIoRuntime>,
    q_tabs: Query<&TerminalTab>,
) {
    let (Some(handle), Some(backend)) = (rt.handle.as_ref(), rt.backend.as_ref()) else {
        return;
    };
    for req in reader.read() {
        let Ok(tab) = q_tabs.get(req.tab) else {
            continue;
        };
        let Some(pty_id) = tab.pty_id else {
            continue;
        };
        let cols = req.cols;
        let rows = req.rows;
        let backend = backend.clone();
        handle.spawn(async move {
            if let Err(e) = backend.resize(pty_id, cols, rows).await {
                tracing::warn!("终端 resize 失败: {e}");
            }
        });
    }
}

/// 消费桥接 channel → 发 [`TerminalOutputChunk`] / [`TerminalExited`] /
/// [`TerminalSpawned`] Message，并回填 `TerminalTab` 状态。
pub fn handle_pty_events(
    bridge: Res<PtyBridgeRx>,
    mut output_writer: MessageWriter<TerminalOutputChunk>,
    mut exited_writer: MessageWriter<TerminalExited>,
    mut spawned_writer: MessageWriter<TerminalSpawned>,
    mut q_tabs: Query<&mut TerminalTab>,
) {
    for ev in bridge.drain() {
        match ev {
            PtyBridgeEvent::Spawned { tab, pty_id } => {
                if let Ok(mut t) = q_tabs.get_mut(tab) {
                    t.pty_id = Some(pty_id);
                    t.status = TerminalTabStatus::Running;
                }
                spawned_writer.write(TerminalSpawned { tab, pty_id });
            }
            PtyBridgeEvent::Pty {
                tab,
                event: xgent_terminal::TerminalEvent::Output(bytes),
            } => {
                output_writer.write(TerminalOutputChunk { tab, bytes });
            }
            PtyBridgeEvent::Pty {
                tab,
                event: xgent_terminal::TerminalEvent::Exited(code),
            } => {
                if let Ok(mut t) = q_tabs.get_mut(tab) {
                    t.status = TerminalTabStatus::Exited;
                    t.exit_code = code;
                }
                exited_writer.write(TerminalExited {
                    tab,
                    exit_code: code,
                });
            }
        }
    }
}

/// 构造默认 [`SpawnRequest`]（shell 跨平台、cwd、80×24）。
pub fn default_spawn_request(cwd: std::path::PathBuf) -> xgent_terminal::SpawnRequest {
    xgent_terminal::SpawnRequest {
        shell: default_shell(),
        cwd,
        cols: 80,
        rows: 24,
    }
}

/// 默认 shell 选择（跨平台）。
pub fn default_shell() -> xgent_terminal::ShellSpec {
    if cfg!(windows) {
        xgent_terminal::ShellSpec::Powershell
    } else {
        xgent_terminal::ShellSpec::FromEnv
    }
}

/// spawn 一个 PTY 会话：backend.spawn + 起 drain task，pty_id 经 bridge 回填。
///
/// 由 `tabs::handle_spawn_tab_requests` 调用。spawn 是 async，drain task 在 spawn
/// 返回 pty_id 后起（drain 不需 pty_id，只需 output_rx + bridge sender）。
pub fn spawn_pty_session(
    handle: &tokio::runtime::Handle,
    backend: std::sync::Arc<dyn TerminalBackend>,
    tab: Entity,
    req: xgent_terminal::SpawnRequest,
    bridge_tx: crossbeam_channel::Sender<PtyBridgeEvent>,
) {
    let (output_tx, output_rx) =
        tokio::sync::mpsc::channel::<xgent_terminal::TerminalEvent>(256);
    handle.spawn(async move {
        match backend.spawn(req, output_tx).await {
            Ok(pty_id) => {
                // 先回填 pty_id + 设 Running，再起 drain task——
                // 确保 drain 转发的 Pty 事件不会落在 pty_id=None 的 tab 上。
                let _ = bridge_tx.send(PtyBridgeEvent::Spawned { tab, pty_id });
                // drain task：output_rx → bridge
                let bridge_tx_drain = bridge_tx.clone();
                let mut output_rx = output_rx;
                while let Some(ev) = output_rx.recv().await {
                    if bridge_tx_drain
                        .send(PtyBridgeEvent::Pty { tab, event: ev })
                        .is_err()
                    {
                        break;
                    }
                }
            }
            Err(e) => {
                tracing::error!("终端 spawn 失败: {e}");
                // spawn 失败 → 标 Exited，让 UI 收起/标灰（tabs 模块可据 Exited despawn）
                let _ = bridge_tx.send(PtyBridgeEvent::Pty {
                    tab,
                    event: xgent_terminal::TerminalEvent::Exited(None),
                });
            }
        }
    });
}

