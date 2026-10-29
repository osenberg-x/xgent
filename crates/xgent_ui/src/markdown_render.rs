//! Markdown 富文本渲染：将 MarkdownBlock 转换为 Bevy UI 实体。
//!
//! M6-T3：单节点多 TextSection，代码块样式（code_bg 底 + mono + 语言标签 + 复制按钮）。

use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;

use crate::fonts::{mono_text, ui_text};
use crate::i18n::tr;
use crate::markdown::MarkdownBlock;
use crate::theme::{Theme, type_scale};

/// Markdown 渲染结果标记。
#[derive(Component)]
pub struct MarkdownRendered;

/// 代码块标记（携带代码内容，用于复制）。
#[derive(Component)]
pub struct CodeBlockMarker {
    pub code: String,
}

/// 复制按钮标记。
#[derive(Component)]
pub struct CopyButtonMarker;

/// 渲染 Markdown 块序列为 UI 实体。
pub fn spawn_markdown(
    commands: &mut Commands,
    blocks: &[MarkdownBlock],
    theme: &Theme,
    fonts: &crate::fonts::UiFonts,
    loc: &xgent_settings::Localizer,
) -> Entity {
    let container = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            },
            MarkdownRendered,
        ))
        .id();

    for block in blocks {
        let block_entity = render_block(commands, block, theme, fonts, loc);
        commands.entity(container).add_child(block_entity);
    }

    container
}

/// 渲染单个 Markdown 块。
fn render_block(
    commands: &mut Commands,
    block: &MarkdownBlock,
    theme: &Theme,
    fonts: &crate::fonts::UiFonts,
    loc: &xgent_settings::Localizer,
) -> Entity {
    match block {
        MarkdownBlock::Heading { level, text } => render_heading(commands, *level, text, theme),
        MarkdownBlock::Paragraph { text } => render_paragraph(commands, text, theme),
        MarkdownBlock::List { items, ordered } => {
            render_list(commands, items, *ordered, theme, loc)
        }
        MarkdownBlock::CodeBlock { language, code } => {
            render_code_block(commands, language, code, theme, fonts, loc)
        }
        MarkdownBlock::Bold { text } => render_bold(commands, text, theme),
        MarkdownBlock::Link { url, text } => render_link(commands, url, text, theme),
    }
}

/// 渲染标题。
fn render_heading(commands: &mut Commands, level: u8, text: &str, theme: &Theme) -> Entity {
    let font_size = match level {
        1 => type_scale::DISPLAY,
        2 => type_scale::H3,
        3 => type_scale::BODY,
        _ => type_scale::BODY_SM,
    };
    let font_weight = if level <= 2 { 590 } else { 510 };

    commands
        .spawn((Node::default(),))
        .with_children(|parent| {
            parent.spawn(ui_text(
                text,
                font_size,
                font_weight,
                theme.text,
                type_scale::line_height::UI,
            ));
        })
        .id()
}

/// 渲染段落。
fn render_paragraph(commands: &mut Commands, text: &str, theme: &Theme) -> Entity {
    commands
        .spawn((Node::default(),))
        .with_children(|parent| {
            parent.spawn(ui_text(
                text,
                type_scale::BODY,
                400,
                theme.text,
                type_scale::line_height::BODY,
            ));
        })
        .id()
}

/// 渲染列表。
fn render_list(
    commands: &mut Commands,
    items: &[crate::markdown::ListItem],
    ordered: bool,
    theme: &Theme,
    _loc: &xgent_settings::Localizer,
) -> Entity {
    commands
        .spawn((Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        },))
        .with_children(|parent| {
            for (i, item) in items.iter().enumerate() {
                let prefix = if ordered {
                    format!("{}. ", i + 1)
                } else {
                    "• ".to_string()
                };
                parent.spawn((Node::default(),)).with_children(|row| {
                    row.spawn(ui_text(
                        &prefix,
                        type_scale::BODY,
                        400,
                        theme.text_muted,
                        type_scale::line_height::BODY,
                    ));
                    row.spawn(ui_text(
                        &item.text,
                        type_scale::BODY,
                        400,
                        theme.text,
                        type_scale::line_height::BODY,
                    ));
                });
            }
        })
        .id()
}

/// 渲染代码块。
fn render_code_block(
    commands: &mut Commands,
    language: &str,
    code: &str,
    theme: &Theme,
    fonts: &crate::fonts::UiFonts,
    loc: &xgent_settings::Localizer,
) -> Entity {
    let code_content = code.to_string();

    commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(theme.code_bg),
            CodeBlockMarker {
                code: code_content.clone(),
            },
        ))
        .with_children(|parent| {
            // 顶部栏：语言标签 + 复制按钮
            parent
                .spawn((Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    padding: UiRect::all(Val::Px(8.0)),
                    ..default()
                },))
                .with_children(|bar| {
                    // 语言标签
                    if !language.is_empty() {
                        bar.spawn(ui_text(
                            language,
                            type_scale::MICRO,
                            500,
                            theme.text_muted,
                            type_scale::line_height::UI,
                        ));
                    } else {
                        bar.spawn(Node::default());
                    }
                    // 复制按钮
                    bar.spawn((
                        Button,
                        bevy::picking::hover::Hovered::default(),
                        Node {
                            padding: UiRect::all(Val::Px(4.0)),
                            border_radius: BorderRadius::all(Val::Px(4.0)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                        CopyButtonMarker,
                    ))
                    .with_children(|btn| {
                        btn.spawn(ui_text(
                            &tr(loc, "copy"),
                            type_scale::MICRO,
                            500,
                            theme.accent,
                            type_scale::line_height::UI,
                        ));
                    });
                });

            // 代码内容
            parent
                .spawn((Node {
                    padding: UiRect::all(Val::Px(12.0)),
                    ..default()
                },))
                .with_children(|code_area| {
                    code_area.spawn(mono_text(
                        fonts,
                        code,
                        type_scale::CODE,
                        theme.text,
                        type_scale::line_height::CTRL,
                    ));
                });
        })
        .id()
}

/// 渲染粗体。
fn render_bold(commands: &mut Commands, text: &str, theme: &Theme) -> Entity {
    commands
        .spawn((Node::default(),))
        .with_children(|parent| {
            parent.spawn(ui_text(
                text,
                type_scale::BODY,
                600,
                theme.text,
                type_scale::line_height::BODY,
            ));
        })
        .id()
}

/// 渲染链接。
fn render_link(commands: &mut Commands, url: &str, text: &str, theme: &Theme) -> Entity {
    commands
        .spawn((Node::default(),))
        .with_children(|parent| {
            parent.spawn(ui_text(
                &format!("[{}]({})", text, url),
                type_scale::BODY,
                400,
                theme.accent,
                type_scale::line_height::BODY,
            ));
        })
        .id()
}

/// 处理复制按钮点击。
pub fn handle_copy_button_click(
    mut q: Query<(&CopyButtonMarker, &ChildOf), Added<Pressed>>,
    q_code: Query<&CodeBlockMarker>,
    mut clipboard: ResMut<bevy::clipboard::Clipboard>,
) {
    for (_copy, child_of) in q.iter_mut() {
        if let Ok(code_block) = q_code.get(child_of.parent()) {
            let _ = clipboard.set_text(code_block.code.clone());
        }
    }
}
