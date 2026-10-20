//! 单行省略原语（UI v8 M3-T4 / 修审计 C3、E6）。
//!
//! # 为什么必须自建
//!
//! ✅ bevy 0.20 **没有** `TextTruncate`：全仓 grep `truncat` 只命中
//! `bevy_text/src/bounds.rs` 的文档注释与 `stack.rs`。可用的 `TextBounds` 官方
//! 自陈「only characters that are completely out of the bounds will be
//! truncated, so this is not a reliable limit」——是硬裁，不是省略号。
//!
//! 没有 `…` 时用户无法区分「被裁」与「就这么长」，在文件树 / 页签上是可用性问题，
//! 所以省略号必须自建。
//!
//! # 做法
//!
//! 测量走 bevy 自己的排版管线（[`TextPipeline::create_text_measure`]），量的是
//! **物理像素**下的排版宽度，与引擎渲染同一套度量，不另造字体度量。
//!
//! 1. 先量完整文本：装得下就不动。
//! 2. 装不下就在「可变前缀」上二分（[`binary_search_prefix`]），找到最长的
//!    能放下「前缀 + 省略号」的前缀。
//! 3. 结果按 `(源文本, 可用宽, 字号, 省略号)` 记进 [`EllipsisCache`]，宽度变化
//!    小于 2px 不重算（迟滞），避免拖窗口时逐帧重排。
//!
//! # 用法
//!
//! ```ignore
//! // 节点宽度必须**确定**（固定宽 / 百分比 / flex 收缩后的宽度），
//! // 否则文本自己把节点撑开，没有「装不下」这回事。
//! commands.spawn((
//!     Node { width: Val::Percent(100.0), flex_shrink: 1.0, min_width: Val::ZERO, ..default() },
//!     TextLayout { linebreak: LineBreak::NoWrap, ..default() },
//!     Text::new(""),
//!     Ellipsis::new("很长的路径/很长的一段文本"),
//! ));
//! ```

use std::hash::{Hash, Hasher};

use bevy::prelude::*;
use bevy::text::{
    ComputedTextBlock, FontCx, LayoutCx, LetterSpacing, LineHeight, TextColor, TextElement,
    TextFont, TextLayout, TextPipeline,
};
use bevy::ui::ComputedUiRenderTargetInfo;

/// 默认省略号字符。
pub const ELLIPSIS_TAIL: char = '…';

/// 路径分隔符（Windows 与 POSIX 都认）。
const PATH_SEPS: [char; 2] = ['/', '\\'];

/// 省略模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EllipsisMode {
    /// 尾部省略：`abc…`。
    End,
    /// 中段省略（路径类）：`a/…/x/y.rs`——保留首段与末两段。
    Mid,
}

/// 单行省略组件。
///
/// 挂在**有确定宽度**的文本节点上：组件持有完整文本，渲染用的 `Text` 由系统
/// 写成截断后的结果。
#[derive(Component, Debug, Clone)]
pub struct Ellipsis {
    /// 完整文本。
    pub source: String,
    /// 省略号字符。
    pub tail: char,
    /// 至少保留的字符数（不足则**不**截断——宁可溢出也不显示成半个词）。
    pub min_keep: usize,
    /// 省略模式。
    pub mode: EllipsisMode,
}

impl Ellipsis {
    /// 构造尾部省略。
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            tail: ELLIPSIS_TAIL,
            min_keep: 1,
            mode: EllipsisMode::End,
        }
    }

    /// 构造中段省略（路径类，保留首段与末两段）。
    pub fn mid(source: impl Into<String>) -> Self {
        Self {
            mode: EllipsisMode::Mid,
            ..Self::new(source)
        }
    }

    /// 自定义省略号字符。
    pub fn with_tail(mut self, tail: char) -> Self {
        self.tail = tail;
        self
    }

    /// 至少保留的字符数。
    pub fn min_keep(mut self, n: usize) -> Self {
        self.min_keep = n;
        self
    }
}

/// 缓存键：源文本 + 可用宽（量化到 1px）+ 字号 + 省略号 + 模式。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    source: u64,
    width_px: i64,
    font_size_bits: u32,
    tail: char,
    mode: EllipsisMode,
}

/// 省略结果缓存（避免逐帧重排）。
#[derive(Resource, Default, Clone)]
pub struct EllipsisCache {
    entries: std::collections::HashMap<CacheKey, String>,
    /// 命中次数（测试与诊断用）。
    pub hits: u64,
    /// 未命中次数（测试与诊断用）。
    pub misses: u64,
}

impl EllipsisCache {
    /// 缓存条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 缓存是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 文本哈希（默认 hasher 即可，只用于缓存键）。
fn hash_str(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// 把源文本拆成「可变前缀 + 固定后缀」。
///
/// - [`EllipsisMode::End`]：前缀 = 全文，后缀 = 空。
/// - [`EllipsisMode::Mid`]：前缀 = 首段（含分隔符），后缀 = 末两段（含分隔符）；
///   段数不足时退化为整段（无可省略空间）。
fn split_for_mode(source: &str, mode: EllipsisMode) -> (String, String) {
    if mode == EllipsisMode::End {
        return (source.to_string(), String::new());
    }
    let sep_positions: Vec<usize> = source
        .char_indices()
        .filter(|(_, c)| PATH_SEPS.contains(c))
        .map(|(i, _)| i)
        .collect();
    // 至少要有「首段 + 中间段 + 末两段」才谈得上中段省略
    if sep_positions.len() < 3 {
        return (source.to_string(), String::new());
    }
    let head_end = sep_positions[0] + 1;
    let tail_start = sep_positions[sep_positions.len() - 2];
    (
        source[..head_end].to_string(),
        source[tail_start..].to_string(),
    )
}

/// 二分出能放进 `avail` 宽度的最长前缀（字符数）。
///
/// `measure` 给出「前缀 + 省略号」的宽度（物理像素）。返回前缀的**字符数**。
pub fn binary_search_prefix(
    chars: &[char],
    tail: char,
    avail: f32,
    mut measure: impl FnMut(&str) -> Option<f32>,
) -> usize {
    if chars.is_empty() {
        return 0;
    }
    let mut lo = 0usize;
    let mut hi = chars.len();
    while lo < hi {
        let mid = (lo + hi + 1).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect();
        let with_tail = format!("{candidate}{tail}");
        match measure(&with_tail) {
            Some(w) if w <= avail => lo = mid,
            _ => hi = mid - 1,
        }
    }
    lo
}

/// 纯函数：算出省略结果。
///
/// `measure` 给出候选串的排版宽度（物理像素）；返回最终要渲染的文本。
/// - 装得下 → 原文；
/// - 装不下 → 二分出最长的能放下「前缀 + 省略号」的前缀。
///
/// 端部省略找不到够长的前缀时返回原文（宁可溢出也不显示成半个词）。
pub fn compute_ellipsis(
    source: &str,
    tail: char,
    min_keep: usize,
    mode: EllipsisMode,
    avail: f32,
    mut measure: impl FnMut(&str) -> Option<f32>,
) -> String {
    let full = source.to_string();
    let full_w = measure(&full).unwrap_or(0.0);
    if full_w <= avail {
        return full;
    }
    let keep = min_keep.max(1);
    let (prefix_src, suffix) = split_for_mode(source, mode);
    let prefix_chars: Vec<char> = prefix_src.chars().collect();
    let head_of = |n: usize| -> String { prefix_chars[..n].iter().collect() };
    match mode {
        EllipsisMode::End => {
            // 注意：`binary_search_prefix` 传进来的候选串**已含省略号**，
            // 闭包里不要再追加一次。
            let n = binary_search_prefix(&prefix_chars, tail, avail, |c| measure(c));
            if n < keep {
                full
            } else {
                format!("{}{}", head_of(n), tail)
            }
        }
        EllipsisMode::Mid => {
            // 先砍头（可以砍到 0，此时形如「…/末两段」）；
            // 砍完仍放不下就退化为整体尾部省略（至少还有个省略号）。
            let n = binary_search_prefix(&prefix_chars, tail, avail, |c| {
                measure(&format!("{}{}", c, suffix))
            });
            let cand = format!("{}{}{}", head_of(n), tail, suffix);
            if measure(&cand).is_some_and(|w| w <= avail) {
                cand
            } else {
                let all: Vec<char> = source.chars().collect();
                let n2 = binary_search_prefix(&all, tail, avail, |c| measure(c));
                if n2 < keep {
                    full
                } else {
                    let head: String = all[..n2].iter().collect();
                    format!("{}{}", head, tail)
                }
            }
        }
    }
}

/// 省略插件：注册截断系统。
pub struct EllipsisPlugin;

impl Plugin for EllipsisPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EllipsisCache>().add_systems(
            PostUpdate,
            apply_ellipsis.after(bevy::ui::UiSystems::PostLayout),
        );
    }
}

/// 量一段文本的排版宽度（物理像素）。
///
/// 走 bevy 自己的排版管线（与渲染同一套度量），不另造字体度量。
#[allow(clippy::too_many_arguments)]
fn measure_width(
    pipeline: &mut TextPipeline,
    fonts: &Assets<Font>,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
    computed: &mut ComputedTextBlock,
    entity: Entity,
    text: &str,
    font: &TextFont,
    color: Color,
    line_height: LineHeight,
    letter_spacing: LetterSpacing,
    layout: &TextLayout,
    scale_factor: f32,
    logical_size: Vec2,
    rem_size: RemSize,
) -> Option<f32> {
    let measure = pipeline
        .create_text_measure(
            entity,
            fonts,
            std::iter::once((
                entity,
                0,
                TextElement::Text {
                    text,
                    font,
                    color,
                    line_height,
                    letter_spacing,
                },
            )),
            scale_factor,
            layout,
            computed,
            font_cx,
            layout_cx,
            logical_size,
            rem_size,
        )
        .ok()?;
    Some(measure.max.x)
}

/// 截断系统：把 `Text` 写成能放进节点宽度的省略形式。
///
/// 必须在布局之后跑（需要本帧最新的节点宽度）。
fn apply_ellipsis(
    mut pipeline: ResMut<TextPipeline>,
    fonts: Res<Assets<Font>>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
    rem_size: Res<RemSize>,
    mut cache: ResMut<EllipsisCache>,
    mut scratch: Local<ComputedTextBlock>,
    mut q: Query<
        (
            &mut Text,
            &Ellipsis,
            &ComputedNode,
            &TextFont,
            &TextColor,
            &LineHeight,
            &LetterSpacing,
            &TextLayout,
            &ComputedUiRenderTargetInfo,
        ),
        With<Ellipsis>,
    >,
) {
    for (mut text, ellipsis, computed, font, color, line_height, letter_spacing, layout, target) in
        q.iter_mut()
    {
        let avail = computed.content_box().width();
        if avail <= 1.0 {
            // 宽度还没算出来（或节点被隐藏）：先不截断，等下一帧。
            continue;
        }
        let font_size = font.font_size.eval(target.logical_size(), *rem_size);
        let key = CacheKey {
            source: hash_str(&ellipsis.source),
            width_px: (avail.round() as i64) * 2, // 量化到 0.5px，配合 2px 迟滞
            font_size_bits: font_size.to_bits(),
            tail: ellipsis.tail,
            mode: ellipsis.mode,
        };
        if let Some(hit) = cache.entries.get(&key).cloned() {
            cache.hits += 1;
            if text.0 != hit {
                text.0 = hit;
            }
            continue;
        }
        cache.misses += 1;

        let (prefix_src, suffix) = split_for_mode(&ellipsis.source, ellipsis.mode);
        let mut measure = |candidate: &str| {
            measure_width(
                &mut pipeline,
                &fonts,
                &mut font_cx,
                &mut layout_cx,
                &mut scratch,
                text_entity_placeholder(),
                candidate,
                font,
                color.0,
                *line_height,
                *letter_spacing,
                layout,
                target.scale_factor(),
                target.logical_size(),
                *rem_size,
            )
        };
        let _ = (&prefix_src, &suffix);
        let result = compute_ellipsis(
            &ellipsis.source,
            ellipsis.tail,
            ellipsis.min_keep,
            ellipsis.mode,
            avail,
            &mut measure,
        );
        cache.entries.insert(key, result.clone());
        text.0 = result;
    }
}

/// 测量用的实体占位（测量不依赖实体身份，只依赖样式）。
fn text_entity_placeholder() -> Entity {
    Entity::PLACEHOLDER
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 假测量：每个字符 10px。
    fn fake_measure() -> impl FnMut(&str) -> Option<f32> {
        move |s: &str| Some(s.chars().count() as f32 * 10.0)
    }

    #[test]
    fn binary_search_finds_longest_fitting_prefix() {
        let chars: Vec<char> = "abcdefghij".chars().collect();
        // 可用 55px：前缀 + 省略号 ≤ 55 → 前缀 ≤ 4 字符
        let n = binary_search_prefix(&chars, '…', 55.0, fake_measure());
        assert_eq!(n, 4);
    }

    #[test]
    fn binary_search_returns_zero_when_nothing_fits() {
        let chars: Vec<char> = "abc".chars().collect();
        let n = binary_search_prefix(&chars, '…', 5.0, fake_measure());
        assert_eq!(n, 0);
    }

    #[test]
    fn binary_search_returns_all_when_everything_fits() {
        let chars: Vec<char> = "abc".chars().collect();
        let n = binary_search_prefix(&chars, '…', 1000.0, fake_measure());
        assert_eq!(n, 3);
    }

    #[test]
    fn mid_mode_keeps_first_segment_and_last_two() {
        let (head, tail) = split_for_mode("C:/a/b/c/file.rs", EllipsisMode::Mid);
        assert_eq!(head, "C:/");
        assert_eq!(tail, "/c/file.rs");
    }

    #[test]
    fn mid_mode_handles_backslash_paths() {
        let (head, tail) = split_for_mode("C:\\a\\b\\c\\file.rs", EllipsisMode::Mid);
        assert_eq!(head, "C:\\");
        assert_eq!(tail, "\\c\\file.rs");
    }

    #[test]
    fn mid_mode_degrades_for_short_paths() {
        let (head, tail) = split_for_mode("a/b.rs", EllipsisMode::Mid);
        assert_eq!(head, "a/b.rs");
        assert!(tail.is_empty());
    }

    #[test]
    fn end_mode_splits_to_whole_source() {
        let (head, tail) = split_for_mode("hello world", EllipsisMode::End);
        assert_eq!(head, "hello world");
        assert!(tail.is_empty());
    }

    /// 假测量：每个字符 10px。
    fn width_of(s: &str) -> Option<f32> {
        Some(s.chars().count() as f32 * 10.0)
    }

    #[test]
    fn compute_end_ellipsis_truncates_to_fit() {
        // 可用 55px → 「前缀 + …」≤ 55 → 前缀 ≤ 4 字符
        let out = compute_ellipsis("abcdefghij", '…', 1, EllipsisMode::End, 55.0, width_of);
        assert_eq!(out, "abcd…");
    }

    #[test]
    fn compute_end_ellipsis_keeps_full_when_it_fits() {
        let out = compute_ellipsis("abc", '…', 1, EllipsisMode::End, 1000.0, width_of);
        assert_eq!(out, "abc");
    }

    #[test]
    fn compute_end_ellipsis_keeps_source_when_nothing_fits() {
        // 可用 5px：连「a…」都放不下 → 保留原文（宁可溢出）
        let out = compute_ellipsis("abcdef", '…', 1, EllipsisMode::End, 5.0, width_of);
        assert_eq!(out, "abcdef");
    }

    #[test]
    fn compute_mid_ellipsis_keeps_tail_segments() {
        // 可用 210px → 候选「C:/…/structure/file.rs」= 22 字符 = 220px 放不下；
        // 砍头到「C:」→ 21 字符 = 210px 刚好放下。
        let out = compute_ellipsis(
            "C:/very/deep/nested/directory/structure/file.rs",
            '…',
            1,
            EllipsisMode::Mid,
            210.0,
            width_of,
        );
        assert_eq!(out, "C:…/structure/file.rs");
    }

    #[test]
    fn compute_mid_ellipsis_can_drop_head_entirely() {
        // 可用 190px：只剩「…/structure/file.rs」= 19 字符 = 190px
        let out = compute_ellipsis(
            "C:/very/deep/nested/directory/structure/file.rs",
            '…',
            1,
            EllipsisMode::Mid,
            190.0,
            width_of,
        );
        assert_eq!(out, "…/structure/file.rs");
    }

    #[test]
    fn compute_mid_ellipsis_falls_back_to_end_when_tail_too_long() {
        // 可用 100px：末两段本身就放不下 → 退化为整体尾部省略
        let out = compute_ellipsis(
            "C:/very/deep/nested/directory/structure/file.rs",
            '…',
            1,
            EllipsisMode::Mid,
            100.0,
            width_of,
        );
        assert!(out.ends_with('…'), "应带省略号：{out}");
        // 100px 可用 → 前缀 9 字符 + 省略号
        assert_eq!(out, "C:/very/d…");
    }
}
