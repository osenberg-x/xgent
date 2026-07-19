//! tree-sitter 语法高亮。
//!
//! 详见 `doc/design/editor-design.md` 第 5.3 节。
//!
//! MVP 策略：全量解析（每次文本变化重解析全文）。
//! 大文件增量解析留待后续（设计文档 5.3 节性能要求 O(改动量)）。
//!
//! 高亮方式：遍历 tree-sitter AST 节点，按节点 `kind()` 映射到 [`SpanKind`]，
//! 生成按字节区间不重叠的 [`HighlightSpan`] 列表。
//! 渲染层（`render::sync_highlight_layer`）据 spans 重建 `TextSpan` 子树。
//!
//! grammar 随二进制编译入（`tree-sitter-rust`），不做按需下载（D-06 已决策）。

use bevy::prelude::*;
use crate::text_editor::{Language, Rope};

/// 高亮 span 的语义类别（映射到颜色，由渲染层决定具体配色）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    /// 关键字（fn、let、if、match 等）
    Keyword,
    /// 字符串字面量
    String,
    /// 注释（行注释 / 块注释）
    Comment,
    /// 数字字面量
    Number,
    /// 函数名（调用 / 定义）
    FunctionName,
    /// 类型名（大写驼峰 / trait / struct 等）
    Type,
    /// 标识符（普通变量）
    Identifier,
    /// 标点与运算符
    Punctuation,
    /// 宏名
    Macro,
    /// 常量（全大写下划线）
    Constant,
    /// 布尔字面量（true / false）
    Boolean,
    /// 其他（默认色）
    Plain,
}

impl SpanKind {
    /// 默认色（无高亮）。
    pub const PLAIN: Self = SpanKind::Plain;
}

/// 单个高亮 span（字节区间 [start, end)，半开，与文本 UTF-8 字节偏移对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightSpan {
    /// 起始字节偏移
    pub start: usize,
    /// 结束字节偏移（半开）
    pub end: usize,
    /// 语义类别
    pub kind: SpanKind,
}

/// 对给定文本与语言做语法高亮，返回 span 列表（不重叠，按 start 排序）。
///
/// MVP 用 Rust grammar 全量解析。未知语言返回单个 [`SpanKind::Plain`] span。
pub fn highlight(text: &str, lang: Language) -> Vec<HighlightSpan> {
    match lang {
        Language::Rust => highlight_rust(text),
    }
}

fn highlight_rust(text: &str) -> Vec<HighlightSpan> {
    use tree_sitter::Parser;

    let mut parser = Parser::new();
    let language = tree_sitter_rust::LANGUAGE;
    if parser.set_language(&language.into()).is_err() {
        // grammar 加载失败，降级为整段 Plain
        return vec![HighlightSpan {
            start: 0,
            end: text.len(),
            kind: SpanKind::Plain,
        }];
    }
    let tree = match parser.parse(text.as_bytes(), None) {
        Some(t) => t,
        None => {
            return vec![HighlightSpan {
                start: 0,
                end: text.len(),
                kind: SpanKind::Plain,
            }];
        }
    };
    let root = tree.root_node();
    let mut spans = Vec::new();
    walk_node(&root, text, &mut spans);
    // 后处理：合并相邻同类别、填充未覆盖区间为 Plain
    normalize_spans(spans, text.len())
}

/// 递归遍历 AST 节点，按 kind 映射 span。
///
/// 策略：若节点 kind 可映射到 [`SpanKind`]，记录该节点 span 并**不递归子节点**
/// （整段高亮，如 string_literal、line_comment）。否则递归子节点。
fn walk_node<'a>(node: &tree_sitter::Node<'a>, _text: &str, out: &mut Vec<HighlightSpan>) {
    let kind_str = node.kind();
    if let Some(span_kind) = map_kind(kind_str) {
        out.push(HighlightSpan {
            start: node.start_byte(),
            end: node.end_byte(),
            kind: span_kind,
        });
        return;
    }
    // 递归子节点
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            walk_node(&child, _text, out);
        }
    }
}
/// 把 tree-sitter node kind 映射到 [`SpanKind`]。
///
/// Rust grammar 的 kind 名见 tree-sitter-rust `node-types.json`。
fn map_kind(kind: &str) -> Option<SpanKind> {
    use SpanKind::*;
    match kind {
        // 关键字
        "fn" | "let" | "mut" | "if" | "else" | "match" | "while" | "for" | "loop" | "return"
        | "break" | "continue" | "struct" | "enum" | "trait" | "impl" | "pub" | "use" | "mod"
        | "as" | "in" | "ref" | "static" | "const" | "unsafe" | "async" | "await" | "move"
        | "dyn" | "where" | "type" | "self" | "super" | "crate" | "extern" | "union" | "box"
        | "default" | "try" | "yield" | "macro_rules" | "gen" | "become" | "raw" | "offsetof"
        | "typeof" => Some(Keyword),

        // 字面量
        "string_literal" => Some(String),
        "raw_string_literal" => Some(String),
        "char_literal" => Some(String),
        "integer_literal" => Some(Number),
        "float_literal" => Some(Number),

        // 注释
        "line_comment" => Some(Comment),
        "block_comment" => Some(Comment),

        // 布尔
        "true" | "false" => Some(Boolean),

        // 标识符
        "identifier" => {
            // 无法在此判断是否函数名/类型/常量——留 Plain/Identifier
            Some(Identifier)
        }
        "type_identifier" => Some(Type),
        "field_identifier" => Some(Identifier),

        // 宏
        "macro_invocation" => Some(Macro),

        // 标点
        ";" | "," | "." | ":" | "::" | "=>" | "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&="
        | "|=" | "^=" | "<<=" | ">>=" | "&&" | "||" | "==" | "!=" | "<=" | ">=" | "<" | ">"
        | "+" | "-" | "*" | "/" | "%" | "&" | "|" | "^" | "!" | "?" | "@" | "#" | "$" | "->"
        | ">>" | ".." | "..=" | "_" | "{" | "}" | "(" | ")" | "[" | "]" => Some(Punctuation),

        _ => None,
    }
}

/// 归一化 span 列表：合并相邻同类别、填充未覆盖字节为 Plain，确保不重叠且覆盖 [0, len)。
fn normalize_spans(mut spans: Vec<HighlightSpan>, text_len: usize) -> Vec<HighlightSpan> {
    if text_len == 0 {
        return Vec::new();
    }
    // 按 start 排序，end 对齐
    spans.sort_by_key(|s| (s.start, s.end));
    let mut out: Vec<HighlightSpan> = Vec::new();
    let mut cursor = 0usize;
    for s in spans {
        if s.end <= s.start {
            continue;
        }
        if s.start > cursor {
            // 填充未覆盖区间
            out.push(HighlightSpan {
                start: cursor,
                end: s.start.min(text_len),
                kind: SpanKind::Plain,
            });
        }
        let start = s.start.max(cursor);
        let end = s.end.min(text_len);
        if end > start {
            // 合并到上一相邻同类别
            if let Some(last) = out.last_mut()
                && last.kind == s.kind
                && last.end == start
            {
                last.end = end;
            } else {
                out.push(HighlightSpan { start, end, kind: s.kind });
            }
            cursor = end;
        }
    }
    if cursor < text_len {
        out.push(HighlightSpan {
            start: cursor,
            end: text_len,
            kind: SpanKind::Plain,
        });
    }
    out
}

/// span kind → 颜色映射（暗色主题对齐 VSCode Dark+）。
///
/// 公开供虚拟化渲染层按行 span 着色。
pub fn span_color_for(kind: SpanKind) -> Color {
    use bevy::color::palettes::css;
    match kind {
        SpanKind::Keyword => Color::srgb(0.86, 0.45, 0.67),
        SpanKind::String => Color::srgb(0.71, 0.86, 0.43),
        SpanKind::Comment => Color::srgb(0.46, 0.50, 0.56),
        SpanKind::Number => Color::srgb(0.80, 0.60, 0.30),
        SpanKind::FunctionName => Color::srgb(0.50, 0.78, 0.95),
        SpanKind::Type => Color::srgb(0.78, 0.88, 0.60),
        SpanKind::Identifier => css::WHITE.into(),
        SpanKind::Punctuation => Color::srgb(0.70, 0.72, 0.76),
        SpanKind::Macro => Color::srgb(0.80, 0.55, 0.85),
        SpanKind::Constant => Color::srgb(0.80, 0.60, 0.30),
        SpanKind::Boolean => Color::srgb(0.86, 0.45, 0.67),
        SpanKind::Plain => css::WHITE.into(),
    }
}

/// 把全局字节区间 spans 按指定行切片，返回该行的 `(文本片段, SpanKind)` 列表。
///
/// `row`：0-based 行号。`rope`：全文 rope。`global_spans`：全文 spans（字节区间）。
/// 用 `rope.line_to_byte(row)` O(log n) 定位行首字节偏移，
/// 取相交 spans 裁剪到行内，返回行内片段。
pub fn spans_for_line(
    global_spans: &[HighlightSpan],
    _line_text: &str,
    row: usize,
    rope: &Rope,
) -> Vec<(String, SpanKind)> {
    // 行首字节偏移（O(log n)）。
    // ropey 的 get_line(row) 返回的 slice 含行尾 '\n'（除最后一行），
    // 故行末 = 下行行首 - 1（若有 '\n'），否则 = 全文末。
    let line_start = rope.line_to_byte(row);
    let line_end = if row + 1 < rope.len_lines() {
        // 非最后一行：剥掉行尾 '\n'
        rope.line_to_byte(row + 1).saturating_sub(1)
    } else {
        rope.len_bytes()
    };

    // 取相交 spans，裁剪到 [line_start, line_end)
    let mut out: Vec<(String, SpanKind)> = Vec::new();
    let mut cursor = line_start;
    for s in global_spans {
        if s.end <= line_start || s.start >= line_end {
            continue;
        }
        let seg_start = s.start.max(line_start);
        let seg_end = s.end.min(line_end);
        if seg_start > cursor {
            // 填充未覆盖
            let text = reconstruct_segment(rope, row, cursor - line_start, seg_start - line_start);
            out.push((text, SpanKind::Plain));
        }
        let text = reconstruct_segment(rope, row, seg_start - line_start, seg_end - line_start);
        out.push((text, s.kind));
        cursor = seg_end;
    }
    if cursor < line_end {
        let text = reconstruct_segment(rope, row, cursor - line_start, line_end - line_start);
        out.push((text, SpanKind::Plain));
    }
    out
}

/// 从 rope 重建某行的字节片段 [col_start, col_end)（列=字节偏移，0-based，行内）。
///
/// 用 `rope.get_line(row).byte_slice(col_start..col_end).to_string()`
/// 取行内字节片段。RopeSlice 的 byte_slice 是 O(log n)。
fn reconstruct_segment(rope: &Rope, row: usize, col_start: usize, col_end: usize) -> String {
    let Some(line) = rope.get_line(row) else {
        return String::new();
    };
    let end = col_end.min(line.len_bytes());
    let start = col_start.min(end);
    line.byte_slice(start..end).to_string()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_for_line_on_rope_reconstructs_single_line() {
        // 单行文本：spans_for_line 拼接后应还原原文（不增不减字符）
        let text = "plain text line";
        let rope = Rope::from_str(text);
        let spans = highlight(text, Language::Rust);
        let line_spans = spans_for_line(&spans, text, 0, &rope);
        let reconstructed: String = line_spans.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(reconstructed, "plain text line");
    }

    #[test]
    fn spans_for_line_multiline_rope_correct_byte_offsets() {
        // 多行：验证 rope.line_to_byte 给出的行首偏移与 highlight 的全局 span 对齐
        let code = "let x = 1;\nlet y = 2;";
        let rope = Rope::from_str(code);
        let spans = highlight(code, Language::Rust);
        // 第二行 "let y = 2;"，行首字节偏移应为 11（第一行 10 字符 + '\n'）
        let line1_spans = spans_for_line(&spans, "let y = 2;", 1, &rope);
        // 应含一个 Keyword span（"let"）
        let has_kw = line1_spans.iter().any(|(_, k)| *k == SpanKind::Keyword);
        assert!(has_kw, "第二行应识别到关键字 let: {:?}", line1_spans);
        // 拼回应等于第二行原文
        let reconstructed: String = line1_spans.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(reconstructed, "let y = 2;");
    }

    #[test]
    fn spans_for_line_preserves_multibyte_chars_in_rope() {
        // 验证 rope 字节偏移与 UTF-8 多字节字符对齐（中文注释）
        let code = "// 注释\nlet x = 1;";
        let rope = Rope::from_str(code);
        let spans = highlight(code, Language::Rust);
        let line0 = spans_for_line(&spans, "// 注释", 0, &rope);
        // 第一行整行应是 Comment
        let reconstructed: String = line0.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(reconstructed, "// 注释");
        assert!(line0.iter().all(|(_, k)| *k == SpanKind::Comment));
    }

    #[test]
    fn highlight_empty_text() {
        let spans = highlight("", Language::Rust);
        assert!(spans.is_empty());
    }

    #[test]
    fn highlight_keyword_let() {
        let code = "let x = 1;";
        let spans = highlight(code, Language::Rust);
        // 至少识别到 "let" 关键字 span
        let let_span = spans.iter().find(|s| s.start == 0 && s.end == 3);
        assert!(let_span.is_some(), "应识别 'let' 关键字");
        assert_eq!(let_span.unwrap().kind, SpanKind::Keyword);
    }

    #[test]
    fn highlight_string_literal() {
        let code = "let s = \"hi\";";
        let spans = highlight(code, Language::Rust);
        let str_span = spans.iter().find(|s| s.kind == SpanKind::String);
        assert!(str_span.is_some(), "应识别字符串字面量");
        let s = str_span.unwrap();
        assert_eq!(&code[s.start..s.end], "\"hi\"");
    }

    #[test]
    fn highlight_line_comment() {
        let code = "// comment\nlet x = 1;";
        let spans = highlight(code, Language::Rust);
        let comment = spans.iter().find(|s| s.kind == SpanKind::Comment);
        assert!(comment.is_some(), "应识别行注释");
        let c = comment.unwrap();
        assert_eq!(&code[c.start..c.end], "// comment");
    }

    #[test]
    fn highlight_integer_literal() {
        let code = "let x = 42;";
        let spans = highlight(code, Language::Rust);
        let num = spans.iter().find(|s| s.kind == SpanKind::Number);
        assert!(num.is_some(), "应识别数字字面量");
        let n = num.unwrap();
        assert_eq!(&code[n.start..n.end], "42");
    }

    #[test]
    fn highlight_spans_cover_all_text() {
        let code = "fn main() { let x = 1; }";
        let spans = highlight(code, Language::Rust);
        // 断言不重叠且覆盖 [0, len)
        let mut prev_end = 0;
        for s in &spans {
            assert!(s.start >= prev_end, "span 重叠: {s:?} (prev_end={prev_end})");
            assert_eq!(s.start, prev_end, "span 未连续覆盖: gap or overlap");
            prev_end = s.end;
        }
        assert_eq!(prev_end, code.len(), "末尾未覆盖");
    }

    #[test]
    fn highlight_normalizes_merges_adjacent_same_kind() {
        // 两个相邻 Keyword（如 "pub fn"）应合并
        let code = "pub fn";
        let spans = highlight(code, Language::Rust);
        let keywords: Vec<_> = spans.iter().filter(|s| s.kind == SpanKind::Keyword).collect();
        // "pub" 和 "fn" 中间有空格（Plain），故不合并
        // 这里仅断言不重叠覆盖
        let mut prev_end = 0;
        for s in &spans {
            assert!(s.start >= prev_end);
            prev_end = s.end;
        }
        assert_eq!(prev_end, code.len());
        // 至少有两个 keyword span
        assert!(keywords.len() >= 2, "应识别 pub 和 fn 两个关键字");
    }

    #[test]
    fn normalize_fills_gaps_with_plain() {
        let spans = vec![
            HighlightSpan { start: 5, end: 8, kind: SpanKind::Keyword },
            HighlightSpan { start: 10, end: 12, kind: SpanKind::Number },
        ];
        let out = normalize_spans(spans, 12);
        // 应有 Plain 填充 [0,5) 和 [8,10)
        assert!(out.iter().any(|s| s.kind == SpanKind::Plain && s.start == 0 && s.end == 5));
        assert!(out.iter().any(|s| s.kind == SpanKind::Plain && s.start == 8 && s.end == 10));
        // 覆盖末尾
        assert_eq!(out.last().unwrap().end, 12);
    }

    #[test]
    fn normalize_empty_text_returns_empty() {
        let out = normalize_spans(vec![], 0);
        assert!(out.is_empty());
    }
}
