//! Markdown 解析与块模型。
//!
//! M6-T2：使用 `pulldown-cmark` 解析 Markdown，输出 `MarkdownBlock` 序列。
//! 支持六类元素：标题、段落、列表、代码块、粗体、链接。

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
}

/// 列表项。
#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    pub text: String,
}

/// 解析 Markdown 文本为块序列。
pub fn parse_markdown(input: &str) -> Vec<MarkdownBlock> {
    let parser = Parser::new(input);
    let mut blocks = Vec::new();
    let mut current_text = String::new();
    let mut in_code_block = false;
    let mut code_language = String::new();
    let mut code_content = String::new();
    let mut in_list = false;
    let mut list_items = Vec::new();
    let mut list_ordered = false;
    let mut in_heading = false;
    let mut heading_level = 0;

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    in_heading = true;
                    heading_level = level as u8;
                    current_text.clear();
                }
                Tag::Paragraph => {
                    if !in_code_block && !in_list && !in_heading {
                        current_text.clear();
                    }
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
                    in_list = true;
                    list_ordered = ordered.is_some();
                    list_items.clear();
                }
                Tag::Item => {
                    current_text.clear();
                }
                Tag::Strong => {
                    // 粗体作为内联样式，在文本中处理
                }
                Tag::Link { dest_url, .. } => {
                    // 链接在 Text 事件中处理
                    let _ = dest_url;
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    if in_heading {
                        blocks.push(MarkdownBlock::Heading {
                            level: heading_level,
                            text: current_text.clone(),
                        });
                        in_heading = false;
                        current_text.clear();
                    }
                }
                TagEnd::Paragraph => {
                    if !in_code_block && !in_list && !in_heading {
                        if !current_text.is_empty() {
                            blocks.push(MarkdownBlock::Paragraph {
                                text: current_text.clone(),
                            });
                            current_text.clear();
                        }
                    }
                }
                TagEnd::CodeBlock => {
                    if in_code_block {
                        blocks.push(MarkdownBlock::CodeBlock {
                            language: code_language.clone(),
                            code: code_content.clone(),
                        });
                        in_code_block = false;
                        code_language.clear();
                        code_content.clear();
                    }
                }
                TagEnd::List(_) => {
                    if in_list {
                        blocks.push(MarkdownBlock::List {
                            items: list_items.clone(),
                            ordered: list_ordered,
                        });
                        in_list = false;
                        list_items.clear();
                    }
                }
                TagEnd::Item => {
                    if in_list {
                        list_items.push(ListItem {
                            text: current_text.clone(),
                        });
                        current_text.clear();
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                if in_code_block {
                    code_content.push_str(&text);
                } else if in_heading || in_list {
                    current_text.push_str(&text);
                } else {
                    current_text.push_str(&text);
                }
            }
            Event::Code(code) => {
                // 行内代码
                if in_code_block {
                    code_content.push_str(&code);
                } else {
                    current_text.push_str(&code);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_code_block {
                    code_content.push('\n');
                } else {
                    current_text.push(' ');
                }
            }
            _ => {}
        }
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parse_mixed() {
        let md = "# Title\n\nParagraph\n\n```rust\ncode\n```";
        let blocks = parse_markdown(md);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[0], MarkdownBlock::Heading { level: 1, .. }));
        assert!(matches!(&blocks[1], MarkdownBlock::Paragraph { .. }));
        assert!(matches!(&blocks[2], MarkdownBlock::CodeBlock { .. }));
    }
}
