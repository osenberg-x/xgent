//! read_file 工具：读取项目内文件内容。

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use xgent_core::chat::ToolSchema;

use crate::path::resolve_in_project;
use crate::tool::{
    Concurrency, Tool, ToolCtx, ToolError, ToolResult, ToolTier, ToolUpdateCallback,
};

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

    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Shared
    }

    fn summarize(&self, input: &Value) -> String {
        let path = input["path"].as_str().unwrap_or("?");
        format!("读取文件 {path}")
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolCtx,
        _signal: CancellationToken,
        _on_update: Option<&ToolUpdateCallback>,
    ) -> Result<ToolResult, ToolError> {
        let Some(path) = input["path"].as_str() else {
            return Ok(ToolResult {
                output: "缺少参数 path".into(),
                is_error: true,
                side_effect: None,
            });
        };
        let full = match resolve_in_project(&ctx.project_root, path) {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult {
                    output: e,
                    is_error: true,
                    side_effect: None,
                });
            }
        };
        match tokio::fs::read_to_string(&full).await {
            Ok(content) => Ok(ToolResult {
                output: content,
                is_error: false,
                side_effect: None,
            }),
            Err(e) => Ok(ToolResult {
                output: format!("读取失败 {}: {e}", full.display()),
                is_error: true,
                side_effect: None,
            }),
        }
    }
}
