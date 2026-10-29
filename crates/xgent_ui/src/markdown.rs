//! Markdown 解析与块模型。
//!
//! M6-T2：使用 `pulldown-cmark` 解析 Markdown，输出 `MarkdownBlock` 序列。
//! 支持六类元素：标题、段落、列表、代码块、粗体、链接。
//!
//! 内联元素（粗体/链接）的归属按上下文区分：
//! - 段落里 → 与前后文本一起收进 [`MarkdownBlock::Inline`]，渲染在同一行；
//! - 标题/列表项里 → 并入该行文本（这两类不保留内联样式，但内容不丢）。

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

/// Markdown 块：解析后的顶层元素。
#[derive(Debug, Clone, PartialEq)]
pub enum MarkdownBlock {
    /// 标题（level 1-6）。
    Heading { level: u8, text: String },
    /// 段落。
    Paragraph { text: String },
    /// 列表（有序或无序）。
    List { items: Vec<ListItem>, ordered: bool },
    /// 代码块（带语言标签）。
    CodeBlock { language: String, code: String },
    /// 粗体（内联样式，作为段落的一部分）。
    Bold { text: String },
    /// 链接。
    Link { url: String, text: String },
    /// 段落内的内联序列：文本/粗体/链接按顺序渲染在同一行。
    ///
    /// 只有 1 个子块时直接铺平（不加这层），保证纯段落与独占一行的
    /// 粗体/链接仍是原来的形状。
    Inline { blocks: Vec<MarkdownBlock> },
}

/// 列表项。
#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    pub text: String,
}

/// 内联内容当前的归属。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sink {
    /// 段落：内联元素拆成子块，收进 `Inline` 组。
    Paragraph,
    /// 标题或列表项：内联元素并进同一行文本。
    Merged,
}

/// 解析 Markdown 文本为块序列。
pub fn parse_markdown(input: &str) -> Vec<MarkdownBlock> {
    let parser = Parser::new(input);
    let mut blocks = Vec::new();
    let mut current_text = String::new();
    let mut in_code_block = false;
    let mut code_language = String::new();
    let mut code_content = String::new();
    // 嵌套列表：每层一个栈帧，内层收尾不会清掉外层已积累的项目。
    let mut lists: Vec<(Vec<ListItem>, bool)> = Vec::new();
    let mut in_heading = false;
    let mut heading_level = 0;
    let mut strong: Option<String> = None;
    let mut link: Option<(String, String)> = None;
    let mut sink: Option<Sink> = None;
    let mut line: Vec<MarkdownBlock> = Vec::new();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    in_heading = true;
                    heading_level = level as u8;
                    current_text.clear();
                    sink = Some(Sink::Merged);
                }
                Tag::Paragraph => {
                    current_text.clear();
                    line.clear();
                    sink = Some(Sink::Paragraph);
                }
                Tag::CodeBlock(kind) => {
                    in_code_block = true;
                    code_language = match kind {
                        pulldown_cmark::CodeBlockKind::Fenced(lang) => lang.to_string(),
                        pulldown_cmark::CodeBlockKind::Indented => String::new(),
                    };
                    code_content.clear();
                }
                Tag::List(ordered) => {
                    // 列表项内的嵌套列表：先把该项已有文本落成项目，
                    // 否则下面 Inner 的 Item 会把它冲掉。
                    if sink == Some(Sink::Merged)
                        && !current_text.is_empty()
                        && let Some((items, _)) = lists.last_mut()
                    {
                        items.push(ListItem {
                            text: std::mem::take(&mut current_text),
                        });
                    }
                    lists.push((Vec::new(), ordered.is_some()));
                }
                Tag::Item => {
                    current_text.clear();
                    sink = Some(Sink::Merged);
                }
                Tag::Strong => {
                    push_inline_boundary(&mut line, &mut current_text, sink);
                    strong = Some(String::new());
                }
                Tag::Link { dest_url, .. } => {
                    push_inline_boundary(&mut line, &mut current_text, sink);
                    link = Some((dest_url.to_string(), String::new()));
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    if in_heading {
                        blocks.push(MarkdownBlock::Heading {
                            level: heading_level,
                            text: std::mem::take(&mut current_text),
                        });
                        in_heading = false;
                        sink = None;
                    }
                }
                TagEnd::Paragraph => {
                    push_inline_boundary(&mut line, &mut current_text, sink);
                    sink = None;
                    match line.len() {
                        0 => {}
                        1 => {
                            if let Some(block) = line.pop() {
                                blocks.push(block);
                            }
                        }
                        _ => blocks.push(MarkdownBlock::Inline {
                            blocks: std::mem::take(&mut line),
                        }),
                    }
                }
                TagEnd::CodeBlock => {
                    if in_code_block {
                        blocks.push(MarkdownBlock::CodeBlock {
                            language: std::mem::take(&mut code_language),
                            code: std::mem::take(&mut code_content),
                        });
                        in_code_block = false;
                    }
                }
                TagEnd::List(_) => {
                    if let Some((items, ordered)) = lists.pop()
                        && !items.is_empty()
                    {
                        blocks.push(MarkdownBlock::List { items, ordered });
                    }
                }
                TagEnd::Item => {
                    if !current_text.is_empty()
                        && let Some((items, _)) = lists.last_mut()
                    {
                        items.push(ListItem {
                            text: std::mem::take(&mut current_text),
                        });
                    }
                    sink = None;
                }
                TagEnd::Strong => {
                    if let Some(text) = strong.take() {
                        emit_inline(&mut line, &mut current_text, sink, || {
                            (MarkdownBlock::Bold { text: text.clone() }, text)
                        });
                    }
                }
                TagEnd::Link => {
                    if let Some((url, text)) = link.take() {
                        emit_inline(&mut line, &mut current_text, sink, || {
                            let merged = if text.is_empty() {
                                url.clone()
                            } else {
                                format!("{text}（{url}）")
                            };
                            (
                                MarkdownBlock::Link {
                                    url: url.clone(),
                                    text,
                                },
                                merged,
                            )
                        });
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                if in_code_block {
                    code_content.push_str(&text);
                } else if let Some(strong) = strong.as_mut() {
                    strong.push_str(&text);
                } else if let Some((_, text_out)) = link.as_mut() {
                    text_out.push_str(&text);
                } else {
                    current_text.push_str(&text);
                }
            }
            Event::Code(code) => {
                if in_code_block {
                    code_content.push_str(&code);
                } else {
                    current_text.push_str(&code);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_code_block {
                    code_content.push('\n');
                } else if let Some(strong) = strong.as_mut() {
                    strong.push(' ');
                } else if let Some((_, text_out)) = link.as_mut() {
                    text_out.push(' ');
                } else {
                    current_text.push(' ');
                }
            }
            _ => {}
        }
    }

    blocks
}

/// 内联元素开始：段落里先把之前积累的文本落成子块，保持先后顺序。
fn push_inline_boundary(
    line: &mut Vec<MarkdownBlock>,
    current_text: &mut String,
    sink: Option<Sink>,
) {
    if sink == Some(Sink::Paragraph) && !current_text.is_empty() {
        line.push(MarkdownBlock::Paragraph {
            text: std::mem::take(current_text),
        });
    }
}

/// 内联元素结束：段落里作为子块入组，标题/列表项里并进同一行文本。
///
/// 闭包同时给出「子块形状」与「并入文本的形状」，链接的两种形态不同，
/// 故返回二元组（子块, 合并文本）。
fn emit_inline(
    line: &mut Vec<MarkdownBlock>,
    current_text: &mut String,
    sink: Option<Sink>,
    make: impl FnOnce() -> (MarkdownBlock, String),
) {
    let (block, merged) = make();
    if sink == Some(Sink::Paragraph) {
        line.push(block);
    } else {
        current_text.push_str(&merged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 顶层块类型名序列（`Inline` 展开为 `inline(...)` 便于断言内联顺序）。
    fn kinds(blocks: &[MarkdownBlock]) -> Vec<String> {
        blocks
            .iter()
            .map(|block| match block {
                MarkdownBlock::Heading { .. } => "heading".to_string(),
                MarkdownBlock::Paragraph { .. } => "paragraph".to_string(),
                MarkdownBlock::List { .. } => "list".to_string(),
                MarkdownBlock::CodeBlock { .. } => "code".to_string(),
                MarkdownBlock::Bold { .. } => "bold".to_string(),
                MarkdownBlock::Link { .. } => "link".to_string(),
                MarkdownBlock::Inline { blocks } => format!(
                    "inline({})",
                    kinds(blocks).into_iter().collect::<Vec<_>>().join("+")
                ),
            })
            .collect()
    }

    #[test]
    fn parse_heading() {
        let blocks = parse_markdown("# Hello");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(
            &blocks[0],
            MarkdownBlock::Heading { level: 1, text } if text == "Hello"
        ));
    }

    #[test]
    fn parse_paragraph() {
        let blocks = parse_markdown("This is a paragraph.");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(
            &blocks[0],
            MarkdownBlock::Paragraph { text } if text == "This is a paragraph."
        ));
    }

    #[test]
    fn parse_code_block() {
        let blocks = parse_markdown("```rust\nfn main() {}\n```");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(
            &blocks[0],
            MarkdownBlock::CodeBlock { language, code } if language == "rust" && code.contains("fn main()")
        ));
    }

    #[test]
    fn parse_list() {
        let blocks = parse_markdown("- Item 1\n- Item 2");
        assert_eq!(blocks.len(), 1);
        if let MarkdownBlock::List { items, ordered } = &blocks[0] {
            assert!(!ordered);
            assert_eq!(items.len(), 2);
        } else {
            panic!("Expected List");
        }
    }

    #[test]
    fn parse_bold() {
        let blocks = parse_markdown("**bold text**");
        assert!(blocks.iter().any(|block| matches!(
            block,
            MarkdownBlock::Bold { text } if text == "bold text"
        )));
    }

    #[test]
    fn parse_link() {
        let blocks = parse_markdown("[XGent](https://example.com)");
        assert!(blocks.iter().any(|block| matches!(
            block,
            MarkdownBlock::Link { url, text } if url == "https://example.com" && text == "XGent"
        )));
    }

    #[test]
    fn inline_bold_stays_on_one_paragraph_line() {
        // 「正文 **粗体**」必须是一段一行：文本与粗体收进同一个 Inline 组且保序。
        let blocks = parse_markdown("正文 **粗体**");
        assert_eq!(kinds(&blocks), vec!["inline(paragraph+bold)"]);
        if let MarkdownBlock::Inline { blocks } = &blocks[0] {
            assert!(matches!(
                &blocks[0],
                MarkdownBlock::Paragraph { text } if text == "正文 "
            ));
            assert!(matches!(
                &blocks[1],
                MarkdownBlock::Bold { text } if text == "粗体"
            ));
        } else {
            panic!("expected inline group: {blocks:?}");
        }
    }

    #[test]
    fn bold_inside_list_item_keeps_item_text() {
        // LLM 高频结构：列表项里的粗体不得把项目文本甩到列表外。
        let blocks = parse_markdown("- **读文件**: 拿配置");
        assert_eq!(kinds(&blocks), vec!["list"]);
        if let MarkdownBlock::List { items, .. } = &blocks[0] {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].text, "读文件: 拿配置");
        } else {
            panic!("expected list: {blocks:?}");
        }
    }

    #[test]
    fn bold_inside_heading_stays_in_heading() {
        let blocks = parse_markdown("# 标题 **重点**");
        assert_eq!(kinds(&blocks), vec!["heading"]);
        if let MarkdownBlock::Heading { level, text } = &blocks[0] {
            assert_eq!(*level, 1);
            assert_eq!(text, "标题 重点");
        } else {
            panic!("expected heading: {blocks:?}");
        }
    }

    #[test]
    fn link_inside_list_item_keeps_url() {
        let blocks = parse_markdown("- 参考 [文档](https://x.dev)");
        if let MarkdownBlock::List { items, .. } = &blocks[0] {
            assert_eq!(items[0].text, "参考 文档（https://x.dev）");
        } else {
            panic!("expected list: {blocks:?}");
        }
    }

    #[test]
    fn nested_list_keeps_outer_items() {
        let blocks = parse_markdown("- 外层一\n  - 内层\n- 外层二");
        let lists: Vec<&Vec<ListItem>> = blocks
            .iter()
            .filter_map(|block| match block {
                MarkdownBlock::List { items, .. } => Some(items),
                _ => None,
            })
            .collect();
        assert_eq!(lists.len(), 2, "内外两层列表都要出块：{blocks:?}");
        let outer_texts: Vec<&str> = lists
            .into_iter()
            .flat_map(|items| items.iter().map(|item| item.text.as_str()))
            .collect();
        assert!(
            outer_texts.contains(&"外层一") && outer_texts.contains(&"外层二"),
            "外层项目不得丢失：{outer_texts:?}"
        );
    }

    #[test]
    fn parse_six_element_sample() {
        let md = "# H

Para

- a
- b

```rust
code
```

正文 **bold** 尾巴

[link](https://e.com)";
        let blocks = parse_markdown(md);
        let joined = kinds(&blocks).join(",");
        for kind in [
            "heading",
            "paragraph",
            "list",
            "code",
            "bold",
            "link",
            "inline(",
        ] {
            assert!(joined.contains(kind), "missing {kind}: {joined}");
        }
    }

    #[test]
    fn parse_mixed() {
        let md = "# Title\n\nParagraph\n\n```rust\ncode\n```";
        let blocks = parse_markdown(md);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(
            &blocks[0],
            MarkdownBlock::Heading { level: 1, .. }
        ));
        assert!(matches!(&blocks[1], MarkdownBlock::Paragraph { .. }));
        assert!(matches!(&blocks[2], MarkdownBlock::CodeBlock { .. }));
    }
}
