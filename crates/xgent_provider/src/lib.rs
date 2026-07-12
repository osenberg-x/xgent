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
/// 据 [`ProviderKind`] 选择适配器；Ollama 兼容模式复用 `OpenAiCompatProvider`。
pub fn build_provider(cfg: &ProviderConfig) -> Box<dyn LlmProvider> {
    match cfg.kind {
        ProviderKind::OpenAiCompat | ProviderKind::Ollama => {
            Box::new(OpenAiCompatProvider::new(
                // id 由调用方在更高层注入；此处用 api_base 派生占位
                derive_id(&cfg.api_base),
                cfg.api_base.clone(),
                cfg.api_key.clone(),
            ))
        }
        ProviderKind::ResponseApi => Box::new(ResponseApiProvider::new(derive_id(&cfg.api_base))),
        ProviderKind::Anthropic => Box::new(AnthropicProvider::new(derive_id(&cfg.api_base))),
        ProviderKind::Custom => Box::new(CustomApiProvider::new(derive_id(&cfg.api_base))),
    }
}

/// 从 api_base 简单派生 provider id（取 host 末段，仅占位）。
///
/// 真实 id 应由调用方（daemon 配置层）按 providers map 的 key 注入。
fn derive_id(api_base: &str) -> String {
    api_base
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("provider")
        .to_string()
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
        let p = build_provider(&cfg);
        assert_eq!(p.id(), "v1");
        // 健康检查会因无网络失败，但类型应正确
        let id = p.id().to_string();
        assert!(!id.is_empty());
    }

    #[test]
    fn build_ollama_reuses_openai_compat() {
        let cfg = ProviderConfig {
            kind: ProviderKind::Ollama,
            api_base: "http://localhost:11434/v1".into(),
            api_key: String::new(),
            ..Default::default()
        };
        let p = build_provider(&cfg);
        assert_eq!(p.id(), "v1");
    }

    #[test]
    fn build_response_api_provider() {
        let cfg = ProviderConfig {
            kind: ProviderKind::ResponseApi,
            api_base: "https://api.openai.com/v1".into(),
            ..Default::default()
        };
        let p = build_provider(&cfg);
        assert_eq!(p.id(), "v1");
    }

    #[test]
    fn build_anthropic_provider() {
        let cfg = ProviderConfig {
            kind: ProviderKind::Anthropic,
            api_base: "https://api.anthropic.com".into(),
            ..Default::default()
        };
        let p = build_provider(&cfg);
        // 无路径段时 rsplit 返回整个 host
        assert_eq!(p.id(), "api.anthropic.com");
    }

    #[test]
    fn build_custom_provider() {
        let cfg = ProviderConfig {
            kind: ProviderKind::Custom,
            api_base: "https://custom.example.com/api".into(),
            ..Default::default()
        };
        let p = build_provider(&cfg);
        assert_eq!(p.id(), "api");
    }

    #[test]
    fn derive_id_handles_edge_cases() {
        assert_eq!(derive_id("https://api.openai.com/v1"), "v1");
        assert_eq!(derive_id("https://api.openai.com/v1/"), "v1");
        assert_eq!(derive_id(""), "provider");
        assert_eq!(derive_id("https://x"), "x");
    }
}
