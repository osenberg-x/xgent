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
///
/// API Key 经 [`xgent_settings_core::keychain::resolve_api_key`] 解析：OS 凭据
/// 存储优先，缺失时回退 `cfg.api_key`（TOML）。TOML 值永不被清除。
pub fn build_provider(id: &str, cfg: &ProviderConfig) -> Box<dyn LlmProvider> {
    let api_key = xgent_settings_core::keychain::resolve_api_key(id, &cfg.api_key);
    match cfg.kind {
        ProviderKind::OpenAiCompat | ProviderKind::Ollama => {
            Box::new(OpenAiCompatProvider::with_timeout(
                id.to_string(),
                cfg.api_base.clone(),
                api_key,
                cfg.timeout_secs,
            ))
        }
        ProviderKind::ResponseApi => Box::new(ResponseApiProvider::with_config(
            id.to_string(),
            cfg,
            api_key,
        )),
        ProviderKind::Anthropic => Box::new(AnthropicProvider::with_timeout(
            id.to_string(),
            cfg.api_base.clone(),
            api_key,
            cfg.timeout_secs,
        )),
        ProviderKind::Custom => {
            Box::new(CustomApiProvider::with_config(id.to_string(), cfg, api_key))
        }
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
