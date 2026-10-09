//! read_file 工具：读取项目内文件内容（支持行区间与输出截断）。

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use xgent_core::chat::ToolSchema;

use crate::path::resolve_in_project;
use crate::tool::{
    Concurrency, Tool, ToolCtx, ToolError, ToolResult, ToolTier, ToolUpdateCallback,
};

/// 读取项目内文件内容（UTF-8 文本）。
///
/// 输出经 [`crate::truncate_output`] 截断——单次读取不能撑爆 LLM 上下文。
/// 可选 `offset`/`limit`（均按行，从 0 起）让模型只读片段，避免整文件回灌。
pub struct ReadFile;

#[async_trait]
impl Tool for ReadFile {
    fn id(&self) -> &str {
        "read_file"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.id().to_string(),
            description: "读取项目内文件内容（UTF-8 文本）。支持 offset/limit 按行读取片段；\
                          输出超长会自动截断。大文件优先用 offset/limit 分段读。"
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "相对项目根的路径（或项目内绝对路径）"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "起始行号（0 起）。省略表示从第 0 行开始"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "最多读取的行数。省略表示读到文件末尾"
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

    async fn summarize(&self, input: &Value) -> String {
        let path = input["path"].as_str().unwrap_or("?");
        format!("读取文件 {path}")
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolCtx,
        _signal: CancellationToken,
        _on_update: Option<Arc<ToolUpdateCallback>>,
    ) -> Result<ToolResult, ToolError> {
        let Some(path) = input["path"].as_str() else {
            return Ok(ToolResult {
                output: "缺少参数 path".into(),
                is_error: true,
                denied: false,
                side_effect: None,
            });
        };
        let full = match resolve_in_project(&ctx.project_root, path) {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult {
                    output: e,
                    is_error: true,
                    denied: false,
                    side_effect: None,
                });
            }
        };
        let content = match tokio::fs::read_to_string(&full).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    output: format!("读取失败 {}: {e}", full.display()),
                    is_error: true,
                    denied: false,
                    side_effect: None,
                });
            }
        };
        // 行区间切片在整段字符串上做，避免重读文件
        let sliced = match slice_lines(&content, &input) {
            Ok(s) => s,
            Err(msg) => {
                return Ok(ToolResult {
                    output: msg,
                    is_error: true,
                    denied: false,
                    side_effect: None,
                });
            }
        };
        Ok(ToolResult {
            output: crate::truncate_output(&sliced),
            is_error: false,
            denied: false,
            side_effect: None,
        })
    }
}

/// 按 `offset`/`limit` 做行区间切片。
///
/// 行数含尾随空行处理：`content.lines()` 忽略末尾换行后的空行，
/// 切片后用 `join("\n")` 重组，不额外增删换行。
///
/// 参数非法（负数 offset）返回错误文本；切片超出文件范围按实际可得行返回，
/// 不视为错误——模型常按估算行号请求。
pub fn slice_lines(content: &str, input: &Value) -> Result<String, String> {
    let offset = match input.get("offset") {
        None | Some(Value::Null) => 0i64,
        Some(v) => v.as_i64().ok_or_else(|| "offset 必须是整数".to_string())?,
    };
    let limit = match input.get("limit") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_i64().ok_or_else(|| "limit 必须是整数".to_string())?),
    };
    if offset < 0 {
        return Err("offset 不能为负".into());
    }
    if let Some(l) = limit
        && l < 0
    {
        return Err("limit 不能为负".into());
    }
    let offset = offset as usize;
    let all: Vec<&str> = content.lines().collect();
    // offset 超出文件范围：返回空而非报错
    if offset >= all.len() {
        return Ok(String::new());
    }
    let end = match limit {
        Some(l) => offset.saturating_add(l as usize).min(all.len()),
        None => all.len(),
    };
    Ok(all[offset..end].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_within_bounds() {
        let c = "l0\nl1\nl2\nl3\nl4";
        let got = slice_lines(c, &json!({"offset": 1, "limit": 2})).unwrap();
        assert_eq!(got, "l1\nl2");
    }

    #[test]
    fn slice_offset_only_reads_to_end() {
        let c = "l0\nl1\nl2";
        let got = slice_lines(c, &json!({"offset": 1})).unwrap();
        assert_eq!(got, "l1\nl2");
    }

    #[test]
    fn slice_limit_only_reads_from_start() {
        let c = "l0\nl1\nl2";
        let got = slice_lines(c, &json!({"limit": 2})).unwrap();
        assert_eq!(got, "l0\nl1");
    }

    #[test]
    fn slice_no_params_returns_whole() {
        let c = "l0\nl1\nl2";
        let got = slice_lines(c, &json!({})).unwrap();
        assert_eq!(got, "l0\nl1\nl2");
    }

    #[test]
    fn slice_offset_past_end_returns_empty_not_error() {
        let c = "l0\nl1";
        let got = slice_lines(c, &json!({"offset": 99})).unwrap();
        assert_eq!(got, "");
    }

    #[test]
    fn slice_limit_past_end_clamps() {
        let c = "l0\nl1";
        let got = slice_lines(c, &json!({"offset": 1, "limit": 99})).unwrap();
        assert_eq!(got, "l1");
    }

    #[test]
    fn slice_negative_offset_errors() {
        let got = slice_lines("l0", &json!({"offset": -1}));
        assert!(got.is_err());
    }

    #[test]
    fn slice_negative_limit_errors() {
        let got = slice_lines("l0", &json!({"limit": -5}));
        assert!(got.is_err());
    }

    #[test]
    fn slice_non_integer_errors() {
        let got = slice_lines("l0", &json!({"offset": "abc"}));
        assert!(got.is_err());
    }

    #[test]
    fn slice_preserves_cjk_whole_lines() {
        let c = "中文第一行\n中文第二行\n中文第三行";
        let got = slice_lines(c, &json!({"offset": 1, "limit": 1})).unwrap();
        assert_eq!(got, "中文第二行");
    }
}
