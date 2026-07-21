//! search_files 工具：在项目内递归搜索文本匹配。
//!
//! MVP 用内置简单递归遍历 + 子串匹配（不依赖系统 ripgrep），保证可移植。
//! 后续可升级为先检测系统 `rg`、降级内置搜索。

use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::Path;
use tokio_util::sync::CancellationToken;
use xgent_core::chat::ToolSchema;

use crate::path::resolve_in_project;
use crate::tool::{
    Concurrency, Tool, ToolCtx, ToolError, ToolResult, ToolTier, ToolUpdateCallback,
};

/// 递归搜索限制的最大结果数与最大深度，避免超大仓库卡死。
const MAX_RESULTS: usize = 200;
const MAX_DEPTH: u32 = 16;

/// 在项目内递归搜索文本匹配（子串匹配）。
pub struct SearchFiles;

#[async_trait]
impl Tool for SearchFiles {
    fn id(&self) -> &str {
        "search_files"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.id().to_string(),
            description: "在项目内递归搜索文本匹配（子串匹配，返回匹配行）。".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "搜索的子串" },
                    "path": { "type": "string", "description": "搜索起始目录（可选，默认项目根）" }
                },
                "required": ["pattern"]
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
        let pattern = input["pattern"].as_str().unwrap_or("?");
        match input["path"].as_str() {
            Some(p) => format!("在 {p} 搜索 “{pattern}”"),
            None => format!("搜索 “{pattern}”"),
        }
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolCtx,
        _signal: CancellationToken,
        _on_update: Option<&ToolUpdateCallback>,
    ) -> Result<ToolResult, ToolError> {
        let Some(pattern) = input["pattern"].as_str() else {
            return Ok(ToolResult { output: "缺少参数 pattern".into(), is_error: true, denied: false, side_effect: None });
        };
        let start_rel = input["path"].as_str().unwrap_or(".");
        let start = match resolve_in_project(&ctx.project_root, start_rel) {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult { output: e, is_error: true, denied: false, side_effect: None });
            }
        };

        let mut matches = Vec::new();
        let mut count = 0usize;
        walk(&start, &start, pattern, 0, &mut matches, &mut count).await;

        let mut output = if matches.is_empty() {
            format!("未找到匹配 “{pattern}”")
        } else {
            let mut s = String::new();
            for m in &matches {
                s.push_str(&format!("{}:{}: {}\n", m.path, m.line_no, m.line));
            }
            s
        };
        if count >= MAX_RESULTS {
            output.push_str(&format!("\n（已达最大结果数 {MAX_RESULTS}，截断）\n"));
        }
        Ok(ToolResult {
            output,
            is_error: false,
            denied: false,
            side_effect: None,
        })
    }
}

struct Match {
    path: String,
    line_no: usize,
    line: String,
}

/// 递归遍历目录，收集匹配行。
async fn walk(
    root: &Path,
    dir: &Path,
    pattern: &str,
    depth: u32,
    matches: &mut Vec<Match>,
    count: &mut usize,
) {
    if depth > MAX_DEPTH || *count >= MAX_RESULTS {
        return;
    }
    let Ok(mut rd) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        if *count >= MAX_RESULTS {
            return;
        }
        let path = entry.path();
        let ft = match entry.file_type().await {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            // 跳过常见忽略目录
            let name = entry.file_name();
            if ignores(&name.to_string_lossy()) {
                continue;
            }
            // Box::pin 递归 async fn
            Box::pin(walk(root, &path, pattern, depth + 1, matches, count)).await;
        } else if ft.is_file() {
            read_and_match(&path, root, pattern, matches, count).await;
        }
    }
}

/// 读取单个文件，按行匹配。
async fn read_and_match(
    path: &Path,
    root: &Path,
    pattern: &str,
    matches: &mut Vec<Match>,
    count: &mut usize,
) {
    let Ok(content) = tokio::fs::read_to_string(path).await else {
        return; // 非文本/无法读，跳过
    };
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    for (i, line) in content.lines().enumerate() {
        if *count >= MAX_RESULTS {
            return;
        }
        if line.contains(pattern) {
            matches.push(Match {
                path: rel.clone(),
                line_no: i + 1,
                line: line.to_string(),
            });
            *count += 1;
        }
    }
}

/// 是否应忽略的目录名。
fn ignores(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".hg" | ".svn" | "node_modules" | "target" | "dist" | "build" | ".next"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn search_finds_matches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        tokio::fs::create_dir_all(root.join("src")).await.unwrap();
        tokio::fs::write(root.join("src/a.rs"), "fn foo() {}\nfn bar() {}\n")
            .await
            .unwrap();
        tokio::fs::write(root.join("b.txt"), "hello foo\n")
            .await
            .unwrap();

        let ctx = ToolCtx {
            project_root: root.clone(),
            tool_policy: Default::default(),
        };
        let r = SearchFiles
            .execute(
                json!({"pattern": "foo"}),
                &ctx,
                CancellationToken::new(),
                None,
            )
            .await
            .unwrap();
        assert!(!r.is_error);
        assert!(r.output.contains("src/a.rs:1"));
        assert!(r.output.contains("b.txt:1"));
    }

    #[tokio::test]
    async fn search_no_match() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx {
            project_root: dir.path().to_path_buf(),
            tool_policy: Default::default(),
        };
        tokio::fs::write(dir.path().join("x.txt"), "abc")
            .await
            .unwrap();
        let r = SearchFiles
            .execute(
                json!({"pattern": "zzz"}),
                &ctx,
                CancellationToken::new(),
                None,
            )
            .await
            .unwrap();
        assert!(!r.is_error);
        assert!(r.output.contains("未找到"));
    }

    #[tokio::test]
    async fn search_ignores_target_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        tokio::fs::create_dir_all(root.join("target"))
            .await
            .unwrap();
        tokio::fs::write(root.join("target/junk.rs"), "findme\n")
            .await
            .unwrap();
        let ctx = ToolCtx {
            project_root: root,
            tool_policy: Default::default(),
        };
        let r = SearchFiles
            .execute(
                json!({"pattern": "findme"}),
                &ctx,
                CancellationToken::new(),
                None,
            )
            .await
            .unwrap();
        assert!(!r.output.contains("target/junk.rs"), "应忽略 target 目录");
    }

    #[test]
    fn ignores_common_dirs() {
        assert!(ignores("target"));
        assert!(ignores(".git"));
        assert!(ignores("node_modules"));
        assert!(!ignores("src"));
    }

    #[test]
    fn summarize_with_path() {
        let s = SearchFiles.summarize(&json!({"pattern": "foo", "path": "src"}));
        assert!(s.contains("src"));
        assert!(s.contains("foo"));
    }
}
