//! read_file 工具：读取项目内文件内容。

use async_trait::async_trait;
use serde_json::{Value, json};
use xgent_core::chat::ToolSchema;

use crate::path::resolve_in_project;
use crate::tool::{Tool, ToolCtx, ToolResult};

/// 读取项目内文件内容（UTF-8 文本）。
pub struct ReadFile;

#[async_trait]
impl Tool for ReadFile {
    fn id(&self) -> &str {
        "read_file"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.id().to_string(),
            description: "读取项目内文件内容（UTF-8 文本）。".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "相对项目根的路径（或项目内绝对路径）"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    fn summarize(&self, input: &Value) -> String {
        let path = input["path"].as_str().unwrap_or("?");
        format!("读取文件 {path}")
    }

    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolResult {
        let Some(path) = input["path"].as_str() else {
            return ToolResult {
                output: "缺少参数 path".into(),
                success: false,
                side_effect: None,
            };
        };
        let full = match resolve_in_project(&ctx.project_root, path) {
            Ok(p) => p,
            Err(e) => {
                return ToolResult {
                    output: e,
                    success: false,
                    side_effect: None,
                };
            }
        };
        match tokio::fs::read_to_string(&full).await {
            Ok(content) => ToolResult {
                output: content,
                success: true,
                side_effect: None,
            },
            Err(e) => ToolResult {
                output: format!("读取失败 {}: {e}", full.display()),
                success: false,
                side_effect: None,
            },
        }
    }
}
