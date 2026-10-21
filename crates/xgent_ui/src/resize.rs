//! 面板拖拽调整大小（v8 M5：遍历分割树生成分割条，修 D6）。
//!
//! 宽度由 [`SplitTree`] 驱动：每个 leaf 的 `LeafSize` 给出尺寸策略，**分割条
//! 遍历分割树生成**，拖动写回被调整 leaf 的当前尺寸。因此**注册一个新的
//! 纵向分割不需要改本模块**（只需在分割树里声明）。
//!
//! 拖拽手柄 = `ui_widgets::Button` + `Hovered`：`Pressed`（ButtonPlugin 经冒泡
//! 事件维护）触发拖拽，配合 `ButtonInput<MouseButton>` 维持 + 释放清除的状态机。
use bevy::input::ButtonInput;
use bevy::input::mouse::{AccumulatedMouseMotion, MouseButton};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;

use crate::layout::MainAreaMarker;
use crate::panels::split::{Axis, LeafSize, SplitHandle, SplitTree, leaf_ids};
use crate::theme::size;

/// 对话主区最小宽度（单源：`theme::size`）。
const CHAT_MIN: f32 = size::CHAT_MIN;
/// 分隔手柄命中宽度（单源：`theme::size`）。
const HANDLE_W: f32 = size::HANDLE_W;
/// 双击判定窗口（秒）。
const DOUBLE_CLICK_SECS: f64 = 0.3;

/// 分割叶子对应的 UI 区域（尺寸由分割树驱动）。
#[derive(Component, Debug, Clone, Copy)]
pub struct SplitLeafMarker {
    /// 叶子 id（`panels::split::leaf_ids`）。
    pub id: &'static str,
    /// 分割轴：横向 = 改**宽度**，纵向 = 改**高度**。
    pub axis: Axis,
}

/// 分割手柄标记（携带被调整的叶子与方向）。
#[derive(Component, Debug, Clone, Copy)]
pub struct SplitHandleMarker {
    /// 被调整的叶子 id。
    pub leaf: &'static str,
    /// 拖动方向（+1 = 向正向拖动使叶子变宽，-1 = 反向）。
    pub dir: f32,
    /// 分割轴。
    pub axis: Axis,
}

/// 当前激活的拖拽（鼠标按到手柄、未释放期间）。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ActiveResize(pub Option<&'static str>);

/// 拖拽调整大小插件。
pub struct ResizePlugin;

impl Plugin for ResizePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SplitTree>()
            .init_resource::<ActiveResize>()
            .add_systems(
                Update,
                (
                    sync_split_handles,
                    apply_split_sizes,
                    handle_resize_drag,
                    handle_double_click_reset,
                    clamp_on_window_resize,
                    responsive_collapse,
                )
                    .chain(),
            );
    }
}

/// 构造一条拖拽手柄节点 Bundle。
///
/// 宽 [`HANDLE_W`]（竖向分割则为高），既是视觉宽度也是命中宽度（透明，
/// hover/拖拽时高亮 + 2px 交互色竖线）。
///
/// `trailing` = 手柄贴在被调整叶子的**尾缘**（右 / 下）；否则贴在**首缘**
/// （左 / 上）。手柄**绝对定位**覆盖在叶子边缘上，不占用 flex 布局空间。
pub fn handle_bundle(axis: Axis, trailing: bool) -> impl Bundle {
    let horizontal = axis == Axis::Horizontal;
    let mut node = Node {
        position_type: PositionType::Absolute,
        width: if horizontal {
            Val::Px(HANDLE_W)
        } else {
            Val::Percent(100.0)
        },
        height: if horizontal {
            Val::Percent(100.0)
        } else {
            Val::Px(HANDLE_W)
        },
        flex_shrink: 0.0,
        ..default()
    };
    if horizontal {
        if trailing {
            node.right = Val::Px(0.0);
        } else {
            node.left = Val::Px(0.0);
        }
    } else if trailing {
        node.bottom = Val::Px(0.0);
    } else {
        node.top = Val::Px(0.0);
    }
    (
        node,
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
        Button,
        Hovered::default(),
        // 拖拽手柄 → 左右箭头指针
        crate::cursor::CursorStyle::EwResize,
        crate::cursor::CursorHit::default(),
    )
}

/// 遍历分割树生成 / 刷新分割条。
///
/// 每条手柄挂在**被调整叶子**的节点上（覆盖层抽屉挂在抽屉右缘，右侧分屏挂在
/// 分屏左缘），因此不会改变既有 flex 布局。
pub fn sync_split_handles(
    tree: Res<SplitTree>,
    mut commands: Commands,
    q_leaves: Query<(Entity, &SplitLeafMarker)>,
    q_handles: Query<(Entity, &SplitHandleMarker)>,
) {
    if !tree.is_changed() && !tree.is_added() {
        return;
    }
    let leaf_entity = |id: &str| -> Option<Entity> {
        q_leaves
            .iter()
            .find(|(_, marker)| marker.id == id)
            .map(|(entity, _)| entity)
    };
    for handle in tree.handles() {
        let Some(leaf) = handle.adjustable_leaf(&tree) else {
            continue;
        };
        let Some(entity) = leaf_entity(leaf) else {
            continue;
        };
        // 已存在同叶子手柄 → 只更新方向/轴
        if let Some((existing, marker)) = q_handles.iter().find(|(_, m)| m.leaf == leaf) {
            if marker.dir != handle_dir(&handle, leaf) || marker.axis != handle.axis {
                commands.entity(existing).insert(SplitHandleMarker {
                    leaf,
                    dir: handle_dir(&handle, leaf),
                    axis: handle.axis,
                });
            }
            continue;
        }
        let dir = handle_dir(&handle, leaf);
        commands.entity(entity).with_children(|p| {
            p.spawn((
                handle_bundle(handle.axis, dir > 0.0),
                SplitHandleMarker {
                    leaf,
                    dir,
                    axis: handle.axis,
                },
            ));
        });
    }
}

/// 手柄方向：被调整叶子位于边界哪一侧。
fn handle_dir(handle: &SplitHandle, leaf: &str) -> f32 {
    if leaf == handle.before {
        // 前一个子节点 → 边界在它的**尾缘**（右 / 下），向右拖动 → 变宽。
        1.0
    } else {
        // 后一个子节点 → 边界在它的**首缘**（左 / 上），向右拖动 → 变窄。
        -1.0
    }
}

/// 按分割树给每个叶子应用宽度。
pub fn apply_split_sizes(tree: Res<SplitTree>, mut q_leaves: Query<(&SplitLeafMarker, &mut Node)>) {
    if !tree.is_changed() && !tree.is_added() {
        return;
    }
    for (marker, mut node) in q_leaves.iter_mut() {
        let Some(size) = tree.leaf_size(marker.id) else {
            continue;
        };
        if matches!(size, LeafSize::Grow(_)) {
            continue;
        }
        // 按轴写尺寸：横向改宽、纵向改高。
        match marker.axis {
            Axis::Horizontal => node.width = Val::Px(size.preferred()),
            Axis::Vertical => node.height = Val::Px(size.preferred()),
        }
    }
}

/// 处理拖拽：手柄 Pressed 启动、鼠标按下期间累积位移、释放清除。
fn handle_resize_drag(
    mut tree: ResMut<SplitTree>,
    mut active: ResMut<ActiveResize>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    main_q: Query<&ComputedNode, With<MainAreaMarker>>,
    handles: Query<(&SplitHandleMarker, &Hovered, Has<Pressed>)>,
    mut q_handle_style: Query<(&SplitHandleMarker, &mut BackgroundColor, &mut BorderColor)>,
    theme: Res<crate::theme::Theme>,
) {
    // 1. 启动：任一手柄被按下且正被悬停（鼠标在按下瞬间位于手柄上）。
    // hovered 门控防御 Pressed 残留：手柄极窄，按下后移出手柄再释放时
    // ButtonPlugin 的 Pressed 不会被移除（release 目标非手柄、位移小无
    // drag_end），残留会让下一次任意位置按下从错误边缘开始拖拽。
    if active.0.is_none() {
        for (marker, hovered, pressed) in handles.iter() {
            if pressed && hovered.get() && mouse.pressed(MouseButton::Left) {
                active.0 = Some(marker.leaf);
                break;
            }
        }
    }

    // 2. 手柄视觉反馈：hover/拖拽 → accent_glow 底 + 交互色竖线
    let active_leaf = active.0;
    for (marker, mut bg, mut border) in q_handle_style.iter_mut() {
        let hovered = handles
            .iter()
            .any(|(m, h, _)| m.leaf == marker.leaf && h.get());
        let highlighted = active_leaf == Some(marker.leaf) || hovered;
        let target = if highlighted {
            theme.accent_glow
        } else {
            Color::NONE
        };
        let line = if highlighted {
            theme.accent_interactive
        } else {
            Color::NONE
        };
        if bg.0 != target {
            bg.0 = target;
        }
        border.set_all(line);
    }

    if active_leaf.is_none() {
        return;
    }

    // 3. 释放或鼠标键松开 → 结束
    if !mouse.pressed(MouseButton::Left) {
        active.0 = None;
        return;
    }

    // 4. 应用位移（AccumulatedMouseMotion.delta 为逻辑像素，按手柄的轴取分量）
    let axis = handles
        .iter()
        .find(|(m, _, _)| m.leaf == active_leaf.unwrap_or(""))
        .map(|(m, _, _)| m.axis)
        .unwrap_or(Axis::Horizontal);
    let delta = match axis {
        Axis::Horizontal => motion.delta.x,
        Axis::Vertical => motion.delta.y,
    };
    if delta == 0.0 {
        return;
    }

    let Ok(main_node) = main_q.single() else {
        return;
    };
    let main_w = main_node.size.x * main_node.inverse_scale_factor;

    let Some(leaf) = active_leaf else {
        return;
    };
    let Some(current) = tree.leaf_size(leaf) else {
        return;
    };
    if matches!(current, LeafSize::Grow(_)) {
        return;
    }
    let dir = handles
        .iter()
        .find(|(m, _, _)| m.leaf == leaf)
        .map(|(m, _, _)| m.dir)
        .unwrap_or(1.0);
    let next = current.preferred() + delta * dir;
    let clamped = clamp_leaf(leaf, axis, next, main_w);
    if clamped != current.preferred() {
        tree.set_leaf_size(leaf, current.with_current(clamped));
    }
}

/// 叶子尺寸钳制（纯函数，单测覆盖边界）。
///
/// `available`：该轴方向上的可用尺寸（横向 = 主区可用宽度，纵向 = 主区可用高度）。
/// 下限取 `LeafSize::Min` 的下限；上限：抽屉有定值上限（480），其余取
/// `available − CHAT_MIN`（不足时取下限）。
///
/// ⚠ 上下界按**叶子 id**取（尺寸策略：抽屉 240–480、其余用上下文面板的界），
/// 这与「按分割拓扑特判」不同——新增叶子时其界应在此表中补一行。
pub fn clamp_leaf(id: &str, axis: Axis, value: f32, available: f32) -> f32 {
    let min = match id {
        leaf_ids::DRAWER => size::DRAWER_W_MIN,
        _ => size::CONTEXT_W_MIN,
    };
    let max = match (id, axis) {
        (leaf_ids::DRAWER, Axis::Horizontal) => size::DRAWER_W_MAX,
        (_, Axis::Horizontal) => (available - CHAT_MIN).max(min),
        // 纵向分割：上界同样留出主区最小高度（`CHAT_MIN` 作为纵向的最小余量）。
        (_, Axis::Vertical) => available.max(min),
    };
    value.clamp(min, max)
}

/// 双击手柄：复位该叶子为默认宽度（300ms 内两次按下）。
fn handle_double_click_reset(
    mut tree: ResMut<SplitTree>,
    handles: Query<&SplitHandleMarker, Added<Pressed>>,
    time: Res<Time>,
    mut last: Local<Option<f64>>,
) {
    for marker in handles.iter() {
        let now = time.elapsed_secs_f64();
        if let Some(prev) = *last
            && now - prev < DOUBLE_CLICK_SECS
        {
            if let Some(current) = tree.leaf_size(marker.leaf) {
                let reset = match marker.leaf {
                    leaf_ids::DRAWER => size::DRAWER_W_PREFERRED,
                    _ => size::CONTEXT_W_DEFAULT,
                };
                tree.set_leaf_size(marker.leaf, current.with_current(reset));
            }
            *last = None;
        } else {
            *last = Some(now);
        }
    }
}

/// 窗口尺寸变化时统一钳制面板宽度（启动/缩窗溢出防护）。
fn clamp_on_window_resize(
    main_q: Query<&ComputedNode, With<MainAreaMarker>>,
    mut tree: ResMut<SplitTree>,
) {
    let Ok(node) = main_q.single() else {
        return;
    };
    let main_w = node.size.x * node.inverse_scale_factor;
    if main_w <= 0.0 {
        return;
    }
    for id in tree.leaf_ids() {
        let Some(current) = tree.leaf_size(id) else {
            continue;
        };
        if matches!(current, LeafSize::Grow(_)) {
            continue;
        }
        let axis = tree.leaf_axis(id).unwrap_or(Axis::Horizontal);
        let clamped = clamp_leaf(id, axis, current.preferred(), main_w);
        if clamped != current.preferred() {
            tree.set_leaf_size(id, current.with_current(clamped));
        }
    }
}

/// 窗口过窄时自动折叠上下文面板（对应原型 <1100px 断点）。
///
/// 只自动收起、不自动展开——恢复走 rail 展开钮或终端快捷键。
fn responsive_collapse(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut layout: ResMut<crate::panels::SlotLayout>,
) {
    let Ok(w) = windows.single() else {
        return;
    };
    if w.width() < 1100.0 {
        layout.set_open(crate::panels::Slot::SideDock, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 抽屉宽度下限保护：不低于 240。
    #[test]
    fn drawer_respects_min() {
        assert_eq!(
            clamp_leaf(leaf_ids::DRAWER, Axis::Horizontal, 100.0, 1600.0),
            size::DRAWER_W_MIN
        );
        assert_eq!(
            clamp_leaf(leaf_ids::DRAWER, Axis::Horizontal, 300.0, 1600.0),
            300.0
        );
    }

    /// 抽屉宽度上限保护：不高于 480。
    #[test]
    fn drawer_respects_max() {
        assert_eq!(
            clamp_leaf(leaf_ids::DRAWER, Axis::Horizontal, 900.0, 1600.0),
            size::DRAWER_W_MAX
        );
        assert_eq!(
            clamp_leaf(leaf_ids::DRAWER, Axis::Horizontal, 480.0, 1600.0),
            480.0
        );
    }

    /// 右侧分屏上限 = available − CHAT_MIN。
    #[test]
    fn side_dock_upper_bound() {
        // available=1600：上限 1600-520=1080
        assert_eq!(
            clamp_leaf(leaf_ids::SIDE_DOCK, Axis::Horizontal, 2000.0, 1600.0),
            1080.0
        );
        assert_eq!(
            clamp_leaf(leaf_ids::SIDE_DOCK, Axis::Horizontal, 720.0, 1600.0),
            720.0
        );
    }

    /// 默认树：抽屉与右侧分屏各有一条手柄。
    #[test]
    fn default_tree_has_two_handles() {
        let tree = SplitTree::default();
        let handles = tree.handles();
        assert_eq!(handles.len(), 2);
        assert_eq!(handles[0].before, leaf_ids::DRAWER);
        assert_eq!(handles[1].after, leaf_ids::SIDE_DOCK);
        // 抽屉手柄调整抽屉；分屏手柄调整分屏
        assert_eq!(handles[0].adjustable_leaf(&tree), Some(leaf_ids::DRAWER));
        assert_eq!(handles[1].adjustable_leaf(&tree), Some(leaf_ids::SIDE_DOCK));
    }
}
