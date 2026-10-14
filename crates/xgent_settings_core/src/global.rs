//! 全局配置类型。
//!
//! 全局配置存于平台规范路径的 `config.toml`（见 [`crate::paths`]），
//! 包含 provider 列表、默认模型与应用偏好。权威副本在 daemon（step6）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 全局配置根结构。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct GlobalConfig {
    /// 已配置的 provider 集合（key 为 provider id，如 `"openai"`）
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,

    /// 默认 provider id
    #[serde(default)]
    pub default_provider: String,

    /// 默认模型名
    #[serde(default)]
    pub default_model: String,

    /// 应用偏好
    #[serde(default)]
    pub preferences: Preferences,

    /// 插件级配置：plugin_id → 配置子表（来自 `[plugin.<id>]` 段）。
    ///
    /// 照设计文档 §10.4。`host.get-config("<id>.<field>")` 从此取值。
    #[serde(default)]
    pub plugin_settings: std::collections::BTreeMap<String, toml::Value>,

    /// 插件全局配置（启用/禁用、自动更新）。照设计文档 §10.2。
    #[serde(default)]
    pub plugin: PluginConfig,
}

/// 插件配置（全局配置的一部分）。照设计文档 §10.2。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginConfig {
    /// 已安装插件 id → 是否启用
    #[serde(default)]
    pub enabled: std::collections::BTreeMap<String, bool>,
    /// 是否自动更新插件
    #[serde(default)]
    pub auto_update: bool,
    /// 自动安装的内建插件列表
    #[serde(default)]
    pub auto_install: Vec<String>,
}

/// 单个 provider 的配置。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProviderConfig {
    /// provider 类型
    pub kind: ProviderKind,
    /// API 基础 URL（如 `"https://api.openai.com/v1"`）
    #[serde(default)]
    pub api_base: String,
    /// API Key（MVP 明文存 TOML，未来考虑 keychain，见 D-02）
    #[serde(default)]
    pub api_key: String,
    /// 额外请求头（`CustomApiProvider` 用：`key` → `value`）。
    ///
    /// 部分第三方接口需要自定义鉴权头（如 `api-key`、`x-api-token`），
    /// 标准 `Authorization: Bearer` 覆盖不到。
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
    /// 模型覆盖（通用名 → 实际模型 id）
    #[serde(default)]
    pub model_overrides: HashMap<String, String>,
    /// 请求超时秒数
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// 最大重试次数。
    ///
    /// `None` 表示无限重试（直到成功或被中断）；`Some(n)` 表示最多重试 n 次。
    /// 仅对可重试错误（`Network`/`StreamParse`）生效，见 agent 侧重试逻辑。
    #[serde(default = "default_max_retries")]
    pub max_retries: Option<u32>,
    /// 重试模式（固定间隔 / 指数退避）
    #[serde(default = "default_retry_mode")]
    pub retry_mode: RetryMode,
    /// 重试初始间隔毫秒。
    ///
    /// 固定模式：每次重试前等待该值；指数模式：作为退避基准值。
    #[serde(default = "default_retry_initial_delay_ms")]
    pub retry_initial_delay_ms: u64,
    /// 重试最大间隔毫秒（指数退避上限，固定模式忽略）。
    #[serde(default = "default_retry_max_delay_ms")]
    pub retry_max_delay_ms: u64,
    /// 指数退避乘数（固定模式忽略）。
    ///
    /// 每次重试间隔 = min(initial * backoff_factor^(n-1), max_delay)。
    #[serde(default = "default_retry_backoff_factor")]
    pub retry_backoff_factor: f64,
    /// 上下文窗口 token 数，compaction 触发依据。
    ///
    /// `0` 表示未设置（用 [`defaults::CONTEXT_WINDOW`]）。此前该值硬编码在
    /// `xgent_app`，小窗口模型永不触发压缩、大窗口模型被过早压缩（R3-17）。
    #[serde(default)]
    pub context_window: u32,
    /// 按模型 id 覆盖上下文窗口（模型 id → token 数）。
    ///
    /// 一个 provider 端点常挂多个窗口不同的模型。取值次序见
    /// [`ProviderConfig::resolve_context_window`]。
    #[serde(default)]
    pub model_context_windows: HashMap<String, u32>,
    /// 显式输出上限 token 数。
    ///
    /// 仅当前真正发送输出上限的适配器（Anthropic Messages 的 `max_tokens`）
    /// 读取它；`0` 表示未设置（用 [`defaults::MAX_OUTPUT_TOKENS`]）。
    /// OpenAI 兼容与 Response API 不发送该字段，本设置对它们无效。
    #[serde(default)]
    pub max_output_tokens: u32,
    /// 等待首个 SSE 事件的秒数（`0` = 用 [`defaults::FIRST_EVENT_TIMEOUT_SECS`]）。
    #[serde(default)]
    pub first_event_timeout_secs: u64,
}

/// 内置缺省值——即本次转配置前各 crate 里的硬编码常量。
///
/// 配置值为 `0` 时回落到这些数，使「不写配置」与「改变前的行为」完全等价。
pub mod defaults {
    /// 上下文窗口（token）。
    pub const CONTEXT_WINDOW: u32 = 128_000;
    /// 单次响应输出上限（token）。
    pub const MAX_OUTPUT_TOKENS: u32 = 8_192;
    /// 等待首个 SSE 事件（秒）。
    pub const FIRST_EVENT_TIMEOUT_SECS: u64 = 30;
}

/// `0` 哨兵下的实际取值：未设置即回落内置缺省。
fn or_default_u32(value: u32, fallback: u32) -> u32 {
    if value == 0 { fallback } else { value }
}

impl ProviderConfig {
    /// 解析给定模型的上下文窗口。
    ///
    /// 次序：`model_context_windows[model]`（非 0）> `context_window`（非 0）
    /// > [`defaults::CONTEXT_WINDOW`]。返回窗口与「是否用了内置回落」，
    /// 调用方据此在启动时打一条告警。
    pub fn resolve_context_window(&self, model: &str) -> (u32, bool) {
        if let Some(v) = self.model_context_windows.get(model).copied()
            && v != 0
        {
            return (v, false);
        }
        if self.context_window != 0 {
            return (self.context_window, false);
        }
        (defaults::CONTEXT_WINDOW, true)
    }

    /// 生效的输出上限 token 数。
    pub fn effective_max_output_tokens(&self) -> u32 {
        or_default_u32(self.max_output_tokens, defaults::MAX_OUTPUT_TOKENS)
    }

    /// 生效的首事件等待秒数。
    pub fn effective_first_event_timeout_secs(&self) -> u64 {
        if self.first_event_timeout_secs == 0 {
            defaults::FIRST_EVENT_TIMEOUT_SECS
        } else {
            self.first_event_timeout_secs
        }
    }
}

impl GlobalConfig {
    /// 解析指定 provider + 模型的上下文窗口。
    ///
    /// 次序：该模型的 `model_context_windows` 条目 > provider 级
    /// `context_window` > [`defaults::CONTEXT_WINDOW`]。返回
    /// `(窗口, 是否走了内置回落)`，调用方据此打启动告警（R3-17）。
    ///
    /// provider 未出现在配置里（命令行 `--provider` 指了个没配过的 id、
    /// 或全局配置为空）同样报回落，而不是静默当作已配置。
    pub fn resolve_context_window(&self, provider_id: &str, model: &str) -> (u32, bool) {
        match self.providers.get(provider_id) {
            Some(cfg) => cfg.resolve_context_window(model),
            None => (defaults::CONTEXT_WINDOW, true),
        }
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            kind: ProviderKind::OpenAiCompat,
            api_base: String::new(),
            api_key: String::new(),
            extra_headers: HashMap::new(),
            model_overrides: HashMap::new(),
            timeout_secs: default_timeout_secs(),
            max_retries: default_max_retries(),
            retry_mode: default_retry_mode(),
            retry_initial_delay_ms: default_retry_initial_delay_ms(),
            retry_max_delay_ms: default_retry_max_delay_ms(),
            retry_backoff_factor: default_retry_backoff_factor(),
            context_window: 0,
            model_context_windows: HashMap::new(),
            max_output_tokens: 0,
            first_event_timeout_secs: 0,
        }
    }
}

fn default_timeout_secs() -> u64 {
    60
}

fn default_max_retries() -> Option<u32> {
    Some(2)
}

/// 重试模式。
///
/// - `Fixed`：每次重试前等待固定间隔（`retry_initial_delay_ms`）。
/// - `Exponential`：指数退避，间隔 = min(initial * factor^(n-1), max_delay)。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetryMode {
    /// 固定间隔
    #[default]
    Fixed,
    /// 指数退避
    Exponential,
}

fn default_retry_mode() -> RetryMode {
    RetryMode::Fixed
}

fn default_retry_initial_delay_ms() -> u64 {
    500
}

fn default_retry_max_delay_ms() -> u64 {
    30_000
}

fn default_retry_backoff_factor() -> f64 {
    2.0
}

/// provider 类型。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// OpenAI 兼容接口
    #[default]
    OpenAiCompat,
    /// Response API 风格
    ResponseApi,
    /// Anthropic 原生
    Anthropic,
    /// 本地 Ollama（兼容模式）
    Ollama,
    /// 用户自定义
    Custom,
}

/// 应用偏好。
///
/// 手写 `Default` 以提供合理的初始语言/主题。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Preferences {
    /// 界面语言，如 `"zh-CN"` / `"en-US"`
    #[serde(default = "default_language")]
    pub language: String,
    /// 主题（MVP 仅 `"dark"`）
    #[serde(default = "default_theme")]
    pub theme: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: default_language(),
            theme: default_theme(),
        }
    }
}

fn default_language() -> String {
    "zh-CN".to_string()
}

fn default_theme() -> String {
    "dark".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_config_default_empty() {
        let cfg = GlobalConfig::default();
        assert!(cfg.providers.is_empty());
        assert!(cfg.default_provider.is_empty());
        assert_eq!(cfg.preferences.language, "zh-CN");
        assert_eq!(cfg.preferences.theme, "dark");
    }

    #[test]
    fn provider_config_default_values() {
        let p = ProviderConfig::default();
        assert_eq!(p.kind, ProviderKind::OpenAiCompat);
        assert_eq!(p.timeout_secs, 60);
        assert_eq!(p.max_retries, Some(2));
        assert_eq!(p.retry_mode, RetryMode::Fixed);
        assert_eq!(p.retry_initial_delay_ms, 500);
        assert_eq!(p.retry_max_delay_ms, 30_000);
        assert_eq!(p.retry_backoff_factor, 2.0);
    }

    #[test]
    fn provider_config_retries_serde() {
        // 省略字段 → default Some(2)
        let j = r#"{"kind":"open_ai_compat"}"#;
        let p: ProviderConfig = serde_json::from_str(j).unwrap();
        assert_eq!(p.max_retries, Some(2));
        // 显式 null → None（无限重试）
        let j = r#"{"kind":"open_ai_compat","max_retries":null}"#;
        let p: ProviderConfig = serde_json::from_str(j).unwrap();
        assert_eq!(p.max_retries, None);
        // 数字 → Some(n)
        let j = r#"{"kind":"open_ai_compat","max_retries":5}"#;
        let p: ProviderConfig = serde_json::from_str(j).unwrap();
        assert_eq!(p.max_retries, Some(5));
        // None 往返保持 None
        let p = ProviderConfig {
            max_retries: None,
            ..Default::default()
        };
        let s = serde_json::to_string(&p).unwrap();
        let p2: ProviderConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(p2.max_retries, None);
    }

    #[test]
    fn retry_mode_serde_snake_case() {
        let j = serde_json::to_string(&RetryMode::Exponential).unwrap();
        assert_eq!(j, r#""exponential""#);
        let m: RetryMode = serde_json::from_str(r#""fixed""#).unwrap();
        assert_eq!(m, RetryMode::Fixed);
    }

    #[test]
    fn provider_kind_serde_snake_case() {
        let j = serde_json::to_string(&ProviderKind::Anthropic).unwrap();
        assert_eq!(j, r#""anthropic""#);
        let k: ProviderKind = serde_json::from_str(r#""ollama""#).unwrap();
        assert_eq!(k, ProviderKind::Ollama);
    }

    #[test]
    fn global_config_roundtrip() {
        let cfg = GlobalConfig {
            default_provider: "openai".to_string(),
            default_model: "gpt-4".to_string(),
            providers: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "openai".to_string(),
                    ProviderConfig {
                        kind: ProviderKind::OpenAiCompat,
                        api_base: "https://api.openai.com/v1".into(),
                        api_key: "sk-xxx".into(),
                        ..Default::default()
                    },
                );
                m
            },
            preferences: Preferences::default(),
            ..Default::default()
        };
        let j = serde_json::to_string(&cfg).unwrap();
        let cfg2: GlobalConfig = serde_json::from_str(&j).unwrap();
        assert_eq!(cfg2.default_provider, "openai");
        assert_eq!(cfg2.providers.len(), 1);
        let p = cfg2.providers.get("openai").unwrap();
        assert_eq!(p.kind, ProviderKind::OpenAiCompat);
        assert_eq!(p.api_key, "sk-xxx");
        assert_eq!(p.timeout_secs, 60);
    }

    /// R3-17 不变量：新键缺省即「未设置」，生效值等于本次转配置前的硬编码常量。
    #[test]
    fn new_provider_keys_default_to_previous_constants() {
        let p = ProviderConfig::default();
        assert_eq!(p.context_window, 0);
        assert!(p.model_context_windows.is_empty());
        assert_eq!(p.max_output_tokens, 0);
        assert_eq!(p.first_event_timeout_secs, 0);
        assert_eq!(p.resolve_context_window("gpt-4").0, 128_000);
        assert!(
            p.resolve_context_window("gpt-4").1,
            "未配置时应标记用了内置回落"
        );
        assert_eq!(p.effective_max_output_tokens(), 8_192);
        assert_eq!(p.effective_first_event_timeout_secs(), 30);
    }

    /// 同一判据从 TOML 文本出发：老配置文件的键不受影响，新键取内置缺省。
    #[test]
    fn toml_without_new_keys_resolves_to_builtin_defaults() {
        let toml_src = r#"
[providers.openai]
kind = "open_ai_compat"
api_base = "https://api.openai.com/v1"
api_key = "sk-x"
"#;
        let cfg: GlobalConfig = toml::from_str(toml_src).unwrap();
        let p = cfg.providers.get("openai").unwrap();
        assert_eq!(p.timeout_secs, 60);
        assert_eq!(p.context_window, 0);
        assert_eq!(p.resolve_context_window("gpt-4o"), (128_000, true));
        assert_eq!(p.effective_max_output_tokens(), 8_192);
        assert_eq!(p.effective_first_event_timeout_secs(), 30);
    }

    #[test]
    fn context_window_precedence_is_model_then_provider_then_fallback() {
        let mut p = ProviderConfig {
            context_window: 32_768,
            ..Default::default()
        };
        // provider 级生效，非回落
        assert_eq!(p.resolve_context_window("gpt-4o"), (32_768, false));
        // 模型级覆盖 provider 级
        p.model_context_windows.insert("gpt-4o".into(), 128_000);
        assert_eq!(p.resolve_context_window("gpt-4o"), (128_000, false));
        // 未列出的模型仍用 provider 级
        assert_eq!(p.resolve_context_window("claude"), (32_768, false));
        // 模型级写 0 视为未设置，不成为回落
        p.model_context_windows.insert("zero".into(), 0);
        assert_eq!(p.resolve_context_window("zero"), (32_768, false));
        // 两处都不设 → 内置回落并标记
        let bare = ProviderConfig::default();
        assert_eq!(bare.resolve_context_window("any"), (128_000, true));
    }

    #[test]
    fn explicit_zero_means_unset_for_provider_limits() {
        let p: ProviderConfig = serde_json::from_str(
            r#"{"kind":"anthropic","max_output_tokens":0,"first_event_timeout_secs":0}"#,
        )
        .unwrap();
        assert_eq!(p.effective_max_output_tokens(), 8_192);
        assert_eq!(p.effective_first_event_timeout_secs(), 30);
        // 非零值直接生效
        let p: ProviderConfig = serde_json::from_str(
            r#"{"kind":"anthropic","max_output_tokens":4096,"first_event_timeout_secs":1}"#,
        )
        .unwrap();
        assert_eq!(p.effective_max_output_tokens(), 4_096);
        assert_eq!(p.effective_first_event_timeout_secs(), 1);
    }

    #[test]
    fn provider_new_keys_roundtrip_and_keep_zero() {
        let p = ProviderConfig {
            kind: ProviderKind::Anthropic,
            ..Default::default()
        };
        let s = serde_json::to_string(&p).unwrap();
        let p2: ProviderConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(p2.context_window, 0);
        assert_eq!(p2.max_output_tokens, 0);
        assert_eq!(p2.first_event_timeout_secs, 0);
        assert!(p2.model_context_windows.is_empty());
        // 显式 map 往返
        let mut m = HashMap::new();
        m.insert("gpt-4o".to_string(), 128_000u32);
        let p = ProviderConfig {
            model_context_windows: m,
            ..Default::default()
        };
        let s = serde_json::to_string(&p).unwrap();
        let p2: ProviderConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(p2.resolve_context_window("gpt-4o").0, 128_000);
    }

    /// A6：全局层的窗口解析覆盖「provider 根本没配」这一路。
    #[test]
    fn global_resolve_context_window_handles_missing_provider() {
        let g = GlobalConfig::default();
        assert_eq!(
            g.resolve_context_window("openai", "gpt-4o"),
            (128_000, true),
            "未配置的 provider 应报内置回落，而不是静默当作已配置"
        );

        let mut providers = std::collections::HashMap::new();
        providers.insert(
            "openai".to_string(),
            ProviderConfig {
                context_window: 32_768,
                ..Default::default()
            },
        );
        let g = GlobalConfig {
            providers,
            ..Default::default()
        };
        assert_eq!(
            g.resolve_context_window("openai", "gpt-4o"),
            (32_768, false)
        );
        // 另一个没配过的 id 仍走回落，不受已配置项影响
        assert_eq!(g.resolve_context_window("other", "m"), (128_000, true));
    }
}
