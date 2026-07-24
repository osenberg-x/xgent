//! [`LocalPtyBackend`]——基于 `portable-pty` 的本地 PTY 实现。
//!
//! 详见 `doc/design/terminal-design.md` §5.3、ADR-0011。
//!
//! 同步 API（`portable-pty` 是同步库）经 `tokio::task::spawn_blocking` 隔离。
//! 所有权模型：每个 PTY 会话的 master/writer/child 在 spawn 时创建，
//! 读循环 + 命令循环各跑在独立 std 线程，经 channel 与 tokio 侧通信，
//! 规避 `portable_pty::MasterPty` 非 `Sync` 的约束（线程独占所有权）。

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use tokio::sync::{mpsc, oneshot};

use crate::backend::{ShellSpec, SpawnRequest, TerminalBackend, TerminalError, TerminalEvent, TerminalId};

/// 读循环/命令循环 task 接收的命令（来自 write/resize/kill）。
enum PtyCmd {
    Write { bytes: Vec<u8>, reply: oneshot::Sender<Result<(), TerminalError>> },
    Resize { cols: u16, rows: u16, reply: oneshot::Sender<Result<(), TerminalError>> },
    Kill { reply: oneshot::Sender<Result<(), TerminalError>> },
}

/// 单个 PTY 会话的命令通道（write/resize/kill 经此发）。
struct PtySession {
    cmd_tx: mpsc::Sender<PtyCmd>,
}

/// 本地 PTY 后端（MVP 唯一实现）。
pub struct LocalPtyBackend {
    sessions: Arc<Mutex<std::collections::HashMap<TerminalId, PtySession>>>,
    next_id: AtomicU64,
}

impl LocalPtyBackend {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
            next_id: AtomicU64::new(1),
        }
    }
}

impl Default for LocalPtyBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TerminalBackend for LocalPtyBackend {
    async fn spawn(
        &self,
        req: SpawnRequest,
        output_tx: mpsc::Sender<TerminalEvent>,
    ) -> Result<TerminalId, TerminalError> {
        let id = TerminalId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<PtyCmd>(64);

        // 在 spawn_blocking 里做 PTY spawn + 起两个 std 线程（读循环 + 命令循环）
        tokio::task::spawn_blocking(move || -> Result<(), TerminalError> {
            let pty_system = NativePtySystem::default();
            let size = PtySize {
                rows: req.rows,
                cols: req.cols,
                pixel_width: 0,
                pixel_height: 0,
            };
            let pair = pty_system
                .openpty(size)
                .map_err(|e| TerminalError::Spawn(format!("openpty: {e}")))?;

            let cmd = build_shell_command(req.shell, &req.cwd);
            let child = pair
                .slave
                .spawn_command(cmd)
                .map_err(|e| TerminalError::Spawn(format!("spawn_command: {e}")))?;

            drop(pair.slave); // slave 用完即弃

            let reader = pair
                .master
                .try_clone_reader()
                .map_err(|e| TerminalError::Spawn(format!("try_clone_reader: {e}")))?;
            let writer = pair
                .master
                .take_writer()
                .map_err(|e| TerminalError::Spawn(format!("take_writer: {e}")))?;

            // writer 共享：读循环需回写 DSR 响应，命令循环写用户输入。
            let writer = Arc::new(std::sync::Mutex::new(writer));
            let writer_for_read = writer.clone();
            let master = pair.master;
            let mut killer = child.clone_killer();
            let child = Arc::new(std::sync::Mutex::new(child));
            let child_for_read = child.clone();
            let output_tx_for_read = output_tx.clone();

            // 同步 channel：tokio 侧 cmd_rx → 命令循环线程的 cmd_rx_sync
            let (cmd_tx_sync, cmd_rx_sync) = std::sync::mpsc::channel::<PtyCmd>();
            // 读循环线程：阻塞读 reader，直接经 tokio mpsc Sender::blocking_send
            // 发给 ECS 侧（blocking_send 专为非 async 线程设计，不会冻结 runtime）。
            // 同时检测 DSR（光标位置查询 \x1b[6n）并回复，避免 shell 卡死等待。
            // reader EOF 后在此线程 wait 子进程取退出码，再发 Exited。
            std::thread::spawn(move || {
                let mut reader = reader;
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            // 检测 DSR 光标位置查询（\x1b[6n），回复 \x1b[1;1R
                            // PowerShell/PSReadLine 启动时发此查询探测终端，
                            // 不回复则阻塞等待，导致后续输入无响应。
                            if buf[..n].windows(4).any(|w| w == b"\x1b[6n") {
                                if let Ok(mut w) = writer_for_read.lock() {
                                    let _ = w.write_all(b"\x1b[1;1R");
                                    let _ = w.flush();
                                }
                            }
                            if output_tx_for_read
                                .blocking_send(TerminalEvent::Output(buf[..n].to_vec()))
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                // reader EOF = PTY 输出结束。wait 取退出码（与 kill 路径竞争——
                // wait 幂等，任一线程先 wait 另一线程得已退出状态）。
                let code = if let Ok(mut c) = child_for_read.lock() {
                    c.wait().ok().map(|s| s.exit_code() as i32)
                } else {
                    None
                };
                let _ = output_tx_for_read.blocking_send(TerminalEvent::Exited(code));
            });

            // 命令循环线程：收 cmd_rx_sync，执行 write/resize/kill
            std::thread::spawn(move || {
                let writer = writer;
                loop {
                    match cmd_rx_sync.recv() {
                        Ok(PtyCmd::Write { bytes, reply }) => {
                            let r = if let Ok(mut w) = writer.lock() {
                                w.write_all(&bytes)
                                    .and_then(|_| w.flush())
                                    .map_err(|e| TerminalError::Write(e.to_string()))
                            } else {
                                Err(TerminalError::Write("writer lock poisoned".into()))
                            };
                            let _ = reply.send(r);
                        }
                        Ok(PtyCmd::Resize { cols, rows, reply }) => {
                            let r = master
                                .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
                                .map_err(|e| TerminalError::Resize(e.to_string()));
                            let _ = reply.send(r);
                        }
                        Ok(PtyCmd::Kill { reply }) => {
                            // kill 语义是"确保进程不再运行"——对已退出进程
                            // 调 TerminateProcess 在 Windows 报 error 87
                            // （参数错误），这是预期行为，忽略即可。对齐
                            // portable-pty 自身 WinChild::kill 的 .ok() 模式。
                            let _ = killer.kill();
                            let _ = reply.send(Ok(()));
                            if let Ok(mut c) = child.lock() {
                                let _ = c.wait();
                            }
                            break;
                        }
                        Err(_) => break,
                    }
                }
            });

            // 桥接 tokio → std（cmd_rx → cmd_tx_sync）
            tokio::spawn(async move {
                while let Some(cmd) = cmd_rx.recv().await {
                    if cmd_tx_sync.send(cmd).is_err() {
                        break;
                    }
                }
            });
            // out 桥接已并入读循环线程（直接 blocking_send），无需独立 task。

            Ok(())
        })
        .await
        .map_err(|e| TerminalError::Spawn(format!("spawn_blocking join: {e}")))??;

        self.sessions.lock().insert(id, PtySession { cmd_tx });
        Ok(id)
    }

    async fn write(&self, id: TerminalId, bytes: Vec<u8>) -> Result<(), TerminalError> {
        let cmd_tx = {
            let guard = self.sessions.lock();
            guard
                .get(&id)
                .ok_or(TerminalError::UnknownId(id.0))?
                .cmd_tx
                .clone()
        };
        let (tx, rx) = oneshot::channel();
        cmd_tx
            .send(PtyCmd::Write { bytes, reply: tx })
            .await
            .map_err(|_| TerminalError::Write("cmd channel closed".into()))?;
        rx.await
            .map_err(|_| TerminalError::Write("reply dropped".into()))?
    }

    async fn resize(&self, id: TerminalId, cols: u16, rows: u16) -> Result<(), TerminalError> {
        let cmd_tx = {
            let guard = self.sessions.lock();
            guard
                .get(&id)
                .ok_or(TerminalError::UnknownId(id.0))?
                .cmd_tx
                .clone()
        };
        let (tx, rx) = oneshot::channel();
        cmd_tx
            .send(PtyCmd::Resize { cols, rows, reply: tx })
            .await
            .map_err(|_| TerminalError::Resize("cmd channel closed".into()))?;
        rx.await
            .map_err(|_| TerminalError::Resize("reply dropped".into()))?
    }

    async fn kill(&self, id: TerminalId) -> Result<(), TerminalError> {
        let cmd_tx = {
            let mut guard = self.sessions.lock();
            guard
                .remove(&id)
                .ok_or(TerminalError::UnknownId(id.0))?
                .cmd_tx
        };
        let (tx, rx) = oneshot::channel();
        cmd_tx
            .send(PtyCmd::Kill { reply: tx })
            .await
            .map_err(|_| TerminalError::Kill("cmd channel closed".into()))?;
        rx.await
            .map_err(|_| TerminalError::Kill("reply dropped".into()))?
    }
}

/// 据 [`ShellSpec`] 构造 shell 命令。
fn build_shell_command(shell: ShellSpec, cwd: &PathBuf) -> CommandBuilder {
    let mut cmd = match shell {
        ShellSpec::Powershell => CommandBuilder::new("powershell.exe"),
        ShellSpec::FromEnv => CommandBuilder::new_default_prog(),
    };
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd
}
