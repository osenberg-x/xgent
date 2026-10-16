//! 设置面板的 i18n 合规守卫（NF-05 / AGENTS.md §5.6）。
//!
//! 判据不是"数一下有几个键"，而是**从源码里把用到的键取出来**逐个查两套语料：
//! 这样漏翻译、漏键、只在一边加了键都会当场失败。`en-US` 侧还额外断言不出现
//! 汉字——除了语言名本身（`settings-lang-zh-CN` 就该写「中文」）。

use xgent_settings::Localizer;
use xui_i18n::StringSource;

/// 面板源码（编译期读入，测试跑的是真正写进代码里的那些键）。
const PANEL_SRC: &str = include_str!("../src/settings_panel.rs");

/// 由 `format!("settings-lang-{lang}")` 动态拼出的键——扫描取不到，显式列出。
const DYNAMIC_KEYS: [&str; 2] = ["settings-lang-zh-CN", "settings-lang-en-US"];

/// 允许在英文语料里出现汉字的键（语言名按各自语言的写法显示）。
const CJK_ALLOWED_IN_EN: [&str; 1] = ["settings-lang-zh-CN"];

/// 从源码里收集所有 `"settings-xxx"` 字面量键。
fn keys_in_source() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes: Vec<char> = PANEL_SRC.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != '"' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut key = String::new();
        while j < bytes.len() && bytes[j] != '"' {
            key.push(bytes[j]);
            j += 1;
        }
        if key.starts_with("settings-")
            // 只要纯键名：带 `{`/`}` 的是 format 串，带空格的是显示文案
            && !key.contains('{')
            && !key.contains(' ')
            && !out.contains(&key)
        {
            out.push(key);
        }
        i = j + 1;
    }
    for k in DYNAMIC_KEYS {
        if !out.contains(&k.to_string()) {
            out.push(k.to_string());
        }
    }
    out.sort();
    out
}

#[test]
fn every_panel_key_exists_in_both_locales() {
    let zh = Localizer::load("zh-CN");
    let en = Localizer::load("en-US");
    let keys = keys_in_source();
    assert!(
        keys.len() > 30,
        "面板应有一批配置键，实际只扫到 {} 个——扫描逻辑失效了",
        keys.len()
    );
    for k in &keys {
        // Localizer 缺键时回退成键本身，据此判"这套语料没这个键"
        assert_ne!(zh.get(k, &[]).as_str(), k.as_str(), "zh-CN 缺失键: {k}");
        assert_ne!(en.get(k, &[]).as_str(), k.as_str(), "en-US 缺失键: {k}");
    }
}

#[test]
fn english_panel_text_has_no_cjk_except_language_names() {
    let en = Localizer::load("en-US");
    for k in keys_in_source() {
        if CJK_ALLOWED_IN_EN.contains(&k.as_str()) {
            continue;
        }
        let v = visible(&en.get(&k, &[]));
        assert!(
            !v.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
            "en-US 的 {k} 混着汉字: {v}"
        );
    }
}

/// Fluent 把每个参数包进双向隔离标记（U+2066..U+2069）。它们在界面上不可见，
/// 但会让 `"a：b"` 变成 `"\u{2068}a\u{2069}：\u{2068}b\u{2069}"`——按可见文本比对才判得准。
fn visible(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{2066}'..='\u{2069}'))
        .collect()
}

#[test]
fn punctuation_belongs_to_the_corpus_not_to_rust() {
    // 报告串的分隔符与「：」都必须来自语料：英文渲染里出现中文标点是缺陷
    let zh_sep = Localizer::load("zh-CN").get("settings-report-sep", &[]);
    let en_sep = Localizer::load("en-US").get("settings-report-sep", &[]);
    assert_eq!(visible(&zh_sep), "；");
    assert_eq!(visible(&en_sep), "; ");
    let zh_item = Localizer::load("zh-CN").get(
        "settings-failed-item",
        &[("key", "a".into()), ("reason", "b".into())],
    );
    let en_item = Localizer::load("en-US").get(
        "settings-failed-item",
        &[("key", "a".into()), ("reason", "b".into())],
    );
    assert_eq!(visible(&zh_item), "a：b");
    assert_eq!(visible(&en_item), "a: b");
    // 源码里不该再有硬编码的「：」拼接这些报告
    assert!(!PANEL_SRC.contains("\"{}：{}\""), "报告拼接应走语料键");
}
