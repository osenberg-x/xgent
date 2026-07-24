//! xgent_terminal — 终端 PTY 抽象层（F-19）。
//!
//! 详见 `doc/design/terminal-design.md` §5、ADR-0011/0012、`CONTEXT.md`「终端（F-19，P1）」。
//!
//! 提供 [`TerminalBackend`] async trait + MVP 实现 [`LocalPtyBackend`]（portable-pty）。
//! 不依赖 Bevy；UI 层（`xgent_ui::terminal`）经 trait 调用，不直接依赖 `portable-pty`。

pub mod backend;
pub mod local_pty;
pub mod render;

pub use backend::{ShellSpec, SpawnRequest, TerminalBackend, TerminalError, TerminalEvent, TerminalId};
pub use local_pty::LocalPtyBackend;
pub use render::{Color, RenderLine, SpanStyle, StyledSpan, TerminalParser};
