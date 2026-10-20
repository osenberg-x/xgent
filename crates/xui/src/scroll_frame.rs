//! 滚动帧：可滚动区域的**唯一入口**（UI v8 M3-T1）。
//!
//! # 为什么需要它
//!
//! 审计 C1：业务层有 9 处用 `Overflow::clip_y()` 配 `ScrollPosition`。但 bevy 的
//! `ScrollPosition` 只在 `OverflowAxis::Scroll` 轴生效，`clip_y` 轴上的偏移会被
//! 布局系统直接丢弃（`bevy_ui/src/layout/mod.rs:476-491`），于是这些容器
//! 「看起来能滚、实际滚不动」，内容被硬裁。
//!
//! 修法不是逐处改 `clip_y` → `scroll_y`（那是靠 review 保证的纪律），而是把
//! 「滚动区域」收敛成一个类型：**滚动语义只由构造器决定，外部改不动**。
//!
//! # 三条保证
//!
//! 1. **overflow 由构造器决定**：[`ScrollFrame::vertical`] / [`ScrollFrame::horizontal`]
//!    写死轴向，`enforce_scroll_frame_overflow` 系统在布局前把 `Node.overflow`
//!    回写成构造器的值——调用方即便改了也会被改回来（类型 + 运行时双保险）。
//! 2. **视口与滚动条成套产出**：bundle 自带 [`Scrollbar`]，轨道节点固定在视口
//!    内（[`IgnoreScroll`]），不再随内容滚走。
//! 3. **定位子项有唯一入口**：[`scroll_to_child`] 最小调整 `ScrollPosition`
//!    让指定子项进入视口（对应 zed `ScrollHandle::scroll_to_item`）。
//!
//! # 时序
//!
//! 贴底维护、贴底跟随、overflow 回写均在布局前后合适的位置运行：
//! - overflow 回写：`PostUpdate` 且 `.before(UiSystems::Layout)`（必须在布局前）。
//! - 贴底维护/跟随：`PostUpdate` 的 `UiSystems::PostLayout` 之后——此时
//!   `ComputedNode.content_size` / `size` 为本帧最新值，`max_offset` 才正确。

use bevy::prelude::*;
use bevy::ui::IgnoreScroll;

use crate::mouse_wheel_scroll::MouseWheelScrolled;
use crate::scrollbar::Scrollbar;

/// 滚动帧标记：记录构造器决定的滚动轴向。
///
/// 系统的查询过滤器用；业务代码一般不需要直接读它（要读滚动位置请读
/// [`ScrollPosition`]）。
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollFrameMarker {
    /// 横向溢出行为
    pub x: OverflowAxis,
    /// 纵向溢出行为
    pub y: OverflowAxis,
}

impl ScrollFrameMarker {
    /// 构造器决定、应写回 `Node.overflow` 的值。
    pub fn overflow(&self) -> Overflow {
        Overflow {
            x: self.x,
            y: self.y,
        }
    }
}

/// 可滚动区域契约 Bundle。
///
/// 预置正确滚动所需的全部节点样式：
/// - `Overflow { x: Hidden, y: Scroll }`：让 `ScrollPosition` 在纵向生效；
///   `x: Hidden` 影响布局+裁剪，防宽内容撑破挤占兄弟节点。
/// - `min_height: Val::ZERO` + `flex_grow: 1.0` + `flex_shrink: 1.0` +
///   `flex_basis: Val::ZERO`：防 flex 主轴被子内容撑到内容高度（否则
///   `size.y ≈ content_size.y`、`max_offset ≈ 0`、滚轮无效）。
/// - [`Scrollbar`]：滚动条与视口成套产出。
///
/// 样式（padding / gap / 背景等）经 [`ScrollFrame::style`] 调整；`overflow`
/// 由构造器决定，外部修改会被 [`ScrollFramePlugin`] 的回写系统改回。
#[derive(Bundle)]
pub struct ScrollFrame {
    node: Node,
    scroll_position: ScrollPosition,
    marker: ScrollFrameMarker,
    scrollbar: Scrollbar,
}

impl ScrollFrame {
    /// 纵向滚动容器（最常用）。`flex_grow` 默认 1.0 填充父容器剩余空间。
    pub fn vertical() -> Self {
        Self::with_axes(OverflowAxis::Hidden, OverflowAxis::Scroll)
    }

    /// 横向滚动容器。
    pub fn horizontal() -> Self {
        Self::with_axes(OverflowAxis::Scroll, OverflowAxis::Hidden)
    }

    /// 纵向滚动 + 贴底跟随（会话区 / 终端输出区）。
    ///
    /// 返回「帧 + 贴底组件」二元组，spawn 时一起挂上：
    /// ```ignore
    /// let (frame, stick) = ScrollFrame::stick_bottom();
    /// commands.spawn((frame, stick));
    /// ```
    pub fn stick_bottom() -> (Self, StickToBottom) {
        (Self::vertical(), StickToBottom::default())
    }

    fn with_axes(x: OverflowAxis, y: OverflowAxis) -> Self {
        Self {
            node: Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                flex_shrink: 1.0,
                flex_basis: Val::ZERO,
                flex_direction: FlexDirection::Column,
                // 防撑破：主轴 + 交叉轴都允许收缩到 0。
                min_height: Val::ZERO,
                min_width: Val::ZERO,
                overflow: Overflow { x, y },
                ..default()
            },
            scroll_position: ScrollPosition::default(),
            marker: ScrollFrameMarker { x, y },
            scrollbar: Scrollbar::default(),
        }
    }

    /// 访问节点样式（padding / gap / 背景 / 边框……）。
    ///
    /// `overflow` 虽在此暴露，但外部修改无效——[`ScrollFramePlugin`] 的回写系统
    /// 会在布局前把它改回构造器决定的值。
    pub fn style(&mut self) -> &mut Node {
        &mut self.node
    }

    /// 只读访问节点样式。
    pub fn node(&self) -> &Node {
        &self.node
    }

    /// 取消「填满父容器」默认（`flex_grow` 归 0、`flex_basis` 回 `Auto`）。
    ///
    /// 用于「高度随内容、超出才滚动」的容器（如设置面板的模型列表），
    /// 避免它们在父列里被拉成固定高度。
    pub fn content_sized(&mut self) -> &mut Self {
        self.node.flex_grow = 0.0;
        self.node.flex_basis = Val::Auto;
        self
    }

    /// 自定义滚动条外观（颜色 / 宽度）。
    pub fn scrollbar(&mut self, scrollbar: Scrollbar) -> &mut Self {
        self.scrollbar = scrollbar;
        self
    }
}

/// 贴底跟随组件：挂在滚动帧上，内容增长时自动滚到底部，
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

/// 滚动帧插件：注册 overflow 回写与贴底跟随系统。
///
/// 滚轮桥接由 [`crate::mouse_wheel_scroll::MouseWheelScrollPlugin`] 提供。
pub struct ScrollFramePlugin;

impl Plugin for ScrollFramePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (
                enforce_scroll_frame_overflow.before(bevy::ui::UiSystems::Layout),
                (maintain_stick_to_bottom, auto_scroll_to_bottom)
                    .after(bevy::ui::UiSystems::PostLayout)
                    .chain(),
            ),
        );
    }
}

/// 把 `Node.overflow` 回写成构造器决定的值。
///
/// 「overflow 由构造器决定、不接受外部覆盖」的运行时保证：调用方改了也会被
/// 改回来。只在值确实不同时才写，避免每帧触发 change detection。
fn enforce_scroll_frame_overflow(mut q: Query<(&mut Node, &ScrollFrameMarker), Changed<Node>>) {
    for (mut node, marker) in q.iter_mut() {
        let want = marker.overflow();
        if node.overflow != want {
            node.overflow = want;
        }
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
    mut q: Query<(&mut StickToBottom, &ScrollPosition, &ComputedNode), With<ScrollFrameMarker>>,
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
    mut q: Query<(&StickToBottom, &mut ScrollPosition, &ComputedNode), With<ScrollFrameMarker>>,
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

/// 消息：把指定子项滚进视口。
///
/// 用于「光标行必须可见」这类场景（长输入框内部滚动、跳转到指定行）。
/// 发出消息的系统不需要知道滚动怎么算。
#[derive(Message, Debug, Clone, Copy)]
pub struct ScrollToChild {
    /// 滚动容器实体（须带 [`ScrollFrameMarker`]）
    pub container: Entity,
    /// 目标子项实体
    pub child: Entity,
}

/// 纯函数：算出把子项滚进视口所需的偏移（逻辑像素）。
///
/// - `viewport_height`：容器可视高度
/// - `content_height`：容器内容总高
/// - `child_top`：子项顶边相对容器内容原点的偏移
/// - `child_height`：子项高度
/// - `current`：当前滚动偏移
///
/// 返回新的滚动偏移（已钳到 `[0, content_height - viewport_height]`）。
pub fn compute_scroll_to_child(
    viewport_height: f32,
    content_height: f32,
    child_top: f32,
    child_height: f32,
    current: f32,
) -> f32 {
    let max_offset = (content_height - viewport_height).max(0.0);
    let child_bottom = child_top + child_height;
    let want = if child_top < current {
        // 子项在视口上方：向上滚到它贴住视口顶。
        child_top
    } else if child_bottom > current + viewport_height {
        // 子项在视口下方：向下滚到它贴住视口底。
        child_bottom - viewport_height
    } else {
        // 已经完整可见：不动。
        current
    };
    want.clamp(0.0, max_offset)
}

/// 处理 [`ScrollToChild`]：最小调整 `ScrollPosition` 让目标子项进入视口。
///
/// 必须在布局之后跑（需要本帧最新的 `ComputedNode` 与 `UiGlobalTransform`）。
fn handle_scroll_to_child(
    mut reader: MessageReader<ScrollToChild>,
    mut q_scroll: Query<&mut ScrollPosition>,
    q_computed: Query<&ComputedNode>,
    q_transform: Query<&UiGlobalTransform>,
) {
    for msg in reader.read() {
        let Ok(mut scroll) = q_scroll.get_mut(msg.container) else {
            continue;
        };
        let Ok(container_node) = q_computed.get(msg.container) else {
            continue;
        };
        let Ok(child_node) = q_computed.get(msg.child) else {
            continue;
        };
        let Ok(container_transform) = q_transform.get(msg.container) else {
            continue;
        };
        let Ok(child_transform) = q_transform.get(msg.child) else {
            continue;
        };

        let scale = container_node.inverse_scale_factor();
        // `UiGlobalTransform.translation` 是节点**中心**的物理像素坐标，
        // 故子项顶边 = 中心 - 自身高度/2；容器内容原点 = 容器中心 - 容器高度/2。
        let child_top_physical = (child_transform.translation.y - child_node.size().y * 0.5)
            - (container_transform.translation.y - container_node.size().y * 0.5)
            + container_node.scroll_position.y;
        let child_top = child_top_physical * scale;
        let child_height = child_node.size().y * scale;
        let viewport_height = container_node.size().y * scale;
        let content_height = container_node.content_size().y * scale;

        let want = compute_scroll_to_child(
            viewport_height,
            content_height,
            child_top,
            child_height,
            scroll.0.y,
        );
        if (want - scroll.0.y).abs() > f32::EPSILON {
            scroll.0.y = want;
        }
    }
}

/// 滚动帧插件的第二部分：把 [`ScrollToChild`] 消息接上。
///
/// 单独拆出来是为了让「贴底跟随」与「定位子项」两套系统各自可测。
pub struct ScrollToChildPlugin;

impl Plugin for ScrollToChildPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ScrollToChild>().add_systems(
            PostUpdate,
            handle_scroll_to_child.after(bevy::ui::UiSystems::PostLayout),
        );
    }
}

/// 滚动条轨道必须固定在视口内（不随内容滚走）。
///
/// bevy 的布局会把父容器的滚动偏移施加给**所有**子节点（含绝对定位的），
/// 所以轨道节点要显式带 [`IgnoreScroll`] 才能停在视口边缘。
pub(crate) fn track_ignore_scroll() -> IgnoreScroll {
    IgnoreScroll(bevy::math::BVec2::TRUE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_frame_scrolls_y_and_hides_x() {
        let f = ScrollFrame::vertical();
        assert_eq!(f.node().overflow.y, OverflowAxis::Scroll);
        assert_eq!(f.node().overflow.x, OverflowAxis::Hidden);
        assert_eq!(f.marker.overflow().y, OverflowAxis::Scroll);
    }

    #[test]
    fn horizontal_frame_scrolls_x_and_hides_y() {
        let f = ScrollFrame::horizontal();
        assert_eq!(f.node().overflow.x, OverflowAxis::Scroll);
        assert_eq!(f.node().overflow.y, OverflowAxis::Hidden);
    }

    #[test]
    fn stick_bottom_returns_frame_and_follow_component() {
        let (frame, stick) = ScrollFrame::stick_bottom();
        assert_eq!(frame.node().overflow.y, OverflowAxis::Scroll);
        assert!(stick.stick);
    }

    #[test]
    fn frame_anti_break_contract() {
        let f = ScrollFrame::vertical();
        let n = f.node();
        assert_eq!(n.min_height, Val::ZERO);
        assert_eq!(n.min_width, Val::ZERO);
        assert_eq!(n.flex_grow, 1.0);
        assert_eq!(n.flex_shrink, 1.0);
        assert_eq!(n.flex_basis, Val::ZERO);
        assert_eq!(n.flex_direction, FlexDirection::Column);
    }

    #[test]
    fn content_sized_cancels_fill_defaults() {
        let mut f = ScrollFrame::vertical();
        f.content_sized();
        assert_eq!(f.node().flex_grow, 0.0);
        assert_eq!(f.node().flex_basis, Val::Auto);
    }

    #[test]
    fn style_keeps_overflow_constructor_owned() {
        // 调用方改了 overflow，标记仍记着构造器的值（回写系统据此改回）。
        let mut f = ScrollFrame::vertical();
        f.style().overflow = Overflow::clip_y();
        assert_eq!(f.marker.overflow().y, OverflowAxis::Scroll);
    }

    #[test]
    fn scroll_to_child_scrolls_down_when_child_below_viewport() {
        // 视口 100，内容 500，当前在顶部；子项顶边 300、高 20 → 需滚到 220。
        let want = compute_scroll_to_child(100.0, 500.0, 300.0, 20.0, 0.0);
        assert!((want - 220.0).abs() < 1e-4, "实际 {want}");
    }

    #[test]
    fn scroll_to_child_scrolls_up_when_child_above_viewport() {
        let want = compute_scroll_to_child(100.0, 500.0, 40.0, 20.0, 200.0);
        assert!((want - 40.0).abs() < 1e-4, "实际 {want}");
    }

    #[test]
    fn scroll_to_child_keeps_position_when_already_visible() {
        let want = compute_scroll_to_child(100.0, 500.0, 120.0, 20.0, 100.0);
        assert!((want - 100.0).abs() < 1e-4, "实际 {want}");
    }

    #[test]
    fn scroll_to_child_clamps_to_max_offset() {
        // 子项顶边 480 + 高 20 = 500，超出内容高 → 钳到 400。
        let want = compute_scroll_to_child(100.0, 500.0, 480.0, 20.0, 0.0);
        assert!((want - 400.0).abs() < 1e-4, "实际 {want}");
    }

    #[test]
    fn scroll_to_child_zero_when_content_fits() {
        let want = compute_scroll_to_child(100.0, 80.0, 10.0, 20.0, 0.0);
        assert_eq!(want, 0.0);
    }
}
