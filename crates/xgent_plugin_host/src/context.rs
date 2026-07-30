//! PluginContextProvider — 插件 ContextProvider 适配器。
//!
//! 照设计文档 §5.5。包装为 `xgent_context::ContextProvider` trait。
//! 失败降级为空结果 + `tracing::warn`（不静默吞错）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use xgent_context::provider::{ContextChunk, ContextProvider, ContextQuery, ContextResult};
use xgent_plugin::{WasmPlugin, WitContextChunk, WitContextQuery, WitContextResult};
use tokio_util::sync::CancellationToken;

/// 插件上下文提供者适配器。
pub struct PluginContextProvider {
    /// 完整 id：plugin.<plugin_id>.<short_id>
    pub full_id: String,
    /// 插件内短 id
    pub short_id: String,
    /// 插件 id（日志用）
    pub plugin_id: String,
    /// 项目根
    pub project_root: PathBuf,
    /// 插件 WASM 实例
    pub plugin: Arc<WasmPlugin>,
}

#[async_trait]
impl ContextProvider for PluginContextProvider {
    async fn retrieve(&self, query: &ContextQuery) -> ContextResult {
        // ContextQuery → WIT context-query（PathBuf → 相对路径 string，§15.3）
        let wit_query = WitContextQuery {
            user_message: query.user_message.clone(),
            current_file: query
                .current_file
                .as_ref()
                .and_then(|p| {
                    p.strip_prefix(&self.project_root)
                        .ok()
                        .map(|r| r.to_string_lossy().into_owned())
                }),
            hints: query.hints.clone(),
            max_tokens: query.max_tokens,
        };
        match self
            .plugin
            .call_retrieve(&self.short_id, wit_query, CancellationToken::new())
            .await
        {
            Ok(r) => context_result_from_wit(&r, &self.project_root),
            Err(e) => {
                // 插件失败不静默——记宿主侧日志 warn，便于调试。
                // 返回空结果降级（agent 拿到空上下文，不阻断对话）。
                tracing::warn!(
                    plugin = %self.plugin_id,
                    error = %e,
                    "插件 ContextProvider retrieve 失败，降级为空"
                );
                ContextResult::default()
            }
        }
    }

    async fn on_file_changed(&self, path: &PathBuf) {
        // 调 WIT context-provider.on-file-changed（path: option<string>）。
        // rel 为 None 时（路径不在项目根内）仍传递，插件可自行决定忽略。
        let rel: Option<String> = path
            .strip_prefix(&self.project_root)
            .ok()
            .map(|p| p.to_string_lossy().into_owned());
        // fire-and-forget；失败仅 warn（不阻断，见 §5.5）。
        if let Err(e) = self.plugin.call_on_file_changed(&self.short_id, rel).await {
            tracing::warn!(
                plugin = %self.plugin_id,
                error = %e,
                "插件 on-file-changed 失败，忽略"
            );
        }
    }
}

/// WIT context-result → 宿主侧 ContextResult。
///
/// `path: string`（相对项目根）→ `PathBuf`（直接包装，渲染时按相对路径处理，§15.3）。
fn context_result_from_wit(r: &WitContextResult, _project_root: &Path) -> ContextResult {
    let chunks: Vec<ContextChunk> = r
        .chunks
        .iter()
        .map(|c: &WitContextChunk| ContextChunk {
            path: PathBuf::from(&c.path),
            content: c.content.clone(),
            relevance: c.relevance.clone(),
            token_estimate: c.token_estimate,
        })
        .collect();
    ContextResult {
        chunks,
        tree_summary: r.tree_summary.clone(),
        total_tokens: r.total_tokens,
    }
}

