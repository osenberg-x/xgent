//! 内置工具集。
//!
//! MVP 提供 ReadFile、WriteFile、SearchFiles、RunCommand。
//! 所有内置工具的 `policy()` 默认 `NeedsConfirmation`（安全模型：默认需确认）。

pub mod read_file;
pub mod run_command;
pub mod search_files;
pub mod write_file;

pub use read_file::ReadFile;
pub use run_command::RunCommand;
pub use search_files::SearchFiles;
pub use write_file::WriteFile;
