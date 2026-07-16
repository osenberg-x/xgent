//! xgent_provider — LLM Provider 抽象层与适配器。
//!
//! 提供 [`LlmProvider`] trait 与具体适配器（OpenAI compatible 等）。
//! 不依赖 Bevy——纯异步逻辑，daemon 侧持有实例池；UI 侧经 IPC 调用。

pub mod anthropic;
pub mod custom;
pub mod openai_compat;
pub mod provider;
pub mod response_api;
pub mod sse;

pub use anthropic::AnthropicProvider;
pub use custom::CustomApiProvider;
pub use openai_compat::OpenAiCompatProvider;
pub use provider::{ChatStream, LlmProvider, ModelInfo, ProviderError};
pub use response_api::ResponseApiProvider;

use xgent_settings_core::{ProviderConfig, ProviderKind};

/// 从配置构造 provider 实例。
///
/// `id` 为 providers map 的 key（如 `"openai"`、`"deepseek"`），作为 provider 标识。
/// 据 [`ProviderKind`] 选择适配器；Ollama 兼容模式复用 `OpenAiCompatProvider`。
pub fn build_provider(id: &str, cfg: &ProviderConfig) -> Box<dyn LlmProvider> {
    match cfg.kind {
        ProviderKind::OpenAiCompat | ProviderKind::Ollama => {
            Box::new(OpenAiCompatProvider::new(
                id.to_string(),
                cfg.api_base.clone(),
                cfg.api_key.clone(),
            ))
        }
        ProviderKind::ResponseApi => Box::new(ResponseApiProvider::new(id.to_string())),
        ProviderKind::Anthropic => Box::new(AnthropicProvider::new(id.to_string())),
        ProviderKind::Custom => Box::new(CustomApiProvider::new(id.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_openai_compat_provider() {
        let cfg = ProviderConfig {
            kind: ProviderKind::OpenAiCompat,
            api_base: "https://api.openai.com/v1".into(),
            api_key: "sk-x".into(),
            ..Default::default()
        };
        let p = build_provider("openai", &cfg);
        assert_eq!(p.id(), "openai");
    }

    #[test]
    fn build_ollama_reuses_openai_compat() {
        let cfg = ProviderConfig {
            kind: ProviderKind::Ollama,
            api_base: "http://localhost:11434/v1".into(),
            api_key: String::new(),
            ..Default::default()
        };
        let p = build_provider("ollama", &cfg);
        assert_eq!(p.id(), "ollama");
    }

    #[test]
    fn build_response_api_provider() {
        let cfg = ProviderConfig {
            kind: ProviderKind::ResponseApi,
            api_base: "https://api.openai.com/v1".into(),
            ..Default::default()
        };
        let p = build_provider("openai-resp", &cfg);
        assert_eq!(p.id(), "openai-resp");
    }

    #[test]
    fn build_anthropic_provider() {
        let cfg = ProviderConfig {
            kind: ProviderKind::Anthropic,
            api_base: "https://api.anthropic.com".into(),
            ..Default::default()
        };
        let p = build_provider("anthropic", &cfg);
        assert_eq!(p.id(), "anthropic");
    }

    #[test]
    fn build_custom_provider() {
        let cfg = ProviderConfig {
            kind: ProviderKind::Custom,
            api_base: "https://custom.example.com/api".into(),
            ..Default::default()
        };
        let p = build_provider("custom", &cfg);
        assert_eq!(p.id(), "custom");
    }
}
