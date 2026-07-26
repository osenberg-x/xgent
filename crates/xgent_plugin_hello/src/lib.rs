//! xgent_plugin_hello — 测试插件。
//!
//! 注册一个 `hello` 工具（read tier），执行时返回 "hello: <input>"。
//! 用于验证插件系统最小链路：加载 → register → execute。

use xgent_plugin_api::{Extension, WitToolDef, WitToolError, WitToolTier};

struct Hello;

impl Extension for Hello {
    fn new() -> Self {
        Hello
    }

    fn register_tools(&mut self) -> Vec<WitToolDef> {
        vec![WitToolDef {
            id: "hello".to_string(),
            description: "says hello".to_string(),
            schema: r#"{"type":"object","properties":{"name":{"type":"string"}}}"#.to_string(),
            tier: WitToolTier::Read,
        }]
    }

    fn execute(&mut self, tool_id: &str, input: &str) -> Result<String, WitToolError> {
        if tool_id != "hello" {
            return Err(WitToolError::Failed(format!("unknown tool: {tool_id}")));
        }
        // 解析 input JSON 取 name，否则用 "world"
        let name = serde_json::from_str::<serde_json::Value>(input)
            .ok()
            .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(String::from))
            .unwrap_or_else(|| "world".to_string());
        // 返回 JSON 序列化的 ToolResult（is_error:false, output: hello 文本）
        let result = serde_json::json!({
            "output": format!("hello: {name}"),
            "is_error": false,
            "denied": false,
            "side_effect": null,
        });
        serde_json::to_string(&result).map_err(|e| WitToolError::Failed(e.to_string()))
    }
}

xgent_plugin_api::register_plugin!(Hello);
