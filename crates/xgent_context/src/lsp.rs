//! 方案 D：LSP 辅助检索（占位，D 阶段实现）。

use async_trait::async_trait;
use std::path::PathBuf;

use crate::provider::{ContextProvider, ContextQuery, ContextResult};

/// 方案 D 上下文提供者（占位）。
pub struct LspContextProvider {
    #[allow(dead_code)]
    project_root: PathBuf,
}

impl LspContextProvider {
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

#[async_trait]
impl ContextProvider for LspContextProvider {
    async fn retrieve(&self, _q: &ContextQuery) -> ContextResult {
        ContextResult::default()
    }
}
