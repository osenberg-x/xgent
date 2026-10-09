//! edit_file 工具：替换文件中指定行范围的内容。
//!
//! 与 `write_file`（整文件覆盖）的区别：
//! - 只提交要替换的新内容，不回传整个文件——大文件改动不撑爆上下文；
//! - 校验目标行范围当前内容是否与调用方预期一致，不一致则报错而非覆盖，
//!   避免覆盖掉期间发生的外部修改；
//! - 写前备份原文件，供回滚路径使用。

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use xgent_core::chat::ToolSchema;

use crate::path::resolve_in_project;
use crate::tool::{
    Concurrency, SideEffect, Tool, ToolCtx, ToolError, ToolResult, ToolTier, ToolUpdateCallback,
};

/// 替换文件行范围的工具。
pub struct EditFile;

#[async_trait]
impl Tool for EditFile {
    fn id(&self) -> &str {
        "edit_file"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.id().to_string(),
            description: "替换已存在文件中的指定行范围。start_line 为 0 起的起始行号，\
                          删除 end_line+1 行并插入 new_content。old_content 为可选的\
                          预期原文，用于校验目标区间未被外部修改；不匹配则报错不写入。"
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "相对项目根的路径" },
                    "start_line": { "type": "integer", "description": "起始行号（0 起，含）" },
                    "end_line": {
                        "type": "integer",
                        "description": "结束行号（0 起，含）。省略表示只替换 start_line 单行"
                    },
                    "new_content": { "type": "string", "description": "替换后的新内容" },
                    "old_content": {
                        "type": "string",
                        "description": "可选：目标区间的预期原文，用于校验"
                    }
                },
                "required": ["path", "start_line", "new_content"]
            }),
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Write
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Exclusive
    }

    async fn summarize(&self, input: &Value) -> String {
        let path = input["path"].as_str().unwrap_or("?");
        let s = input["start_line"].as_i64().unwrap_or(0);
        let e = input.get("end_line").and_then(|v| v.as_i64()).unwrap_or(s);
        format!("编辑文件 {path} 第 {}~{} 行", s + 1, e + 1)
    }

    /// 返回替换后的完整内容与原完整内容，供确认弹窗 diff。
    async fn preview_diff(&self, input: &Value, ctx: &ToolCtx) -> Option<(String, String)> {
        let path = input["path"].as_str()?;
        let full = resolve_in_project(&ctx.project_root, path).ok()?;
        let old = tokio::fs::read_to_string(&full).await.unwrap_or_default();
        let new = apply_edit(&old, input).ok()?;
        Some((old, new))
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolCtx,
        _signal: CancellationToken,
        _on_update: Option<Arc<ToolUpdateCallback>>,
    ) -> Result<ToolResult, ToolError> {
        let Some(path) = input["path"].as_str() else {
            return Ok(err("缺少参数 path"));
        };
        let full = match resolve_in_project(&ctx.project_root, path) {
            Ok(p) => p,
            Err(e) => return Ok(err(e)),
        };
        // 写入大小上限：与 write_file 同量级，防单次编辑写爆磁盘
        if let Some(nc) = input["new_content"].as_str()
            && nc.len() > crate::atomic::MAX_WRITE_BYTES
        {
            return Ok(err(format!(
                "替换内容过大（{} 字节，上限 {} 字节），请分多次编辑",
                nc.len(),
                crate::atomic::MAX_WRITE_BYTES
            )));
        }
        // edit_file 只针对已存在文件（新建走 write_file）
        let old = match tokio::fs::read_to_string(&full).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(err(format!(
                    "读取失败 {}: {e}（edit_file 只能编辑已存在文件，新建文件请用 write_file）",
                    full.display()
                )));
            }
        };
        let new = match apply_edit(&old, &input) {
            Ok(n) => n,
            Err(e) => return Ok(err(e)),
        };
        if let Err(e) = crate::atomic::backup_existing(&full).await {
            return Ok(err(format!("备份原文件失败 {}: {e}", full.display())));
        }
        match crate::atomic::atomic_write(&full, &new).await {
            Ok(()) => Ok(ToolResult {
                output: format!(
                    "已编辑 {}（{} → {} 字节）",
                    full.display(),
                    old.len(),
                    new.len()
                ),
                is_error: false,
                denied: false,
                side_effect: Some(SideEffect::FileWritten(full)),
            }),
            Err(e) => Ok(err(format!("写入失败 {}: {e}", full.display()))),
        }
    }
}

fn err(msg: impl Into<String>) -> ToolResult {
    ToolResult {
        output: msg.into(),
        is_error: true,
        denied: false,
        side_effect: None,
    }
}

/// 按输入把行范围替换应用到原内容，返回新的完整内容。
///
/// 语义：`start_line`（0 起）到 `end_line`（0 起，含）之间的行整体替换为
/// `new_content`。`end_line` 省略时只替换 `start_line` 单行。
///
/// `old_content` 非空时先校验目标区间当前内容，不匹配返回错误文本——
/// 防止覆盖期间发生的外部修改。
pub fn apply_edit(old: &str, input: &Value) -> Result<String, String> {
    let start = input
        .get("start_line")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "缺少参数 start_line".to_string())?;
    if start < 0 {
        return Err("start_line 不能为负".into());
    }
    let end = match input.get("end_line") {
        None | Some(Value::Null) => start,
        Some(v) => v
            .as_i64()
            .ok_or_else(|| "end_line 必须是整数".to_string())?,
    };
    if end < start {
        return Err(format!("end_line（{end}）不能小于 start_line（{start}）"));
    }
    let new_content = input
        .get("new_content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "缺少参数 new_content".to_string())?;

    let lines: Vec<&str> = old.lines().collect();
    let start = start as usize;
    // start 超出文件行数 → 无法替换
    if start >= lines.len() {
        return Err(format!(
            "start_line（{start}）超出文件行数（共 {} 行），请先用 read_file 确认当前内容",
            lines.len()
        ));
    }
    let end = (end as usize).min(lines.len() - 1);
    let current = lines[start..=end].join("\n");

    if let Some(expect) = input.get("old_content").and_then(|v| v.as_str())
        && !expect.is_empty()
        && normalize(expect) != normalize(&current)
    {
        return Err(format!(
            "目标区间内容与 old_content 不匹配，未写入。当前内容为：\n{current}"
        ));
    }

    let mut out: Vec<&str> = Vec::with_capacity(lines.len() + 1);
    out.extend_from_slice(&lines[..start]);
    if !new_content.is_empty() {
        out.extend(new_content.lines());
    }
    out.extend_from_slice(&lines[end + 1..]);
    let mut result = out.join("\n");
    // 保留原文件末尾换行（若原文件有）
    if old.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

/// 比较时忽略行尾空白差异，避免 CRLF/尾随空格造成假不匹配。
fn normalize(s: &str) -> String {
    s.lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_single_line() {
        let old = "a\nb\nc";
        let got = apply_edit(old, &json!({"start_line": 1, "new_content": "B"})).unwrap();
        assert_eq!(got, "a\nB\nc");
    }

    #[test]
    fn replaces_line_range() {
        let old = "a\nb\nc\nd";
        let got = apply_edit(
            old,
            &json!({"start_line": 1, "end_line": 2, "new_content": "X\nY"}),
        )
        .unwrap();
        assert_eq!(got, "a\nX\nY\nd");
    }

    #[test]
    fn deletes_lines_with_empty_new_content() {
        let old = "a\nb\nc";
        let got = apply_edit(
            old,
            &json!({"start_line": 1, "end_line": 1, "new_content": ""}),
        )
        .unwrap();
        assert_eq!(got, "a\nc");
    }

    /// 替换语义（而非插入）：被替换区间消失，新内容占据其位置。
    /// 单行被替换为多行 = 该行展开。
    #[test]
    fn replacing_single_line_with_multi_line_expands_it() {
        let old = "a\nd";
        let got = apply_edit(old, &json!({"start_line": 1, "new_content": "b\nc"})).unwrap();
        assert_eq!(got, "a\nb\nc");
    }

    /// 在中间插入：把 b、c 之间的某行替换为多行，其余行保留。
    #[test]
    fn replacing_middle_line_with_multi_line_keeps_surrounding() {
        let old = "a\nX\nd";
        let got = apply_edit(old, &json!({"start_line": 1, "new_content": "b\nc"})).unwrap();
        assert_eq!(got, "a\nb\nc\nd");
    }

    #[test]
    fn preserves_trailing_newline() {
        let old = "a\nb\n";
        let got = apply_edit(old, &json!({"start_line": 0, "new_content": "A"})).unwrap();
        assert_eq!(got, "A\nb\n");
    }

    #[test]
    fn end_line_past_eof_clamps() {
        let old = "a\nb";
        let got = apply_edit(
            old,
            &json!({"start_line": 0, "end_line": 99, "new_content": "Z"}),
        )
        .unwrap();
        assert_eq!(got, "Z");
    }

    #[test]
    fn start_line_past_eof_errors() {
        let got = apply_edit("a", &json!({"start_line": 9, "new_content": "Z"}));
        assert!(got.is_err());
    }

    #[test]
    fn negative_start_errors() {
        let got = apply_edit("a", &json!({"start_line": -1, "new_content": "Z"}));
        assert!(got.is_err());
    }

    #[test]
    fn end_before_start_errors() {
        let got = apply_edit(
            "a\nb\nc",
            &json!({"start_line": 2, "end_line": 0, "new_content": "Z"}),
        );
        assert!(got.is_err());
    }

    #[test]
    fn matching_old_content_proceeds() {
        let old = "a\nb\nc";
        let got = apply_edit(
            old,
            &json!({"start_line": 1, "new_content": "B", "old_content": "b"}),
        )
        .unwrap();
        assert_eq!(got, "a\nB\nc");
    }

    #[test]
    fn mismatched_old_content_reports_error() {
        let old = "a\nb\nc";
        let got = apply_edit(
            old,
            &json!({"start_line": 1, "new_content": "B", "old_content": "WRONG"}),
        );
        assert!(got.is_err());
        assert!(got.unwrap_err().contains("不匹配"));
    }

    #[test]
    fn old_content_ignores_trailing_space() {
        let old = "a\nb   \nc";
        let got = apply_edit(
            old,
            &json!({"start_line": 1, "new_content": "B", "old_content": "b"}),
        )
        .unwrap();
        assert_eq!(got, "a\nB\nc");
    }

    #[test]
    fn missing_start_line_errors() {
        let got = apply_edit("a", &json!({"new_content": "Z"}));
        assert!(got.is_err());
    }

    #[test]
    fn missing_new_content_errors() {
        let got = apply_edit("a", &json!({"start_line": 0}));
        assert!(got.is_err());
    }

    /// truncate_output 的输出可能略超 MAX（标记与两段内容之和），
    /// 但不得超过 MAX 的 110%，否则"单次工具不突破共享上限"的约束失效。
    #[test]
    fn truncated_output_stays_near_cap() {
        let big = "a".repeat(crate::MAX_TOOL_OUTPUT_BYTES * 2);
        let out = crate::truncate_output(&big);
        assert!(out.len() > crate::MAX_TOOL_OUTPUT_BYTES);
        assert!(
            out.len() < crate::MAX_TOOL_OUTPUT_BYTES * 110 / 100,
            "截断后仍应贴近上限，实际 {} vs {}",
            out.len(),
            crate::MAX_TOOL_OUTPUT_BYTES
        );
    }
}
