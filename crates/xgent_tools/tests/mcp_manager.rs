//! MCP manager 的真实 stdio 集成测试（fixture 子进程）。

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use xgent_settings_core::{McpServerConfig, project::ToolPolicyConfig};
use xgent_tools::mcp::{McpClient, McpManager, McpTransport, StdioTransport};
use xgent_tools::{ToolCtx, ToolExecutor, confirm::ConfirmRequest, executor::ConfirmCallback};

fn fixture(id: &str, mode: Option<&str>) -> McpServerConfig {
    McpServerConfig {
        id: id.to_string(),
        command: env!("CARGO_BIN_EXE_xgent_mcp_fixture").to_string(),
        args: mode.map(|mode| vec![mode.to_string()]).unwrap_or_default(),
        ..Default::default()
    }
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    for _ in 0..300 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("等待 MCP 注册超时");
}

struct NeverConfirm;

#[async_trait::async_trait]
impl ConfirmCallback for NeverConfirm {
    async fn confirm(
        &self,
        _req: ConfirmRequest,
    ) -> oneshot::Receiver<xgent_tools::confirm::ConfirmDecision> {
        panic!("已 Approved 的 MCP 工具不应进入确认");
    }
}

#[tokio::test]
async fn fixture_connects_directly() {
    let root = tempfile::tempdir().expect("临时目录");
    let config = fixture("fixture", None);
    let transport =
        StdioTransport::spawn(&config, root.path(), Duration::from_secs(5)).expect("spawn");
    let client = McpClient::connect(transport.clone(), "fixture")
        .await
        .expect("initialize");
    let tools = client.list_tools().await.expect("list");
    assert_eq!(tools.len(), 2, "tools: {tools:?}");
    transport.close().await;
}

#[tokio::test]
async fn manager_registers_fixture_tools_and_executes_them() {
    let root = tempfile::tempdir().expect("临时目录");
    let executor = Arc::new(ToolExecutor::new(Vec::new()));
    let manager = McpManager::start(
        vec![fixture("fixture", None)],
        executor.clone(),
        root.path().to_path_buf(),
    );

    wait_until(|| manager.registered_tool_ids().len() == 2).await;
    let ids = manager.registered_tool_ids();
    assert!(ids.contains(&"mcp.fixture.echo".to_string()));
    assert!(ids.contains(&"mcp.fixture.fail".to_string()));
    assert!(
        executor
            .schemas()
            .iter()
            .any(|schema| schema.name == "mcp.fixture.echo")
    );

    let policy = ToolPolicyConfig {
        approved: vec!["mcp.fixture.echo".to_string()],
        ..Default::default()
    };
    let ctx = ToolCtx::with_policy(root.path().to_path_buf(), policy);
    let result = executor
        .execute(
            "mcp.fixture.echo",
            serde_json::json!({"text": "hi"}),
            &ctx,
            CancellationToken::new(),
            &NeverConfirm,
            None,
        )
        .await
        .expect("执行");
    assert_eq!(result.output, "echo:hi");
    assert!(!result.is_error);

    assert_eq!(manager.connection_status().len(), 1);
    manager.shutdown().await;
    assert!(manager.connection_status().is_empty());
}

#[tokio::test]
async fn broken_server_does_not_block_good_server() {
    let root = tempfile::tempdir().expect("临时目录");
    let executor = Arc::new(ToolExecutor::new(Vec::new()));
    let broken = McpServerConfig {
        id: "broken".to_string(),
        command: "xgent-definitely-not-a-real-command".to_string(),
        ..Default::default()
    };
    let manager = McpManager::start(
        vec![fixture("good", None), broken],
        executor.clone(),
        root.path().to_path_buf(),
    );
    wait_until(|| manager.registered_tool_ids().len() == 2).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let status = manager.connection_status();
    assert!(
        status
            .iter()
            .any(|(id, connected)| id == "good" && *connected)
    );
    assert!(!status.iter().any(|(id, _)| id == "broken"));
    assert!(
        manager
            .registered_tool_ids()
            .contains(&"mcp.good.echo".to_string())
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn crash_after_init_does_not_register_tools() {
    let root = tempfile::tempdir().expect("临时目录");
    let executor = Arc::new(ToolExecutor::new(Vec::new()));
    let manager = McpManager::start(
        vec![fixture("crash", Some("crash-after-init"))],
        executor.clone(),
        root.path().to_path_buf(),
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(manager.registered_tool_ids().is_empty());
    assert!(manager.connection_status().is_empty());
    manager.shutdown().await;
}

#[tokio::test]
async fn mcp_client_does_not_write_project_files() {
    let root = tempfile::tempdir().expect("临时目录");
    let project_file = root.path().join("keep.txt");
    std::fs::write(&project_file, "unchanged").expect("写文件");
    let executor = Arc::new(ToolExecutor::new(Vec::new()));
    let manager = McpManager::start(
        vec![fixture("fixture", None)],
        executor,
        root.path().to_path_buf(),
    );
    wait_until(|| manager.registered_tool_ids().len() == 2).await;
    manager.shutdown().await;
    assert_eq!(
        std::fs::read_to_string(&project_file).expect("读文件"),
        "unchanged"
    );
}
