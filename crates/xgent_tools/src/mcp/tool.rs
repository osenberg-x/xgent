//! MCP 工具适配：把 Server 工具接入现有 `Tool` / 确认 / 策略链路。

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use xgent_core::chat::ToolSchema;

use crate::tool::{Concurrency, Tool, ToolCtx, ToolError, ToolResult, ToolTier};
use crate::truncate_output;

use super::McpError;
use super::client::{McpClient, McpToolDef};

const MAX_TOOL_ID_LEN: usize = 64;
const MAX_SERVER_ID_LEN: usize = 24;
const MAX_SCHEMA_BYTES: usize = 64 * 1024;
const MAX_DESCRIPTION_CHARS: usize = 2048;

/// 一个已注册的 MCP 工具。
pub struct McpTool {
    client: Arc<McpClient>,
    id: String,
    original_name: String,
    description: String,
    input_schema: Value,
}

impl McpTool {
    /// 由工具定义构造；server id/工具名非法、schema 过大时返回 `None`。
    pub fn new(client: Arc<McpClient>, def: McpToolDef) -> Option<Self> {
        let server_id = client.server_id();
        if !valid_server_id(server_id) {
            return None;
        }
        let schema_size = serde_json::to_vec(&def.input_schema).ok()?.len();
        if schema_size > MAX_SCHEMA_BYTES {
            return None;
        }
        let id = full_tool_id(server_id, &def.name);
        let description: String = def
            .description
            .chars()
            .take(MAX_DESCRIPTION_CHARS)
            .collect();
        Some(Self {
            client,
            id,
            original_name: def.name,
            description,
            input_schema: def.input_schema,
        })
    }

    /// 原始 MCP 工具名（调用时回传）。
    pub fn original_name(&self) -> &str {
        &self.original_name
    }

    /// 所属 Server id。
    pub fn server_id(&self) -> &str {
        self.client.server_id()
    }
}

#[async_trait]
impl Tool for McpTool {
    fn id(&self) -> &str {
        &self.id
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.id.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Exec
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Exclusive
    }

    async fn summarize(&self, input: &Value) -> String {
        let args = serde_json::to_string(input).unwrap_or_else(|_| "<无效参数>".to_string());
        let args: String = args.chars().take(200).collect();
        format!("MCP {}/{}: {args}", self.server_id(), self.original_name)
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolCtx,
        signal: CancellationToken,
        _on_update: Option<Arc<crate::ToolUpdateCallback>>,
    ) -> Result<ToolResult, ToolError> {
        if !input.is_null() && !input.is_object() {
            return Ok(ToolResult {
                output: "MCP 工具参数必须是 JSON 对象".to_string(),
                is_error: true,
                denied: false,
                side_effect: None,
            });
        }
        let arguments = if input.is_null() { json!({}) } else { input };
        let result = self
            .client
            .call_tool(&self.original_name, arguments, Some(&signal))
            .await;
        match result {
            Ok(result) => Ok(ToolResult {
                output: truncate_output(&result.output),
                is_error: result.is_error,
                denied: false,
                side_effect: None,
            }),
            Err(McpError::Cancelled) => Err(ToolError::Aborted),
            Err(McpError::Timeout(secs)) => Err(ToolError::Timeout(secs)),
            Err(error) => Err(ToolError::Failed(error.to_string())),
        }
    }
}

fn valid_server_id(server_id: &str) -> bool {
    let mut chars = server_id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    server_id.len() <= MAX_SERVER_ID_LEN
        && first.is_ascii_alphanumeric()
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn full_tool_id(server_id: &str, tool_name: &str) -> String {
    let prefix = format!("mcp.{server_id}.");
    let sanitized: String = tool_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let mut name = if sanitized.is_empty() {
        "tool".to_string()
    } else {
        sanitized
    };
    let available = MAX_TOOL_ID_LEN.saturating_sub(prefix.len());
    if name.len() > available {
        let hash = crate::fnv1a64(tool_name.as_bytes());
        let suffix = format!("_{}", &hash[..8]);
        let keep = available.saturating_sub(suffix.len());
        name = format!("{}{}", &name[..keep.min(name.len())], suffix);
    }
    format!("{prefix}{name}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MAX_TOOL_OUTPUT_BYTES;
    use async_trait::async_trait;
    use xgent_settings_core::project::ToolPolicyConfig;

    struct FakeTransport {
        handler: Box<dyn Fn(&str, &Value) -> Result<Value, McpError> + Send + Sync>,
    }

    #[async_trait]
    impl super::super::McpTransport for FakeTransport {
        async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
            (self.handler)(method, &params)
        }

        async fn notify(&self, _method: &str, _params: Value) {}

        async fn close(&self) {}

        fn is_connected(&self) -> bool {
            true
        }
    }

    fn tool_def(name: &str) -> McpToolDef {
        McpToolDef {
            name: name.to_string(),
            description: "读取文件".to_string(),
            input_schema: json!({"type": "object"}),
        }
    }

    fn tool_with(transport: FakeTransport, server: &str, def: McpToolDef) -> McpTool {
        let client = Arc::new(McpClient::new(Arc::new(transport), server));
        McpTool::new(client, def).expect("工具构造")
    }

    fn default_transport() -> FakeTransport {
        FakeTransport {
            handler: Box::new(|_, _| Ok(json!({"content": [{"type": "text", "text": "ok"}]}))),
        }
    }

    #[test]
    fn id_is_namespaced_and_sanitized() {
        let tool = tool_with(default_transport(), "srv", tool_def("read.file/../x"));
        assert_eq!(tool.id(), "mcp.srv.read_file____x");
        assert!(tool.id().len() <= MAX_TOOL_ID_LEN);
        assert_eq!(tool.original_name(), "read.file/../x");
        assert_eq!(tool.tier(), ToolTier::Exec);
        assert_eq!(tool.concurrency(), Concurrency::Exclusive);
    }

    #[test]
    fn long_names_are_capped_and_distinct() {
        let long_a = "a".repeat(100);
        let long_b = "b".repeat(100);
        let tool_a = tool_with(default_transport(), "srv", tool_def(&long_a));
        let tool_b = tool_with(default_transport(), "srv", tool_def(&long_b));
        assert!(tool_a.id().len() <= MAX_TOOL_ID_LEN);
        assert!(tool_b.id().len() <= MAX_TOOL_ID_LEN);
        assert_ne!(tool_a.id(), tool_b.id());
    }

    #[test]
    fn invalid_server_or_huge_schema_is_rejected() {
        let client = Arc::new(McpClient::new(Arc::new(default_transport()), "bad id"));
        assert!(McpTool::new(client, tool_def("ok")).is_none());

        let huge = json!({"type": "object", "description": "x".repeat(MAX_SCHEMA_BYTES)});
        let def = McpToolDef {
            name: "ok".into(),
            description: String::new(),
            input_schema: huge,
        };
        let client = Arc::new(McpClient::new(Arc::new(default_transport()), "srv"));
        assert!(McpTool::new(client, def).is_none());
    }

    #[test]
    fn policy_defaults_to_confirmation_and_can_be_approved() {
        let tool = tool_with(default_transport(), "srv", tool_def("read"));
        let policy = ToolPolicyConfig::default();
        assert_eq!(
            crate::security::resolve_policy(tool.id(), tool.tier(), &json!({}), &tool, &policy),
            crate::SecurityPolicy::NeedsConfirmation
        );
        let approved = ToolPolicyConfig {
            approved: vec![tool.id().to_string()],
            ..Default::default()
        };
        assert_eq!(
            crate::security::resolve_policy(tool.id(), tool.tier(), &json!({}), &tool, &approved),
            crate::SecurityPolicy::Approved
        );
    }

    #[test]
    fn readonly_annotation_does_not_change_tier() {
        let mut def = tool_def("read");
        def.input_schema = json!({"type": "object", "annotations": {"readOnlyHint": true}});
        let tool = tool_with(default_transport(), "srv", def);
        assert_eq!(tool.tier(), ToolTier::Exec);
        assert_eq!(
            crate::security::resolve_policy(
                tool.id(),
                tool.tier(),
                &json!({}),
                &tool,
                &ToolPolicyConfig::default()
            ),
            crate::SecurityPolicy::NeedsConfirmation
        );
    }

    #[tokio::test]
    async fn execute_maps_text_and_is_error() {
        let transport = FakeTransport {
            handler: Box::new(|method, _| {
                assert_eq!(method, "tools/call");
                Ok(json!({
                    "content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}],
                    "isError": true,
                }))
            }),
        };
        let tool = tool_with(transport, "srv", tool_def("read"));
        let ctx = ToolCtx::with_policy(std::env::temp_dir(), ToolPolicyConfig::default());
        let result = tool
            .execute(json!({"path": "a"}), &ctx, CancellationToken::new(), None)
            .await
            .expect("执行");
        assert_eq!(result.output, "a\nb");
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn execute_maps_transport_errors() {
        for (error, expected) in [
            (McpError::Cancelled, "aborted"),
            (McpError::Timeout(5), "timeout"),
            (McpError::Protocol("boom".into()), "failed"),
        ] {
            let transport = FakeTransport {
                handler: Box::new(move |_, _| Err(error.clone())),
            };
            let tool = tool_with(transport, "srv", tool_def("read"));
            let ctx = ToolCtx::with_policy(std::env::temp_dir(), ToolPolicyConfig::default());
            let result = tool
                .execute(json!({}), &ctx, CancellationToken::new(), None)
                .await;
            match (result, expected) {
                (Err(ToolError::Aborted), "aborted") => {}
                (Err(ToolError::Timeout(5)), "timeout") => {}
                (Err(ToolError::Failed(_)), "failed") => {}
                (other, _) => panic!("错误映射不符：{other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn non_object_input_is_rejected_without_call() {
        let transport = default_transport();
        let tool = tool_with(transport, "srv", tool_def("read"));
        let ctx = ToolCtx::with_policy(std::env::temp_dir(), ToolPolicyConfig::default());
        let result = tool
            .execute(json!("not-object"), &ctx, CancellationToken::new(), None)
            .await
            .expect("执行");
        assert!(result.is_error);
        assert!(result.output.contains("JSON 对象"));
    }

    #[tokio::test]
    async fn output_is_truncated() {
        let transport = FakeTransport {
            handler: Box::new(|_, _| {
                Ok(
                    json!({"content": [{"type": "text", "text": "x".repeat(MAX_TOOL_OUTPUT_BYTES + 100)}]}),
                )
            }),
        };
        let tool = tool_with(transport, "srv", tool_def("read"));
        let ctx = ToolCtx::with_policy(std::env::temp_dir(), ToolPolicyConfig::default());
        let result = tool
            .execute(json!({}), &ctx, CancellationToken::new(), None)
            .await
            .expect("执行");
        assert!(result.output.contains("输出过长"));
    }
}
