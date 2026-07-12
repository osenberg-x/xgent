//! 方案 C：向量检索（占位，C 阶段实现）。

use async_trait::async_trait;
use std::path::PathBuf;

use crate::provider::{ContextProvider, ContextQuery, ContextResult};

/// 方案 C 上下文提供者（占位）。
pub struct VectorContextProvider {
    #[allow(dead_code)]
    project_root: PathBuf,
}

impl VectorContextProvider {
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

#[async_trait]
impl ContextProvider for VectorContextProvider {
    async fn retrieve(&self, _q: &ContextQuery) -> ContextResult {
        ContextResult::default()
    }
}
