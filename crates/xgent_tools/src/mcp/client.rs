//! MCP 2025-06-18 协议层：初始化、工具发现与调用。

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::{McpError, McpTransport};

/// MCP 协议版本（首期只支持有状态握手）。
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
/// 单个 Server 最多接受的工具数。
pub const MAX_TOOLS_PER_SERVER: usize = 256;
/// 工具列表最大页数，防止异常 Server 用不断变化的 cursor 无限翻页。
pub const MAX_LIST_PAGES: usize = 32;
const INIT_TIMEOUT: Duration = Duration::from_secs(30);
const LIST_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// 从 `tools/list` 解析出的工具定义。
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolDef {
    /// 原始工具名（调用时回传）。
    pub name: String,
    /// 人类可读描述。
    pub description: String,
    /// JSON Schema。
    pub input_schema: Value,
}

/// 工具调用结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCallResult {
    /// 拼接后的文本输出。
    pub output: String,
    /// MCP 的 `isError` 标记。
    pub is_error: bool,
}

/// 一个已初始化的 MCP 连接。
pub struct McpClient {
    transport: Arc<dyn McpTransport>,
    server_id: String,
    protocol_version: String,
}

impl McpClient {
    /// 用已有传输构造（不执行握手；测试与已握手场景使用）。
    pub fn new(transport: Arc<dyn McpTransport>, server_id: impl Into<String>) -> Self {
        Self {
            transport,
            server_id: server_id.into(),
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
        }
    }

    /// 完成 initialize / initialized 握手。
    pub async fn connect(
        transport: Arc<dyn McpTransport>,
        server_id: impl Into<String>,
    ) -> Result<Self, McpError> {
        let server_id = server_id.into();
        let result = transport
            .request_with_timeout(
                "initialize",
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {
                        "name": "xgent",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                }),
                INIT_TIMEOUT,
            )
            .await?;
        let protocol_version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(MCP_PROTOCOL_VERSION)
            .to_string();
        if protocol_version != MCP_PROTOCOL_VERSION {
            return Err(McpError::Protocol(format!(
                "不支持的 MCP 协议版本：{protocol_version}"
            )));
        }
        transport
            .notify("notifications/initialized", json!({}))
            .await;
        Ok(Self {
            transport,
            server_id,
            protocol_version,
        })
    }

    /// Server id。
    pub fn server_id(&self) -> &str {
        &self.server_id
    }

    /// 协商后的协议版本。
    pub fn protocol_version(&self) -> &str {
        &self.protocol_version
    }

    /// 发现全部工具；跟随 `nextCursor`，最多 [`MAX_TOOLS_PER_SERVER`] 个。
    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>, McpError> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen_cursors = BTreeSet::new();
        let mut pages = 0usize;
        loop {
            pages += 1;
            if pages > MAX_LIST_PAGES {
                tracing::warn!(
                    server = %self.server_id,
                    limit = MAX_LIST_PAGES,
                    "MCP tools/list 超过页数上限，已停止翻页"
                );
                break;
            }
            let params = cursor
                .as_ref()
                .map(|cursor| json!({ "cursor": cursor }))
                .unwrap_or_else(|| json!({}));
            let result = self
                .transport
                .request_with_timeout("tools/list", params, LIST_TIMEOUT)
                .await?;
            let Some(items) = result.get("tools").and_then(Value::as_array) else {
                return Err(McpError::Protocol(
                    "tools/list 响应缺少 tools 数组".to_string(),
                ));
            };
            for item in items {
                if tools.len() >= MAX_TOOLS_PER_SERVER {
                    tracing::warn!(
                        server = %self.server_id,
                        limit = MAX_TOOLS_PER_SERVER,
                        "MCP 工具数超过上限，已截断"
                    );
                    return Ok(tools);
                }
                match parse_tool(item) {
                    Some(tool) => tools.push(tool),
                    None => tracing::warn!(
                        server = %self.server_id,
                        "MCP 工具缺少 name 或对象型 inputSchema，已跳过"
                    ),
                }
            }
            let next = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|cursor| !cursor.is_empty());
            match next {
                Some(next) if seen_cursors.insert(next.to_string()) => {
                    cursor = Some(next.to_string());
                }
                _ => break,
            }
        }
        Ok(tools)
    }

    /// 调用工具；`cancel` 存在时支持取消通知。
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
        cancel: Option<&CancellationToken>,
    ) -> Result<McpCallResult, McpError> {
        let params = json!({"name": name, "arguments": arguments});
        let result = match cancel {
            Some(cancel) => {
                self.transport
                    .request_cancellable("tools/call", params, CALL_TIMEOUT, cancel)
                    .await?
            }
            None => {
                self.transport
                    .request_with_timeout("tools/call", params, CALL_TIMEOUT)
                    .await?
            }
        };
        let is_error = result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(McpCallResult {
            output: text_from_result(&result),
            is_error,
        })
    }
}

fn parse_tool(value: &Value) -> Option<McpToolDef> {
    let name = value.get("name").and_then(Value::as_str)?.trim();
    if name.is_empty() {
        return None;
    }
    let input_schema = value.get("inputSchema")?;
    if !input_schema.is_object() {
        return None;
    }
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some(McpToolDef {
        name: name.to_string(),
        description,
        input_schema: input_schema.clone(),
    })
}

fn text_from_result(result: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(content) = result.get("content").and_then(Value::as_array) {
        for item in content {
            if item.get("type").and_then(Value::as_str) == Some("text")
                && let Some(text) = item.get("text").and_then(Value::as_str)
            {
                parts.push(text.to_string());
            }
        }
    }
    if parts.is_empty() {
        result.to_string()
    } else {
        parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use parking_lot::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    type Handler = Arc<dyn Fn(&str, &Value) -> Result<Value, McpError> + Send + Sync>;

    struct FakeTransport {
        handler: Handler,
        requests: Mutex<Vec<(String, Value)>>,
        notifications: Mutex<Vec<(String, Value)>>,
        connected: AtomicBool,
    }

    impl FakeTransport {
        fn new(handler: Handler) -> Arc<Self> {
            Arc::new(Self {
                handler,
                requests: Mutex::new(Vec::new()),
                notifications: Mutex::new(Vec::new()),
                connected: AtomicBool::new(true),
            })
        }
    }

    #[async_trait]
    impl McpTransport for FakeTransport {
        async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
            self.requests
                .lock()
                .push((method.to_string(), params.clone()));
            (self.handler)(method, &params)
        }

        async fn notify(&self, method: &str, params: Value) {
            self.notifications.lock().push((method.to_string(), params));
        }

        async fn close(&self) {
            self.connected.store(false, Ordering::SeqCst);
        }

        fn is_connected(&self) -> bool {
            self.connected.load(Ordering::SeqCst)
        }
    }

    fn tool(name: &str) -> Value {
        json!({
            "name": name,
            "description": format!("{name} desc"),
            "inputSchema": {"type": "object"},
        })
    }

    #[tokio::test]
    async fn connect_sends_initialize_then_initialized() {
        let transport = FakeTransport::new(Arc::new(|method, params| {
            assert_eq!(method, "initialize");
            assert_eq!(params["protocolVersion"], MCP_PROTOCOL_VERSION);
            Ok(json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "serverInfo": {"name": "fake"},
            }))
        }));
        let client = McpClient::connect(transport.clone(), "fake")
            .await
            .expect("握手");
        assert_eq!(client.protocol_version(), MCP_PROTOCOL_VERSION);
        let notifications = transport.notifications.lock();
        assert_eq!(notifications[0].0, "notifications/initialized");
    }

    #[tokio::test]
    async fn list_tools_follows_cursor() {
        let transport = FakeTransport::new(Arc::new(|method, params| {
            if method == "initialize" {
                return Ok(json!({"protocolVersion": MCP_PROTOCOL_VERSION}));
            }
            assert_eq!(method, "tools/list");
            if params.get("cursor").is_none() {
                Ok(json!({"tools": [tool("a"), tool("b")], "nextCursor": "p2"}))
            } else {
                assert_eq!(params["cursor"], "p2");
                Ok(json!({"tools": [tool("c")]}))
            }
        }));
        let client = McpClient::connect(transport, "fake").await.expect("握手");
        let tools = client.list_tools().await.expect("列表");
        assert_eq!(
            tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }

    #[tokio::test]
    async fn invalid_tools_are_skipped() {
        let transport = FakeTransport::new(Arc::new(|method, _| {
            if method == "initialize" {
                return Ok(json!({"protocolVersion": MCP_PROTOCOL_VERSION}));
            }
            Ok(json!({"tools": [
                tool("ok"),
                {"name": "no-schema"},
                {"name": "", "inputSchema": {"type": "object"}},
                {"name": "bad-schema", "inputSchema": "nope"},
            ]}))
        }));
        let client = McpClient::connect(transport, "fake").await.expect("握手");
        let tools = client.list_tools().await.expect("列表");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "ok");
    }

    #[tokio::test]
    async fn list_tools_is_capped() {
        let many: Vec<Value> = (0..300).map(|i| tool(&format!("t{i}"))).collect();
        let transport = FakeTransport::new(Arc::new(move |method, _| {
            if method == "initialize" {
                return Ok(json!({"protocolVersion": MCP_PROTOCOL_VERSION}));
            }
            Ok(json!({"tools": many.clone()}))
        }));
        let client = McpClient::connect(transport, "fake").await.expect("握手");
        let tools = client.list_tools().await.expect("列表");
        assert_eq!(tools.len(), MAX_TOOLS_PER_SERVER);
    }

    #[tokio::test]
    async fn call_tool_joins_text_and_maps_is_error() {
        let transport = FakeTransport::new(Arc::new(|method, params| {
            if method == "initialize" {
                return Ok(json!({"protocolVersion": MCP_PROTOCOL_VERSION}));
            }
            assert_eq!(method, "tools/call");
            assert_eq!(params["name"], "read");
            Ok(json!({
                "content": [
                    {"type": "text", "text": "line1"},
                    {"type": "image", "data": "xx"},
                    {"type": "text", "text": "line2"},
                ],
                "isError": true,
            }))
        }));
        let client = McpClient::connect(transport, "fake").await.expect("握手");
        let result = client
            .call_tool("read", json!({"path": "a"}), None)
            .await
            .expect("调用");
        assert_eq!(result.output, "line1\nline2");
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn list_tools_stops_when_cursor_never_ends() {
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter_for_handler = counter.clone();
        let transport = FakeTransport::new(Arc::new(move |method, _| {
            if method == "initialize" {
                return Ok(json!({"protocolVersion": MCP_PROTOCOL_VERSION}));
            }
            let page = counter_for_handler.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(json!({"tools": [], "nextCursor": format!("page-{page}")}))
        }));
        let client = McpClient::connect(transport.clone(), "fake")
            .await
            .expect("握手");
        let tools = client.list_tools().await.expect("列表");
        assert!(tools.is_empty());
        let list_calls = transport
            .requests
            .lock()
            .iter()
            .filter(|(method, _)| method == "tools/list")
            .count();
        assert_eq!(list_calls, MAX_LIST_PAGES);
    }

    #[tokio::test]
    async fn unsupported_protocol_version_is_rejected() {
        let transport = FakeTransport::new(Arc::new(|_, _| {
            Ok(json!({"protocolVersion": "2026-07-28"}))
        }));
        let result = McpClient::connect(transport, "fake").await;
        assert!(matches!(result, Err(McpError::Protocol(_))));
    }

    #[tokio::test]
    async fn protocol_errors_propagate() {
        let transport = FakeTransport::new(Arc::new(|method, _| {
            if method == "initialize" {
                return Ok(json!({"protocolVersion": MCP_PROTOCOL_VERSION}));
            }
            Err(McpError::Protocol("boom".to_string()))
        }));
        let client = McpClient::connect(transport, "fake").await.expect("握手");
        let error = client.list_tools().await.expect_err("应失败");
        assert!(matches!(error, McpError::Protocol(_)));
    }
}
