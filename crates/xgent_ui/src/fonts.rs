//! 字体资源与文本构造器（v7 排版体系）。
//!
//! 全局默认字体为 Inter Variable（由 `xgent_app` 启动时覆盖 `Assets<Font>`
//! 的默认 `AssetId`），未显式指定 `font` 的 [`TextFont`] 自动继承；
//! 等宽文本经 [`UiFonts::mono`] 显式引用。OpenType 特性（cv01/ss03）在
//! 构造器内统一附加，业务代码不感知。
//!
//! 字号/行高取值见 [`crate::theme::type_scale`]，禁止就地发明字号。

use bevy::prelude::*;
use bevy::text::{FontFeatureTag, FontFeatures, FontSize, FontWeight, LineHeight};

/// UI 字体句柄。
#[derive(Resource, Debug, Clone)]
pub struct UiFonts {
    /// 全局默认字体（Inter Variable；weak 句柄指向 `Assets<Font>` 默认 `AssetId`）。
    pub ui: Handle<Font>,
    /// 等宽字体（macOS Menlo；非 macOS 为默认句柄兜底）。
    pub mono: Handle<Font>,
}

/// Inter 的字面特征（cv01/ss03——Linear 视觉标识的一部分）。
fn inter_font_features() -> FontFeatures {
    FontFeatures::builder()
        .enable(FontFeatureTag::new(b"cv01"))
        .enable(FontFeatureTag::new(b"ss03"))
        .build()
}

/// UI 文本构造器（Inter，随全局默认）——全部业务文本经此，禁止裸拼 `TextFont`。
///
/// `size`/`line_height` 取 [`crate::theme::type_scale`] 档位；`weight` 用
/// 400（正文）/510（强调）/590（宣告）三值。
pub fn ui_text(
    text: impl Into<String>,
    size: f32,
    weight: u16,
    color: Color,
    line_height: f32,
) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont {
            font_size: FontSize::Px(size),
            weight: FontWeight(weight),
            font_features: inter_font_features(),
            ..default()
        },
        TextColor(color),
        LineHeight::RelativeToFont(line_height),
    )
}

/// UI 文本**样式**构造器（不含 `Text` 组件）——给可编辑文本（`EditableText`）用。
///
/// 可编辑文本的内容由编辑器自己驱动（`EditableText` + `visible_lines`）。若同时
/// 挂 `Text`，bevy 的两个系统会争同一个 `ContentSize`（`measure_text_system` 按
/// `Text` 量、`update_editable_text_content_size` 按 `visible_lines` 量），
/// 高度会时对时错。可编辑文本节点因此只给样式，不给 `Text`。
pub fn ui_text_style(size: f32, weight: u16, color: Color, line_height: f32) -> impl Bundle {
    (
        TextFont {
            font_size: FontSize::Px(size),
            weight: FontWeight(weight),
            font_features: inter_font_features(),
            ..default()
        },
        TextColor(color),
        LineHeight::RelativeToFont(line_height),
    )
}

/// 单行省略文本构造器（尾部省略）。
///
/// 与 [`ui_text`] 的差别：额外挂 `TextLayout{NoWrap}` 与 `xui::Ellipsis`——
/// 节点宽度不够时渲染成 `前缀…`（而不是溢出或换行）。
///
/// 节点仍需**确定宽度**才会截断（固定宽 / 百分比 / flex 收缩后的宽度）；
/// 文本节点本身要能收缩：`flex_shrink: 1` + `min_width: Val::ZERO`。
pub fn ellipsis_text(
    source: impl Into<String>,
    size: f32,
    weight: u16,
    color: Color,
    line_height: f32,
) -> impl Bundle {
    let source = source.into();
    (
        Text::new(source.clone()),
        TextFont {
            font_size: FontSize::Px(size),
            weight: FontWeight(weight),
            font_features: inter_font_features(),
            ..default()
        },
        TextColor(color),
        LineHeight::RelativeToFont(line_height),
        TextLayout {
            linebreak: LineBreak::NoWrap,
            ..default()
        },
        xui::Ellipsis::new(source),
    )
}

/// 单行省略文本构造器（中段省略，路径类：保留首段与末两段）。
pub fn ellipsis_path_text(
    source: impl Into<String>,
    size: f32,
    weight: u16,
    color: Color,
    line_height: f32,
) -> impl Bundle {
    let source = source.into();
    (
        Text::new(source.clone()),
        TextFont {
            font_size: FontSize::Px(size),
            weight: FontWeight(weight),
            font_features: inter_font_features(),
            ..default()
        },
        TextColor(color),
        LineHeight::RelativeToFont(line_height),
        TextLayout {
            linebreak: LineBreak::NoWrap,
            ..default()
        },
        xui::Ellipsis::mid(source),
    )
}

/// 等宽文本构造器（带字重，等宽标签/强调用）。
pub fn mono_text_weighted(
    fonts: &UiFonts,
    text: impl Into<String>,
    size: f32,
    weight: u16,
    color: Color,
    line_height: f32,
) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont {
            font: fonts.mono.clone().into(),
            font_size: FontSize::Px(size),
            weight: FontWeight(weight),
            ..default()
        },
        TextColor(color),
        LineHeight::RelativeToFont(line_height),
    )
}

/// 等宽文本构造器（显式引用 [`UiFonts::mono`]）。
pub fn mono_text(
    fonts: &UiFonts,
    text: impl Into<String>,
    size: f32,
    color: Color,
    line_height: f32,
) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont {
            font: fonts.mono.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        LineHeight::RelativeToFont(line_height),
    )
}
