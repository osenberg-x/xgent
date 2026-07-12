//! xgent_settings_core — XGent 配置体系纯类型层（不依赖 Bevy）。
//!
//! 提供配置类型（全局/项目）、平台规范路径、TOML 读写。
//! daemon 与 provider 可使用配置类型而不被 Bevy 拖重。

pub mod global;
pub mod paths;
pub mod project;
pub mod store;

pub use global::{GlobalConfig, Preferences, ProviderConfig, ProviderKind};
pub use project::{ContextStrategy, ProjectConfig, ToolPolicyConfig};
pub use store::{GlobalConfigStore, ProjectConfigStore};
