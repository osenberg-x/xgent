//! LLM Provider 抽象层。
//!
//! 定义统一的 provider 接口（id、列模型、流式对话、健康检查）与流式输出类型。
//! 具体适配器（OpenAI compatible 等）实现 [`LlmProvider`] trait。

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::mpsc;
use xgent_core::chat::{ChatEvent, ChatRequest};
use xgent_core::ids::StreamId;

/// 流式对话的接收端。
///
/// provider 在 [`LlmProvider::chat`] 中 spawn 异步任务，把 SSE 事件转换为
/// [`ChatEvent`] 后发送到此 channel，调用方从 `ChatStream` 读取。
pub type ChatStream = mpsc::Receiver<ChatEvent>;

/// LLM Provider 抽象 trait。
///
/// 不依赖 Bevy——纯异步逻辑，daemon 侧持有实例池；UI 侧经 IPC 调用，不直接实例化。
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// provider 唯一标识，如 `"openai"`、`"ollama"`
    fn id(&self) -> &str;

    /// 列出可用模型。
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;

    /// 流式对话。
    ///
    /// 返回 `(StreamId, ChatStream)`，调用方据 `StreamId` 关联后续 chunk
    /// （daemon 侧用于 IPC 路由；本地直调可忽略）。
    async fn chat(&self, req: ChatRequest) -> Result<(StreamId, ChatStream), ProviderError>;

    /// 健康检查。
    async fn health_check(&self) -> Result<(), ProviderError>;
}

/// 模型信息。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelInfo {
    /// 模型 id（API 实际标识，如 `"gpt-4"`）
    pub id: String,
    /// 显示名称
    pub name: String,
    /// 上下文窗口大小（token 数），未知则 None
    pub context_window: Option<u32>,
}

/// Provider 错误。
///
/// 区分网络/API/流解析/配置，便于上层重试与提示。
#[derive(Debug, Error)]
pub enum ProviderError {
    /// 网络层错误（连接、超时等）
    #[error("network: {0}")]
    Network(String),
    /// API 返回非成功状态码
    #[error("api: status {status}, body: {body}")]
    Api { status: u16, body: String },
    /// SSE 流解析错误
    #[error("stream parse: {0}")]
    Stream(String),
    /// 配置错误（如缺 api_base/key）
    #[error("config: {0}")]
    Config(String),
}

impl ProviderError {
    /// 把 ProviderError 映射为 UI 侧 ErrorKind（ADR 0003）。
    ///
    /// UI 不感知 HTTP 状态码：401/403 → AuthFailed，其余 Api → ProviderError。
    pub fn to_error_kind(&self) -> xgent_core::chat::ErrorKind {
        match self {
            ProviderError::Network(_) => xgent_core::chat::ErrorKind::Network,
            ProviderError::Stream(_) => xgent_core::chat::ErrorKind::StreamParse,
            ProviderError::Config(_) => xgent_core::chat::ErrorKind::NotConfigured,
            ProviderError::Api { status, .. } => {
                if *status == 401 || *status == 403 {
                    xgent_core::chat::ErrorKind::AuthFailed
                } else {
                    xgent_core::chat::ErrorKind::ProviderError
                }
            }
        }
    }
}

impl From<reqwest::Error> for ProviderError {
    fn from(e: reqwest::Error) -> Self {
        ProviderError::Network(e.to_string())
    }
}

impl From<serde_json::Error> for ProviderError {
    fn from(e: serde_json::Error) -> Self {
        ProviderError::Stream(format!("json: {e}"))
    }
}
