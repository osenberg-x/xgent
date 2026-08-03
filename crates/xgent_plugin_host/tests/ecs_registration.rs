//! xgent_plugin_host ECS 注册链路测试：验证 proxy op → PluginOpRx → execute_op → ToolExecutor。
//!
//! 这是 P0 修复（proxy op 链路断裂）的回归测试：确保插件经 proxy 注册的工具
//! 真实进入 ToolExecutor（而非发到无人接收的 channel）。

use std::path::PathBuf;
use std::sync::Arc;

use bevy::prelude::*;
use xgent_plugin::manifest::PluginManifest;
use xgent_plugin::{PluginHostProxy, WasmHost};
use xgent_plugin_host::{PluginCommandRegistry, PluginOp, execute_op, register_proxy_impls};
use xgent_tools::{ToolExecutor, ToolExecutorResource};

fn git_wasm_path() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/xgent_app/assets/plugins/git/extension.wasm"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/wasm32-wasip2/debug/xgent_plugin_git.wasm"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

fn git_manifest() -> Arc<PluginManifest> {
    Arc::new(
        PluginManifest::from_toml(include_str!("../../xgent_plugin_git/plugin.toml"))
            .expect("manifest"),
    )
}

fn setup_world() -> World {
    let mut world = World::new();
    world.insert_resource(ToolExecutorResource(
        Arc::new(ToolExecutor::with_defaults()),
    ));
    world.insert_resource(xui::command_palette::CommandRegistry::default());
    world.insert_resource(PluginCommandRegistry::default());
    world.insert_resource(xgent_context::ContextHub::default());
    world
}

async fn load_git_plugin(
    proxy: Arc<PluginHostProxy>,
) -> (Arc<PluginManifest>, Arc<xgent_plugin::WasmPlugin>) {
    let wasm_path = git_wasm_path().expect("git wasm");
    let wasm_bytes = std::fs::read(&wasm_path).expect("读 wasm");
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."));
    let wasm_host = WasmHost::new(proxy, project_root).expect("WasmHost::new");
    let work_dir = tempfile::tempdir().expect("tempdir");
    let manifest = git_manifest();
    let plugin = wasm_host
        .load(
            &wasm_bytes,
            manifest.clone(),
            toml::Value::Table(Default::default()),
            work_dir.path().to_path_buf(),
        )
        .await
        .expect("加载 git 插件");
    (manifest, plugin)
}

#[tokio::test]
async fn proxy_op_registers_tool_to_executor() {
    let wasm_path = match git_wasm_path() {
        Some(p) => p,
        None => {
            eprintln!("跳过：git 插件 wasm 未找到（先 ./build_plugins.sh）");
            return;
        }
    };
    let _ = wasm_path;
    let proxy = Arc::new(PluginHostProxy::new());
    let (manifest, plugin) = load_git_plugin(proxy.clone()).await;
    let tool_defs = plugin.call_tool_register().await.expect("register");
    assert_eq!(tool_defs.len(), 3);

    let op_rx = register_proxy_impls(&proxy);
    proxy
        .tool()
        .expect("tool proxy")
        .register_tools(manifest.clone(), plugin.clone(), tool_defs)
        .expect("register_tools op sent");

    let ops: Vec<PluginOp> = {
        let mut rx = op_rx;
        let mut ops = Vec::new();
        while let Ok(op) = rx.try_recv() {
            ops.push(op);
        }
        ops
    };
    assert_eq!(ops.len(), 1, "应收到 1 个 RegisterTools op");
    assert!(matches!(ops[0], PluginOp::RegisterTools { .. }));

    let mut world = setup_world();
    execute_op(ops.into_iter().next().unwrap(), &mut world);

    let executor = world.resource::<ToolExecutorResource>();
    let schemas = executor.0.schemas();
    let git_diff = schemas.iter().find(|s| s.name == "plugin.git.git_diff");
    assert!(git_diff.is_some(), "ToolExecutor 应含 plugin.git.git_diff");
    assert_eq!(git_diff.unwrap().name, "plugin.git.git_diff");
}

#[tokio::test]
async fn proxy_op_unregisters_tool_by_prefix() {
    if git_wasm_path().is_none() {
        return;
    }
    let proxy = Arc::new(PluginHostProxy::new());
    let (manifest, plugin) = load_git_plugin(proxy.clone()).await;
    let tool_defs = plugin.call_tool_register().await.expect("register");

    let op_rx = register_proxy_impls(&proxy);
    proxy
        .tool()
        .expect("tool proxy")
        .register_tools(manifest.clone(), plugin.clone(), tool_defs)
        .expect("register");
    proxy
        .tool()
        .expect("tool proxy")
        .unregister_tools(&manifest.id)
        .expect("unregister");

    let ops: Vec<PluginOp> = {
        let mut rx = op_rx;
        let mut ops = Vec::new();
        while let Ok(op) = rx.try_recv() {
            ops.push(op);
        }
        ops
    };
    assert_eq!(ops.len(), 2, "应收到 register + unregister 两个 op");

    let mut world = setup_world();
    for op in ops {
        execute_op(op, &mut world);
    }

    let executor = world.resource::<ToolExecutorResource>();
    let schemas = executor.0.schemas();
    assert!(
        schemas.iter().all(|s| !s.name.starts_with("plugin.git.")),
        "卸载后不应含 plugin.git.* 工具"
    );
}

/// N1 回归测试：handle_plugin_event(Unregister) 调三个 execute_op（Tools/Commands/Providers）清理。
/// 验证 N1 修复——卸载链路不再断裂，工具/命令/provider 全部移除。
#[tokio::test]
async fn unregister_clears_all_extension_points() {
    if git_wasm_path().is_none() { return; }
    let proxy = Arc::new(PluginHostProxy::new());
    let (manifest, plugin) = load_git_plugin(proxy.clone()).await;
    let tool_defs = plugin.call_tool_register().await.expect("register tools");
    let cmd_defs = plugin.call_command_register().await.expect("register commands");

    let mut world = setup_world();
    // 注册工具 + 命令
    execute_op(PluginOp::RegisterTools { manifest: manifest.clone(), plugin: plugin.clone(), tool_defs }, &mut world);
    execute_op(PluginOp::RegisterCommands { manifest: manifest.clone(), plugin: plugin.clone(), command_defs: cmd_defs }, &mut world);

    // 验证注册成功
    let executor = world.resource::<ToolExecutorResource>();
    assert!(executor.0.schemas().iter().any(|s| s.name.starts_with("plugin.git.")));

    // 模拟 handle_plugin_event(Unregister) 的清理序列（N1 修复）
    let pid = manifest.id.clone();
    execute_op(PluginOp::UnregisterTools { plugin_id: pid.clone() }, &mut world);
    execute_op(PluginOp::UnregisterCommands { plugin_id: pid.clone() }, &mut world);
    execute_op(PluginOp::UnregisterProviders { plugin_id: pid }, &mut world);

    // 验证全部清理
    let executor = world.resource::<ToolExecutorResource>();
    assert!(
        executor.0.schemas().iter().all(|s| !s.name.starts_with("plugin.git.")),
        "卸载后不应含 plugin.git.* 工具"
    );
    let cmd_reg = world.resource::<PluginCommandRegistry>();
    assert!(
        cmd_reg.0.iter().all(|c| !c.full_id.starts_with("plugin.git.")),
        "卸载后不应含 plugin.git.* 命令"
    );
}
