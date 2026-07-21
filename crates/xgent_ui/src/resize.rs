//! 面板拖拽调整大小。
//!
//! 主区（[`crate::layout::MainAreaMarker`]）是水平行布局，子节点顺序：
//! 文件面板 → 左手柄 → 对话主区 → 右手柄 → 右侧分屏。
//! - 左手柄（[`ResizeEdge::Left`]）拖拽改变文件面板宽度；
//! - 右手柄（[`ResizeEdge::Right`]）拖拽改变右侧分屏宽度（分屏展开时才可见/可拖）。
//!
//! 宽度由 [`PanelWidths`] Resource 驱动：文件面板/右侧分屏用显式像素宽度，
//! 对话主区用 `flex_grow: 1.0` 填充剩余空间。拖拽时据每帧
//! [`AccumulatedMouseMotion`] 增量更新宽度，并据主区 `ComputedNode` 物理尺寸做上下限钳制。
//!
//! 不引入 `bevy_picking`（默认未启用，会拉重依赖）；改用手柄 `Interaction::Pressed`
//! 触发拖拽 + `ButtonInput<MouseButton>` 维持 + 释放清除的状态机。

use bevy::input::mouse::{AccumulatedMouseMotion, MouseButton};
use bevy::input::ButtonInput;
use bevy::prelude::*;

use crate::layout::{FilePanelCollapsed, MainAreaMarker, SideViewCollapsed};
use crate::theme::size;

/// 文件面板最小宽度（逻辑像素）。
const FILE_PANEL_MIN: f32 = 160.0;
/// 右侧分屏最小宽度（逻辑像素）。
const SIDE_VIEW_MIN: f32 = 200.0;
/// 对话主区最小宽度（逻辑像素）——拖拽时为其保留的最小空间。
const CHAT_MIN: f32 = 240.0;
/// 分隔手柄命中宽度（逻辑像素）。
const HANDLE_W: f32 = 6.0;
/// 手柄 hover/拖拽时的强调色背景。
const HANDLE_ACTIVE_COLOR: Color = Color::srgba(0.5, 0.65, 1.0, 0.6);

/// 拖拽边界标识。
#[derive(Component, Copy, Clone, Eq, PartialEq, Debug, Default)]
pub enum ResizeEdge {
    /// 文件面板 ↔ 对话主区（拖拽改变文件面板宽度）。
    #[default]
    Left,
    /// 对话主区 ↔ 右侧分屏（拖拽改变右侧分屏宽度）。
    Right,
}

/// 手柄标记（携带边界标识），挂于手柄节点。
#[derive(Component, Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct ResizeEdgeMarker(pub ResizeEdge);

/// 面板显式宽度（逻辑像素），由拖拽更新。
///
/// 启动时初始化为 [`size::FILE_PANEL_W`] / [`size::CHAT_SIDEBAR_W`]；
/// [`crate::layout`] 的折叠系统改读此 Resource（不再直接写常量）。
#[derive(Resource, Debug, Clone, Copy)]
pub struct PanelWidths {
    /// 文件面板宽度。
    pub file_panel: f32,
    /// 右侧分屏宽度（展开时生效）。
    pub side_view: f32,
}

impl Default for PanelWidths {
    fn default() -> Self {
        Self {
            file_panel: size::FILE_PANEL_W,
            side_view: size::CHAT_SIDEBAR_W,
        }
    }
}

/// 当前激活的拖拽边界（鼠标按到手柄、未释放期间）。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ActiveResize(pub Option<ResizeEdge>);

/// 拖拽调整大小插件。
pub struct ResizePlugin;

impl Plugin for ResizePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PanelWidths>()
            .init_resource::<ActiveResize>()
            .add_systems(
                Update,
                (apply_panel_widths, handle_resize_drag)
                    .chain()
                    .after(crate::layout::toggle_panel_visibility),
            );
    }
}
/// 构造一条竖向拖拽手柄节点 Bundle（在 [`crate::layout::spawn_layout`] 中 spawn）。
///
/// 宽 [`HANDLE_W`]，既是视觉宽度也是命中宽度（透明，hover/拖拽时变色）。
/// 不用负 margin 扩展命中区——flex 负 margin 在 bevy_ui 行为不稳且易被邻面板背景遮盖。
pub fn handle_bundle(edge: ResizeEdge) -> impl Bundle {
    (
        Node {
            width: Val::Px(HANDLE_W),
            height: Val::Percent(100.0),
            flex_shrink: 0.0,
            // 右手柄初始隐藏（右侧分屏默认收起，display:none）；
            // 左手柄初始显示（文件面板默认展开）。
            // 后续由 layout::toggle_panel_visibility 据 SideViewCollapsed/FilePanelCollapsed 切换。
            display: match edge {
                ResizeEdge::Left => Display::Flex,
                ResizeEdge::Right => Display::None,
            },
            ..default()
        },
        BackgroundColor(Color::NONE),
        Button,
        ResizeEdgeMarker(edge),
    )
}

/// 每帧据 [`PanelWidths`] 应用文件面板/右侧分屏宽度（折叠态下置 0 / 不影响）。
///
/// - 文件面板折叠态下强制宽 0，否则写资源宽度；
/// - 右侧分屏折叠时 `display:none`（由布局系统管），宽度即便写入也不影响渲染。
pub(crate) fn apply_panel_widths(
    widths: Res<PanelWidths>,
    file_collapsed: Res<FilePanelCollapsed>,
    mut q_file: Query<&mut Node, (With<crate::layout::FilePanelMarker>, Without<crate::layout::SideViewMarker>)>,
    mut q_side: Query<&mut Node, (With<crate::layout::SideViewMarker>, Without<crate::layout::FilePanelMarker>)>,
) {
    if !widths.is_changed() && !widths.is_added() && !file_collapsed.is_changed() {
        return;
    }
    if let Ok(mut node) = q_file.single_mut() {
        node.width = if file_collapsed.0 {
            Val::Px(0.0)
        } else {
            Val::Px(widths.file_panel)
        };
    }
    if let Ok(mut node) = q_side.single_mut() {
        node.width = Val::Px(widths.side_view);
    }
}

/// 处理拖拽：手柄 Pressed 启动、鼠标按下期间累积位移、释放清除。
///
/// 手柄 hover/拖拽时变色，提供视觉反馈。
pub(crate) fn handle_resize_drag(
    mut widths: ResMut<PanelWidths>,
    mut active: ResMut<ActiveResize>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    main_q: Query<&ComputedNode, With<MainAreaMarker>>,
    handles: Query<(&ResizeEdgeMarker, &Interaction)>,
    side_collapsed: Res<SideViewCollapsed>,
    mut q_handle_bg: Query<(&ResizeEdgeMarker, &mut BackgroundColor)>,
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

    // 2. 手柄视觉反馈：hover/拖拽时高亮
    let active_edge = active.0;
    for (marker, mut bg) in q_handle_bg.iter_mut() {
        let hovered = handles
            .iter()
            .any(|(m, i)| m.0 == marker.0 && *i == Interaction::Hovered);
        let highlighted = active_edge == Some(marker.0) || hovered;
        let target = if highlighted {
            HANDLE_ACTIVE_COLOR
        } else {
            Color::NONE
        };
        if bg.0 != target {
            bg.0 = target;
        }
    }

    let Some(edge) = active_edge else {
        return;
    };

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

    match edge {
        ResizeEdge::Left => {
            // 拖右→文件面板变宽；上限 = 主区宽 - 右侧分屏占用 - 对话主区最小
            let side_occupied = if side_collapsed.0 {
                0.0
            } else {
                widths.side_view
            };
            let max_w = (main_w - side_occupied - CHAT_MIN).max(FILE_PANEL_MIN);
            widths.file_panel = (widths.file_panel + dx).clamp(FILE_PANEL_MIN, max_w);
        }
        ResizeEdge::Right => {
            // 右侧分屏折叠时不可拖（手柄本就隐藏），防御：跳过
            if side_collapsed.0 {
                return;
            }
            // 手柄位于对话主区与右侧分屏之间，是分屏的左边界：
            // 鼠标右移（dx>0）→ 手柄右移 → 分屏变窄；左移 → 分屏变宽。
            // 故 side_view 随 dx 反向变化。
            let max_w = (main_w - widths.file_panel - CHAT_MIN).max(SIDE_VIEW_MIN);
            widths.side_view = (widths.side_view - dx).clamp(SIDE_VIEW_MIN, max_w);
        }
    }
}
