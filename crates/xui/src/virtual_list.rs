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
use std::collections::HashMap;

/// 启用虚拟化的条目数阈值（低于此数直接全量渲染，避免抖动）。
pub const VIRTUAL_LIST_THRESHOLD: usize = 50;

/// 虚拟列表项标记。
///
/// 业务侧把它挂在**渲染出来的每一行**上：系统据此把该行的实测高度写回
/// 列表的行高表（不等高模式的「首帧实测回写」）。
#[derive(Component, Debug, Clone, Copy)]
pub struct VirtualListItem {
    /// 该行对应的数据下标
    pub index: usize,
}

/// 虚拟列表组件。
///
/// 调用方将其挂在滚动容器节点上，并提供 `item_count` 与 `item_height`。
/// 系统每帧根据视口高度与滚动位置更新 `first_visible` 与 `visible_count`。
#[derive(Component, Debug)]
pub struct VirtualList {
    /// 总项数
    pub item_count: usize,
    /// 单项高度（逻辑像素）。固定行高模式直接用它；不等高模式用作**估算值**。
    pub item_height: f32,
    /// 已实测的行高（不等高模式）。未实测的项按 `item_height` 估算。
    pub measured: HashMap<usize, f32>,
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
            measured: HashMap::new(),
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
        app.add_systems(Update, (update_virtual_list, measure_virtual_list_items));
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

/// 构造累积偏移表（不等高模式）。
///
/// `offsets[i]` 是第 `i` 项的顶边，`offsets[count]` 是总高度。未实测的项按
/// `estimate` 估算，实测过的按实测值。
pub fn build_measured_offsets(
    count: usize,
    estimate: f32,
    measured: &HashMap<usize, f32>,
) -> Vec<f32> {
    let mut offsets = Vec::with_capacity(count + 1);
    let mut acc = 0.0f32;
    for i in 0..count {
        offsets.push(acc);
        acc += measured.get(&i).copied().unwrap_or(estimate);
    }
    offsets.push(acc);
    offsets
}

/// 计算可见项区间（不等高模式，纯函数便于测试）。
///
/// `offsets` 为累积偏移表（见 [`build_measured_offsets`]）。返回
/// `(first_visible, visible_count)`。
pub fn compute_visible_range_measured(
    offsets: &[f32],
    viewport_height: f32,
    scroll_y: f32,
    overscan: usize,
) -> (usize, usize) {
    let count = offsets.len().saturating_sub(1);
    if count == 0 || viewport_height <= 0.0 {
        return (0, 0);
    }
    // 第一个顶边 >= scroll_y 的项
    let first = offsets
        .partition_point(|&top| top < scroll_y)
        .saturating_sub(1);
    // 视口底部对应的项
    let bottom = scroll_y + viewport_height;
    let last = offsets.partition_point(|&top| top <= bottom);
    let first = first.saturating_sub(overscan);
    let last = (last + overscan).min(count);
    (first, last.saturating_sub(first))
}

/// 每帧读视口大小与滚动位置，更新可见区间。
fn update_virtual_list(mut q: Query<(&mut VirtualList, &ComputedNode)>) {
    for (mut list, node) in &mut q.iter_mut() {
        // ComputedNode.size / scroll_position 是物理像素，item_height 是逻辑
        // 像素，须乘 inverse_scale_factor 转换——否则 HiDPI（Retina）下
        // first_visible 偏移约一倍、visible_count 约 2 倍，渲染错窗口的 item
        // （同 text_editor/virtual_render.rs、scroll_area.rs 的处理）。
        let scale = node.inverse_scale_factor();
        let viewport_height = node.size().y * scale;
        let scroll_y = node.scroll_position.y * scale;
        let (first, count) = if list.measured.is_empty() {
            compute_visible_range(
                list.item_count,
                list.item_height,
                viewport_height,
                scroll_y,
                2,
            )
        } else {
            let offsets = build_measured_offsets(list.item_count, list.item_height, &list.measured);
            compute_visible_range_measured(&offsets, viewport_height, scroll_y, 2)
        };
        list.first_visible = first;
        list.visible_count = count;
    }
}

/// 把渲染出来的行的**实测高度**写回虚拟列表（不等高模式）。
///
/// 业务侧给每行挂 [`VirtualListItem`]；本系统读该行的 `ComputedNode` 高度
/// 写进列表的 `measured` 表，偏移表随之更准（首帧之后不再靠估算）。
fn measure_virtual_list_items(
    q_items: Query<(&VirtualListItem, &ComputedNode, &ChildOf)>,
    mut q_lists: Query<&mut VirtualList>,
) {
    for (item, node, parent) in q_items.iter() {
        let Ok(mut list) = q_lists.get_mut(parent.parent()) else {
            continue;
        };
        let h = node.size().y * node.inverse_scale_factor();
        if h <= 0.0 {
            continue;
        }
        if list
            .measured
            .get(&item.index)
            .is_none_or(|cur| (cur - h).abs() > 0.5)
        {
            list.measured.insert(item.index, h);
        }
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

    #[test]
    fn measured_offsets_use_estimate_for_unmeasured_items() {
        let mut measured = HashMap::new();
        measured.insert(1, 100.0);
        let offsets = build_measured_offsets(4, 20.0, &measured);
        // 第 0 项 20，第 1 项 100（实测），第 2/3 项各 20
        assert_eq!(offsets, vec![0.0, 20.0, 120.0, 140.0, 160.0]);
    }

    #[test]
    fn measured_range_finds_items_by_offset() {
        let offsets = vec![0.0, 20.0, 120.0, 140.0, 160.0];
        // 视口 100，滚到 0 → 覆盖第 0、1 项（第 1 项高 100）
        let (f, c) = compute_visible_range_measured(&offsets, 100.0, 0.0, 0);
        assert_eq!(f, 0);
        assert!(c >= 2, "应覆盖前两项（实测 {c}）");
    }

    #[test]
    fn measured_range_advances_with_scroll() {
        let offsets = vec![0.0, 20.0, 120.0, 140.0, 160.0];
        // 滚到 130：第 2 项（顶边 120、高 20）跨过视口顶 → 它要渲染
        let (f, _c) = compute_visible_range_measured(&offsets, 50.0, 130.0, 0);
        assert_eq!(f, 2);
        // 滚到 145：第 3 项（顶边 140）跨过视口顶
        let (f, _c) = compute_visible_range_measured(&offsets, 50.0, 145.0, 0);
        assert_eq!(f, 3);
    }

    #[test]
    fn measured_range_clamps_to_count() {
        let offsets = vec![0.0, 20.0, 120.0];
        let (f, c) = compute_visible_range_measured(&offsets, 100.0, 1000.0, 0);
        assert!(f + c <= 2, "不应超出总项数（{f}+{c}）");
    }

    #[test]
    fn measured_range_empty_list() {
        assert_eq!(compute_visible_range_measured(&[], 100.0, 0.0, 2), (0, 0));
    }
}
