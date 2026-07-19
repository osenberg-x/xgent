//! @ 引用解析（输入预处理）。
//!
//! 详见 `doc/design/editor-design.md` 第 3.5 节 / 6.5 节。
//!
//! MVP 三种 @ 引用：
//! - `@file:src/main.rs` — 拉取该文件内容作为上下文。
//! - `@cursor` — 拉取当前光标位置所在符号 + 周边若干行。
//! - `@selection` — 拉取当前选区文本。
//!
//! 不识别的 `@xxx` 原样保留，不做补全 UI（P1+ 再加）。

use xgent_core::EditorQuery;

/// 解析输入文本中的 @ 引用，替换为占位标记，并收集 [`EditorQuery`]。
///
/// 返回 `(替换后的文本, 查询列表)`。
/// - `@file:<path>` → 占位 `[@file:<path>]`，收集 `EditorQuery::File { path }`
/// - `@cursor` → 占位 `[@cursor]`，收集 `EditorQuery::Cursor`
/// - `@selection` → 占位 `[@selection]`，收集 `EditorQuery::Selection`
/// - 其他 `@xxx`（非上述三种）原样保留
pub fn parse_at_references(input: &str) -> (String, Vec<EditorQuery>) {
    let mut out = String::with_capacity(input.len());
    let mut queries = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            // 尝试匹配已知 @ 引用
            let rest = &input[i..];
            if let Some(path) = parse_file_ref(rest) {
                let path_str = path.to_string_lossy().to_string();
                out.push_str(&format!("[@file:{path_str}]"));
                queries.push(EditorQuery::File { path });
                // 只跳过 @file:<path> 的长度（不含后续空白/文本）
                i += "@file:".len() + path_str.len();
                continue;
            }
            if rest.starts_with("@cursor") {
                out.push_str("[@cursor]");
                queries.push(EditorQuery::Cursor);
                i += "@cursor".len();
                continue;
            }
            if rest.starts_with("@selection") {
                out.push_str("[@selection]");
                queries.push(EditorQuery::Selection);
                i += "@selection".len();
                continue;
            }
            // 未知 @ 引用，原样保留
            out.push('@');
            i += 1;
        } else {
            // 安全推进一个 UTF-8 字符
            let ch = input[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    (out, queries)
}

/// 尝试匹配 `@file:<path>`，返回 path。
///
/// path 终止于空白或字符串末尾。允许路径含 `/`、`.`、`-`、`_`、字母数字。
fn parse_file_ref(rest: &str) -> Option<std::path::PathBuf> {
    let s = rest.strip_prefix("@file:")?;
    if s.is_empty() {
        return None;
    }
    // path 终止于空白
    let end = s
        .char_indices()
        .find(|(_, c)| c.is_whitespace())
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    let path_str = &s[..end];
    if path_str.is_empty() {
        return None;
    }
    // 基本校验：不含空白、引号
    if path_str
        .chars()
        .any(|c| c.is_whitespace() || c == '"' || c == '\'')
    {
        return None;
    }
    Some(std::path::PathBuf::from(path_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_file_ref_basic() {
        let (text, q) = parse_at_references("看 @file:src/main.rs 这个文件");
        assert_eq!(text, "看 [@file:src/main.rs] 这个文件");
        assert_eq!(q.len(), 1);
        match &q[0] {
            EditorQuery::File { path } => assert_eq!(path, std::path::Path::new("src/main.rs")),
            _ => panic!("应为 File 查询"),
        }
    }

    #[test]
    fn parse_cursor_ref() {
        let (text, q) = parse_at_references("看 @cursor 这个函数");
        assert_eq!(text, "看 [@cursor] 这个函数");
        assert_eq!(q, vec![EditorQuery::Cursor]);
    }

    #[test]
    fn parse_selection_ref() {
        let (text, q) = parse_at_references("@selection 有问题吗");
        assert_eq!(text, "[@selection] 有问题吗");
        assert_eq!(q, vec![EditorQuery::Selection]);
    }

    #[test]
    fn multiple_refs_in_one_message() {
        let (text, q) = parse_at_references("@cursor 和 @file:src/lib.rs");
        assert_eq!(text, "[@cursor] 和 [@file:src/lib.rs]");
        assert_eq!(q.len(), 2);
        assert_eq!(q[0], EditorQuery::Cursor);
        match &q[1] {
            EditorQuery::File { path } => assert_eq!(path, std::path::Path::new("src/lib.rs")),
            _ => panic!("第二个应为 File"),
        }
    }

    #[test]
    fn unknown_at_kept_as_is() {
        let (text, q) = parse_at_references("@unknown 引用 @email");
        assert_eq!(text, "@unknown 引用 @email");
        assert!(q.is_empty());
    }

    #[test]
    fn no_refs_returns_unchanged() {
        let (text, q) = parse_at_references("普通消息无引用");
        assert_eq!(text, "普通消息无引用");
        assert!(q.is_empty());
    }

    #[test]
    fn file_ref_at_end_of_string() {
        let (text, q) = parse_at_references("打开 @file:src/main.rs");
        assert_eq!(text, "打开 [@file:src/main.rs]");
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn file_ref_empty_path_returns_none() {
        // @file: 后无路径——不识别为 file ref，原样保留
        let (text, q) = parse_at_references("@file: 后面空");
        assert_eq!(text, "@file: 后面空");
        assert!(q.is_empty());
    }

    #[test]
    fn file_ref_with_spaces_in_path_rejected() {
        // path 含空格——截断到空格前
        let (text, q) = parse_at_references("@file:src/main.rs 后续");
        assert_eq!(text, "[@file:src/main.rs] 后续");
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn unicode_text_preserved() {
        let (text, q) = parse_at_references("你好 @cursor 世界");
        assert_eq!(text, "你好 [@cursor] 世界");
        assert_eq!(q, vec![EditorQuery::Cursor]);
    }
}
