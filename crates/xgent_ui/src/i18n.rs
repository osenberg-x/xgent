//! i18n 集成辅助：经 [`xgent_settings::Localizer`]（实现 `StringSource`）取本地化字符串。
//!
//! `Localizer` 作为 Bevy Resource 注入；语言切换时调用 `Localizer::switch`。
//! UI 渲染时调用 [`tr`] / [`tr_with`] 取串，不硬编码字符串。

use bevy::prelude::*;
use xgent_settings::Localizer;
use xui_i18n::StringSource;

/// 取 `key` 对应的本地化字符串（无参数）。
pub fn tr(loc: &Localizer, key: &str) -> String {
    loc.get(key, &[])
}

/// 取 `key` 对应的本地化字符串（带命名参数）。
pub fn tr_with(loc: &Localizer, key: &str, args: &[(&str, String)]) -> String {
    loc.get(key, args)
}

/// 切换语言并返回是否成功切换。
pub fn switch_language(loc: &mut Localizer, lang: &str) -> bool {
    let before = loc.current_lang().to_string();
    loc.switch(lang);
    loc.current_lang() != before.as_str()
}
