//! MCP 启动接线：加载配置、后台连接、持有 manager、退出清理。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use bevy::prelude::*;
use xgent_agent::bridge::AgentBridge;
use xgent_settings_core::mcp::load_mcp_config;
use xgent_settings_core::paths::{mcp_config_file, project_mcp_config_file};
use xgent_tools::ToolExecutor;
use xgent_tools::mcp::McpManager;

/// 应用持有的 MCP manager；无配置时为 `None`。
#[derive(Resource)]
pub struct McpRuntime(pub Option<Arc<McpManager>>);

/// 从默认路径加载配置并启动后台连接。
pub fn start(
    project_root: &Path,
    executor: Arc<ToolExecutor>,
    handle: &tokio::runtime::Handle,
) -> Option<Arc<McpManager>> {
    start_with_paths(
        &mcp_config_file(),
        &project_mcp_config_file(project_root),
        project_root,
        executor,
        handle,
    )
}

fn start_with_paths(
    global_path: &Path,
    project_path: &Path,
    project_root: &Path,
    executor: Arc<ToolExecutor>,
    handle: &tokio::runtime::Handle,
) -> Option<Arc<McpManager>> {
    let env: BTreeMap<String, String> = std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .collect();
    let outcome = load_mcp_config(global_path, project_path, &env);
    for warning in &outcome.warnings {
        tracing::warn!("{warning}");
    }
    if outcome.config.servers.is_empty() {
        return None;
    }
    let servers = outcome.config.servers.into_values().collect();
    let _guard = handle.enter();
    Some(McpManager::start(
        servers,
        executor,
        project_root.to_path_buf(),
    ))
}

/// AppExit 时关闭 MCP 子进程。
pub fn shutdown_on_exit(
    mut exit: MessageReader<AppExit>,
    runtime: Option<Res<McpRuntime>>,
    bridge: Res<AgentBridge>,
) {
    for _ in exit.read() {
        let Some(runtime) = runtime.as_ref() else {
            continue;
        };
        let Some(manager) = runtime.0.as_ref() else {
            continue;
        };
        bridge.runtime.block_on(manager.shutdown());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("测试 runtime")
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("目录");
        }
        fs::write(path, contents).expect("写文件");
    }

    #[test]
    fn missing_config_is_noop() {
        let temp = tempfile::tempdir().expect("临时目录");
        let rt = runtime();
        let executor = Arc::new(ToolExecutor::new(Vec::new()));
        let manager = start_with_paths(
            &temp.path().join("global.toml"),
            &temp.path().join("project.toml"),
            temp.path(),
            executor,
            rt.handle(),
        );
        assert!(manager.is_none());
    }

    #[test]
    fn corrupt_config_is_noop_without_panic() {
        let temp = tempfile::tempdir().expect("临时目录");
        let global = temp.path().join("global.toml");
        write(
            &global,
            "[[servers
id = ",
        );
        let rt = runtime();
        let executor = Arc::new(ToolExecutor::new(Vec::new()));
        let manager = start_with_paths(
            &global,
            &temp.path().join("project.toml"),
            temp.path(),
            executor,
            rt.handle(),
        );
        assert!(manager.is_none());
    }

    #[test]
    fn disabled_server_creates_manager_without_connections() {
        let temp = tempfile::tempdir().expect("临时目录");
        let global = temp.path().join("global.toml");
        write(
            &global,
            "[[servers]]\nid = \"off\"\ncommand = \"server\"\nenabled = false\n",
        );
        let rt = runtime();
        let executor = Arc::new(ToolExecutor::new(Vec::new()));
        let manager = start_with_paths(
            &global,
            &temp.path().join("project.toml"),
            temp.path(),
            executor,
            rt.handle(),
        )
        .expect("有配置应创建 manager");
        assert!(
            !manager
                .connection_status()
                .iter()
                .any(|(id, _)| id == "off")
        );
        rt.block_on(manager.shutdown());
    }
}
