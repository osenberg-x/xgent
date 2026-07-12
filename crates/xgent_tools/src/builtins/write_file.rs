//! write_file 工具：写入项目内文件（覆盖）。

use async_trait::async_trait;
use serde_json::{Value, json};
use xgent_core::chat::ToolSchema;

use crate::path::resolve_in_project;
use crate::tool::{SideEffect, Tool, ToolCtx, ToolResult};

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

    fn summarize(&self, input: &Value) -> String {
        let path = input["path"].as_str().unwrap_or("?");
        format!("写入文件 {path}")
    }

    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolResult {
        let Some(path) = input["path"].as_str() else {
            return ToolResult {
                output: "缺少参数 path".into(),
                success: false,
                side_effect: None,
            };
        };
        let Some(content) = input["content"].as_str() else {
            return ToolResult {
                output: "缺少参数 content".into(),
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
        // 确保父目录存在
        if let Some(parent) = full.parent()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            return ToolResult {
                output: format!("创建目录失败: {e}"),
                success: false,
                side_effect: None,
            };
        }
        match tokio::fs::write(&full, content).await {
            Ok(()) => {
                let written = full.clone();
                ToolResult {
                    output: format!("已写入 {}（{} 字节）", full.display(), content.len()),
                    success: true,
                    side_effect: Some(SideEffect::FileWritten(written)),
                }
            }
            Err(e) => ToolResult {
                output: format!("写入失败 {}: {e}", full.display()),
                success: false,
                side_effect: None,
            },
        }
    }
}
