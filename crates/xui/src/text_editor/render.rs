//! 渲染系统：行号列、高亮显示层、光标条。
//!
//! 详见 `doc/design/editor-design.md` 2.2 节。
//!
//! # 架构
//!
//! 每个编辑器实体下挂三个子节点（由调用方在 spawn 时建好，或由本模块同步）：
//! - `LineNumbersMarker`：行号列（右侧对齐，text_dim 色）
//! - `HighlightLayerMarker`：只读高亮显示层（`Text` + `TextSpan` 子树，按 spans 重建）
//! - `CursorBarMarker`：光标矩形（独立节点，定位到光标像素位置）
//!
//! 编辑态：`HighlightLayerMarker` 隐藏（`Visibility::Hidden`），
//! 输入由 `EditableText` 单色处理；只读态：`HighlightLayerMarker` 显示，
//! `EditableText` 隐藏。
//!
//! 行号与光标条两种模式都渲染。

use bevy::prelude::*;

use crate::text_editor::highlight::{HighlightSpan, SpanKind};
use crate::text_editor::{HighlightCache, TextEditor};

/// 行号列标记（含 `Text` 组件的实体）。
#[derive(Component, Default)]
pub struct LineNumbersMarker;

/// 高亮显示层标记（含 `Text` 组件，按 spans 重建子 span）。
#[derive(Component, Default)]
pub struct HighlightLayerMarker;

/// 光标条标记（矩形节点）。
#[derive(Component, Default)]
pub struct CursorBarMarker;

/// 每个编辑器的渲染子节点句柄。
#[derive(Component, Default)]
pub struct TextEditorChildren {
    /// 行号列实体
    pub line_numbers: Option<Entity>,
    /// 高亮层实体
    pub highlight_layer: Option<Entity>,
    /// 光标条实体
    pub cursor_bar: Option<Entity>,
}

/// 同步高亮显示层：根据 `TextEditor::spans` 重建 `TextSpan` 子树。
///
/// 只在 spans 变化（`HighlightCache` 哈希变）时重建。MVP 全量重建（despawn 旧 span、
/// spawn 新 span）；增量更新留待后续。
pub fn sync_highlight_layer(
    mut q: Query<(&TextEditor, &HighlightCache, &mut TextEditorChildren), Changed<HighlightCache>>,
    q_layer: Query<&Text, With<HighlightLayerMarker>>,
    mut commands: Commands,
) {
    for (editor, _cache, children) in q.iter_mut() {
        let Some(layer) = children.highlight_layer else {
            continue;
        };
        if q_layer.get(layer).is_err() {
            continue;
        }
        // 先清空旧 span
        commands.entity(layer).despawn_children();
        // 重建：用 spans 切分文本，每段一个 TextSpan
        // 需要原始文本——从 spans 不能反推，故从 layer 上的 Text 取
        let Ok(text_comp) = q_layer.get(layer) else {
            continue;
        };
        let full = text_comp.0.clone();
        let mut spans = editor.spans.clone();
        if spans.is_empty() {
            spans.push(HighlightSpan {
                start: 0,
                end: full.len(),
                kind: SpanKind::Plain,
            });
        }
        commands.entity(layer).with_children(|p| {
            for s in &spans {
                let end = s.end.min(full.len());
                if s.start >= end {
                    continue;
                }
                let segment = full[s.start..end].to_string();
                let color = crate::text_editor::highlight::span_color_for(s.kind);
                p.spawn((TextSpan::new(segment), TextColor(color)));
            }
        });
    }
}
/// 编辑器主题（颜色 + 字号），由宿主注入。xui 不依赖 xgent_ui 的 Theme。
#[derive(Resource, Debug, Clone)]
pub struct EditorTheme {
    /// 主文本色
    pub text: Color,
    /// 次要文本色（行号等）
    pub text_dim: Color,
    /// 字体大小
    pub font_size: f32,
}

impl Default for EditorTheme {
    fn default() -> Self {
        Self {
            text: Color::WHITE,
            text_dim: Color::srgb(0.62, 0.64, 0.68),
            font_size: 14.0,
        }
    }
}

/// 更新行号列：根据当前文本行数重建。
///
/// 只在文本变化（`HighlightCache` 变）时重建。
pub fn update_line_numbers(
    q: Query<
        (
            &TextEditor,
            &HighlightCache,
            &TextEditorChildren,
            &bevy::text::EditableText,
        ),
        Changed<HighlightCache>,
    >,
    mut q_num: Query<&mut Text, With<LineNumbersMarker>>,
) {
    for (_editor, _cache, children, editable) in q.iter() {
        let Some(num_entity) = children.line_numbers else {
            continue;
        };
        let Ok(mut num_text) = q_num.get_mut(num_entity) else {
            continue;
        };
        let text = editable.value().to_string();
        let line_count = text.lines().count().max(1);
        let nums = (1..=line_count)
            .map(|i| format!("{i:>4}"))
            .collect::<Vec<_>>()
            .join("\n");
        *num_text = Text::new(nums);
    }
}

/// 更新光标条位置。
///
/// MVP：光标条宽度固定（2px），高度 = 行高。位置由 `TextEditor::cursor` 决定，
/// 简化为按 (cursor.0-1) * line_height 定位 y，x 由列粗估（列宽 = 行高 × 0.6）。
/// 精确像素定位需访问 `TextLayoutInfo`（留待后续）。
pub fn update_cursor_bar(
    q: Query<(&TextEditor, &TextEditorChildren), Changed<TextEditor>>,
    mut q_bar: Query<&mut Node, With<CursorBarMarker>>,
) {
    for (editor, children) in q.iter() {
        let Some(bar) = children.cursor_bar else {
            continue;
        };
        let Ok(mut node) = q_bar.get_mut(bar) else {
            continue;
        };
        let (line, col) = editor.cursor;
        let lh = editor.line_height;
        let col_w = lh * 0.6;
        node.top = Val::Px((line.saturating_sub(1)) as f32 * lh);
        node.left = Val::Px((col.saturating_sub(1)) as f32 * col_w);
        node.height = Val::Px(lh);
        node.width = Val::Px(2.0);
    }
}

#[cfg(test)]
mod tests {
    use crate::text_editor::highlight::{SpanKind, span_color_for};

    #[test]
    fn span_color_plain_is_white() {
        let c = span_color_for(SpanKind::Plain);
        let white = bevy::color::palettes::css::WHITE.into();
        assert_eq!(c, white);
    }

    #[test]
    fn span_color_keyword_is_not_white() {
        let c = span_color_for(SpanKind::Keyword);
        let white = bevy::color::palettes::css::WHITE.into();
        assert_ne!(c, white);
    }
}
