//! xgent_tools — Agent 可调用的工具体系。
//!
//! 提供工具抽象 [`Tool`] trait、内置工具（ReadFile/WriteFile/SearchFiles/RunCommand）、
//! 安全策略分级（Approved/NeedsConfirmation/Denied）与执行器 [`ToolExecutor`]。
//!
//! 不依赖 Bevy——工具是纯异步逻辑。Bevy 桥接放 xgent_agent。

pub mod builtins;
pub mod confirm;
pub mod executor;
pub mod path;
pub mod security;
pub mod tool;

pub use builtins::{ReadFile, RunCommand, SearchFiles, WriteFile};
pub use confirm::{ConfirmDecision, ConfirmRequest};
pub use executor::{ConfirmCallback, ToolExecutor};
pub use path::resolve_in_project;
pub use security::resolve_policy;
pub use tool::{SecurityPolicy, SideEffect, Tool, ToolCtx, ToolResult};

use std::sync::Arc;

/// 默认内置工具集合。
pub fn default_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(ReadFile),
        Arc::new(WriteFile),
        Arc::new(SearchFiles),
        Arc::new(RunCommand),
    ]
}
