//! xgent_plugin — 插件宿主核心。
//!
//! 提供 `PluginHost`（加载/卸载/索引）、`WasmHost`（wasmtime 引擎 + Store）、
//! `PluginHostProxy`（反转依赖枢纽）与清单解析。
//!
//! 详见 `doc/design/plugin-system-design.md` §6 / §7 / §13 Step P2。

pub mod host;
pub mod index;
pub mod manifest;
pub mod proxy;
pub mod wasm_host;

pub use host::{PluginEvent, PluginHost, PluginHostError};
pub use index::{PluginIndex, PluginIndexEntry};
pub use manifest::{
    CommandManifestEntry, ContextProviderManifestEntry, LibManifest, ManifestError,
    PermissionsManifest, PluginManifest, ToolManifestEntry,
};
pub use proxy::{
    PluginCommandProxy, PluginContextProxy, PluginHostProxy, PluginToolProxy, ProxyError,
};
pub use wasm_host::{WasmCallError, WasmHost, WasmPlugin};

// 重新导出 host 侧 WIT 绑定生成的类型（供 xgent_plugin_host 适配器使用）。
// `exports` / `xgent` 模块由 `wasmtime::component::bindgen!` 在 crate 根生成。
pub use wasm_host::exports::xgent::plugin::command::CommandDef as WitCommandDef;
pub use wasm_host::exports::xgent::plugin::context_provider::{
    ContextChunk as WitContextChunk, ContextQuery as WitContextQuery,
    ContextResult as WitContextResult, ProviderDef as WitContextProviderDef,
};
pub use wasm_host::exports::xgent::plugin::tool::{
    ToolDef as WitToolDef, ToolError as WitToolError, ToolTier as WitToolTier,
};
