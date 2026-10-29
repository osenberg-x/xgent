//! 全量 i18n 合规守卫（v8 M7-T3，NF-05 / AGENTS.md §5.6）。
//!
//! 与 `settings_panel_i18n.rs` 的区别：那个只扫设置面板源码里用到的键；
//! 本文件直接读两套 `.ftl` 语料，逐键检查：
//! 1. en-US 值不含 CJK 码位（除显式白名单）
//! 2. zh-CN 值不含 ASCII-only 的"假英文"（防止漏翻译）
//! 3. 两套语料的键集完全一致（防止一边加了键另一边忘加）

use std::collections::HashSet;

const ZH_CN_FTL: &str = include_str!("../../xgent_settings/locales/zh-CN/main.ftl");
const EN_US_FTL: &str = include_str!("../../xgent_settings/locales/en-US/main.ftl");

/// 允许在 en-US 语料里出现汉字的键（语言名按各自语言写法显示）。
const CJK_ALLOWED_IN_EN: &[&str] = &["settings-lang-zh-CN"];

/// 允许在 zh-CN 语料里出现纯 ASCII 的键（技术标识、代码片段、品牌名等）。
const ASCII_ALLOWED_IN_ZH: &[&str] = &[
    "settings-lang-en-US",
    "status-encoding",
    "settings-provider-id",
    "settings-api-base",
    "settings-api-key",
    "settings-model",
    "role-assistant",
    "settings-kind-response-api",
    "settings-kind-anthropic",
    "settings-kind-ollama",
    "terminal-shell-powershell",
    "terminal-shell-shell",
];

/// 从 `.ftl` 文件解析出所有键名。
fn parse_keys(ftl: &str) -> HashSet<String> {
    ftl.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            line.split_once('=').map(|(k, _)| k.trim().to_string())
        })
        .collect()
}

/// 从 `.ftl` 文件解析出键→值映射。
fn parse_map(ftl: &str) -> Vec<(String, String)> {
    ftl.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            line.split_once('=')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// 检查字符串是否包含 CJK 统一表意文字（基本区 U+4E00..U+9FFF）。
fn has_cjk(s: &str) -> bool {
    s.chars()
        .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3400}'..='\u{4dbf}').contains(&c))
}

/// 检查字符串是否纯 ASCII（不含任何非 ASCII 字符）。
fn is_pure_ascii(s: &str) -> bool {
    s.is_ascii()
}

#[test]
fn en_us_corpus_has_no_cjk_except_whitelist() {
    let entries = parse_map(EN_US_FTL);
    let mut violations = Vec::new();
    for (k, v) in &entries {
        if CJK_ALLOWED_IN_EN.contains(&k.as_str()) {
            continue;
        }
        // 去掉 Fluent 参数占位符再检查
        let cleaned: String = v.chars().filter(|c| !matches!(*c, '{' | '}')).collect();
        if has_cjk(&cleaned) {
            violations.push(format!("{k} = {v}"));
        }
    }
    assert!(
        violations.is_empty(),
        "en-US 语料中以下键混入汉字（应使用英文或白名单）:\n{}",
        violations.join("\n")
    );
}

#[test]
fn zh_cn_corpus_has_no_pure_ascii_except_whitelist() {
    let entries = parse_map(ZH_CN_FTL);
    let mut violations = Vec::new();
    for (k, v) in &entries {
        if ASCII_ALLOWED_IN_ZH.contains(&k.as_str()) {
            continue;
        }
        // 跳过纯符号/数字的值（如 "＋"、"✕"）
        let has_alpha = v.chars().any(|c| c.is_alphabetic());
        if has_alpha && is_pure_ascii(v) {
            violations.push(format!("{k} = {v}"));
        }
    }
    assert!(
        violations.is_empty(),
        "zh-CN 语料中以下键为纯 ASCII（疑似漏翻译）:\n{}",
        violations.join("\n")
    );
}

#[test]
fn both_locales_have_identical_key_sets() {
    let zh_keys = parse_keys(ZH_CN_FTL);
    let en_keys = parse_keys(EN_US_FTL);

    let missing_in_en: Vec<_> = zh_keys.difference(&en_keys).collect();
    let missing_in_zh: Vec<_> = en_keys.difference(&zh_keys).collect();

    assert!(
        missing_in_en.is_empty(),
        "zh-CN 有但 en-US 缺失的键: {:?}",
        missing_in_en
    );
    assert!(
        missing_in_zh.is_empty(),
        "en-US 有但 zh-CN 缺失的键: {:?}",
        missing_in_zh
    );
}
