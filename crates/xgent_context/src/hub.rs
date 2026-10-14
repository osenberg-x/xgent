//! ContextHub — 上下文提供者注册中心（Bevy Resource）。
//!
//! 照设计文档 §5.5。聚合内置策略 provider（按 ContextStrategy 构造，启动时注入）
//! 与动态注册的插件 provider。agent 检索时遍历全部 provider，合并结果。
//!
//! `ContextHub` 自身实现 `ContextProvider` trait，这样 `xgent_app` 可把它作为
//! `Arc<dyn ContextProvider>` 注入 `AgentBridgeConfig.context`，agent 侧无感于
//! 内置 vs 插件 provider 的区分（设计文档 §5.5：agent 从 ContextHub 取用）。

use std::sync::Arc;

use async_trait::async_trait;
use bevy::prelude::*;
use parking_lot::RwLock;

use crate::provider::{ContextChunk, ContextProvider, ContextQuery, ContextResult};

/// 上下文提供者注册中心（Bevy Resource）。
///
/// 内置 provider 在前（不受插件卸载影响），插件 provider 在后。
/// 用 `RwLock<Vec<...>>` 因 `ContextHub` impl `ContextProvider` 的 `retrieve`
/// 是 `&self`（不能 `&mut self`），但插件动态注册/卸载需改 vec。
#[derive(Resource, Default)]
pub struct ContextHub {
    /// 内置 provider（id 为策略名，如 "on_demand"），不受插件卸载影响。
    builtin: RwLock<Vec<Arc<dyn ContextProvider>>>,
    /// 插件 provider，full_id 已含 `plugin.` 前缀。
    plugin_providers: RwLock<Vec<(String, Arc<dyn ContextProvider>)>>,
}

impl ContextHub {
    /// 启动时注入内置 provider（由 `xgent_app` 在启动时调）。
    pub fn set_builtin(&self, providers: Vec<Arc<dyn ContextProvider>>) {
        *self.builtin.write() = providers;
    }

    /// 动态注册插件 provider（full_id 已含 `plugin.` 前缀）。
    pub fn register_provider(&self, full_id: String, p: Arc<dyn ContextProvider>) {
        self.plugin_providers.write().push((full_id, p));
    }

    /// 按前缀移除（插件卸载时批量清理 `plugin.<id>.`）。
    pub fn remove_by_prefix(&self, prefix: &str) {
        self.plugin_providers
            .write()
            .retain(|(id, _)| !id.starts_with(prefix));
    }

    /// 遍历所有 provider 检索并合并（内置在前，插件补充）。
    ///
    /// 合并策略：拼接 chunks（内置在前），tree_summary 取首个非 None，
    /// 最后按调用方预算复核（见 [`clamp_to_budget`]，R3-9）。
    ///
    /// 注意：先 clone Arc 出 RwLock 再 await，避免持有 `RwLockReadGuard` 跨 await
    /// （parking_lot guard 非 Send，跨 await 会破坏 future 的 Send 约束）。
    pub async fn retrieve_all(&self, query: &ContextQuery) -> ContextResult {
        let builtin: Vec<Arc<dyn ContextProvider>> = self.builtin.read().clone();
        let plugin: Vec<(String, Arc<dyn ContextProvider>)> = self.plugin_providers.read().clone();
        let mut result = ContextResult::default();
        for p in &builtin {
            let r = p.retrieve(query).await;
            result.chunks.extend(r.chunks);
            if result.tree_summary.is_none() {
                result.tree_summary = r.tree_summary;
            }
            result.total_tokens += r.total_tokens;
        }
        for (_, p) in &plugin {
            let r = p.retrieve(query).await;
            result.chunks.extend(r.chunks);
            if result.tree_summary.is_none() {
                result.tree_summary = r.tree_summary;
            }
            result.total_tokens += r.total_tokens;
        }
        let (chunks, total_tokens) = clamp_to_budget(result.chunks, query.max_tokens);
        result.chunks = chunks;
        result.total_tokens = total_tokens;
        result
    }
}

/// 按调用方预算复核合并结果。
///
/// 各 provider 只管自己的预算，合并后的总量可以远超 `query.max_tokens`
/// （R3-9）。这里按 provider 顺序贪心保留装得进的 chunk，装不下的**整块丢弃**
/// 而不改写内容：hub 截断别的 provider 交来的文件片段，会破坏 LLM 读到的代码。
/// 丢弃后继续看后面的 chunk，小的仍可能装得进剩余预算。
///
/// 返回的 `total_tokens` 由保留 chunk 的 `token_estimate` 重算——provider 自己的
/// 总数里混了目录树摘要，hub 无法据此断言 chunk 预算。`tree_summary` 不占 chunk 预算。
fn clamp_to_budget(chunks: Vec<ContextChunk>, max_tokens: u32) -> (Vec<ContextChunk>, u32) {
    let mut kept: Vec<ContextChunk> = Vec::new();
    let mut used: u32 = 0;
    for chunk in chunks {
        if used.saturating_add(chunk.token_estimate) > max_tokens {
            continue;
        }
        used += chunk.token_estimate;
        kept.push(chunk);
    }
    (kept, used)
}

/// `ContextHub` 自身实现 `ContextProvider`，便于 `xgent_app` 把它注入
/// `AgentBridgeConfig.context`（agent 侧无感于内置 vs 插件区分）。
#[async_trait]
impl ContextProvider for ContextHub {
    async fn retrieve(&self, query: &ContextQuery) -> ContextResult {
        self.retrieve_all(query).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 固定返回值的假 provider，用于验证合并与预算复核。
    struct Fixed {
        result: ContextResult,
    }

    impl Fixed {
        fn new(tree: Option<&str>, chunks: &[(&str, u32)]) -> Self {
            Self {
                result: ContextResult {
                    chunks: chunks
                        .iter()
                        .map(|(p, t)| ContextChunk {
                            path: PathBuf::from(p),
                            content: format!("{p} content"),
                            relevance: "测试片段".into(),
                            token_estimate: *t,
                        })
                        .collect(),
                    tree_summary: tree.map(str::to_string),
                    total_tokens: chunks.iter().map(|(_, t)| *t).sum(),
                },
            }
        }
    }

    #[async_trait]
    impl ContextProvider for Fixed {
        async fn retrieve(&self, _query: &ContextQuery) -> ContextResult {
            self.result.clone()
        }
    }

    fn query(max_tokens: u32) -> ContextQuery {
        ContextQuery {
            user_message: "问题".into(),
            current_file: None,
            hints: vec![],
            max_tokens,
        }
    }

    fn hub_with(builtin: Vec<ContextResult>, plugins: Vec<ContextResult>) -> ContextHub {
        let hub = ContextHub::default();
        hub.set_builtin(
            builtin
                .into_iter()
                .map(|r| Arc::new(Fixed { result: r }) as Arc<dyn ContextProvider>)
                .collect(),
        );
        for (i, r) in plugins.into_iter().enumerate() {
            hub.register_provider(
                format!("plugin.test{i}"),
                Arc::new(Fixed { result: r }) as Arc<dyn ContextProvider>,
            );
        }
        hub
    }

    fn result(tree: Option<&str>, chunks: &[(&str, u32)]) -> ContextResult {
        Fixed::new(tree, chunks).result
    }

    fn paths(r: &ContextResult) -> Vec<String> {
        r.chunks
            .iter()
            .map(|c| c.path.to_string_lossy().to_string())
            .collect()
    }

    /// A6：多 provider 合并超预算时按顺序保留、整块丢弃、重算总数。
    #[tokio::test]
    async fn over_budget_merge_keeps_prefix_and_recomputes_total() {
        let hub = hub_with(
            vec![
                result(Some("树"), &[("a.rs", 30), ("b.rs", 30)]),
                result(None, &[("c.rs", 30), ("d.rs", 5)]),
            ],
            vec![result(None, &[("e.rs", 5)])],
        );
        let r = hub.retrieve_all(&query(70)).await;
        // 30+30+5+5 = 70：装不下的整块丢弃，后面小的仍能装进
        assert_eq!(paths(&r), vec!["a.rs", "b.rs", "d.rs", "e.rs"]);
        assert_eq!(r.total_tokens, 70);
        assert_eq!(r.tree_summary.as_deref(), Some("树"));
        assert!(
            r.chunks.iter().all(|c| c.content.ends_with("content")),
            "保留的 chunk 内容不应被 hub 改写"
        );
    }

    /// A6：合并结果本就装得下时，chunk 集合与顺序不变。
    #[tokio::test]
    async fn within_budget_merge_is_unchanged() {
        let hub = hub_with(
            vec![
                result(Some("内置树"), &[("a.rs", 10)]),
                result(None, &[("b.rs", 10)]),
            ],
            vec![result(Some("插件树"), &[("c.rs", 10)])],
        );
        let r = hub.retrieve_all(&query(100)).await;
        assert_eq!(paths(&r), vec!["a.rs", "b.rs", "c.rs"]);
        assert_eq!(r.total_tokens, 30);
        assert_eq!(
            r.tree_summary.as_deref(),
            Some("内置树"),
            "取首个提供者的树"
        );
    }

    /// A6：预算为 0 时不保留任何 chunk，而不是全部保留。
    #[tokio::test]
    async fn zero_budget_keeps_nothing() {
        let hub = hub_with(vec![result(None, &[("a.rs", 1), ("b.rs", 1)])], vec![]);
        let r = hub.retrieve_all(&query(0)).await;
        assert!(r.chunks.is_empty(), "零预算不应保留 chunk：{:?}", paths(&r));
        assert_eq!(r.total_tokens, 0);
    }

    /// A6：空 provider 不影响合并结果，也不会让合并失败。
    #[tokio::test]
    async fn empty_provider_contributes_nothing() {
        let hub = hub_with(
            vec![result(None, &[]), result(Some("树"), &[("a.rs", 10)])],
            vec![result(None, &[])],
        );
        let r = hub.retrieve_all(&query(50)).await;
        assert_eq!(paths(&r), vec!["a.rs"]);
        assert_eq!(r.total_tokens, 10);
        assert_eq!(r.tree_summary.as_deref(), Some("树"));
    }

    #[test]
    fn clamp_drops_chunks_larger_than_budget() {
        let chunks = vec![ContextChunk {
            path: PathBuf::from("big.rs"),
            content: "x".into(),
            relevance: "测试".into(),
            token_estimate: 100,
        }];
        let (kept, total) = clamp_to_budget(chunks, 10);
        assert!(kept.is_empty(), "整块超预算应丢弃");
        assert_eq!(total, 0);
    }
}
