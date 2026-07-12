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
}

/// 最近一次上下文检索结果。
#[derive(Resource, Debug, Clone, Default)]
pub struct ContextState {
    pub result: ContextResult,
}
