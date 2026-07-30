//! PTY 后端抽象。
//!
//! [`TerminalBackend`] 是终端 PTY 操作的异步 trait，MVP 唯一实现
//! [`LocalPtyBackend`](crate::LocalPtyBackend)（基于 `portable-pty`）。
//! 将来 Web 端 / 多窗口共享场景需上移 daemon 时，新增 `DaemonPtyBackend`
//! 走 JSON-RPC，调用方（`xgent_ui::terminal` ECS 系统）不改——
//! 对齐 AGENTS.md §5.1 "可上移职责用 trait 抽象，切换不破坏调用方"。
//!
//! 详见 `doc/design/terminal-design.md` §3.1、§5、ADR-0011/0012。

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::sync::mpsc;

/// 终端会话标识（每个 tab 一个）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TerminalId(pub u64);

/// PTY 输出事件（经 channel 回 ECS）。
#[derive(Debug, Clone)]
pub enum TerminalEvent {
    /// PTY stdout/stderr 字节流（可能含 ANSI 转义序列）。
    Output(Vec<u8>),
    /// PTY 进程退出（exit code，None 表示信号终止或无法获取）。
    Exited(Option<i32>),
}

/// spawn 请求。
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    /// shell 选择。
    pub shell: ShellSpec,
    /// 初始工作目录。
    pub cwd: PathBuf,
    /// 初始列数。
    pub cols: u16,
    /// 初始行数。
    pub rows: u16,
}

/// 默认 shell 选择。
///
/// MVP 不在 settings 暴露 `terminal.shell` 配置项——
/// Windows 用 powershell、Unix 从 `$SHELL` 取、fallback sh。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellSpec {
    /// Windows：`powershell.exe`。
    Powershell,
    /// Unix：从 `$SHELL` 环境变量取，缺失则 fallback `sh`。
    FromEnv,
}

/// PTY 后端错误。
#[derive(thiserror::Error, Debug)]
pub enum TerminalError {
    #[error("spawn 失败: {0}")]
    Spawn(String),
    #[error("write 失败: {0}")]
    Write(String),
    #[error("resize 失败: {0}")]
    Resize(String),
    #[error("kill 失败: {0}")]
    Kill(String),
    #[error("未知终端 id: {0}")]
    UnknownId(u64),
}

/// PTY 后端异步 trait。
///
/// 实现需保证：spawn 起 PTY 进程 + 读写 task；经 channel 发
/// [`TerminalEvent`]；kill 杀进程组并释放资源。
///
/// 注：MVP 保持 shell cooked 模式（`portable-pty` 无跨平台 raw 模式 API），
/// shell 自带 readline 回显。UI 侧透传按键字节，shell 回显为唯一显示源。
#[async_trait]
pub trait TerminalBackend: Send + Sync {
    /// spawn 新 PTY 会话，返回其 id。
    ///
    /// `output_tx` 由调用方传入，backend 在 PTY 退出前持续经此 channel 发
    /// [`TerminalEvent`]（Output / Exited）。调用方（`xgent_ui::terminal` 的
    /// IO 桥接系统）持 `Receiver`，每帧 drain。
    async fn spawn(
        &self,
        req: SpawnRequest,
        output_tx: mpsc::Sender<TerminalEvent>,
    ) -> Result<TerminalId, TerminalError>;
    /// 写字节到 PTY stdin（透传模式：字符/控制字节/转义序列原样发，shell 回显）。
    async fn write(&self, id: TerminalId, bytes: Vec<u8>) -> Result<(), TerminalError>;
    /// 调整 PTY 窗口大小（响应 SideView/窗口 resize）。
    async fn resize(&self, id: TerminalId, cols: u16, rows: u16) -> Result<(), TerminalError>;
    /// kill PTY 进程并释放资源。
    async fn kill(&self, id: TerminalId) -> Result<(), TerminalError>;
}
