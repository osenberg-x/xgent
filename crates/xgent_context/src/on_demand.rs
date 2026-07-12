//! 方案 A：无索引·按需读取上下文提供者。
//!
//! 策略：
//! 1. 生成项目目录树摘要（限制深度与条目数，token 预算内）；
//! 2. 若有 current_file，优先读其内容；
//! 3. 用用户问题关键词搜索（优先系统 rg，降级内置子串匹配），读匹配文件；
//! 4. 组装 chunks + tree_summary，控制总 token 不超预算。

use async_trait::async_trait;
use std::path::{Path, PathBuf};

use crate::provider::{
    ContextChunk, ContextProvider, ContextQuery, ContextResult, estimate_tokens,
};

/// 目录树最大深度。
const TREE_MAX_DEPTH: u32 = 4;
/// 目录树最大条目数。
const TREE_MAX_ENTRIES: usize = 200;
/// ripgrep 超时（秒）。
const RG_TIMEOUT_SECS: u64 = 10;
/// 搜索返回的最大文件数。
const MAX_SEARCH_FILES: usize = 20;

/// 方案 A 上下文提供者。
pub struct OnDemandContextProvider {
    project_root: PathBuf,
}

impl OnDemandContextProvider {
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// 项目根。
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }
}

#[async_trait]
impl ContextProvider for OnDemandContextProvider {
    async fn retrieve(&self, query: &ContextQuery) -> ContextResult {
        let mut total_tokens: u32 = 0;
        let max = query.max_tokens.max(1);

        // 1. 目录树摘要
        let tree = self.tree_summary().await;
        let tree_tokens = tree.as_ref().map(|t| estimate_tokens(t)).unwrap_or(0);
        if tree_tokens < max {
            total_tokens += tree_tokens;
        }
        // 即使超预算也保留树摘要（它对 LLM 理解结构很关键），但不计入后续预算

        let mut chunks: Vec<ContextChunk> = Vec::new();
        let remaining = max.saturating_sub(total_tokens);

        // 2. 优先读 current_file
        if let Some(cur) = &query.current_file
            && let Some(chunk) = self.read_file_chunk(cur, remaining).await
        {
            total_tokens += chunk.token_estimate;
            chunks.push(chunk);
        }

        // 3. 关键词搜索
        let keywords = extract_keywords(&query.user_message, &query.hints);
        if !keywords.is_empty() && total_tokens < max {
            let files = self.search(&keywords).await;
            for f in files {
                if total_tokens >= max {
                    break;
                }
                let budget = max - total_tokens;
                if let Some(chunk) = self.read_file_chunk(&f, budget).await {
                    total_tokens += chunk.token_estimate;
                    chunks.push(chunk);
                }
            }
        }

        ContextResult {
            chunks,
            tree_summary: tree,
            total_tokens,
        }
    }
}

impl OnDemandContextProvider {
    /// 生成目录树摘要（跳过 .git/target 等）。
    async fn tree_summary(&self) -> Option<String> {
        let mut lines = Vec::new();
        let mut count = 0usize;
        walk_tree(
            &self.project_root,
            &self.project_root,
            0,
            &mut lines,
            &mut count,
        );
        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    }

    /// 关键词搜索：优先系统 rg，降级内置遍历。
    async fn search(&self, keywords: &[String]) -> Vec<PathBuf> {
        if let Some(files) = self.rg_search(keywords).await {
            return files;
        }
        self.fallback_search(keywords).await
    }

    /// 调系统 ripgrep，返回匹配文件列表（相对项目根）。
    /// rg 不存在或失败时返回 None，由调用方降级。
    async fn rg_search(&self, keywords: &[String]) -> Option<Vec<PathBuf>> {
        let pattern = keywords.join("|");
        let mut cmd = tokio::process::Command::new("rg");
        cmd.args(["--files-with-matches", "--no-ignore", "-i"])
            .arg(&pattern)
            .arg(&self.project_root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(RG_TIMEOUT_SECS),
            cmd.output(),
        )
        .await
        .ok()?
        .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() && stdout.is_empty() {
            return None;
        }
        let files: Vec<PathBuf> = stdout
            .lines()
            .filter_map(|l| {
                Path::new(l)
                    .strip_prefix(&self.project_root)
                    .ok()
                    .map(|p| p.to_path_buf())
            })
            .take(MAX_SEARCH_FILES)
            .collect();
        Some(files)
    }

    /// 降级搜索：内置递归遍历 + 子串匹配（任一关键词命中即返回文件）。
    async fn fallback_search(&self, keywords: &[String]) -> Vec<PathBuf> {
        let mut results = Vec::new();
        let mut visited = 0usize;
        walk_search(
            &self.project_root,
            &self.project_root,
            keywords,
            0,
            &mut results,
            &mut visited,
        );
        results
    }

    /// 读文件内容（带 token 估算与裁剪）。
    async fn read_file_chunk(&self, rel: &Path, max_tokens: u32) -> Option<ContextChunk> {
        let full = if rel.is_absolute() {
            rel.to_path_buf()
        } else {
            self.project_root.join(rel)
        };
        let content = tokio::fs::read_to_string(&full).await.ok()?;
        let full_tokens = estimate_tokens(&content);
        let (content, tokens) = if full_tokens > max_tokens {
            // 按字符裁剪到约 max_tokens
            let limit = (max_tokens as usize).saturating_mul(4);
            let truncated: String = content.chars().take(limit).collect();
            let t = estimate_tokens(&truncated);
            (format!("{truncated}\n...（已截断）"), t)
        } else {
            (content, full_tokens)
        };
        Some(ContextChunk {
            path: rel.to_path_buf(),
            content,
            relevance: "匹配用户问题或当前文件".into(),
            token_estimate: tokens,
        })
    }
}

/// 从用户消息与 hints 提取搜索关键词。
fn extract_keywords(user_message: &str, hints: &[String]) -> Vec<String> {
    let mut kws: Vec<String> = hints
        .iter()
        .filter(|s| !s.trim().is_empty())
        .cloned()
        .collect();
    // 从消息中取长度 >=3 的词（简单分词：按空白与标点）
    for word in user_message.split(|c: char| c.is_whitespace() || c.is_ascii_punctuation()) {
        let w = word.trim();
        if w.len() >= 3 && !kws.iter().any(|k| k == w) {
            kws.push(w.to_string());
        }
    }
    kws.truncate(8);
    kws
}

/// 是否应忽略的目录/文件名。
fn ignores(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".hg" | ".svn" | "node_modules" | "target" | "dist" | "build" | ".next" | ".xgent"
    )
}

/// 递归生成目录树文本。
fn walk_tree(root: &Path, dir: &Path, depth: u32, lines: &mut Vec<String>, count: &mut usize) {
    if depth > TREE_MAX_DEPTH || *count >= TREE_MAX_ENTRIES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if *count >= TREE_MAX_ENTRIES {
            lines.push("...".into());
            return;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let indent = "  ".repeat(depth as usize);
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        if ft.is_dir() {
            if ignores(&name_str) {
                continue;
            }
            lines.push(format!("{indent}{name_str}/"));
            *count += 1;
            walk_tree(root, &path, depth + 1, lines, count);
        } else if ft.is_file() {
            lines.push(format!("{indent}{name_str}"));
            *count += 1;
        }
        let _ = rel;
    }
}

/// 降级搜索的递归遍历。
fn walk_search(
    root: &Path,
    dir: &Path,
    keywords: &[String],
    depth: u32,
    results: &mut Vec<PathBuf>,
    visited: &mut usize,
) {
    if depth > TREE_MAX_DEPTH || results.len() >= MAX_SEARCH_FILES || *visited > 2000 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        if results.len() >= MAX_SEARCH_FILES {
            return;
        }
        *visited += 1;
        let name = entry.file_name();
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let path = entry.path();
        if ft.is_dir() {
            if ignores(&name.to_string_lossy()) {
                continue;
            }
            walk_search(root, &path, keywords, depth + 1, results, visited);
        } else if ft.is_file()
            && let Ok(content) = std::fs::read_to_string(&path)
            && keywords.iter().any(|k| content.contains(k.as_str()))
        {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            if !results.contains(&rel) {
                results.push(rel);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ContextQuery;

    fn provider(root: &Path) -> OnDemandContextProvider {
        OnDemandContextProvider::new(root.to_path_buf())
    }

    #[tokio::test]
    async fn tree_summary_includes_files_skips_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(root.join("src")).await.unwrap();
        tokio::fs::write(root.join("src/main.rs"), "fn main(){}")
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("target"))
            .await
            .unwrap();
        tokio::fs::write(root.join("target/junk.rs"), "x")
            .await
            .unwrap();
        let p = provider(root);
        let tree = p.tree_summary().await.unwrap();
        assert!(tree.contains("main.rs"));
        assert!(!tree.contains("junk.rs"), "应忽略 target");
    }

    #[tokio::test]
    async fn fallback_search_finds_matches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::write(root.join("a.rs"), "fn findme() {}\n")
            .await
            .unwrap();
        tokio::fs::write(root.join("b.txt"), "no match here")
            .await
            .unwrap();
        let p = provider(root);
        let files = p.fallback_search(&["findme".into()]).await;
        assert!(files.iter().any(|f| f == Path::new("a.rs")));
        assert!(!files.iter().any(|f| f == Path::new("b.txt")));
    }

    #[tokio::test]
    async fn read_file_chunk_truncates_over_budget() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let long = "a".repeat(1000);
        tokio::fs::write(root.join("big.txt"), &long).await.unwrap();
        let p = provider(root);
        // 极小预算
        let chunk = p.read_file_chunk(Path::new("big.txt"), 5).await.unwrap();
        assert!(chunk.content.contains("截断"));
        assert!(chunk.token_estimate <= 10);
    }

    #[tokio::test]
    async fn retrieve_returns_tree_and_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(root.join("src")).await.unwrap();
        tokio::fs::write(root.join("src/main.rs"), "fn findme() {}\n")
            .await
            .unwrap();
        let p = provider(root);
        let q = ContextQuery {
            user_message: "findme 在哪里".into(),
            current_file: None,
            hints: vec![],
            max_tokens: 500,
        };
        let r = p.retrieve(&q).await;
        assert!(r.tree_summary.is_some());
        // 应搜到含 findme 的文件
        assert!(r.chunks.iter().any(|c| c.path.ends_with("main.rs")));
    }

    #[tokio::test]
    async fn retrieve_prioritizes_current_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::write(root.join("cur.txt"), "current content")
            .await
            .unwrap();
        let p = provider(root);
        let q = ContextQuery {
            user_message: "看这个文件".into(),
            current_file: Some(PathBuf::from("cur.txt")),
            hints: vec![],
            max_tokens: 200,
        };
        let r = p.retrieve(&q).await;
        assert!(
            r.chunks
                .iter()
                .any(|c| c.content.contains("current content"))
        );
    }

    #[tokio::test]
    async fn retrieve_respects_token_budget() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // 多个文件，每个较大
        for i in 0..5 {
            tokio::fs::write(root.join(format!("f{i}.txt")), &"word ".repeat(200))
                .await
                .unwrap();
        }
        let p = provider(root);
        let q = ContextQuery {
            user_message: "word".into(),
            current_file: None,
            hints: vec![],
            max_tokens: 100,
        };
        let r = p.retrieve(&q).await;
        assert!(
            r.total_tokens <= 100 + 50,
            "总 token 不应远超预算: {}",
            r.total_tokens
        );
    }

    #[test]
    fn extract_keywords_filters_short() {
        let kws = extract_keywords("fn the hello world abc", &[]);
        // "the"/"fn" 长度 <3 被过滤，"hello"/"world"/"abc" 保留
        assert!(kws.contains(&"hello".to_string()));
        assert!(kws.contains(&"world".to_string()));
        assert!(!kws.iter().any(|k| k == "fn"));
    }

    #[test]
    fn extract_keywords_includes_hints() {
        let kws = extract_keywords("msg", &["hint1".into()]);
        assert!(kws.contains(&"hint1".to_string()));
    }
}
