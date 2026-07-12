//! xgent_context — 项目上下文检索层。
//!
//! 提供统一的 [`ContextProvider`] 抽象，MVP 实现方案 A（无索引·按需读取）。
//! 方案 B/C/D/E 为占位，后续迭代实现。agent 侧通过 trait 调用，无感于策略切换。

pub mod hybrid;
pub mod lsp;
pub mod on_demand;
pub mod provider;
pub mod repo_map;
pub mod vector;

pub use hybrid::HybridContextProvider;
pub use lsp::LspContextProvider;
pub use on_demand::OnDemandContextProvider;
pub use provider::{ContextChunk, ContextProvider, ContextQuery, ContextResult, estimate_tokens};
pub use repo_map::RepoMapContextProvider;
pub use vector::VectorContextProvider;

use std::path::PathBuf;
use xgent_settings_core::ContextStrategy;

/// 按 [`ContextStrategy`] 构造上下文提供者。
///
/// 调用方（agent）无感于具体策略实现。
pub fn build_context_provider(
    strategy: ContextStrategy,
    project_root: PathBuf,
) -> Box<dyn ContextProvider> {
    match strategy {
        ContextStrategy::OnDemand => Box::new(OnDemandContextProvider::new(project_root)),
        ContextStrategy::RepoMap => Box::new(RepoMapContextProvider::new(project_root)),
        ContextStrategy::Vector => Box::new(VectorContextProvider::new(project_root)),
        ContextStrategy::Hybrid => Box::new(HybridContextProvider::new(project_root)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ondemand() {
        let p = build_context_provider(ContextStrategy::OnDemand, PathBuf::from("/proj"));
        // OnDemand 的 retrieve 在无项目时不 panic
        let rt = tokio::runtime::Runtime::new().unwrap();
        let q = ContextQuery {
            user_message: "x".into(),
            current_file: None,
            hints: vec![],
            max_tokens: 100,
        };
        let r = rt.block_on(p.retrieve(&q));
        // /proj 不存在，树摘要为 None，无 chunks
        assert!(r.chunks.is_empty());
    }

    #[test]
    fn build_placeholders_do_not_error() {
        for s in [
            ContextStrategy::RepoMap,
            ContextStrategy::Vector,
            ContextStrategy::Hybrid,
        ] {
            let p = build_context_provider(s, PathBuf::from("/proj"));
            let q = ContextQuery {
                user_message: "x".into(),
                current_file: None,
                hints: vec![],
                max_tokens: 100,
            };
            let rt = tokio::runtime::Runtime::new().unwrap();
            let r = rt.block_on(p.retrieve(&q));
            assert!(r.chunks.is_empty());
        }
    }
}
