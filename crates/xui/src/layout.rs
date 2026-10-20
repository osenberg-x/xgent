//! 布局契约原语（UI v8 M3-T3）。
//!
//! # 区域填满（`region_fill`）
//!
//! 审计 D2：业务层多处用「有兄弟节点的列里 `height: Val::Percent(100.0)`」表达
//! 「填满剩余空间」。但百分比高度是相对父容器**内容盒**算的，父容器一旦同时
//! 有页签条等固定高度兄弟，子节点就会按「父容器全高」排布，把兄弟挤到视口外
//! ——终端底部状态条永久掉出视口 38px 就是这么来的。
//!
//! 正确写法是 flex 填满：`flex_grow: 1` + `min_width/min_height: ZERO`
//! （允许收缩到 0，避免内容过大时反过来撑破父容器）。
//!
//! 本模块给出**唯一入口**，让「填满」成为可 grep 的契约而不是各处手抄的
//! `height: 100%`。

use bevy::prelude::*;

/// 区域填满的节点样式：填满父容器剩余空间，且允许收缩到 0。
///
/// 典型用法是「列布局里的内容区」——列里同时有固定高度的页签条与状态条，
/// 内容区用本样式占满剩余高度。
pub fn region_fill() -> Node {
    Node {
        flex_grow: 1.0,
        min_width: Val::ZERO,
        min_height: Val::ZERO,
        ..default()
    }
}

/// 把已有节点改成区域填满语义（保留其余样式）。
///
/// 会清掉 `height`（百分比高度是 D2 的病根），其余字段不动。
pub fn fill_region(node: &mut Node) {
    node.flex_grow = 1.0;
    node.min_width = Val::ZERO;
    node.min_height = Val::ZERO;
    node.height = Val::Auto;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_fill_grows_and_allows_shrink_to_zero() {
        let n = region_fill();
        assert_eq!(n.flex_grow, 1.0);
        assert_eq!(n.min_width, Val::ZERO);
        assert_eq!(n.min_height, Val::ZERO);
        // 关键：不是百分比高度
        assert_eq!(n.height, Val::Auto);
    }

    #[test]
    fn fill_region_clears_percentage_height_but_keeps_other_styles() {
        let mut n = Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            padding: UiRect::all(Val::Px(4.0)),
            ..default()
        };
        fill_region(&mut n);
        assert_eq!(n.height, Val::Auto);
        assert_eq!(n.width, Val::Percent(100.0));
        assert_eq!(n.padding, UiRect::all(Val::Px(4.0)));
        assert_eq!(n.flex_grow, 1.0);
    }
}
