//! xgent_context — 项目上下文检索层。
//!
//! 提供统一的 [`ContextProvider`] 抽象，MVP 实现方案 A（无索引·按需读取）。
//! 方案 B/C/D/E 为占位，后续迭代实现。agent 侧通过 trait 调用，无感于策略切换。

pub mod hub;
pub mod hybrid;
pub mod lsp;
pub mod on_demand;
pub mod provider;
pub mod repo_map;
pub mod vector;

pub use hub::ContextHub;
pub use hybrid::HybridContextProvider;
pub use lsp::LspContextProvider;
pub use on_demand::OnDemandContextProvider;
pub use provider::{ContextChunk, ContextProvider, ContextQuery, ContextResult, estimate_tokens};
pub use repo_map::RepoMapContextProvider;
pub use vector::VectorContextProvider;

use std::path::PathBuf;
use xgent_settings_core::ContextStrategy;

/// 检索策略的构造结果。
pub enum BuiltContextProvider {
    /// 该策略有实现，可用
    Ready(Box<dyn ContextProvider>),
    /// 该策略尚无实现（占位 provider），启动时应显式报错而非静默回退
    Unsupported(ContextStrategy),
}

/// 按 [`ContextStrategy`] 构造上下文提供者。
///
/// 无实现的策略返回 [`BuiltContextProvider::Unsupported`] 而**不是**静默换成
/// 另一个策略：静默回退会让误配置的项目看起来完全正常，用户无从察觉
/// 自己的 `context_strategy` 没生效（R2-4）。
pub fn build_context_provider(
    strategy: ContextStrategy,
    project_root: PathBuf,
) -> BuiltContextProvider {
    match strategy {
        ContextStrategy::OnDemand => {
            BuiltContextProvider::Ready(Box::new(OnDemandContextProvider::new(project_root)))
        }
        // B/C/D/E 仍是占位（repo_map/vector/lsp/hybrid 返回空结果）
        ContextStrategy::RepoMap => BuiltContextProvider::Unsupported(strategy),
        ContextStrategy::Vector => BuiltContextProvider::Unsupported(strategy),
        ContextStrategy::Hybrid => BuiltContextProvider::Unsupported(strategy),
    }
}

/// XGent 上下文插件：初始化 `ContextHub` Resource。
///
/// 照设计文档 §5.5。`build()` 内 `init_resource::<ContextHub>()`。
/// 内置 provider 的注入由 `xgent_app` 在启动时调 `ContextHub::set_builtin`
/// （因内置 provider 需 project_root，而 Plugin build 时无此信息）。
pub struct XgentContextPlugin;

impl bevy::prelude::Plugin for XgentContextPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_resource::<ContextHub>();
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ondemand_is_ready() {
        match build_context_provider(ContextStrategy::OnDemand, PathBuf::from("/proj")) {
            BuiltContextProvider::Ready(p) => {
                // /proj 不存在：树摘要为 None，无 chunks，且不 panic
                let rt = tokio::runtime::Runtime::new().unwrap();
                let q = ContextQuery {
                    user_message: "x".into(),
                    current_file: None,
                    hints: vec![],
                    max_tokens: 100,
                };
                let r = rt.block_on(p.retrieve(&q));
                assert!(r.chunks.is_empty());
            }
            BuiltContextProvider::Unsupported(_) => {
                panic!("OnDemand 应有实现")
            }
        }
    }

    /// 无实现的策略必须报 Unsupported，**不得**静默回退到 OnDemand——
    /// 否则误配置的项目看起来完全正常（R2-4）。
    #[test]
    fn unimplemented_strategies_report_unsupported() {
        for s in [
            ContextStrategy::RepoMap,
            ContextStrategy::Vector,
            ContextStrategy::Hybrid,
        ] {
            match build_context_provider(s, PathBuf::from("/proj")) {
                BuiltContextProvider::Unsupported(got) => {
                    assert_eq!(format!("{got:?}"), format!("{s:?}"), "应回报原策略");
                }
                BuiltContextProvider::Ready(_) => {
                    panic!("{s:?} 尚无实现，不应返回可用 provider")
                }
            }
        }
    }
}
