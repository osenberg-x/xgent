//! provider 配置与上下文状态的 Bevy Resource。
//!
//! `ProviderInfo`：当前会话使用的 provider id 与 model（从全局配置派生）。
//! `ContextState`：最近一次上下文检索结果，供构造请求时注入。

use bevy::prelude::*;
use xgent_context::provider::ContextResult;

/// 当前 provider 配置信息。
#[derive(Resource, Debug, Clone, Default)]
pub struct ProviderInfo {
    /// provider id，如 "openai"
    pub id: String,
    /// 模型名
    pub model: String,
    /// provider 是否就绪（按 kind 分判据，由 config_bridge 刷新时判定）
    pub ready: bool,
    /// provider 类型（就绪判据按 kind 分，如 Ollama 无 key 合法）
    pub kind: Option<xgent_settings_core::global::ProviderKind>,
}

/// 最近一次上下文检索结果。
#[derive(Resource, Debug, Clone, Default)]
pub struct ContextState {
    pub result: ContextResult,
}
