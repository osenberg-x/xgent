//! 方案 B：基于 tree-sitter 符号索引的仓库结构映射（F-21）。

use async_trait::async_trait;
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use xgent_codeintel::{CodeIndex, ImportTarget, Symbol};

use crate::provider::{
    ContextChunk, ContextProvider, ContextQuery, ContextResult, estimate_tokens,
};

/// 方案 B 上下文提供者：懒构建 Rust 符号索引，按查询返回结构化 repo map。
pub struct RepoMapContextProvider {
    project_root: PathBuf,
    index: Arc<RwLock<Option<Arc<CodeIndex>>>>,
    build_lock: Mutex<()>,
}

impl RepoMapContextProvider {
    /// 构造 provider；首次检索时才在阻塞线程池扫描索引。
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            project_root,
            index: Arc::new(RwLock::new(None)),
            build_lock: Mutex::new(()),
        }
    }

    async fn ensure_index(&self) -> Option<Arc<CodeIndex>> {
        if let Some(index) = self.index.read().clone() {
            return Some(index);
        }
        let _guard = self.build_lock.lock().await;
        if let Some(index) = self.index.read().clone() {
            return Some(index);
        }
        let root = self.project_root.clone();
        match tokio::task::spawn_blocking(move || CodeIndex::scan(&root)).await {
            Ok(Ok(index)) => {
                let index = Arc::new(index);
                *self.index.write() = Some(index.clone());
                Some(index)
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, root = %self.project_root.display(), "RepoMap 索引构建失败");
                None
            }
            Err(error) => {
                tracing::warn!(%error, "RepoMap 索引任务失败");
                None
            }
        }
    }

    fn relative_path(&self, path: &std::path::Path) -> Option<PathBuf> {
        path.strip_prefix(&self.project_root)
            .ok()
            .map(std::path::Path::to_path_buf)
    }
}

#[async_trait]
impl ContextProvider for RepoMapContextProvider {
    async fn retrieve(&self, query: &ContextQuery) -> ContextResult {
        let Some(index) = self.ensure_index().await else {
            return ContextResult::default();
        };
        let current = query
            .current_file
            .as_deref()
            .and_then(|path| self.relative_path(path));
        let terms = query_terms(query);
        if terms.is_empty() && current.is_none() {
            return ContextResult::default();
        }

        let mut candidates: Vec<(i32, &std::path::Path, Vec<&Symbol>)> = Vec::new();
        for (path, entry) in index.files() {
            let mut score = 0;
            let path_lower = path.to_string_lossy().to_lowercase();
            let path_matched = terms.iter().any(|term| path_lower.contains(term));
            for term in &terms {
                if path_lower.contains(term) {
                    score += 3;
                }
            }
            let current_match = current.as_deref() == Some(path);
            if current_match {
                score += 50;
            }
            let mut selected: Vec<&Symbol> = Vec::new();
            for symbol in &entry.symbols {
                let name = symbol.name.to_lowercase();
                let mut symbol_score = 0;
                for term in &terms {
                    if name == *term {
                        symbol_score += 20;
                    } else if name.contains(term) {
                        symbol_score += 8;
                    }
                    if let Some(container) = &symbol.container
                        && container.to_lowercase().contains(term)
                    {
                        symbol_score += 4;
                    }
                }
                if symbol_score > 0 {
                    score += symbol_score;
                }
                if symbol_score > 0 || current_match || path_matched {
                    selected.push(symbol);
                }
            }
            if score > 0 {
                candidates.push((score, path, selected));
            }
        }
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));

        let mut chunks = Vec::new();
        let mut total_tokens = 0;
        for (score, path, symbols) in candidates {
            let content = render_map(path, &symbols, &index);
            let tokens = estimate_tokens(&content);
            if total_tokens + tokens > query.max_tokens.max(1) {
                break;
            }
            total_tokens += tokens;
            chunks.push(ContextChunk {
                path: path.to_path_buf(),
                content,
                relevance: if current.as_deref() == Some(path) {
                    "repo map：当前文件".to_string()
                } else if score >= 20 {
                    "repo map：符号匹配".to_string()
                } else {
                    "repo map：路径/上下文匹配".to_string()
                },
                token_estimate: tokens,
            });
        }
        ContextResult {
            chunks,
            tree_summary: None,
            total_tokens,
        }
    }

    async fn on_file_changed(&self, path: &PathBuf) {
        if path.extension().is_none_or(|ext| ext != "rs") || !path.starts_with(&self.project_root) {
            return;
        }
        let _guard = self.build_lock.lock().await;
        let Some(current) = self.index.read().clone() else {
            return;
        };
        let root = self.project_root.clone();
        let changed = path.clone();
        let updated = tokio::task::spawn_blocking(move || {
            let mut index = (*current).clone();
            let result = index.update_file_from_disk(&root, &changed);
            (index, result)
        })
        .await;
        match updated {
            Ok((index, Ok(()))) => {
                *self.index.write() = Some(Arc::new(index));
            }
            Ok((_, Err(error))) => {
                tracing::warn!(%error, path = %path.display(), "RepoMap 增量更新失败");
            }
            Err(error) => {
                tracing::warn!(%error, "RepoMap 增量更新任务失败");
            }
        }
    }
}

fn query_terms(query: &ContextQuery) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "the", "and", "for", "with", "this", "that", "from", "what", "when", "where", "which",
        "how", "why", "is", "are", "was", "were", "not", "but", "can", "will", "should", "please",
    ];
    let mut terms = Vec::new();
    let mut push = |text: &str| {
        for word in text
            .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
            .filter(|word| word.len() >= 3)
        {
            let word = word.to_lowercase();
            if !STOPWORDS.contains(&word.as_str()) && !terms.contains(&word) {
                terms.push(word);
            }
        }
    };
    push(&query.user_message);
    for hint in &query.hints {
        if let Some(path) = hint.strip_prefix("@file:") {
            if let Some(stem) = std::path::Path::new(path).file_stem() {
                push(&stem.to_string_lossy());
            }
        } else {
            push(hint);
        }
    }
    terms
}

fn render_map(path: &std::path::Path, symbols: &[&Symbol], index: &CodeIndex) -> String {
    let mut text = format!("{}:\n", path.display());
    for symbol in symbols.iter().take(24) {
        let mut line = format!(
            "  {} {} (line {}-{})",
            symbol.kind.as_str(),
            symbol.name,
            symbol.range.start_line,
            symbol.range.end_line
        );
        if symbol.complexity > 1 {
            line.push_str(&format!(" c{}", symbol.complexity));
        }
        text.push_str(&line);
        text.push('\n');
    }
    // 带一条 import 摘要，帮助模型理解文件边界（不展开外部依赖）。
    let workspace_targets: Vec<String> = index
        .file(path)
        .map(|entry| {
            entry
                .imports
                .iter()
                .filter_map(|import| match &import.target {
                    ImportTarget::Workspace(target) => Some(target.display().to_string()),
                    _ => None,
                })
                .take(6)
                .collect()
        })
        .unwrap_or_default();
    if !workspace_targets.is_empty() {
        text.push_str("  imports: ");
        text.push_str(&workspace_targets.join(", "));
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &std::path::Path, relative: &str, contents: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("创建目录");
        }
        fs::write(path, contents).expect("写文件");
    }

    fn query(text: &str, current: Option<PathBuf>) -> ContextQuery {
        ContextQuery {
            user_message: text.into(),
            current_file: current,
            hints: Vec::new(),
            max_tokens: 2000,
        }
    }

    #[tokio::test]
    async fn retrieves_matching_symbols() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(
            temp.path(),
            "src/lib.rs",
            "pub fn parse_manifest() {}\npub struct Manifest;\n",
        );
        write(temp.path(), "src/other.rs", "pub fn unrelated() {}\n");
        let provider = RepoMapContextProvider::new(temp.path().to_path_buf());
        let result = provider
            .retrieve(&query("where is parse_manifest", None))
            .await;
        assert!(!result.chunks.is_empty(), "应返回 repo map chunk");
        assert!(result.total_tokens > 0);
        assert!(
            result
                .chunks
                .iter()
                .any(|chunk| chunk.path == std::path::Path::new("src/lib.rs")
                    && chunk.content.contains("parse_manifest")),
            "应命中 lib.rs 的 parse_manifest：{:?}",
            result.chunks
        );
        assert!(
            !result
                .chunks
                .iter()
                .any(|chunk| chunk.content.contains("unrelated"))
        );
    }

    #[tokio::test]
    async fn file_hints_and_path_queries_return_the_whole_file_map() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(
            temp.path(),
            "src/parser.rs",
            "pub fn parse_manifest() {}\npub struct Manifest;\n",
        );
        write(temp.path(), "src/other.rs", "pub fn unrelated() {}\n");
        let provider = RepoMapContextProvider::new(temp.path().to_path_buf());

        let mut hinted = query("please explain", None);
        hinted.hints.push("@file:src/parser.rs".into());
        let result = provider.retrieve(&hinted).await;
        assert!(
            result
                .chunks
                .iter()
                .any(|chunk| chunk.path == std::path::Path::new("src/parser.rs")
                    && chunk.content.contains("parse_manifest")),
            "文件 hint 应返回该文件的结构：{:?}",
            result.chunks
        );
        assert!(
            !result
                .chunks
                .iter()
                .any(|chunk| chunk.path == std::path::Path::new("src/other.rs"))
        );

        let by_path = provider.retrieve(&query("src/parser.rs", None)).await;
        assert!(
            by_path
                .chunks
                .iter()
                .any(|chunk| chunk.path == std::path::Path::new("src/parser.rs")),
            "路径查询应命中文件：{:?}",
            by_path.chunks
        );
    }

    #[tokio::test]
    async fn retrieve_and_change_notification_do_not_write_files() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(temp.path(), "src/a.rs", "pub fn alpha() {}\n");
        let path = temp.path().join("src/a.rs");
        let before = fs::read(&path).expect("读文件");
        let provider = RepoMapContextProvider::new(temp.path().to_path_buf());
        assert!(
            !provider
                .retrieve(&query("alpha", None))
                .await
                .chunks
                .is_empty()
        );
        provider.on_file_changed(&path).await;
        assert_eq!(
            fs::read(&path).expect("再读文件"),
            before,
            "扫描/查询不得写盘"
        );
    }

    #[tokio::test]
    async fn current_file_is_prioritized_even_without_query_match() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(temp.path(), "src/a.rs", "pub fn alpha() {}\n");
        write(temp.path(), "src/b.rs", "pub fn beta() {}\n");
        let provider = RepoMapContextProvider::new(temp.path().to_path_buf());
        let result = provider
            .retrieve(&query("explain this", Some(temp.path().join("src/b.rs"))))
            .await;
        assert_eq!(result.chunks[0].path, std::path::Path::new("src/b.rs"));
        assert!(result.chunks[0].content.contains("beta"));
    }

    #[tokio::test]
    async fn on_file_changed_updates_index_and_ignores_non_rust() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(temp.path(), "src/a.rs", "pub fn old_name() {}\n");
        let provider = RepoMapContextProvider::new(temp.path().to_path_buf());
        assert!(
            !provider
                .retrieve(&query("old_name", None))
                .await
                .chunks
                .is_empty()
        );

        let path = temp.path().join("src/a.rs");
        write(temp.path(), "src/a.rs", "pub fn new_name() {}\n");
        provider.on_file_changed(&path).await;
        assert!(
            provider
                .retrieve(&query("new_name", None))
                .await
                .chunks
                .iter()
                .any(|chunk| chunk.content.contains("new_name")),
            "变更后应看到新符号"
        );
        provider
            .on_file_changed(&temp.path().join("README.md"))
            .await;
        provider
            .on_file_changed(&temp.path().join("missing.rs"))
            .await;
        assert!(
            !provider
                .retrieve(&query("new_name", None))
                .await
                .chunks
                .is_empty()
        );
    }

    #[tokio::test]
    async fn no_match_returns_empty_without_error() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(temp.path(), "src/a.rs", "pub fn alpha() {}\n");
        let provider = RepoMapContextProvider::new(temp.path().to_path_buf());
        let result = provider.retrieve(&query("zzzz-no-such", None)).await;
        assert!(result.chunks.is_empty());
        assert_eq!(result.total_tokens, 0);
    }

    #[tokio::test]
    async fn respects_token_budget() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(
            temp.path(),
            "src/a.rs",
            "pub fn alpha() {}
pub fn beta() {}
",
        );
        let provider = RepoMapContextProvider::new(temp.path().to_path_buf());
        let mut q = query("alpha beta", None);
        q.max_tokens = 1;
        let result = provider.retrieve(&q).await;
        assert!(result.total_tokens <= 1);
    }

    #[tokio::test]
    async fn missing_root_degrades_to_empty() {
        let provider = RepoMapContextProvider::new(PathBuf::from("/definitely/not/a/repo"));
        let result = provider.retrieve(&query("anything", None)).await;
        assert!(result.chunks.is_empty());
    }
}
