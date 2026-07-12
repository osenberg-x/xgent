//! 方案 E：混合检索（占位，E 阶段实现）。

use async_trait::async_trait;
use std::path::PathBuf;

use crate::provider::{ContextProvider, ContextQuery, ContextResult};

/// 方案 E 上下文提供者（占位）。
pub struct HybridContextProvider {
    #[allow(dead_code)]
    project_root: PathBuf,
}

impl HybridContextProvider {
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

#[async_trait]
impl ContextProvider for HybridContextProvider {
    async fn retrieve(&self, _q: &ContextQuery) -> ContextResult {
        ContextResult::default()
    }
}
