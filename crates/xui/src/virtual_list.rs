//! 虚拟列表（K-02）：大列表只渲染可见项。
//!
//! 官方 `ListBox` / `ScrollArea` 非虚拟，大列表性能不足。`VirtualList` 只渲染可见项。
//!
//! MVP 策略：固定 item 高度。xui 提供 [`VirtualList`] 组件与可见区间计算系统，
//! 把 `first_visible` / `visible_count` 写入组件；实际 item 实体的 spawn/despawn
//! 由调用方业务层读这些字段处理（xui 不绑定具体渲染风格，保持通用）。
//!
//! 用 entity pool 复用节点避免每帧 spawn/despawn 抖动的建议留给调用方实现。

use bevy::prelude::*;

/// 虚拟列表组件。
///
/// 调用方将其挂在滚动容器节点上，并提供 `item_count` 与 `item_height`。
/// 系统每帧根据视口高度与滚动位置更新 `first_visible` 与 `visible_count`。
#[derive(Component, Debug)]
pub struct VirtualList {
    /// 总项数
    pub item_count: usize,
    /// 单项高度（逻辑像素）
    pub item_height: f32,
    /// 第一个可见项下标（由系统更新）
    pub first_visible: usize,
    /// 可见项数量（含缓冲，由系统更新）
    pub visible_count: usize,
}

impl VirtualList {
    /// 构造。
    pub fn new(item_count: usize, item_height: f32) -> Self {
        Self {
            item_count,
            item_height,
            first_visible: 0,
            visible_count: 0,
        }
    }
}

/// item 构造回调 trait（供未来 entity pool 方案使用，MVP 调用方可直接读可见区间自行 spawn）。
pub trait VirtualItemBuilder: Send + Sync {
    /// 为 `index` 项构造子节点并挂到 `parent` 下。
    fn build(&self, commands: &mut Commands, parent: Entity, index: usize);
}

/// 虚拟列表插件。
pub struct VirtualListPlugin;

impl Plugin for VirtualListPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_virtual_list);
    }
}

/// 计算可见项区间（纯函数，便于测试）。
///
/// 返回 `(first_visible, visible_count)`。`viewport_height` 为视口高度（逻辑像素），
/// `scroll_y` 为内容向下滚动的偏移量（即顶部被隐藏的高度）。
/// 多渲染 `overscan` 个缓冲项，减少快速滚动时的空白。
pub fn compute_visible_range(
    item_count: usize,
    item_height: f32,
    viewport_height: f32,
    scroll_y: f32,
    overscan: usize,
) -> (usize, usize) {
    if item_count == 0 || item_height <= 0.0 || viewport_height <= 0.0 {
        return (0, 0);
    }
    let first = ((scroll_y / item_height).floor() as isize).max(0) as usize;
    let visible_raw = (viewport_height / item_height).ceil() as usize + 1;
    let first = first.saturating_sub(overscan);
    let visible = visible_raw + 2 * overscan;
    let last_exclusive = (first + visible).min(item_count);
    (first, last_exclusive.saturating_sub(first))
}

/// 每帧读视口大小与滚动位置，更新可见区间。
fn update_virtual_list(mut q: Query<(&mut VirtualList, &ComputedNode)>) {
    for (mut list, node) in &mut q {
        let (first, count) = compute_visible_range(
            list.item_count,
            list.item_height,
            node.size.y,
            node.scroll_position.y,
            2,
        );
        list.first_visible = first;
        list.visible_count = count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_list_visible_zero() {
        let (f, c) = compute_visible_range(0, 40.0, 800.0, 0.0, 2);
        assert_eq!((f, c), (0, 0));
    }

    #[test]
    fn top_view_shows_first_items_with_overscan() {
        // 800px 视口，40px 每项 → 20 项 + 1，overscan 2 → 上下各 2，共 25
        let (f, c) = compute_visible_range(1000, 40.0, 800.0, 0.0, 2);
        assert_eq!(f, 0);
        // 800/40=20 ceil=20 +1=21 +2*2=25
        assert_eq!(c, 25);
    }

    #[test]
    fn scrolled_view_advances_first() {
        // 滚动 400px → 第 10 项，overscan 后 first=8
        let (f, c) = compute_visible_range(1000, 40.0, 800.0, 400.0, 2);
        assert_eq!(f, 8);
        assert_eq!(c, 25);
    }

    #[test]
    fn near_end_clamps_to_item_count() {
        // 滚动到接近末尾，可见区间不超 item_count
        let (f, c) = compute_visible_range(1000, 40.0, 800.0, 39_000.0, 2);
        assert!(f + c <= 1000);
        assert!(c > 0);
    }

    #[test]
    fn tiny_list_shows_all() {
        let (f, c) = compute_visible_range(5, 40.0, 800.0, 0.0, 2);
        assert_eq!(f, 0);
        assert_eq!(c, 5);
    }
}
