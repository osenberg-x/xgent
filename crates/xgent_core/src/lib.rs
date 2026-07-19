// xgent_core — 跨进程共享类型与协议契约

pub mod chat;
pub mod config;
pub mod editor;
pub mod error;
pub mod fs;
pub mod ids;
pub mod methods;
pub mod notifications;
pub mod proto;
pub mod session;

pub use editor::{BufferStatus, EditorQuery, EditorState};
pub use error::{XgentError, XgentResult};
