//! 极简 glob 路径匹配：把用户写的 `*.rs`、`src/**/*.ts` 翻译成整串锚定的正则。
//!
//! 只覆盖 glob 的四个构造（`*`、`**`、`?`、`{a,b}`），其余字符按字面量转义。
//! 不引入 `globset`：它带来的能力（gitignore 语义、大小写折叠、字集）本工具
//! 用不到，而 `search_files` 的 glob 规则必须能在工具描述里一次说清。
//!
//! 语义取自 ripgrep 的 `--glob` 约定：**不含 `/` 的模式只匹配文件名**
//! （`*.rs` 在任意深度命中），含 `/` 的模式匹配相对路径整串。

use regex::Regex;

/// 已编译的 glob 模式。
#[derive(Debug)]
pub(crate) struct Glob {
    /// 锚定后的正则
    re: Regex,
    /// 是否只对照文件名（模式不含 `/`）
    name_only: bool,
}

impl Glob {
    /// 匹配项目相对路径（`/` 分隔）与其文件名。
    ///
    /// 大小写敏感：glob 描述的是「哪些文件」，与内容检索的
    /// `case_insensitive` 是两回事。
    pub(crate) fn matches(&self, rel_path: &str, file_name: &str) -> bool {
        let target = if self.name_only { file_name } else { rel_path };
        self.re.is_match(target)
    }
}

/// 编译 glob 模式；语法非法时返回中文错因（供工具报错直接展示）。
pub(crate) fn compile(pattern: &str) -> Result<Glob, String> {
    let name_only = !pattern.contains('/');
    let translated = translate(pattern)?;
    let anchored = format!("^{translated}$");
    let re = Regex::new(&anchored).map_err(|e| format!("glob 无法编译：{e}"))?;
    Ok(Glob { re, name_only })
}

/// 把 glob 翻译成正则源串（不含锚点）。
fn translate(pattern: &str) -> Result<String, String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    let mut prev_was_sep = true; // 段首（串首或刚处理过 `/`）
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' => {
                if chars.get(i + 1) == Some(&'*') {
                    // `**`：跨目录。`**/`（段首）吃掉随后的 `/` 并允许零层目录
                    let followed_by_slash = chars.get(i + 2) == Some(&'/');
                    if prev_was_sep && followed_by_slash {
                        out.push_str("(?:[^/]+/)*");
                        i += 3;
                        prev_was_sep = true;
                        continue;
                    }
                    out.push_str(".*");
                    i += 2;
                    prev_was_sep = false;
                    continue;
                }
                out.push_str("[^/]*");
                i += 1;
                prev_was_sep = false;
            }
            '?' => {
                out.push_str("[^/]");
                i += 1;
                prev_was_sep = false;
            }
            '{' => {
                let close = (i + 1..chars.len())
                    .find(|&j| chars[j] == '}')
                    .ok_or_else(|| "glob 中 '{' 未闭合".to_string())?;
                let body: String = chars[i + 1..close].iter().collect();
                if body.is_empty() {
                    return Err("glob 中 '{...}' 分支为空".to_string());
                }
                let mut alts: Vec<String> = Vec::new();
                for part in body.split(',') {
                    alts.push(translate(part)?);
                }
                out.push_str("(?:");
                out.push_str(&alts.join("|"));
                out.push(')');
                i = close + 1;
                prev_was_sep = false;
            }
            '}' => return Err("glob 中 '}' 无对应 '{'".to_string()),
            '/' => {
                out.push('/');
                i += 1;
                prev_was_sep = true;
            }
            other => {
                out.push_str(&regex_lite_escape(other));
                i += 1;
                prev_was_sep = false;
            }
        }
    }
    Ok(out)
}

/// 单个字符的正则字面量转义。
fn regex_lite_escape(c: char) -> String {
    if regex_syntax_is_meta(c) {
        format!("\\{c}")
    } else {
        c.to_string()
    }
}

/// 正则元字符集合（需要反斜杠转义才能作字面量）。
fn regex_syntax_is_meta(c: char) -> bool {
    matches!(
        c,
        '.' | '\\' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, rel: &str) -> bool {
        let g = compile(pattern).expect("合法 glob");
        let name = rel.rsplit('/').next().unwrap_or(rel);
        g.matches(rel, name)
    }

    #[test]
    fn bare_extension_glob_matches_name_at_any_depth() {
        assert!(m("*.rs", "a.rs"));
        assert!(m("*.rs", "src/a.rs"));
        assert!(m("*.rs", "deep/nested/b.rs"));
        assert!(!m("*.rs", "src/a.ts"));
    }

    #[test]
    fn star_does_not_cross_directories() {
        assert!(m("src/*.rs", "src/a.rs"));
        assert!(!m("src/*.rs", "src/deep/a.rs"));
        assert!(!m("src/*.rs", "tests/a.rs"));
    }

    #[test]
    fn globstar_crosses_directories() {
        assert!(m("src/**/*.rs", "src/a.rs"));
        assert!(m("src/**/*.rs", "src/deep/x.rs"));
        assert!(m("src/**/*.rs", "src/a/b/c.rs"));
        assert!(!m("src/**/*.rs", "tests/a.rs"));
        assert!(m("**/*.md", "x/y/z.md"));
    }

    #[test]
    fn question_mark_is_one_char() {
        assert!(m("?at.rs", "cat.rs"));
        assert!(!m("?at.rs", "at.rs"));
        assert!(!m("?at.rs", "chat.rs"));
    }

    #[test]
    fn alternation_branches() {
        assert!(m("{src,tests}/*.rs", "src/a.rs"));
        assert!(m("{src,tests}/*.rs", "tests/a.rs"));
        assert!(!m("{src,tests}/*.rs", "docs/a.rs"));
        assert!(m("*.rs.{rs,py}", "x.rs.rs"));
    }

    #[test]
    fn literal_dots_are_not_wildcards() {
        // `a.rs` 不得命中 `axrs`
        assert!(m("a.rs", "a.rs"));
        assert!(!m("a.rs", "axrs"));
    }

    #[test]
    fn invalid_patterns_report_reason() {
        assert!(compile("src/{a,b").is_err());
        assert!(compile("src/a}b").is_err());
        assert!(compile("src/{}").is_err());
        let err = compile("src/{a,b").unwrap_err();
        assert!(err.contains("'{' 未闭合"), "错因需可辨识: {err}");
    }

    #[test]
    fn pattern_with_slash_is_not_name_only() {
        let g = compile("src/*.rs").unwrap();
        assert!(!g.name_only);
        let g = compile("*.rs").unwrap();
        assert!(g.name_only);
    }
}
