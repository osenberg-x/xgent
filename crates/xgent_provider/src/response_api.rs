//! Response API 风格适配器（占位）。
//!
//! MVP 不实现，trait 方法返回 `ProviderError::Config`。
//! 在后续迭代中完善。

use async_trait::async_trait;
use tokio::sync::mpsc;
use xgent_core::chat::ChatRequest;

use crate::provider::{ChatStream, LlmProvider, ModelInfo, ProviderError};
use xgent_core::ids::StreamId;

/// Response API 风格适配器（占位）。
pub struct ResponseApiProvider {
    id: String,
}

impl ResponseApiProvider {
    pub fn new(id: String) -> Self {
        Self { id }
    }
}

#[async_trait]
impl LlmProvider for ResponseApiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Err(ProviderError::Config("ResponseApiProvider 尚未实现".into()))
    }

    async fn chat(&self, _req: ChatRequest) -> Result<(StreamId, ChatStream), ProviderError> {
        Err(ProviderError::Config("ResponseApiProvider 尚未实现".into()))
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        Err(ProviderError::Config("ResponseApiProvider 尚未实现".into()))
    }
}

// 避免 unused 警告：mpsc 在 trait 签名中间接使用
const _: fn() = || {
    let _ = mpsc::channel::<()>(1);
};
