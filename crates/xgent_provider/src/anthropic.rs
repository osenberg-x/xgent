//! Anthropic 原生适配器（占位）。
//!
//! MVP 不实现，trait 方法返回 `ProviderError::Config`。

use async_trait::async_trait;
use xgent_core::chat::ChatRequest;
use xgent_core::ids::StreamId;

use crate::provider::{ChatStream, LlmProvider, ModelInfo, ProviderError};

/// Anthropic 原生适配器（占位）。
pub struct AnthropicProvider {
    id: String,
}

impl AnthropicProvider {
    pub fn new(id: String) -> Self {
        Self { id }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Err(ProviderError::Config("AnthropicProvider 尚未实现".into()))
    }

    async fn chat(&self, _req: ChatRequest) -> Result<(StreamId, ChatStream), ProviderError> {
        Err(ProviderError::Config("AnthropicProvider 尚未实现".into()))
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        Err(ProviderError::Config("AnthropicProvider 尚未实现".into()))
    }
}
