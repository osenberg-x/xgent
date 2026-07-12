//! 用户自定义 API 适配器（占位）。
//!
//! MVP 不实现，trait 方法返回 `ProviderError::Config`。
//! 预留 endpoint/headers/body 模板的扩展点。

use async_trait::async_trait;
use xgent_core::chat::ChatRequest;
use xgent_core::ids::StreamId;

use crate::provider::{ChatStream, LlmProvider, ModelInfo, ProviderError};

/// 用户自定义 API 适配器（占位）。
pub struct CustomApiProvider {
    id: String,
}

impl CustomApiProvider {
    pub fn new(id: String) -> Self {
        Self { id }
    }
}

#[async_trait]
impl LlmProvider for CustomApiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Err(ProviderError::Config("CustomApiProvider 尚未实现".into()))
    }

    async fn chat(&self, _req: ChatRequest) -> Result<(StreamId, ChatStream), ProviderError> {
        Err(ProviderError::Config("CustomApiProvider 尚未实现".into()))
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        Err(ProviderError::Config("CustomApiProvider 尚未实现".into()))
    }
}
