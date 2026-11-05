//! MCP 连接管理：并发连接、工具注册、失败隔离与退出清理。

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use tokio::task::JoinHandle;
use xgent_settings_core::McpServerConfig;

use crate::ToolExecutor;
use crate::tool::Tool;

use super::McpTransport;
use super::client::McpClient;
use super::tool::McpTool;
use super::transport::StdioTransport;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(120);

struct ManagedConnection {
    server_id: String,
    transport: Arc<dyn McpTransport>,
    tool_ids: Vec<String>,
}

/// MCP 连接管理器。启动后台连接，持有连接供退出时清理。
pub struct McpManager {
    connections: Arc<Mutex<Vec<ManagedConnection>>>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    registration: tokio::sync::Mutex<()>,
    shutting_down: AtomicBool,
}

impl McpManager {
    /// 启动所有 `enabled && auto_start` 的 Server；不等待连接完成。
    pub fn start(
        servers: Vec<McpServerConfig>,
        executor: Arc<ToolExecutor>,
        project_root: PathBuf,
    ) -> Arc<Self> {
        let manager = Arc::new(Self {
            connections: Arc::new(Mutex::new(Vec::new())),
            tasks: Mutex::new(Vec::new()),
            registration: tokio::sync::Mutex::new(()),
            shutting_down: AtomicBool::new(false),
        });
        for server in servers {
            if !server.enabled || !server.auto_start {
                continue;
            }
            let manager_for_task = manager.clone();
            let executor = executor.clone();
            let root = project_root.clone();
            let task = tokio::spawn(async move {
                manager_for_task.connect_one(server, executor, root).await;
            });
            manager.tasks.lock().push(task);
        }
        manager
    }

    async fn connect_one(
        &self,
        server: McpServerConfig,
        executor: Arc<ToolExecutor>,
        project_root: PathBuf,
    ) {
        if self.shutting_down.load(Ordering::SeqCst) {
            return;
        }
        let server_id = server.id.clone();
        let transport = match StdioTransport::spawn(&server, &project_root, CONNECT_TIMEOUT) {
            Ok(transport) => transport,
            Err(error) => {
                tracing::warn!(server = %server_id, %error, "MCP Server 启动失败");
                return;
            }
        };
        let client = match McpClient::connect(transport.clone(), server_id.clone()).await {
            Ok(client) => Arc::new(client),
            Err(error) => {
                tracing::warn!(server = %server_id, %error, "MCP 初始化失败");
                transport.close().await;
                return;
            }
        };
        let defs = match client.list_tools().await {
            Ok(defs) => defs,
            Err(error) => {
                tracing::warn!(server = %server_id, %error, "MCP 工具发现失败");
                transport.close().await;
                return;
            }
        };

        if self.shutting_down.load(Ordering::SeqCst) {
            transport.close().await;
            return;
        }
        let _guard = self.registration.lock().await;
        if self.shutting_down.load(Ordering::SeqCst) {
            drop(_guard);
            transport.close().await;
            return;
        }
        let mut existing: HashSet<String> = executor
            .policy_targets()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let mut tool_ids = Vec::new();
        for def in defs {
            let Some(tool) = McpTool::new(client.clone(), def) else {
                tracing::warn!(server = %server_id, "MCP 工具名/schema 非法，已跳过");
                continue;
            };
            let id = tool.id().to_string();
            if !existing.insert(id.clone()) {
                tracing::warn!(server = %server_id, tool = %id, "MCP 工具 id 冲突，已跳过");
                continue;
            }
            executor.register(Arc::new(tool));
            tool_ids.push(id);
        }
        drop(_guard);

        if self.shutting_down.load(Ordering::SeqCst) {
            executor.remove_by_prefix(&format!("mcp__{server_id}__"));
            transport.close().await;
            return;
        }
        let leftover = {
            let mut connections = self.connections.lock();
            connections.push(ManagedConnection {
                server_id: server_id.clone(),
                transport,
                tool_ids: tool_ids.clone(),
            });
            if self.shutting_down.load(Ordering::SeqCst) {
                connections.pop()
            } else {
                None
            }
        };
        if let Some(connection) = leftover {
            executor.remove_by_prefix(&format!("mcp__{server_id}__"));
            connection.transport.close().await;
            return;
        }
        tracing::info!(
            server = %server_id,
            tools = tool_ids.len(),
            "MCP Server 已连接"
        );
    }

    /// 已注册的 MCP 工具 id（用于测试/诊断）。
    pub fn registered_tool_ids(&self) -> Vec<String> {
        self.connections
            .lock()
            .iter()
            .flat_map(|connection| connection.tool_ids.iter().cloned())
            .collect()
    }

    /// Server id 与连接状态（用于测试/诊断）。
    pub fn connection_status(&self) -> Vec<(String, bool)> {
        self.connections
            .lock()
            .iter()
            .map(|connection| {
                (
                    connection.server_id.clone(),
                    connection.transport.is_connected(),
                )
            })
            .collect()
    }

    /// 关闭全部连接并终止子进程。
    pub async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        let tasks: Vec<JoinHandle<()>> = self.tasks.lock().drain(..).collect();
        for task in tasks {
            task.abort();
        }
        let connections: Vec<ManagedConnection> = self.connections.lock().drain(..).collect();
        for connection in connections {
            connection.transport.close().await;
        }
    }
}

impl Drop for McpManager {
    fn drop(&mut self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        for task in self.tasks.get_mut().drain(..) {
            task.abort();
        }
        for connection in self.connections.lock().drain(..) {
            drop(connection);
        }
    }
}

impl std::fmt::Debug for McpManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpManager")
            .field("connections", &self.connection_status())
            .finish()
    }
}
