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
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use tokio::sync::{mpsc, oneshot};

use crate::backend::{
    ShellSpec, SpawnRequest, TerminalBackend, TerminalError, TerminalEvent, TerminalId,
};

/// 读循环/命令循环 task 接收的命令（来自 write/resize/kill）。
enum PtyCmd {
    Write {
        bytes: Vec<u8>,
        reply: oneshot::Sender<Result<(), TerminalError>>,
    },
    Resize {
        cols: u16,
        rows: u16,
        reply: oneshot::Sender<Result<(), TerminalError>>,
    },
    Kill {
        reply: oneshot::Sender<Result<(), TerminalError>>,
    },
}

/// 单个 PTY 会话的命令通道（write/resize/kill 经此发）。
struct PtySession {
    cmd_tx: mpsc::Sender<PtyCmd>,
    /// 子进程 killer 句柄（共享给命令循环线程 + Drop 清理）。
    /// `Kill` 命令和 Drop 都经此 kill；`take` 后为 None（幂等）。
    killer: Arc<Mutex<Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>>>,
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

impl Drop for LocalPtyBackend {
    fn drop(&mut self) {
        // 清理所有未 kill 的 PTY 会话：take killer 后 kill 子进程，
        // 防止 backend 被 drop 时子进程变孤儿 + 线程泄漏。
        // take 保证幂等（kill 命令路径已 take 过的 session 此处得 None）。
        let sessions = std::mem::take(&mut *self.sessions.lock());
        for (_, session) in sessions {
            if let Some(mut k) = session.killer.lock().take() {
                let _ = k.kill();
            }
            // cmd_tx drop → 桥接 task 结束 → cmd_tx_sync drop → 命令循环线程退出。
            // killer kill 后子进程退出 → reader EOF → 读循环线程结束。
        }
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
        // 共享 killer 句柄：spawn_blocking 内填入，PtySession + Drop 经此 kill。
        let killer_slot: Arc<Mutex<Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>>> =
            Arc::new(Mutex::new(None));
        let killer_slot_for_blocking = killer_slot.clone();

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

            // spawn_command 成功后子进程已启动。try_clone_reader / take_writer
            // 失败时需 kill 子进程防止孤儿——portable_pty::Child 的 Drop 不杀进程。
            // 提前 clone killer，用 guard 保证错误路径（? 提前返回）自动 kill；
            // 成功路径 disarm 并填入共享 slot 供 PtySession/Drop 使用。
            let killer = child.clone_killer();
            let guard = KillGuard(Some(killer));

            let reader = pair
                .master
                .try_clone_reader()
                .map_err(|e| TerminalError::Spawn(format!("try_clone_reader: {e}")))?;
            let writer = pair
                .master
                .take_writer()
                .map_err(|e| TerminalError::Spawn(format!("take_writer: {e}")))?;

            // reader/writer 均成功：取出 killer，解除 guard（不再自动 kill）
            let killer = guard.disarm();

            // writer 共享：读循环需回写 DSR 响应，命令循环写用户输入。
            let writer = Arc::new(std::sync::Mutex::new(writer));
            let writer_for_read = writer.clone();
            let master = pair.master;
            // 填入共享 slot 供 PtySession/Drop kill
            *killer_slot_for_blocking.lock() = Some(killer);
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
                // 跨 read 的残余尾部：DSR 查询 \x1b[6n 可能被拆到两次 read，
                // 仅在单次 buffer 内 windows(4) 匹配会漏检导致 shell 卡死。
                // 保留最多 3 字节尾部（序列长 4，残余 ≤3 才能拼上下次头部）。
                let mut tail: Vec<u8> = Vec::with_capacity(3);
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let chunk = &buf[..n];
                            // 跨 read 拼接残余 + 新数据扫描 DSR 光标查询（\x1b[6n）。
                            // 序列可能被拆到两次 read，单 buffer 内 windows(4) 会漏检
                            // 导致 shell 卡死。保留最多 3 字节尾部拼接下次头部。
                            // 回复 \x1b[1;1R：PowerShell/PSReadLine 启动时探测终端，
                            // 不回复则阻塞等待，输入无响应。
                            let has_dsr = if tail.is_empty() {
                                chunk.windows(4).any(|w| w == b"\x1b[6n")
                            } else {
                                let mut joined = std::mem::take(&mut tail);
                                joined.extend_from_slice(chunk);
                                let found = joined.windows(4).any(|w| w == b"\x1b[6n");
                                tail = joined[joined.len().saturating_sub(3)..].to_vec();
                                found
                            };
                            // tail 为空时保留本次 chunk 尾部（≤3 字节）供下次拼接
                            if tail.is_empty() {
                                let start = chunk.len().saturating_sub(3);
                                tail = chunk[start..].to_vec();
                            }
                            if has_dsr && let Ok(mut w) = writer_for_read.lock() {
                                let _ = w.write_all(b"\x1b[1;1R");
                                let _ = w.flush();
                            }
                            if output_tx_for_read
                                .blocking_send(TerminalEvent::Output(chunk.to_vec()))
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
            let killer_slot_for_cmd = killer_slot_for_blocking.clone();
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
                                .resize(PtySize {
                                    rows,
                                    cols,
                                    pixel_width: 0,
                                    pixel_height: 0,
                                })
                                .map_err(|e| TerminalError::Resize(e.to_string()));
                            let _ = reply.send(r);
                        }
                        Ok(PtyCmd::Kill { reply }) => {
                            // kill 语义是"确保进程不再运行"——take killer 后 kill。
                            // 对已退出进程调 kill 在 Windows 报 error 87（参数错误），
                            // 这是预期行为，忽略即可。对齐 portable-pty 自身
                            // WinChild::kill 的 .ok() 模式。take 保证幂等（Drop
                            // 再 take 得 None）。
                            if let Some(mut k) = killer_slot_for_cmd.lock().take() {
                                let _ = k.kill();
                            }
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

        self.sessions.lock().insert(
            id,
            PtySession {
                cmd_tx,
                killer: killer_slot,
            },
        );
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
            .send(PtyCmd::Resize {
                cols,
                rows,
                reply: tx,
            })
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

/// RAII guard：drop 时自动 kill 子进程，防止 spawn 中途失败导致孤儿进程。
///
/// `portable_pty::Child` 的 Drop 不杀进程，故 `try_clone_reader` / `take_writer`
/// 失败时需显式 kill。成功路径调 [`disarm`](Self::disarm) 取出 killer 解除自动 kill。
struct KillGuard(Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>);

impl KillGuard {
    /// 取出 killer，解除自动 kill（成功路径调用）。
    fn disarm(mut self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
        self.0.take().expect("KillGuard disarm 后不可再用")
    }
}

impl Drop for KillGuard {
    fn drop(&mut self) {
        if let Some(mut k) = self.0.take() {
            let _ = k.kill();
        }
    }
}
