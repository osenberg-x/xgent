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

use crate::provider::{ContextProvider, ContextQuery, ContextResult};

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
    /// total_tokens 累加。
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
        result
    }
}

/// `ContextHub` 自身实现 `ContextProvider`，便于 `xgent_app` 把它注入
/// `AgentBridgeConfig.context`（agent 侧无感于内置 vs 插件区分）。
#[async_trait]
impl ContextProvider for ContextHub {
    async fn retrieve(&self, query: &ContextQuery) -> ContextResult {
        self.retrieve_all(query).await
    }
}
