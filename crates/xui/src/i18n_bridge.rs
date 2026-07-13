//! i18n 桥接：xui 经 `xui_i18n::StringSource` trait 取本地化字符串。
//!
//! trait 定义在独立的 `xui_i18n` crate，由宿主（如 `xgent_settings::Localizer`）实现，
//! xgent_app 注入到 [`Strings`] Resource。xui 不直接依赖 fluent 或任何 xgent_*，
//! 保持可被任意 i18n 方案的 Bevy 项目复用。

use bevy::prelude::*;
use xui_i18n::StringSource;

/// 注入的字符串源（运行期由宿主提供）。
#[derive(Resource)]
pub struct Strings(pub Box<dyn StringSource>);

impl std::fmt::Debug for Strings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Strings")
            .field("lang", &self.0.current_lang())
            .finish()
    }
}

/// 取 `key` 对应的本地化字符串（无参数）。
pub fn tr(res: &Strings, key: &str) -> String {
    res.0.get(key, &[])
}

/// 取 `key` 对应的本地化字符串（带命名参数）。
pub fn tr_with(res: &Strings, key: &str, args: &[(&str, String)]) -> String {
    res.0.get(key, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mock {
        lang: &'static str,
    }
    impl StringSource for Mock {
        fn get(&self, key: &str, args: &[(&str, String)]) -> String {
            if args.is_empty() {
                format!("[{}]{}", self.lang, key)
            } else {
                let pairs: Vec<String> = args.iter().map(|(n, v)| format!("{n}={v}")).collect();
                format!("[{}]{}({})", self.lang, key, pairs.join(","))
            }
        }
        fn current_lang(&self) -> &str {
            self.lang
        }
    }

    #[test]
    fn tr_returns_mock_value() {
        let s = Strings(Box::new(Mock { lang: "zh-CN" }));
        assert_eq!(tr(&s, "hello"), "[zh-CN]hello");
        assert_eq!(
            tr_with(&s, "greet", &[("name", "world".into())]),
            "[zh-CN]greet(name=world)"
        );
    }
}
