use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Provider 唯一标识
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderId(pub String);

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 模型标识
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelId(pub String);

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 模型信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: ModelId,
    pub display_name: String,
    pub content_window: usize,
}

/// LLM Provider 抽象 trait
///
/// 所有 Provider 都必须实现这个 trait
/// Agent 引擎只依赖此 trait，不绑定具体实现
#[async_trait::async_trait]
pub trait LLMProvider: Send + Sync {
    /// Provider 唯一标识
    fn id(&self) -> &ProviderId;

    /// 列出可用模型
    fn list_models(&self) -> Vec<ModelInfo>;

    /// 发送 Chat 请求（支持 SSE 流式）
    async fn chat(&self, request: ChatRequest) -> Result<ChatStream, ProviderError>;

    /// 健康检查
    async fn health(&self) -> Result<(), ProviderError>;
}

/// Provider 错误类型
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error: {status} {message}")]
    Api { status: u16, message: String },

    #[error("Stream error: {0}")]
    Stream(String),

    #[error("Rate limited, retry after {0:?}")]
    RateLimited(Option<std::time::Duration>),

    #[error("Configuration error: {0}")]
    Config(String),
}
