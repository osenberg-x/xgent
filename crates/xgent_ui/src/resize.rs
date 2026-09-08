//! 面板拖拽调整大小。
//!
//! 主区（[`crate::layout::MainAreaMarker`]）为 v7 四列布局：
//! 图标轨 → 对话主区 → 右手柄 → 上下文面板。
//! - 右手柄（[`ResizeEdge::Right`]）拖拽改变上下文面板宽度，双击复位默认宽度。
//!
//! 宽度由 [`PanelWidths`] Resource 驱动：上下文面板用显式像素宽度，
//! 对话主区用 `flex_grow: 1.0` 填充剩余空间。拖拽时据每帧
//! [`AccumulatedMouseMotion`] 增量更新宽度，钳制经纯函数 [`clamp_side_view`]
//! （配单测）。启动/缩窗时统一钳制防溢出（方案 §8.8）；窗口 <1100px 自动收起面板。
//!
//! 不引入 `bevy_picking`（默认未启用，会拉重依赖）；改用手柄 `Interaction::Pressed`
//! 触发拖拽 + `ButtonInput<MouseButton>` 维持 + 释放清除的状态机。
use bevy::input::ButtonInput;
use bevy::input::mouse::{AccumulatedMouseMotion, MouseButton};
use bevy::prelude::*;

use crate::layout::{MainAreaMarker, SideViewCollapsed};
use crate::theme::size;

/// 上下文面板最小宽度（逻辑像素）。
const SIDE_VIEW_MIN: f32 = 380.0;
/// 对话主区最小宽度（逻辑像素）——拖拽时为其保留的最小空间。
const CHAT_MIN: f32 = 520.0;
/// 分隔手柄命中宽度（逻辑像素）。
const HANDLE_W: f32 = 6.0;
/// 双击判定窗口（秒）。
const DOUBLE_CLICK_SECS: f64 = 0.3;

/// 拖拽边界标识。
#[derive(Component, Copy, Clone, Eq, PartialEq, Debug, Default)]
pub enum ResizeEdge {
    /// 对话主区 ↔ 上下文面板（拖拽改变面板宽度）。
    #[default]
    Right,
}

/// 手柄标记（携带边界标识），挂于手柄节点。
#[derive(Component, Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct ResizeEdgeMarker(pub ResizeEdge);

/// 面板显式宽度（逻辑像素），由拖拽更新。
#[derive(Resource, Debug, Clone, Copy)]
pub struct PanelWidths {
    /// 上下文面板宽度。
    pub side_view: f32,
}

impl Default for PanelWidths {
    fn default() -> Self {
        Self {
            side_view: size::CONTEXT_W_DEFAULT,
        }
    }
}

/// 当前激活的拖拽边界（鼠标按到手柄、未释放期间）。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ActiveResize(pub Option<ResizeEdge>);

/// 上下文面板宽度钳制（纯函数，单测覆盖边界）。
///
/// `available`：主区可用宽度。
/// 下限 [`SIDE_VIEW_MIN`]；上限 = available − [`CHAT_MIN`]（不足时取下限）。
fn clamp_side_view(w: f32, available: f32) -> f32 {
    let max = (available - CHAT_MIN).max(SIDE_VIEW_MIN);
    w.clamp(SIDE_VIEW_MIN, max)
}

/// 拖拽调整大小插件。
pub struct ResizePlugin;

impl Plugin for ResizePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PanelWidths>()
            .init_resource::<ActiveResize>()
            .add_systems(
                Update,
                (
                    apply_panel_widths,
                    handle_resize_drag,
                    handle_double_click_reset,
                    clamp_on_window_resize,
                    responsive_collapse,
                )
                    .chain()
                    .after(crate::layout::toggle_panel_visibility),
            );
    }
}
/// 构造一条竖向拖拽手柄节点 Bundle（在 [`crate::layout::spawn_layout`] 中 spawn）。
///
/// 宽 [`HANDLE_W`]，既是视觉宽度也是命中宽度（透明，hover/拖拽时高亮 +
/// 2px 交互色竖线）。上下文面板默认展开时手柄可见。
pub fn handle_bundle(edge: ResizeEdge) -> impl Bundle {
    (
        Node {
            width: Val::Px(HANDLE_W),
            height: Val::Percent(100.0),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
        Button,
        ResizeEdgeMarker(edge),
        // 拖拽手柄 → 左右箭头指针
        crate::cursor::CursorStyle::EwResize,
        crate::cursor::CursorHit::default(),
    )
}

/// 每帧据 [`PanelWidths`] 应用上下文面板宽度。
pub(crate) fn apply_panel_widths(
    widths: Res<PanelWidths>,
    mut q_side: Query<&mut Node, With<crate::layout::SideViewMarker>>,
) {
    if !widths.is_changed() && !widths.is_added() {
        return;
    }
    if let Ok(mut node) = q_side.single_mut() {
        node.width = Val::Px(widths.side_view);
    }
}

/// 处理拖拽：手柄 Pressed 启动、鼠标按下期间累积位移、释放清除。
///
/// 手柄 hover/拖拽时高亮（`accent_glow` 底 + 2px `accent_interactive` 竖线）。
fn handle_resize_drag(
    mut widths: ResMut<PanelWidths>,
    mut active: ResMut<ActiveResize>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    main_q: Query<&ComputedNode, With<MainAreaMarker>>,
    handles: Query<(&ResizeEdgeMarker, &Interaction)>,
    side_collapsed: Res<SideViewCollapsed>,
    mut q_handle_style: Query<(&ResizeEdgeMarker, &mut BackgroundColor, &mut BorderColor)>,
    theme: Res<crate::theme::Theme>,
) {
    // 1. 启动：任一手柄被按下（鼠标在按下瞬间位于手柄上）
    if active.0.is_none() {
        for (marker, interaction) in handles.iter() {
            if *interaction == Interaction::Pressed && mouse.pressed(MouseButton::Left) {
                active.0 = Some(marker.0);
                break;
            }
        }
    }

    // 2. 手柄视觉反馈：hover/拖拽 → accent_glow 底 + 交互色竖线
    let active_edge = active.0;
    for (marker, mut bg, mut border) in q_handle_style.iter_mut() {
        let hovered = handles
            .iter()
            .any(|(m, i)| m.0 == marker.0 && *i == Interaction::Hovered);
        let highlighted = active_edge == Some(marker.0) || hovered;
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

    if active_edge.is_none() {
        return;
    }

    // 3. 释放或鼠标键松开 → 结束
    if !mouse.pressed(MouseButton::Left) {
        active.0 = None;
        return;
    }

    // 4. 应用位移（AccumulatedMouseMotion.delta 为逻辑像素）
    let dx = motion.delta.x;
    if dx == 0.0 {
        return;
    }

    // 主区可用宽度（逻辑像素）：ComputedNode.size 为物理像素，乘 inverse_scale_factor
    let Ok(main_node) = main_q.single() else {
        return;
    };
    let main_w = main_node.size.x * main_node.inverse_scale_factor;

    // 上下文面板折叠时不可拖（手柄本就隐藏），防御：跳过
    if side_collapsed.0 {
        return;
    }
    // 手柄右移（dx>0）→ 面板变窄；钳制走纯函数（单测覆盖）
    widths.side_view = clamp_side_view(widths.side_view - dx, main_w);
}

/// 双击右手柄：复位上下文面板为默认宽度（300ms 内两次按下，方案 §8.8）。
fn handle_double_click_reset(
    mut widths: ResMut<PanelWidths>,
    handles: Query<(&ResizeEdgeMarker, &Interaction), Changed<Interaction>>,
    time: Res<Time>,
    mut last: Local<Option<f64>>,
) {
    for (marker, interaction) in handles.iter() {
        if marker.0 != ResizeEdge::Right || *interaction != Interaction::Pressed {
            continue;
        }
        let now = time.elapsed_secs_f64();
        if let Some(prev) = *last
            && now - prev < DOUBLE_CLICK_SECS
        {
            widths.side_view = size::CONTEXT_W_DEFAULT;
            *last = None;
        } else {
            *last = Some(now);
        }
    }
}

/// 窗口尺寸变化时统一钳制面板宽度（启动/缩窗溢出防护；方案 §8.8）。
fn clamp_on_window_resize(
    main_q: Query<&ComputedNode, With<MainAreaMarker>>,
    mut widths: ResMut<PanelWidths>,
) {
    let Ok(node) = main_q.single() else {
        return;
    };
    let main_w = node.size.x * node.inverse_scale_factor;
    if main_w <= 0.0 {
        return;
    }
    let clamped = clamp_side_view(widths.side_view, main_w);
    if clamped != widths.side_view {
        widths.side_view = clamped;
    }
}

/// 窗口过窄时自动折叠上下文面板（对应原型 <1100px 断点，方案 §8.8）。
///
/// 只自动收起、不自动展开——恢复走 rail 展开钮（M3-T5）或终端快捷键。
fn responsive_collapse(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut side: ResMut<SideViewCollapsed>,
) {
    let Ok(w) = windows.single() else {
        return;
    };
    if w.width() < 1100.0 && !side.0 {
        side.0 = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 下限保护：可用空间再小，也不低于 SIDE_VIEW_MIN。
    #[test]
    fn clamp_respects_min() {
        assert_eq!(clamp_side_view(100.0, 0.0), SIDE_VIEW_MIN);
        assert_eq!(clamp_side_view(720.0, 100.0), SIDE_VIEW_MIN);
    }

    /// 正常区间：夹在 [SIDE_VIEW_MIN, available − CHAT_MIN]。
    #[test]
    fn clamp_upper_bound() {
        // available=1600：上限 1600-520=1080
        assert_eq!(clamp_side_view(2000.0, 1600.0), 1080.0);
        assert_eq!(clamp_side_view(720.0, 1600.0), 720.0);
        assert_eq!(clamp_side_view(500.0, 1600.0), 500.0);
    }

    /// 双击复位值即默认宽度（回归保护）。
    #[test]
    fn reset_value_matches_default() {
        assert_eq!(PanelWidths::default().side_view, size::CONTEXT_W_DEFAULT);
    }
}
