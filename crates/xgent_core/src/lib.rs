// xgent_core — 跨进程共享类型与协议契约

pub mod chat;
pub mod config;
pub mod error;
pub mod fs;
pub mod ids;
pub mod methods;
pub mod notifications;
pub mod proto;

pub use error::{XgentError, XgentResult};
