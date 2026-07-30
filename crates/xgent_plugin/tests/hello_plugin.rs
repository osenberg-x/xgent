//! xgent_plugin 集成测试：加载 hello 插件，验证 register + execute 链路。
//!
//! 照 Step 3 验证：加载 Step 2 产出的 hello.wasm，call register 返回 1 个 ToolDef，
//! call_tool_execute("hello", "{}", cancel) 返回 Ok。

use std::path::PathBuf;
use std::sync::Arc;

use xgent_plugin::{PluginHostProxy, WasmHost};
use xgent_plugin::manifest::PluginManifest;

/// 定位 hello 插件 wasm（cargo build --target wasm32-wasip2 产出）。
fn hello_wasm_path() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/wasm32-wasip2/debug/xgent_plugin_hello.wasm"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/wasm32-wasip2/release/xgent_plugin_hello.wasm"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

fn hello_manifest() -> Arc<PluginManifest> {
    Arc::new(
        PluginManifest::from_toml(
            r#"
id = "hello"
name = "Hello 测试插件"
version = "0.1.0"
schema_version = 1
[lib]
kind = "rust"
[[tools]]
id = "hello"
tier = "read"
description = "says hello"
[permissions]
fs-read = []
fs-write = []
command = []
"#,
        )
        .expect("manifest"),
    )
}

#[tokio::test]
async fn load_and_call_hello_plugin() {
    let wasm_path = match hello_wasm_path() {
        Some(p) => p,
        None => {
            eprintln!("跳过：hello.wasm 未找到（先 cargo build -p xgent_plugin_hello --target wasm32-wasip2）");
            return;
        }
    };
    let wasm_bytes = std::fs::read(&wasm_path).expect("读 wasm");

    let proxy = Arc::new(PluginHostProxy::new());
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let wasm_host = WasmHost::new(proxy, project_root).expect("WasmHost::new");

    let work_dir = tempfile::tempdir().expect("tempdir");
    let plugin = wasm_host
        .load(
            &wasm_bytes,
            hello_manifest(),
            toml::Value::Table(Default::default()),
            work_dir.path().to_path_buf(),
        )
        .await
        .expect("加载插件");

    // register 应返回 1 个工具
    let tools = plugin.call_tool_register().await.expect("register");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].id, "hello");

    // execute 应返回 Ok（JSON 含 output 字段）
    let result = plugin
        .call_tool_execute("hello", r#"{"name":"world"}"#, tokio_util::sync::CancellationToken::new(), None)
        .await
        .expect("execute");
    assert!(result.contains("hello: world"), "结果应含 hello: world, got: {result}");
}
