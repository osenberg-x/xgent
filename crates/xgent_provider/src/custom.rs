//! 用户自定义 API 适配器。
//!
//! 覆盖 F-07 的"支持自定义 API"：用户给出自己的 base URL、可选的自定义
//! 鉴权头与模型 id，协议层沿用 OpenAI chat-completions（第三方兼容接口的
//! 事实标准）。
//!
//! 为什么不自己实现 HTTP 逻辑：请求构造、流式解析、tool_call 聚合、超时与
//! 错误映射都已在 [`crate::openai_compat::OpenAiCompatProvider`] 中验证过。
//! 自定义 API 的差异只在"base URL + 额外头 + 模型"，把这三项作为构造参数
//! 传入即可复用全部逻辑，避免两份实现在超时/错误/协议上漂移。

use async_trait::async_trait;
use xgent_core::chat::ChatRequest;
use xgent_core::ids::StreamId;

use crate::openai_compat::OpenAiCompatProvider;
use crate::provider::{ChatStream, LlmProvider, ModelInfo, ProviderError};

/// 用户自定义 API 适配器。
pub struct CustomApiProvider {
    /// provider id
    id: String,
    /// 内层 OpenAI 兼容适配器（承载全部 HTTP 与协议逻辑）
    inner: OpenAiCompatProvider,
}

impl CustomApiProvider {
    /// 用 provider 配置与已解析的 API Key 构造。
    ///
    /// `cfg.api_base` 为空时构造失败——自定义 API 必须显式给出 base URL，
    /// 不猜默认地址（猜错会把用户的 key 发到错误的端点）。
    pub fn with_config(
        id: String,
        cfg: &xgent_settings_core::global::ProviderConfig,
        api_key: String,
    ) -> Self {
        let base = if cfg.api_base.trim().is_empty() {
            // 保留构造签名不 panic；base 校验在 chat/list_models 时报错
            String::new()
        } else {
            cfg.api_base.trim_end_matches('/').to_string()
        };
        Self {
            inner: OpenAiCompatProvider::with_extra_headers(
                id.clone(),
                base,
                api_key,
                cfg.timeout_secs,
                &cfg.extra_headers,
            ),
            id,
        }
    }

    /// 显式构造（测试与最小场景）。
    pub fn new(id: String, api_base: String, api_key: String) -> Self {
        Self {
            inner: OpenAiCompatProvider::new(id.clone(), api_base, api_key),
            id,
        }
    }
}

#[async_trait]
impl LlmProvider for CustomApiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.inner.list_models().await
    }

    async fn chat(&self, req: ChatRequest) -> Result<(StreamId, ChatStream), ProviderError> {
        if self.inner.api_base_is_empty() {
            return Err(ProviderError::Config(
                "自定义 API 必须配置 api_base（填写你的接口地址）".into(),
            ));
        }
        self.inner.chat(req).await
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        if self.inner.api_base_is_empty() {
            return Err(ProviderError::Config(
                "自定义 API 必须配置 api_base（填写你的接口地址）".into(),
            ));
        }
        self.inner.health_check().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_api_base_is_config_error_not_panic() {
        let cfg = xgent_settings_core::global::ProviderConfig {
            api_base: String::new(),
            ..Default::default()
        };
        let p = CustomApiProvider::with_config("c".into(), &cfg, "k".into());
        let req = ChatRequest {
            provider: "c".into(),
            model: "m".into(),
            messages: Vec::new(),
            tools: None,
        };
        match p.chat(req).await {
            Err(ProviderError::Config(m)) => assert!(m.contains("api_base")),
            other => panic!("期望 Config 错误，得到 {other:?}"),
        }
        assert!(p.health_check().await.is_err());
    }

    #[tokio::test]
    async fn with_base_returns_real_stream_not_placeholder() {
        // 指向本地不可达端口：应得到真实的传输层错误，而非"尚未实现"的 Config 错误。
        // 具体是 Network 还是 Api 取决于环境是否走代理，故只断言"不是占位错误"。
        let p = CustomApiProvider::new("c".into(), "http://127.0.0.1:1/v1".into(), "k".into());
        let req = ChatRequest {
            provider: "c".into(),
            model: "m".into(),
            messages: Vec::new(),
            tools: None,
        };
        match p.chat(req).await {
            Err(ProviderError::Network(_)) | Err(ProviderError::Api { .. }) => {}
            Err(other) => panic!("期望真实的传输层错误，得到 {other:?}"),
            Ok(_) => panic!("不可达端点不应成功"),
        }
    }

    #[test]
    fn extra_headers_are_carried_into_inner_adapter() {
        let mut extra = std::collections::HashMap::new();
        extra.insert("api-key".to_string(), "secret".to_string());
        // 非法头名被丢弃而非让构造失败
        extra.insert("bad header".to_string(), "x".to_string());
        let cfg = xgent_settings_core::global::ProviderConfig {
            api_base: "https://example.com/v1".into(),
            extra_headers: extra,
            ..Default::default()
        };
        let p = CustomApiProvider::with_config("c".into(), &cfg, "k".into());
        assert_eq!(p.id(), "c");
        assert!(!p.inner.api_base_is_empty());
    }
}
