//! Proxy 实现：把 `xgent_plugin::PluginHostProxy` 的三 trait 对接到 ECS。
//!
//! 照设计文档 §5.3-§5.5 + Step P4 "proxy 取用机制"。
//!
//! **取用机制（硬性）**：proxy impl 不在 build 时抓 `ResMut`（跨 tokio task
//! 持有破坏 Send+Sync）。改为持 `Sender<PluginOp>`，发指令到 `PluginPollSystem`
//! （主线程 system）内 `world.resource_mut::<T>()` 执行。
//!
//! **注册路径**：`register_tools` 等在 `PluginPollSystem` 主线程内调
//! `world.resource_mut::<ToolExecutor>()` 后 `register(Arc<PluginTool>)`。
//! **卸载路径**：`unregister_tools` 发 `PluginUnregisterMessage` 到 ECS，
//! `PluginPollSystem` 消费后 `remove_by_prefix`。

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use xgent_plugin::{
    PluginCommandProxy, PluginContextProxy, PluginHostProxy, PluginManifest,
    PluginToolProxy, ProxyError, WasmPlugin, WitCommandDef, WitContextProviderDef, WitToolDef,
};

use crate::command::PluginCommand;
use crate::context::PluginContextProvider;
use crate::tool::PluginTool;

/// 主线程执行的插件操作（经 channel 从 proxy impl 发到 PluginPollSystem）。
pub enum PluginOp {
    /// 注册工具到 ToolExecutor。
    RegisterTools {
        manifest: Arc<PluginManifest>,
        plugin: Arc<WasmPlugin>,
        tool_defs: Vec<WitToolDef>,
    },
    /// 卸载插件工具（按 plugin.<id>. 前缀）。
    UnregisterTools { plugin_id: String },
    /// 注册命令到 CommandRegistry。
    RegisterCommands {
        manifest: Arc<PluginManifest>,
        plugin: Arc<WasmPlugin>,
        command_defs: Vec<WitCommandDef>,
    },
    /// 卸载命令。
    UnregisterCommands { plugin_id: String },
    /// 注册 ContextProvider 到 ContextHub。
    RegisterProviders {
        manifest: Arc<PluginManifest>,
        plugin: Arc<WasmPlugin>,
        provider_defs: Vec<WitContextProviderDef>,
        project_root: std::path::PathBuf,
    },
    /// 卸载 ContextProvider。
    UnregisterProviders { plugin_id: String },
}

/// 工具 proxy impl：发 PluginOp 到 PluginPollSystem。
pub struct PluginToolProxyImpl {
    op_tx: mpsc::UnboundedSender<PluginOp>,
}

impl PluginToolProxyImpl {
    pub fn new(op_tx: mpsc::UnboundedSender<PluginOp>) -> Arc<Self> {
        Arc::new(Self { op_tx })
    }
}

impl PluginToolProxy for PluginToolProxyImpl {
    fn register_tools(
        &self,
        manifest: Arc<PluginManifest>,
        plugin: Arc<WasmPlugin>,
        tool_defs: Vec<WitToolDef>,
    ) -> Result<(), ProxyError> {
        self.op_tx
            .send(PluginOp::RegisterTools {
                manifest,
                plugin,
                tool_defs,
            })
            .map_err(|_| ProxyError::Failed("PluginPollSystem 已退出".into()))
    }

    fn unregister_tools(&self, plugin_id: &str) -> Result<(), ProxyError> {
        self.op_tx
            .send(PluginOp::UnregisterTools {
                plugin_id: plugin_id.to_string(),
            })
            .map_err(|_| ProxyError::Failed("PluginPollSystem 已退出".into()))
    }
}

/// 命令 proxy impl。
pub struct PluginCommandProxyImpl {
    op_tx: mpsc::UnboundedSender<PluginOp>,
}

impl PluginCommandProxyImpl {
    pub fn new(op_tx: mpsc::UnboundedSender<PluginOp>) -> Arc<Self> {
        Arc::new(Self { op_tx })
    }
}

impl PluginCommandProxy for PluginCommandProxyImpl {
    fn register_commands(
        &self,
        manifest: Arc<PluginManifest>,
        plugin: Arc<WasmPlugin>,
        command_defs: Vec<WitCommandDef>,
    ) -> Result<(), ProxyError> {
        self.op_tx
            .send(PluginOp::RegisterCommands {
                manifest,
                plugin,
                command_defs,
            })
            .map_err(|_| ProxyError::Failed("PluginPollSystem 已退出".into()))
    }

    fn unregister_commands(&self, plugin_id: &str) -> Result<(), ProxyError> {
        self.op_tx
            .send(PluginOp::UnregisterCommands {
                plugin_id: plugin_id.to_string(),
            })
            .map_err(|_| ProxyError::Failed("PluginPollSystem 已退出".into()))
    }
}

/// ContextProvider proxy impl。
pub struct PluginContextProxyImpl {
    op_tx: mpsc::UnboundedSender<PluginOp>,
}

impl PluginContextProxyImpl {
    pub fn new(op_tx: mpsc::UnboundedSender<PluginOp>) -> Arc<Self> {
        Arc::new(Self { op_tx })
    }
}

impl PluginContextProxy for PluginContextProxyImpl {
    fn register_providers(
        &self,
        manifest: Arc<PluginManifest>,
        plugin: Arc<WasmPlugin>,
        provider_defs: Vec<WitContextProviderDef>,
        project_root: std::path::PathBuf,
    ) -> Result<(), ProxyError> {
        self.op_tx
            .send(PluginOp::RegisterProviders {
                manifest,
                plugin,
                provider_defs,
                project_root,
            })
            .map_err(|_| ProxyError::Failed("PluginPollSystem 已退出".into()))
    }

    fn unregister_providers(&self, plugin_id: &str) -> Result<(), ProxyError> {
        self.op_tx
            .send(PluginOp::UnregisterProviders {
                plugin_id: plugin_id.to_string(),
            })
            .map_err(|_| ProxyError::Failed("PluginPollSystem 已退出".into()))
    }
}

/// 在主线程 system 内执行一个 PluginOp（操作 ToolExecutor/CommandRegistry/ContextHub）。
///
/// 由 `plugin_poll_system` 调用，传入 `&mut World` 取用 Resources。
pub fn execute_op(
    op: PluginOp,
    world: &mut bevy::prelude::World,
) {
    use bevy::prelude::Resource;
    match op {
        PluginOp::RegisterTools {
            manifest,
            plugin,
            tool_defs,
        } => {
            let executor = world.resource::<xgent_tools::ToolExecutorResource>();
            for td in &tool_defs {
                match PluginTool::new(&manifest.id, td, plugin.clone()) {
                    Ok(tool) => executor.0.register(std::sync::Arc::new(tool)),
                    Err(e) => tracing::warn!(
                        plugin = %manifest.id,
                        tool = %td.id,
                        error = %e,
                        "PluginTool 构造失败"
                    ),
                }
            }
        }
        PluginOp::UnregisterTools { plugin_id } => {
            let prefix = format!("plugin.{plugin_id}.");
            let executor = world.resource::<xgent_tools::ToolExecutorResource>();
            executor.0.remove_by_prefix(&prefix);
        }
        PluginOp::RegisterCommands {
            manifest,
            plugin,
            command_defs,
        } => {
            // 先注册 PaletteCommand 到 CommandRegistry
            {
                let mut registry = world.resource_mut::<xui::command_palette::CommandRegistry>();
                for cd in &command_defs {
                    let full_id = format!("plugin.{}.{}", manifest.id, cd.id);
                    let palette_cmd = xui::command_palette::PaletteCommand {
                        id: full_id,
                        label: cd.label.clone(),
                        kind: xui::command_palette::CommandKind::Action,
                    };
                    if let Err(e) = registry.try_register(palette_cmd) {
                        tracing::warn!(plugin = %manifest.id, error = %e, "命令注册失败");
                    }
                }
            }
            // 再存 PluginCommand 到独立 registry（供 PluginCommandTriggered 调用）
            {
                let mut cmd_reg = world.resource_mut::<crate::PluginCommandRegistry>();
                for cd in &command_defs {
                    let full_id = format!("plugin.{}.{}", manifest.id, cd.id);
                    let cmd = PluginCommand {
                        full_id,
                        short_id: cd.id.clone(),
                        label: cd.label.clone(),
                        plugin: plugin.clone(),
                    };
                    cmd_reg.0.push(cmd);
                }
            }
        }
        PluginOp::UnregisterCommands { plugin_id } => {
            let prefix = format!("plugin.{plugin_id}.");
            let mut registry = world.resource_mut::<xui::command_palette::CommandRegistry>();
            registry.remove_by_prefix(&prefix);
            let mut cmd_reg = world.resource_mut::<crate::PluginCommandRegistry>();
            cmd_reg.0.retain(|c| !c.full_id.starts_with(&prefix));
        }
        PluginOp::RegisterProviders {
            manifest,
            plugin,
            provider_defs,
            project_root,
        } => {
            let hub = world.resource::<xgent_context::ContextHub>();
            for pd in &provider_defs {
                let full_id = format!("plugin.{}.{}", manifest.id, pd.id);
                let provider = PluginContextProvider {
                    full_id: full_id.clone(),
                    short_id: pd.id.clone(),
                    plugin_id: manifest.id.clone(),
                    project_root: project_root.clone(),
                    plugin: plugin.clone(),
                };
                hub.register_provider(full_id, std::sync::Arc::new(provider));
            }
        }
        PluginOp::UnregisterProviders { plugin_id } => {
            let prefix = format!("plugin.{plugin_id}.");
            let hub = world.resource::<xgent_context::ContextHub>();
            hub.remove_by_prefix(&prefix);
        }
    }
    // 静默未用 trait
    let _ = std::marker::PhantomData::<Mutex<()>>;
}
