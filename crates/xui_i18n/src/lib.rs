// xui_i18n — 极小 i18n 字符串源 trait（框架无关、零依赖）。
//
// 本 crate 只放 `StringSource` trait，解决"反转依赖"的归属问题：
// - `xui`（可独立发布的 UI 库）依赖本 crate 作为 trait 使用方；
// - `xgent_settings::Localizer` 依赖本 crate 作为 trait 实现方；
// 两者不直接依赖，`xui` 仍可独立发布。

/// i18n 字符串提供者 trait。
///
/// 由宿主应用实现（如 XGent 的 `xgent_settings::Localizer`），
/// UI 库（如 `xui`）通过此 trait 取本地化字符串，无需依赖具体 i18n 实现。
///
/// 设计为框架无关：实现可基于 fluent、gettext 或任何方案。
///
/// # 线程安全
///
/// 需 `Send + Sync`，以便作为 Bevy Resource 跨系统、跨线程访问。
pub trait StringSource: Send + Sync {
    /// 取 `key` 对应的已翻译字符串。
    ///
    /// `args` 为命名参数列表 `(name, value)`，用于格式化（如复数、占位符替换）。
    /// 若 key 不存在，实现可返回 key 本身或占位文本。
    fn get(&self, key: &str, args: &[(&str, String)]) -> String;

    /// 当前语言标识，如 `"zh-CN"`、`"en-US"`。
    fn current_lang(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// mock 实现：直接拼接 key 与参数，用于验证 trait 可被实现与调用。
    struct MockSource {
        lang: &'static str,
    }

    impl StringSource for MockSource {
        fn get(&self, key: &str, args: &[(&str, String)]) -> String {
            if args.is_empty() {
                format!("[{lang}]{key}", lang = self.lang)
            } else {
                let pairs: Vec<String> = args.iter().map(|(n, v)| format!("{n}={v}")).collect();
                format!(
                    "[{lang}]{key}({args})",
                    lang = self.lang,
                    args = pairs.join(",")
                )
            }
        }

        fn current_lang(&self) -> &str {
            self.lang
        }
    }

    #[test]
    fn mock_get_without_args() {
        let s = MockSource { lang: "zh-CN" };
        assert_eq!(s.get("hello", &[]), "[zh-CN]hello");
        assert_eq!(s.current_lang(), "zh-CN");
    }

    #[test]
    fn mock_get_with_args() {
        let s = MockSource { lang: "en-US" };
        let args = [("name", "world".to_string()), ("count", "3".to_string())];
        assert_eq!(s.get("greet", &args), "[en-US]greet(name=world,count=3)");
    }

    #[test]
    fn trait_object_dyn_works() {
        // 验证可作为 trait 对象跨边界使用
        let s: Box<dyn StringSource> = Box::new(MockSource { lang: "zh-CN" });
        assert_eq!(s.get("k", &[]), "[zh-CN]k");
        assert_eq!(s.current_lang(), "zh-CN");
    }
}
