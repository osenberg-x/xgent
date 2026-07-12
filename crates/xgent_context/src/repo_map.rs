//! 方案 B：基于 tree-sitter 仓库结构映射（占位，P1 实现）。

use async_trait::async_trait;
use std::path::PathBuf;

use crate::provider::{ContextProvider, ContextQuery, ContextResult};

/// 方案 B 上下文提供者（占位）。
pub struct RepoMapContextProvider {
    #[allow(dead_code)]
    project_root: PathBuf,
}

impl RepoMapContextProvider {
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

#[async_trait]
impl ContextProvider for RepoMapContextProvider {
    async fn retrieve(&self, _q: &ContextQuery) -> ContextResult {
        // P1 实现：tree-sitter 符号图
        ContextResult::default()
    }
}
