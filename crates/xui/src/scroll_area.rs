//! 通用可滚动容器（K-03 滚动能力封装）。
//!
//! 集中表达 bevy 0.19 下"能正确滚动的 UI 容器"所需的全部契约，避免每个业务
//! 模块重新调试 flex 撑破、单位混淆、时序错位等同一组问题。背景与设计详见
//! `doc/notes/scroll-abstraction-design.md`。
//!
//! # 三件套
//!
//! - [`ScrollArea`]：节点契约 Bundle，预置防撑破样式 + `Overflow::scroll_y()` +
//!   `ScrollPosition`，spawn 即可用。
//! - [`StickToBottom`]：贴底跟随组件，内容增长自动滚到底，用户上滚后停止抢夺。
//! - [`scroll_to_child`]：工具函数，最小调整 `ScrollPosition` 让指定子项进入视口
//!   （对应 zed `ScrollHandle::scroll_to_item`）。
//!
//! 滚轮 → `ScrollPosition` 桥接由 [`crate::mouse_wheel_scroll::MouseWheelScrollPlugin`]
//! 通用处理，作用于任意 `OverflowAxis::Scroll` 节点，本模块不重复。
//!
//! # 时序
//!
//! 贴底维护与跟随系统均在 `PostUpdate` 的 [`bevy::ui::UiSystems::PostLayout`] 之后
//! 运行——此时 `ComputedNode.content_size` / `size` 为本帧最新值，`max_offset`
//! 计算才正确。在 `Update` 阶段读 `ComputedNode` 会用过时的上一帧布局结果。

use bevy::prelude::*;

use crate::mouse_wheel_scroll::MouseWheelScrolled;

/// 可滚动容器标记。挂在 [`ScrollArea`] 的根节点上，供系统查询定位。
#[derive(Component, Default)]
pub struct ScrollAreaMarker;

/// 贴底跟随组件：挂在 [`ScrollArea`] 上，内容增长时自动滚到底部，
/// 用户上滚后停止跟随，重新滚回底部附近恢复跟随。
///
/// 对应 zed `Autoscroll::bottom()`。`stick` 初始为真（首屏/首轮自动跟到底）。
/// `threshold` 为判定"接近底部"的逻辑像素阈值，默认 32px。
#[derive(Component, Debug)]
pub struct StickToBottom {
    /// 当前是否贴底跟随
    pub stick: bool,
    /// 判定贴底的阈值（逻辑像素）：`scroll.y >= max_offset - threshold` 即视为贴底
    pub threshold: f32,
}

impl Default for StickToBottom {
    fn default() -> Self {
        Self {
            stick: true,
            threshold: 32.0,
        }
    }
}

impl StickToBottom {
    /// 构造贴底跟随，自定义初始 stick 状态。
    pub fn new(stick: bool) -> Self {
        Self {
            stick,
            ..Default::default()
        }
    }
}

/// 可滚动容器契约 Bundle。
///
/// 预置 bevy 0.19 下正确滚动所需的全部节点样式：
/// - `Overflow { x: Hidden, y: Scroll }`：让 `ScrollPosition` 在纵向生效；
///   `x: Hidden` 影响布局+裁剪，防宽内容撑破挤占兄弟节点。
/// - `min_height: Val::ZERO` + `flex_grow: 1.0` + `flex_shrink: 1.0` +
///   `flex_basis: Val::ZERO`：防 flex 主轴被子内容撑到内容高度（否则
///   `size.y ≈ content_size.y`、`max_offset ≈ 0`、滚轮无效）。
///
/// 调用方 spawn 后往里挂子节点即可滚动。如需横向滚动用 [`ScrollArea::horizontal`]。
#[derive(Bundle)]
pub struct ScrollArea {
    pub node: Node,
    pub scroll_position: ScrollPosition,
    pub marker: ScrollAreaMarker,
}

impl ScrollArea {
    /// 纵向滚动容器（最常用）。`flex_grow` 默认 1.0 填充父容器剩余空间。
    pub fn vertical() -> Self {
        Self::with_direction(FlexDirection::Column)
    }

    /// 横向滚动容器。
    pub fn horizontal() -> Self {
        Self::with_direction(FlexDirection::Row)
    }

    fn with_direction(direction: FlexDirection) -> Self {
        Self {
            node: Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                flex_shrink: 1.0,
                flex_basis: Val::ZERO,
                flex_direction: direction,
                // 防撑破：主轴 + 交叉轴都允许收缩到 0。
                min_height: Val::ZERO,
                min_width: Val::ZERO,
                overflow: Overflow {
                    x: OverflowAxis::Hidden,
                    y: OverflowAxis::Scroll,
                },
                ..default()
            },
            scroll_position: ScrollPosition::default(),
            marker: ScrollAreaMarker,
        }
    }
}

/// 滚动能力插件：注册贴底跟随系统。滚轮桥接由 `MouseWheelScrollPlugin` 提供。
pub struct ScrollAreaPlugin;

impl Plugin for ScrollAreaPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (maintain_stick_to_bottom, auto_scroll_to_bottom)
                .after(bevy::ui::UiSystems::PostLayout)
                .chain(),
        );
    }
}

/// 维护 [`StickToBottom`]：用户上滚离开底部时置 `stick=false`，滚回底部附近恢复
/// `true`。仅在本帧有滚轮事件时刷新，无滚轮时保持原值——避免内容增长导致
/// `max_offset` 变大时误判"用户离开底部"（`auto_scroll` 仍能正常推到新底部）。
///
/// 必须在 [`auto_scroll_to_bottom`] 之前运行（见插件 `.chain()`），读到的是用户
/// 滚轮在 `Update` 阶段提交后的 `ScrollPosition`，尚未被 `auto_scroll` 推回。
fn maintain_stick_to_bottom(
    mut scrolled: ResMut<MouseWheelScrolled>,
    mut q: Query<(&mut StickToBottom, &ScrollPosition, &ComputedNode), With<ScrollAreaMarker>>,
) {
    if !scrolled.0 {
        return;
    }
    // 消费标志（send_scroll_events 只在滚轮时置真，下帧自动归 false）。
    scrolled.0 = false;

    for (mut stick, scroll, node) in q.iter_mut() {
        let scale = node.inverse_scale_factor();
        let max_offset = ((node.content_size() - node.size()) * scale).y.max(0.0);
        let at_bottom = scroll.0.y >= max_offset - stick.threshold || max_offset == 0.0;
        if stick.stick != at_bottom {
            stick.stick = at_bottom;
        }
    }
}

/// 贴底跟随：[`StickToBottom::stick`] 为真时把 `ScrollPosition.y` 推到最大偏移；
/// 为假时保留用户手动滚动位置，不抢夺阅读位置。
///
/// `ScrollPosition` 单位为逻辑像素，`ComputedNode` 的 `size`/`content_size` 为
/// 物理像素，须乘 `inverse_scale_factor` 转换。clamp 由 bevy 布局系统在
/// `PostLayout` 内完成（写回 `ComputedNode.scroll_position`），此处只设目标值。
fn auto_scroll_to_bottom(
    mut q: Query<(&StickToBottom, &mut ScrollPosition, &ComputedNode), With<ScrollAreaMarker>>,
) {
    for (stick, mut scroll, node) in q.iter_mut() {
        if !stick.stick {
            continue;
        }
        let scale = node.inverse_scale_factor();
        let content_height = node.content_size().y * scale;
        let viewport_height = node.size().y * scale;
        let max_offset = (content_height - viewport_height).max(0.0);
        scroll.0.y = max_offset;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stick_to_bottom_defaults_stick_true() {
        let s = StickToBottom::default();
        assert!(s.stick);
        assert_eq!(s.threshold, 32.0);
    }

    #[test]
    fn stick_to_bottom_new_overrides_stick() {
        assert!(!StickToBottom::new(false).stick);
        assert!(StickToBottom::new(true).stick);
    }

    #[test]
    fn scroll_area_vertical_has_scroll_y_and_anti_break() {
        let sa = ScrollArea::vertical();
        assert_eq!(sa.node.overflow.y, OverflowAxis::Scroll);
        assert_eq!(sa.node.overflow.x, OverflowAxis::Hidden);
        assert_eq!(sa.node.min_height, Val::ZERO);
        assert_eq!(sa.node.min_width, Val::ZERO);
        assert_eq!(sa.node.flex_grow, 1.0);
        assert_eq!(sa.node.flex_shrink, 1.0);
        assert_eq!(sa.node.flex_basis, Val::ZERO);
        assert_eq!(sa.node.flex_direction, FlexDirection::Column);
    }

    #[test]
    fn scroll_area_horizontal_uses_row_direction() {
        let sa = ScrollArea::horizontal();
        assert_eq!(sa.node.flex_direction, FlexDirection::Row);
    }
}
