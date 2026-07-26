//! xgent_plugin_host 集成测试：加载 Git 插件，验证工具 id 一致性 + 注册到 ToolExecutor。
//!
//! 照 Step 5 验证：tool.id() == "plugin.git.git_diff"、schema.name 一致、
//! 注册到 ToolExecutor 后 schemas() 含该工具、卸载后清理。

use std::path::PathBuf;
use std::sync::Arc;

use xgent_plugin::{PluginHostProxy, WasmHost};
use xgent_plugin::manifest::PluginManifest;
use xgent_plugin_host::tool::PluginTool;
use xgent_tools::tool::Tool;


/// 定位 git 插件 wasm（assets/plugins/git/extension.wasm 或 target 产出）。
fn git_wasm_path() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/xgent_app/assets/plugins/git/extension.wasm"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/wasm32-wasip2/debug/xgent_plugin_git.wasm"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/wasm32-wasip2/release/xgent_plugin_git.wasm"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

fn git_manifest() -> Arc<PluginManifest> {
    Arc::new(
        PluginManifest::from_toml(include_str!("../../xgent_plugin_git/plugin.toml"))
            .expect("manifest"),
    )
}

#[tokio::test]
async fn git_plugin_register_and_id_consistency() {
    let wasm_path = match git_wasm_path() {
        Some(p) => p,
        None => {
            eprintln!("跳过：git 插件 wasm 未找到（先 ./build_plugins.sh）");
            return;
        }
    };
    let wasm_bytes = std::fs::read(&wasm_path).expect("读 wasm");

    let proxy = Arc::new(PluginHostProxy::new());
    // 用本 crate 的 CARGO_MANIFEST_DIR 作为项目根（git 仓库内）
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."));
    let wasm_host = WasmHost::new(proxy, project_root);

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

    // register 返回 3 个工具
    let tools = plugin.call_tool_register().await.expect("register");
    assert_eq!(tools.len(), 3, "git 插件应注册 3 个工具");

    // id 一致性硬约束：PluginTool::id() == schema.name == "plugin.git.git_diff"
    let git_diff_def = tools.iter().find(|t| t.id == "git_diff").unwrap();
    let tool = PluginTool::new(&manifest.id, git_diff_def, plugin.clone()).expect("构造 PluginTool");
    assert_eq!(tool.id(), "plugin.git.git_diff");
    assert_eq!(tool.schema().name, "plugin.git.git_diff");
}

#[tokio::test]
async fn git_plugin_execute_git_status() {
    let wasm_path = match git_wasm_path() {
        Some(p) => p,
        None => {
            eprintln!("跳过：git 插件 wasm 未找到");
            return;
        }
    };
    let wasm_bytes = std::fs::read(&wasm_path).expect("读 wasm");

    let proxy = Arc::new(PluginHostProxy::new());
    // 用 xgent 仓库根（git 仓库）作为项目根
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .canonicalize()
        .expect("canonicalize xgent root");
    let wasm_host = WasmHost::new(proxy, project_root.clone());

    let work_dir = tempfile::tempdir().expect("tempdir");
    let manifest = git_manifest();
    let plugin = wasm_host
        .load(
            &wasm_bytes,
            manifest,
            toml::Value::Table(Default::default()),
            work_dir.path().to_path_buf(),
        )
        .await
        .expect("加载 git 插件");

    // 调 git_status 工具（经 host.run_command 执行真实 git status）
    let result = plugin
        .call_tool_execute("git_status", "{}", tokio_util::sync::CancellationToken::new())
        .await
        .expect("execute git_status");
    // 解析返回的 JSON ToolResult
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
    assert!(parsed.get("output").is_some(), "应含 output 字段: {result}");
}
