//! fluent Localizer，实现 [`xui_i18n::StringSource`]。
//!
//! 加载内嵌的 `.ftl` 资源，支持运行时切换语言。作为 Bevy Resource 注入，
//! 经 `StringSource` trait 供 `xui` 调用（反转依赖）。
//!
//! 使用 fluent 的 `concurrent::FluentBundle`（基于 `Mutex` 的国际化 memoizer），
//! 使 `Localizer` 满足 `Send + Sync`，符合 Bevy Resource 的线程安全要求。

use bevy::prelude::*;
use fluent::concurrent::FluentBundle;
use fluent::{FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;
use xui_i18n::StringSource;

/// 内嵌的本地化资源（语言标识 → FTL 源）。
///
/// 前期以中文为主，架构保证可翻译、可运行时切换。
const ZH_CN_FTL: &str = include_str!("../locales/zh-CN/main.ftl");
const EN_US_FTL: &str = include_str!("../locales/en-US/main.ftl");

/// 默认语言（无配置时）。
pub const DEFAULT_LANG: &str = "zh-CN";

/// 已注册的（语言标识, FTL 源）列表。
const RESOURCES: &[(&str, &str)] = &[("zh-CN", ZH_CN_FTL), ("en-US", EN_US_FTL)];

/// fluent 本地化器。
///
/// 持有当前语言的 `concurrent::FluentBundle`（`Send + Sync`），
/// 实现 `StringSource` 供 UI 库取本地化字符串。
#[derive(Resource)]
pub struct Localizer {
    /// 当前语言的 bundle
    bundle: FluentBundle<FluentResource>,
    /// 当前语言标识，如 "zh-CN"
    lang: String,
}

impl Localizer {
    /// 加载指定语言的 Localizer。
    ///
    /// `lang` 不在已注册资源中时回退到 [`DEFAULT_LANG`]。
    pub fn load(lang: &str) -> Self {
        let (resolved, source) = resolve(lang);
        let bundle = build_bundle(resolved, source);
        Self {
            bundle,
            lang: resolved.to_string(),
        }
    }

    /// 切换语言并重新加载 bundle。
    ///
    /// `lang` 不在已注册资源中时保持不变。
    pub fn switch(&mut self, lang: &str) {
        let Some((resolved, source)) = resolve_option(lang) else {
            return;
        };
        if resolved == self.lang {
            return;
        }
        self.bundle = build_bundle(resolved, source);
        self.lang = resolved.to_string();
    }

    /// 当前语言标识。
    pub fn current_lang(&self) -> &str {
        &self.lang
    }
}

impl Default for Localizer {
    fn default() -> Self {
        Self::load(DEFAULT_LANG)
    }
}

/// 从已注册资源中解析语言与对应源。
///
/// 优先精确匹配 `lang`；找不到则回退到 [`DEFAULT_LANG`]；仍找不到则取第一个。
fn resolve(lang: &str) -> (&'static str, &'static str) {
    resolve_option(lang)
        .or_else(|| resolve_option(DEFAULT_LANG))
        .unwrap_or(RESOURCES[0])
}

fn resolve_option(lang: &str) -> Option<(&'static str, &'static str)> {
    RESOURCES.iter().find(|(l, _)| *l == lang).copied()
}

/// 构建指定语言的 bundle。
fn build_bundle(lang: &str, source: &str) -> FluentBundle<FluentResource> {
    let langid: LanguageIdentifier = lang.parse().unwrap_or_else(|_| {
        DEFAULT_LANG
            .parse()
            .expect("DEFAULT_LANG must be a valid language identifier")
    });
    let mut bundle = FluentBundle::new_concurrent(vec![langid]);
    let res = FluentResource::try_new(source.to_string()).expect("内嵌 .ftl 资源必须可解析");
    bundle.add_resource(res).unwrap_or_else(|errs| {
        // .ftl 内嵌资源是编译期固定的，解析失败属程序错误
        panic!("无法加载 fluent 资源 {lang}: {errs:?}");
    });
    bundle
}

impl StringSource for Localizer {
    fn get(&self, key: &str, args: &[(&str, String)]) -> String {
        let Some(msg) = self.bundle.get_message(key) else {
            // key 不存在时返回 key 本身，便于排查缺失翻译
            return key.to_string();
        };
        let Some(pattern) = msg.value() else {
            return key.to_string();
        };
        let mut fluent_args = FluentArgs::new();
        for (k, v) in args {
            fluent_args.set(*k, FluentValue::from(v.as_str()));
        }
        let mut errors = vec![];
        let value = self.bundle.format_pattern(
            pattern,
            if args.is_empty() {
                None
            } else {
                Some(&fluent_args)
            },
            &mut errors,
        );
        value.into_owned()
    }

    fn current_lang(&self) -> &str {
        &self.lang
    }
}

// 显式断言 Localizer 满足 StringSource 的 Send + Sync 约束。
#[allow(dead_code)]
const _: () = {
    fn assert_send_sync<T: Send + Sync>() {}
    fn _assert() {
        assert_send_sync::<Localizer>();
    }
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_default_is_zh_cn() {
        let loc = Localizer::default();
        assert_eq!(loc.current_lang(), "zh-CN");
    }

    #[test]
    fn get_welcome_zh_cn() {
        let loc = Localizer::load("zh-CN");
        assert_eq!(loc.get("welcome", &[]), "欢迎");
    }

    #[test]
    fn get_welcome_en_us() {
        let loc = Localizer::load("en-US");
        assert_eq!(loc.get("welcome", &[]), "Welcome");
    }

    #[test]
    fn get_with_args() {
        let loc = Localizer::load("zh-CN");
        let args = [("path", "/tmp/x.rs".to_string())];
        let s = loc.get("confirm-write-file", &args);
        assert!(s.contains("/tmp/x.rs"), "got: {s}");
    }

    #[test]
    fn get_unknown_key_returns_key() {
        let loc = Localizer::load("zh-CN");
        assert_eq!(loc.get("nonexistent-key", &[]), "nonexistent-key");
    }

    #[test]
    fn switch_changes_language() {
        let mut loc = Localizer::load("zh-CN");
        assert_eq!(loc.get("welcome", &[]), "欢迎");
        loc.switch("en-US");
        assert_eq!(loc.current_lang(), "en-US");
        assert_eq!(loc.get("welcome", &[]), "Welcome");
    }

    #[test]
    fn switch_unknown_lang_noop() {
        let mut loc = Localizer::load("zh-CN");
        loc.switch("fr-FR");
        // 找不到则保持原语言
        assert_eq!(loc.current_lang(), "zh-CN");
        assert_eq!(loc.get("welcome", &[]), "欢迎");
    }

    #[test]
    fn switch_to_same_lang_noop() {
        let mut loc = Localizer::load("zh-CN");
        loc.switch("zh-CN");
        assert_eq!(loc.current_lang(), "zh-CN");
    }

    #[test]
    fn dyn_string_source_works() {
        let loc = Localizer::load("en-US");
        let s: &dyn StringSource = &loc;
        assert_eq!(s.get("welcome", &[]), "Welcome");
        assert_eq!(s.current_lang(), "en-US");
    }

    #[test]
    fn all_keys_present_in_both_langs() {
        // 确保两语言资源都包含所有 key
        let zh = Localizer::load("zh-CN");
        let en = Localizer::load("en-US");
        let keys = [
            "app-title",
            "welcome",
            "chat-placeholder",
            "confirm-write-file",
            "confirm-run-command",
            "provider-not-configured",
            "settings-saved",
        ];
        for k in keys {
            let z = zh.get(k, &[]);
            let e = en.get(k, &[]);
            // 都不应回退到 key 本身
            assert_ne!(z, k, "zh-CN 缺失 key: {k}");
            assert_ne!(e, k, "en-US 缺失 key: {k}");
        }
    }
}
