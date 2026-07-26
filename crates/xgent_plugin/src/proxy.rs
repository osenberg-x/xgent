//! PluginHostProxy — 反转依赖枢纽。
//!
//! 照设计文档 §7.2。各业务子系统（ToolExecutor / CommandRegistry / ContextHub）
//! 注册 proxy impl，插件经 proxy 回调宿主。`xgent_plugin` crate 不依赖任何业务 crate
//! （Tool / CommandRegistry / ContextProvider），经 proxy trait 反转依赖。
//! 对标 Zed `ExtensionHostProxy`。
//!
//! **注册时机硬性**：proxy 未注册时 `register_*` / `unregister_*` 返回 `Err`，
//! 不 panic、不静默跳过（设计文档 §7.2）。
//!
//! **调用上下文硬性**：proxy impl 不在 `PluginHostPlugin::build` 时抓 `ResMut` 存引用。
//! 正确做法见设计文档 §13 Step P4 "proxy 取用机制"：
//! - 注册路径：proxy impl 在 `PluginPollSystem`（ECS 主线程）内调 `world.resource_mut`；
//! - 卸载路径：发 `PluginUnregisterMessage` 到 ECS，system 消费后清理。

use std::sync::Arc;

use parking_lot::RwLock;
use thiserror::Error;

use crate::manifest::PluginManifest;
use crate::wasm_host::WasmPlugin;
use crate::{WitCommandDef, WitContextProviderDef, WitToolDef};

/// proxy 操作错误。
#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("proxy 未注册（启动时序错误：业务 Plugin 应先于 PluginHostPlugin 注册 proxy）")]
    NotRegistered,
    #[error("proxy 操作失败: {0}")]
    Failed(String),
}

/// 插件工具 proxy trait。
///
/// `register_tools` 接收 `Arc<WasmPlugin>` + `Vec<WitToolDef>`，由 impl（在
/// `xgent_plugin_host`）构造 `PluginTool` 并注册到 `ToolExecutor`。
pub trait PluginToolProxy: Send + Sync {
    fn register_tools(
        &self,
        manifest: Arc<PluginManifest>,
        plugin: Arc<WasmPlugin>,
        tool_defs: Vec<WitToolDef>,
    ) -> Result<(), ProxyError>;
    fn unregister_tools(&self, plugin_id: &str) -> Result<(), ProxyError>;
}

/// 插件命令 proxy trait。
pub trait PluginCommandProxy: Send + Sync {
    fn register_commands(
        &self,
        manifest: Arc<PluginManifest>,
        plugin: Arc<WasmPlugin>,
        command_defs: Vec<WitCommandDef>,
    ) -> Result<(), ProxyError>;
    fn unregister_commands(&self, plugin_id: &str) -> Result<(), ProxyError>;
}

/// 插件 ContextProvider proxy trait。
pub trait PluginContextProxy: Send + Sync {
    fn register_providers(
        &self,
        manifest: Arc<PluginManifest>,
        plugin: Arc<WasmPlugin>,
        provider_defs: Vec<WitContextProviderDef>,
        project_root: std::path::PathBuf,
    ) -> Result<(), ProxyError>;
    fn unregister_providers(&self, plugin_id: &str) -> Result<(), ProxyError>;
}

/// 插件宿主代理：各子系统注册 proxy impl，插件经 proxy 回调宿主。
///
/// 照设计文档 §7.2。三把 `RwLock<Option<Arc<dyn ...>>>` 持各 proxy impl。
#[derive(Default)]
pub struct PluginHostProxy {
    tool_proxy: RwLock<Option<Arc<dyn PluginToolProxy>>>,
    command_proxy: RwLock<Option<Arc<dyn PluginCommandProxy>>>,
    context_proxy: RwLock<Option<Arc<dyn PluginContextProxy>>>,
}

impl PluginHostProxy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_tool_proxy(&self, proxy: Arc<dyn PluginToolProxy>) {
        *self.tool_proxy.write() = Some(proxy);
    }

    pub fn set_command_proxy(&self, proxy: Arc<dyn PluginCommandProxy>) {
        *self.command_proxy.write() = Some(proxy);
    }

    pub fn set_context_proxy(&self, proxy: Arc<dyn PluginContextProxy>) {
        *self.context_proxy.write() = Some(proxy);
    }

    pub fn tool(&self) -> Result<Arc<dyn PluginToolProxy>, ProxyError> {
        self.tool_proxy
            .read()
            .clone()
            .ok_or(ProxyError::NotRegistered)
    }

    pub fn command(&self) -> Result<Arc<dyn PluginCommandProxy>, ProxyError> {
        self.command_proxy
            .read()
            .clone()
            .ok_or(ProxyError::NotRegistered)
    }

    pub fn context(&self) -> Result<Arc<dyn PluginContextProxy>, ProxyError> {
        self.context_proxy
            .read()
            .clone()
            .ok_or(ProxyError::NotRegistered)
    }
}
