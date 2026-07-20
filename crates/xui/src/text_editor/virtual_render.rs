//! 虚拟化行渲染（单层 Text 流式布局）。
//!
//! # 架构
//!
//! 放弃"每逻辑行一个绝对定位行容器"模型——该模型在长行软换行时崩溃
//! （一逻辑行被 parley 布局成多个视觉行，固定行高的行容器装不下，
//!  溢出到下一逻辑行造成覆盖，见 row=2 text_layout_h=48 > container_h=44）。
//!
//! 新模型：**单层 Text 流式布局**。
//! - `VirtualContentMarker` 占位节点撑高滚动范围（`height = line_count × line_height`）
//! - 其下挂一个 `VirtualTextMarker`（Text 节点），内容是可见行区间 `[start, end)`
//!   的所有行拼接（行间 `\n`），带 TextSpan 高亮
//! - Text 节点挂 `LineHeight::Px(line_height)`，parley 给每行精确像素高度
//!   软换行的视觉行也在该行行盒内，不溢出到下一逻辑行——这是消除行间覆盖的关键
//! - Text 节点 `position: absolute, top = start × line_height`（正值，Text 放在
//!   content 内的 start 行位置），配合 `ScrollPosition` 上移 scroll_y 让 start 行对齐视口顶
//!
//! 性能：只渲染可见行区间（视口高度/行高 + 2×overscan），内存 O(可见行)。
//! 滚动时若区间不变则只更新 `top` 偏移，区间变化才重建 TextSpan 子树。
//!
//! 参考：helix-tui `Paragraph` widget + `WordWrapper`——单层文本流式布局，
//! 软换行由布局引擎处理，行高由 `LineHeight` 精确控制，无逐行独立容器。

use crate::text_editor::HighlightCache;
use crate::text_editor::TextEditor;
use crate::text_editor::highlight::{HighlightSpan, span_color_for, spans_for_line};
use crate::text_editor::render::EditorTheme;
use bevy::prelude::*;
use bevy::text::LineHeight;

/// 滚动内容占位节点标记（撑高滚动范围，其下挂可见行 Text 节点）。
#[derive(Component, Default)]
pub struct VirtualContentMarker;

/// 可见行 Text 节点标记（含 `Text` 组件，内容为可见行区间拼接）。
///
/// 挂在 `VirtualContentMarker` 下，`position: absolute`，`top` 随滚动偏移。
#[derive(Component, Default)]
pub struct VirtualTextMarker {
    /// 当前渲染的区间起始行（0-based）。滚动时若 start 不变则只移 top，不变内容。
    pub start_row: usize,
    /// 当前渲染的区间结束行（exclusive）。与 start_row 一起判断是否需重建内容。
    pub end_row: usize,
}

/// 可见行 overscan 缓冲行数（上下各加），减少快速滚动时的空白。
const OVERSCAN: usize = 4;

/// 计算可见行区间（纯函数，便于测试）。
///
/// `scroll_y` / `viewport_h` 单位：逻辑像素。`line_height`：行高。
/// 返回 `(start_row, end_row_exclusive)`，已 clamp 到 `[0, line_count]`。
pub fn visible_row_range(
    line_count: usize,
    line_height: f32,
    viewport_h: f32,
    scroll_y: f32,
) -> (usize, usize) {
    if line_count == 0 || line_height <= 0.0 || viewport_h <= 0.0 {
        return (0, 0);
    }
    let start = ((scroll_y / line_height).floor() as isize).max(0) as usize;
    let visible_raw = (viewport_h / line_height).ceil() as usize + 1;
    let start = start.saturating_sub(OVERSCAN);
    let end = (start + visible_raw + 2 * OVERSCAN).min(line_count);
    (start, end)
}

/// 每帧更新虚拟化行渲染：
/// 1. 更新占位节点高度 = `rope.len_lines() * line_height`（撑滚动范围）
/// 2. 计算可见行区间
/// 3. 若区间变化 → 重建 VirtualTextMarker 的 TextSpan 子树（可见行拼接 + 高亮）
/// 4. 始终更新 Text 节点的 `top` 偏移 = `-(start × line_height)`（让可见行对齐视口）
pub fn update_virtual_lines(
    mut q: Query<
        (&mut TextEditor, &ScrollPosition, &ComputedNode, &Children),
        With<HighlightCache>,
    >,
    mut q_content: Query<&mut Node, (With<VirtualContentMarker>, Without<VirtualTextMarker>)>,
    q_text_marker: Query<&VirtualTextMarker>,
    mut q_text_mut: Query<&mut Node, (With<VirtualTextMarker>, Without<VirtualContentMarker>)>,
    q_content_children: Query<&Children, With<VirtualContentMarker>>,
    q_text_children: Query<&Children, With<VirtualTextMarker>>,
    q_span: Query<&TextSpan>,
    mut commands: Commands,
    theme: Res<EditorTheme>,
) {
    for (mut editor, scroll, node, children) in q.iter_mut() {
        // 行高：font_size × 1.6 派生，CJK 字体度量裕量充足。
        // 关键：LineHeight::Px(line_height) 让 parley 给每行精确像素高度，
        // 软换行的视觉行也在该行行盒内，不溢出到下一逻辑行——
        // 这是消除行间覆盖的关键（旧模型固定行容器高度，软换行溢出覆盖下一行）。
        let target_lh = (theme.font_size * 1.6).round();
        if (editor.line_height - target_lh).abs() > 0.01 {
            editor.line_height = target_lh;
        }
        let line_height = editor.line_height;
        let line_count = editor.rope.len_lines();
        // ComputedNode.size 是物理像素，ScrollPosition.y 与 line_height 是逻辑像素，
        // 须乘 inverse_scale_factor 转换，否则 HiDPI 下 visible_raw 算成 2 倍行数。
        let scale = node.inverse_scale_factor();
        let viewport_h = node.size.y * scale;
        let scroll_y = scroll.y;

        // 找占位节点（VirtualContentMarker）
        let mut content_entity: Option<Entity> = None;
        for c in children.iter() {
            if q_content.get(c).is_ok() {
                content_entity = Some(c);
                break;
            }
        }
        let Some(content_entity) = content_entity else {
            continue;
        };

        // 1. 更新占位高度（撑滚动范围）
        if let Ok(mut content_node) = q_content.get_mut(content_entity) {
            let target_h = line_count as f32 * line_height;
            let cur_h = match content_node.height {
                Val::Px(h) => h,
                _ => -1.0,
            };
            if (cur_h - target_h).abs() > 0.5 {
                content_node.height = Val::Px(target_h);
            }
        }

        // 2. 计算可见行区间
        let (start, end) = visible_row_range(line_count, line_height, viewport_h, scroll_y);

        // 3. 找或 spawn VirtualTextMarker 节点
        let mut text_entity: Option<Entity> = None;
        if let Ok(content_children) = q_content_children.get(content_entity) {
            for c in content_children.iter() {
                if q_text_marker.get(c).is_ok() {
                    text_entity = Some(c);
                    break;
                }
            }
        }
        let text_entity = match text_entity {
            Some(e) => e,
            None => {
                // 首次 spawn Text 节点（挂 LineHeight::Px 精确行高）
                let e = commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(0.0),
                            left: Val::Px(0.0),
                            width: Val::Percent(100.0),
                            ..default()
                        },
                        Text::new(String::new()),
                        TextFont {
                            font_size: FontSize::Px(theme.font_size),
                            ..default()
                        },
                        LineHeight::Px(line_height),
                        TextColor(theme.text),
                        VirtualTextMarker {
                            start_row: 0,
                            end_row: 0,
                        },
                    ))
                    .id();
                commands.entity(content_entity).add_child(e);
                e
            }
        };

        // 4. 更新 Text 节点 top 偏移：让 start 行对齐视口顶。
        //
        //    Text 节点是 VirtualContentMarker 的 absolute 子节点，其 `top` 是
        //    相对 content 顶部的位置。bevy 的 ScrollPosition 会把整个 content
        //    （含 absolute 子节点）上移 scroll_y——这是滚动容器的标准行为。
        //
        //    故 Text 节点应放在 content 内的 start 行位置：top = start × line_height
        //    （正值）。配合 ScrollPosition 上移 scroll_y，start 行（位于
        //    start × line_height 处）出现在 start × line_height - scroll_y ≈ 0
        //    （视口顶，因 start ≈ scroll_y / line_height）。
        //
        //    旧代码用负值 -(start × line_height)，导致双重偏移：ScrollPosition
        //    上移 scroll_y + top 上移 start×lh ≈ 2×scroll_y，滚得越多内容飞得越远，
        //    视口变空——"滚动后内容显示不全"的根因。
        let target_top = start as f32 * line_height;
        if let Ok(mut text_node) = q_text_mut.get_mut(text_entity) {
            let cur_top = match text_node.top {
                Val::Px(h) => h,
                _ => f32::NAN,
            };
            if !cur_top.is_finite() || (cur_top - target_top).abs() > 0.5 {
                text_node.top = Val::Px(target_top);
            }
        }

        // 5. 区间变化 → 重建 TextSpan 子树
        let need_rebuild = if let Ok(marker) = q_text_marker.get(text_entity) {
            marker.start_row != start || marker.end_row != end
        } else {
            false
        };
        if need_rebuild {
            // 先 despawn 旧 TextSpan 子节点
            if let Ok(text_children) = q_text_children.get(text_entity) {
                for c in text_children.iter() {
                    if q_span.get(c).is_ok() {
                        commands.entity(c).despawn();
                    }
                }
            }
            // 更新 marker（通过插入覆盖，bevy 0.19 无直接 ComponentMut 单字段改）
            commands.entity(text_entity).insert(VirtualTextMarker {
                start_row: start,
                end_row: end,
            });
            // spawn 新 TextSpan 子节点（可见行拼接 + 高亮 span 切分）
            rebuild_visible_spans(
                &mut commands,
                text_entity,
                &editor.rope,
                &editor.spans,
                start,
                end,
                theme.text,
            );
        }
    }
}

/// 重建可见行区间的 TextSpan 子树。
///
/// 把 rope 的 `[start, end)` 行逐行调 `spans_for_line`，行间插入 `\n`，
/// 生成 `(文本片段, SpanKind)` 序列，每个片段 spawn 为一个 TextSpan 子节点
/// （带对应颜色）。Plain 片段用 default_color。
fn rebuild_visible_spans(
    commands: &mut Commands,
    text_entity: Entity,
    rope: &crate::text_editor::Rope,
    global_spans: &[HighlightSpan],
    start: usize,
    end: usize,
    default_color: Color,
) {
    if start >= end {
        return;
    }
    // 逐行收集 (片段, 颜色)，行间插入换行
    let mut segments: Vec<(String, Color)> = Vec::new();
    for row in start..end {
        let line_text = rope
            .get_line(row)
            .map(|s| s.to_string())
            .unwrap_or_default();
        let line_spans = spans_for_line(global_spans, &line_text, row, rope);
        if line_spans.is_empty() {
            // 空行：放一个空串占位 + 换行（让 parley 产生空行行盒）
            segments.push((String::new(), default_color));
            segments.push(("\n".to_string(), default_color));
        } else {
            for (seg, kind) in line_spans {
                segments.push((seg, span_color_for(kind)));
            }
            // 行尾换行（最后一行除外，避免末尾空行）
            if row + 1 < end {
                segments.push(("\n".to_string(), default_color));
            }
        }
    }
    // spawn TextSpan 子节点。合并相邻同色片段减少节点数。
    let mut buf = String::new();
    let mut cur_color = default_color;
    let mut cur_color_set = false;
    for (seg, color) in segments {
        if !cur_color_set || color != cur_color {
            // flush 前一段
            if !buf.is_empty() {
                let c = cur_color;
                let s = std::mem::take(&mut buf);
                commands
                    .entity(text_entity)
                    .with_child((TextSpan::new(s), TextColor(c)));
            }
            cur_color = color;
            cur_color_set = true;
            buf.push_str(&seg);
        } else {
            buf.push_str(&seg);
        }
    }
    if !buf.is_empty() {
        commands
            .entity(text_entity)
            .with_child((TextSpan::new(buf), TextColor(cur_color)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_range_clamps_to_line_count() {
        let (s, e) = visible_row_range(10, 20.0, 400.0, 0.0);
        assert_eq!(s, 0);
        assert_eq!(e, 10);
    }

    #[test]
    fn visible_range_empty() {
        let (s, e) = visible_row_range(0, 20.0, 400.0, 0.0);
        assert_eq!(s, 0);
        assert_eq!(e, 0);
    }

    #[test]
    fn visible_range_overscan_does_not_underflow() {
        let (s, _e) = visible_row_range(1000, 20.0, 400.0, 10.0);
        assert_eq!(s, 0);
    }

    /// 核心：start_row ≈ scroll_y / line_height（向下取整）。
    /// 这是 Text 节点 `top = start × line_height`（正值）配合 ScrollPosition
    /// 让 start 行对齐视口顶的前提。若此关系被破坏，top 偏移与滚动量错位，
    /// 导致"滚动后内容显示不全"（旧 bug：top 用负值，双重偏移把内容拉飞）。
    #[test]
    fn start_row_tracks_scroll_y_divided_by_line_height() {
        let line_height = 22.0;
        // 滚动 0：start=0（减 overscan clamp 到 0）
        let (s, _) = visible_row_range(1000, line_height, 400.0, 0.0);
        assert_eq!(s, 0);
        // 滚动 3 行：start = 3（减 overscan 4 clamp 到 0）
        let (s, _) = visible_row_range(1000, line_height, 400.0, 3.0 * line_height);
        assert_eq!(s, 0); // 3 - 4(overscan) < 0 → clamp 0
        // 滚动 10 行：start = 10 - 4(overscan) = 6
        let (s, _) = visible_row_range(1000, line_height, 400.0, 10.0 * line_height);
        assert_eq!(s, 6); // 10 - 4 = 6
        // start × line_height ≈ scroll_y（误差 ≤ overscan × line_height）
        // 这保证 top = start × line_height 与 ScrollPosition 上移 scroll_y 对齐
        assert!((s as f32 * line_height - 10.0 * line_height).abs() <= 4.0 * line_height);
    }

    /// 行高派生：line_height 应为 font_size 的 1.6 倍（CJK 安全裕量）。
    #[test]
    fn line_height_derives_from_font_size() {
        let font_size: f32 = 14.0;
        let line_height = (font_size * 1.6_f32).round();
        assert_eq!(line_height, 22.0);
        assert!(line_height >= font_size * 1.4);
    }

    /// 集成：spawn buffer + VirtualContentMarker，跑 update_virtual_lines，
    /// 断言会 spawn VirtualTextMarker 子节点（不 panic）。
    /// MinimalPlugins 无 UI 布局，ComputedNode.size.y 为 0，区间为 (0,0)，
    /// 但 VirtualTextMarker 仍应被 spawn（首次进入分支）。
    #[test]
    fn update_virtual_lines_spawns_text_marker() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<crate::text_editor::render::EditorTheme>()
            .add_systems(Update, update_virtual_lines);

        let content = app
            .world_mut()
            .spawn((Node::default(), VirtualContentMarker))
            .id();
        let _buffer = app
            .world_mut()
            .spawn((
                Node {
                    height: Val::Percent(100.0),
                    overflow: Overflow {
                        x: OverflowAxis::Hidden,
                        y: OverflowAxis::Scroll,
                    },
                    ..default()
                },
                ScrollPosition::default(),
                crate::text_editor::TextEditor {
                    rope: ropey::Rope::from_str("line1\nline2\nline3"),
                    ..Default::default()
                },
                crate::text_editor::HighlightCache::default(),
            ))
            .add_child(content)
            .id();

        app.update();
        app.update();

        // VirtualTextMarker 应被 spawn（首次进入 spawn 分支）
        let count = app
            .world_mut()
            .query::<&VirtualTextMarker>()
            .iter(app.world())
            .count();
        assert_eq!(count, 1, "应 spawn 一个 VirtualTextMarker");
    }
}
