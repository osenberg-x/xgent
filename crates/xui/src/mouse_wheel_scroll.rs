//! 鼠标滚轮 → `ScrollPosition` 桥接。
//!
//! Bevy 0.19 的 `bevy_ui` 仅提供 `ScrollPosition` 组件与 `Overflow::Scroll` 的布局
//! 支持，但**不自动**把 `MouseWheel` 输入映射到 `ScrollPosition`。官方在
//! `examples/ui/scroll_and_overflow/scroll.rs` 里用两个系统补齐：读 `MouseWheel`
//! 与 `HoverMap`，按指针悬停路径自顶向下触发 `Scroll` 实体事件，由 handler 在
//! 命中的可滚动节点上累加 `ScrollPosition` 并在到达边界时停止传播。
//!
//! 本模块把该模式封装为通用插件，作用于任意带 `OverflowAxis::Scroll` 的节点，
//! 不需要调用方做任何标记。挂在 `XuiPlugin` 上，依赖 `DefaultPlugins` 默认开启的
//! picking 后端（`HoverMap` 由它填充）。
//!
//! 与 `ScrollPosition` 单位一致：delta 与最终偏移均为逻辑像素，`ComputedNode` 的
//! 物理量乘 `inverse_scale_factor` 转换。

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;

/// 滚动事件：作用在 `entity` 上、沿 `delta`（逻辑像素）方向滚动。
///
/// `delta.y > 0` 表示内容向上移动（用户向下滚轮）。与 `ScrollPosition` 的语义
/// 一致：`scroll_position.y += delta.y`。
#[derive(EntityEvent, Debug)]
#[entity_event(propagate, auto_propagate)]
struct Scroll {
    /// 命中的可滚动节点。
    entity: Entity,
    /// 滚动增量（逻辑像素）。handler 会消费已吸收的分量。
    delta: Vec2,
}

/// 鼠标滚轮 → `ScrollPosition` 插件。
pub struct MouseWheelScrollPlugin;

/// 本帧是否有鼠标滚轮事件（供贴底跟随等逻辑区分"用户主动滚动"与
/// "内容增长导致的偏移变化"）。`send_scroll_events` 置真，消费方复位。
#[derive(Resource, Default)]
pub struct MouseWheelScrolled(pub bool);

impl Plugin for MouseWheelScrollPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MouseWheelScrolled>()
            .add_observer(on_scroll_handler)
            .add_systems(Update, send_scroll_events);
    }
}

/// 一行文本的高度（逻辑像素），用于把 `MouseScrollUnit::Line` 换算成像素增量。
const LINE_HEIGHT: f32 = 21.0;

/// 读 `MouseWheel` 事件，沿指针悬停路径自顶向下触发 [`Scroll`] 实体事件。
///
/// 仅当 `HoverMap` 存在（picking 后端已注册）时生效；悬停顺序为父→子，
/// 配合 handler 的传播控制可让内层滚动容器优先吸收滚轮。
fn send_scroll_events(
    mut mouse_wheel_reader: MessageReader<MouseWheel>,
    hover_map: Option<Res<HoverMap>>,
    mut commands: Commands,
    mut scrolled: ResMut<MouseWheelScrolled>,
) {
    let Some(hover_map) = hover_map else {
        return;
    };
    if mouse_wheel_reader.is_empty() {
        return;
    }
    // 标记本帧有滚轮事件，供 maintain_stick_bottom 等消费方区分用户主动滚动。
    scrolled.0 = true;
    for mouse_wheel in mouse_wheel_reader.read() {
        // winit 的 wheel.y 正向是"向上滚"，而 ScrollPosition.y 正向是"内容上移=
        // 用户向下滚"，故取反使滚轮方向符合直觉（向下滚→内容上移）。
        let mut delta = -Vec2::new(mouse_wheel.x, mouse_wheel.y);
        if mouse_wheel.unit == MouseScrollUnit::Line {
            delta *= LINE_HEIGHT;
        }
        for pointer_map in hover_map.values() {
            // 按 hover 顺序（父在前）触发；handler 在内层节点吸收完整 delta 后
            // 会停止传播，外层不再叠加滚动。
            for entity in pointer_map.keys().copied() {
                commands.trigger(Scroll { entity, delta });
            }
        }
    }
}

/// 把 [`Scroll`] delta 累加到命中的可滚动节点的 `ScrollPosition`，到达边界时
/// 停止向上传播，避免父容器被误带动。
///
/// 不在此处手动 clamp `ScrollPosition`：`ComputedNode.content_size()` 是上一帧
/// 布局结果，若占位撑高节点刚更新（content_size 变大但布局尚未重跑），按过时
/// 的 `max_offset` 钳位会把 `scroll_position` 锁死在 0，滚轮完全失效。
/// bevy 布局系统（`UiSystems::Layout`，`PostUpdate`）自身会用当帧最新 content_size
/// 对 `ScrollPosition` 做 `[0, max]` 钳位，无需此处重复。
fn on_scroll_handler(
    mut scroll: On<Scroll>,
    mut query: Query<(&mut ScrollPosition, &Node, &ComputedNode)>,
) {
    let Ok((mut scroll_position, node, computed)) = query.get_mut(scroll.entity) else {
        return;
    };

    let max_offset = (computed.content_size() - computed.size()) * computed.inverse_scale_factor();
    let delta = &mut scroll.delta;

    if node.overflow.x == OverflowAxis::Scroll && delta.x != 0.0 {
        let at_edge = if delta.x > 0.0 {
            scroll_position.x >= max_offset.x
        } else {
            scroll_position.x <= 0.0
        };
        if !at_edge {
            scroll_position.x += delta.x;
            delta.x = 0.0;
        }
    }

    if node.overflow.y == OverflowAxis::Scroll && delta.y != 0.0 {
        let at_edge = if delta.y > 0.0 {
            scroll_position.y >= max_offset.y
        } else {
            scroll_position.y <= 0.0
        };
        if !at_edge {
            scroll_position.y += delta.y;
            delta.y = 0.0;
        }
    }

    // delta 已完全消费 → 停止向上传播；否则交给父级可滚动节点继续吸收。
    if *delta == Vec2::ZERO {
        scroll.propagate(false);
    }
}
