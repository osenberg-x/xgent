//! write_file 工具：写入项目内文件（覆盖）。

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use xgent_core::chat::ToolSchema;

use crate::path::resolve_in_project;
use crate::tool::{
    Concurrency, SideEffect, Tool, ToolCtx, ToolError, ToolResult, ToolTier, ToolUpdateCallback,
};

/// 写入项目内文件（覆盖已存在文件）。
pub struct WriteFile;

#[async_trait]
impl Tool for WriteFile {
    fn id(&self) -> &str {
        "write_file"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.id().to_string(),
            description: "写入项目内文件（覆盖已存在文件）。".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "相对项目根的路径" },
                    "content": { "type": "string", "description": "文件内容" }
                },
                "required": ["path", "content"]
            }),
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Write
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Exclusive
    }

    fn summarize(&self, input: &Value) -> String {
        let path = input["path"].as_str().unwrap_or("?");
        format!("写入文件 {path}")
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
        let Some(content) = input["content"].as_str() else {
            return Ok(ToolResult {
                output: "缺少参数 content".into(),
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
        // 确保父目录存在
        if let Some(parent) = full.parent()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            return Ok(ToolResult {
                output: format!("创建目录失败: {e}"),
                is_error: true,
                side_effect: None,
            });
        }
        match tokio::fs::write(&full, content).await {
            Ok(()) => {
                let written = full.clone();
                Ok(ToolResult {
                    output: format!("已写入 {}（{} 字节）", full.display(), content.len()),
                    is_error: false,
                    side_effect: Some(SideEffect::FileWritten(written)),
                })
            }
            Err(e) => Ok(ToolResult {
                output: format!("写入失败 {}: {e}", full.display()),
                is_error: true,
                side_effect: None,
            }),
        }
    }
}
