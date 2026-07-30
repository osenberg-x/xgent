//! xgent_plugin_host — 插件 ECS 桥接与扩展点适配器。
//!
//! 把插件能力桥接进现有 ECS 体系：`PluginTool`（→ Tool trait）、
//! `PluginCommand`（→ CommandRegistry）、`PluginContextProvider`（→ ContextProvider）。
//!
//! 详见 `doc/design/plugin-system-design.md` §5.3-§5.5 / §13 Step P3。

pub mod command;
pub mod context;
pub mod proxy_impl;
pub mod tool;

use std::sync::Arc;

use bevy::ecs::message::Messages;
use bevy::prelude::*;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use xgent_agent::{
    CommandResultMessage, PluginCommandTriggered, PluginLoadedMessage, PluginUnregisterMessage,
};
use xgent_plugin::{PluginEvent, PluginHost, PluginHostProxy};

pub use command::PluginCommand;
pub use context::PluginContextProvider;
pub use proxy_impl::{
    execute_op, PluginCommandProxyImpl, PluginContextProxyImpl, PluginOp, PluginToolProxyImpl,
};
pub use tool::PluginTool;

/// 插件命令注册表（存储 PluginCommand 实例，供 PluginCommandTriggered 调用）。
#[derive(Resource, Default)]
pub struct PluginCommandRegistry(pub Vec<PluginCommand>);

/// 插件宿主 Resource（由 `xgent_app` 注入）。
#[derive(Resource)]
pub struct PluginHostResource(pub Arc<PluginHost>);

/// 插件事件接收器（由 `xgent_app` 注入，持 `PluginHost::new` 返回的 rx）。
#[derive(Resource)]
pub struct PluginEventRx(pub Mutex<mpsc::UnboundedReceiver<PluginEvent>>);

/// 插件操作接收器（proxy impl → plugin_poll_system）。
///
/// 持 `mpsc::UnboundedReceiver<PluginOp>`，由 `register_proxy_impls` 返回的 rx
/// 包成 Resource 注入。`plugin_poll_system` 每帧 `try_recv` drain（上限 64/帧）。
#[derive(Resource)]
pub struct PluginOpRx(pub Mutex<mpsc::UnboundedReceiver<PluginOp>>);

/// 插件宿主 Bevy Plugin。
///
/// 照设计文档 §13 Step P3。`build()` 内注册 Message/Resource + `plugin_poll_system`。
pub struct PluginHostPlugin;

impl Plugin for PluginHostPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CommandResultMessage>()
            .add_message::<PluginUnregisterMessage>()
            .add_message::<PluginLoadedMessage>()
            .add_message::<PluginCommandTriggered>()
            .init_resource::<PluginCommandRegistry>()
            // PluginOpRx / PluginEventRx / PluginHostResource 由 xgent_app 注入
            // （持 mpsc receiver / Arc<PluginHost>，无法 Default 构造）
            .add_systems(Update, plugin_poll_system);
    }
}

/// 每帧轮询：drain PluginEvent + 执行 PluginOp + 处理 PluginCommandTriggered。
///
/// 对齐 `agent_poll_system`（`xgent_agent/src/agent_loop.rs:14`）：
/// - `try_recv` drain `PluginEvent`（上限 64/帧），发对应 Message；
/// - 执行 `PluginOpQueue` 中的 ops（主线程 `world.resource_mut`）；
/// - 消费 `PluginCommandTriggered` Message，spawn async `PluginCommand::run`。
pub fn plugin_poll_system(world: &mut World) {
    // 1. drain PluginEvent → Message（先 collect 释放 world 不可变借用）
    let events: Vec<PluginEvent> = if let Some(ev_rx) = world.get_resource::<PluginEventRx>() {
        let mut rx = ev_rx.0.lock();
        let mut evs = Vec::new();
        for _ in 0..64 {
            match rx.try_recv() {
                Ok(ev) => evs.push(ev),
                Err(_) => break,
            }
        }
        evs
    } else {
        Vec::new()
    };
    for ev in events {
        handle_plugin_event(world, ev);
    }
    // 2. 执行 PluginOp（从 PluginOpRx mpsc drain，主线程 world.resource_mut）
    let ops: Vec<PluginOp> = if let Some(op_rx) = world.get_resource::<PluginOpRx>() {
        let mut rx = op_rx.0.lock();
        let mut ops = Vec::new();
        for _ in 0..64 {
            match rx.try_recv() {
                Ok(op) => ops.push(op),
                Err(_) => break,
            }
        }
        ops
    } else {
        Vec::new()
    };
    for op in ops {
        execute_op(op, world);
    }

    // 3. 处理 PluginCommandTriggered Message：调 PluginCommand::run（async spawn）
    let triggered: Vec<PluginCommandTriggered> = {
        let mut messages = world.resource_mut::<Messages<PluginCommandTriggered>>();
        messages.drain().collect()
    };
    if !triggered.is_empty() {
        let cmd_reg = world.resource::<PluginCommandRegistry>();
        let host_res = world.get_resource::<PluginHostResource>();
        for t in triggered {
            let cmd = cmd_reg.0.iter().find(|c| c.full_id == t.command_id).cloned();
            if let Some(cmd) = cmd {
                let host = host_res.map(|h| h.0.clone());
                // spawn async run；结果经 host.emit_command_result 回传
                tokio::spawn(async move {
                    let (success, message) = cmd.run().await;
                    if let Some(host) = host {
                        host.emit_command_result(cmd.full_id.clone(), success, message);
                    }
                });
            }
        }
    }

    // 4. 清理 pending_drop 中 in-flight=0 的旧 WasmPlugin 实例（§8.4 升级 in-flight 处理）
    if let Some(host_res) = world.get_resource::<PluginHostResource>() {
        host_res.0.drain_pending_drop();
    }
}

/// 处理单个 PluginEvent：发对应 ECS Message，Unregister 额外触发清理。
///
/// Unregister 经 `execute_op` 调三类 `Unregister*` 清理 ToolExecutor/
/// CommandRegistry/ContextHub（remove_by_prefix），并写 PluginUnregisterMessage
/// 供 UI 刷新。
fn handle_plugin_event(world: &mut World, ev: PluginEvent) {
    match ev {
        PluginEvent::CommandResult {
            command_id,
            success,
            message,
        } => {
            world
                .resource_mut::<Messages<CommandResultMessage>>()
                .write(CommandResultMessage {
                    command_id,
                    success,
                    message,
                });
        }
        PluginEvent::Unregister { plugin_id } => {
            // 清理 ToolExecutor/CommandRegistry/ContextHub（remove_by_prefix）
            execute_op(PluginOp::UnregisterTools { plugin_id: plugin_id.clone() }, world);
            execute_op(PluginOp::UnregisterCommands { plugin_id: plugin_id.clone() }, world);
            execute_op(PluginOp::UnregisterProviders { plugin_id: plugin_id.clone() }, world);
            world
                .resource_mut::<Messages<PluginUnregisterMessage>>()
                .write(PluginUnregisterMessage { plugin_id });
        }
        PluginEvent::Loaded { plugin_id } => {
            world
                .resource_mut::<Messages<PluginLoadedMessage>>()
                .write(PluginLoadedMessage { plugin_id });
        }
    }
}

/// 注册各 proxy impl 到 `PluginHostProxy`。
///
/// 由 `xgent_app` 在创建 `PluginHost` 后、`load_builtin_plugins` 前调用。
/// 创建 `UnboundedSender<PluginOp>` → 三 proxy impl → set 到 proxy。
pub fn register_proxy_impls(proxy: &Arc<PluginHostProxy>) -> mpsc::UnboundedReceiver<PluginOp> {
    let (op_tx, op_rx) = mpsc::unbounded_channel::<PluginOp>();
    proxy.set_tool_proxy(PluginToolProxyImpl::new(op_tx.clone()));
    proxy.set_command_proxy(PluginCommandProxyImpl::new(op_tx.clone()));
    proxy.set_context_proxy(PluginContextProxyImpl::new(op_tx));
    op_rx
}
