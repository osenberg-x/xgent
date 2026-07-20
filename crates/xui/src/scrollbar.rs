//! 通用竖向滚动条（K-03 滚动能力补充）。
//!
//! bevy 0.19 无内置 scrollbar widget。本模块提供轻量竖向滚动条：
//! 挂在任意带 `ScrollPosition` + `Overflow::Scroll(y)` 的容器上，
//! 自动在其右侧 overlay 一个滑块，高度 = size/content_size 比例，
//! 位置 = scroll_position/content_size 比例。
//!
//! # 用法
//!
//! ```ignore
//! commands.entity(scroll_area_entity).insert(Scrollbar {
//!     thumb_color: Color::srgb(0.4, 0.4, 0.4),
//!     track_color: Color::srgb(0.2, 0.2, 0.2),
//!     width: 10.0,
//! });
//! ```
//!
//! 滑块节点由本插件自动 spawn 为容器的子节点（`position: absolute, right: 0`）。
//! 拖动交互留后续（MVP 先只读指示）。

use bevy::prelude::*;

/// 滚动条配置组件。挂在可滚动容器（带 `ScrollPosition` + `Overflow::Scroll(y)`）上。
///
/// 容器需 `position: Relative`（默认）让绝对定位的 thumb 能相对其定位。
#[derive(Component, Debug)]
pub struct Scrollbar {
    /// 滑块颜色
    pub thumb_color: Color,
    /// 轨道颜色（背景）。`Color::NONE` 表示透明（无轨道）
    pub track_color: Color,
    /// 滚动条宽度（逻辑像素）
    pub width: f32,
    /// 滑块最小高度（逻辑像素），避免内容过长时滑块消失
    pub min_thumb_height: f32,
}

impl Default for Scrollbar {
    fn default() -> Self {
        Self {
            thumb_color: Color::srgb(0.45, 0.45, 0.48),
            track_color: Color::srgb(0.15, 0.15, 0.17),
            width: 10.0,
            min_thumb_height: 24.0,
        }
    }
}

/// 滑块标记（由本插件 spawn 为容器的子节点）。
#[derive(Component, Default)]
pub struct ScrollbarThumb;

/// 滚动条轨道标记（由本插件 spawn 为容器的子节点，thumb 挂其下）。
#[derive(Component, Default)]
pub struct ScrollbarTrack;

/// 滚动条插件。
pub struct ScrollbarPlugin;

impl Plugin for ScrollbarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (spawn_scrollbar_nodes, update_scrollbar_thumb).after(bevy::ui::UiSystems::PostLayout),
        );
    }
}

/// 为挂了 `Scrollbar` 但还没子轨道节点的容器 spawn 轨道 + 滑块。
fn spawn_scrollbar_nodes(
    q: Query<(Entity, &Scrollbar), Without<ScrollbarTrack>>,
    mut commands: Commands,
) {
    for (entity, bar) in q.iter() {
        let track = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(0.0),
                    right: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    width: Val::Px(bar.width),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(bar.track_color),
                ScrollbarTrack,
            ))
            .id();
        let thumb = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(0.0),
                    left: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Px(bar.min_thumb_height),
                    ..default()
                },
                BackgroundColor(bar.thumb_color),
                ScrollbarThumb,
            ))
            .id();
        commands.entity(track).add_child(thumb);
        commands.entity(entity).add_child(track);
    }
}

/// 每帧根据容器 `ScrollPosition` + `ComputedNode` 更新滑块高度与位置。
///
/// 必须在 `UiSystems::PostLayout` 之后跑，此时 `ComputedNode.content_size`/`size`
/// 为本帧最新值。`ScrollPosition` 与 `ComputedNode` 物理量需乘 `inverse_scale_factor`
/// 转逻辑像素。
fn update_scrollbar_thumb(
    q: Query<(&Scrollbar, &ScrollPosition, &ComputedNode, &Children), With<ScrollbarTrack>>,
    q_track: Query<&Children, With<ScrollbarTrack>>,
    mut q_thumb: Query<&mut Node, With<ScrollbarThumb>>,
) {
    for (bar, scroll, computed, children) in q.iter() {
        // 找轨道子节点
        let mut track_entity: Option<Entity> = None;
        for c in children.iter() {
            if q_track.get(c).is_ok() {
                track_entity = Some(c);
                break;
            }
        }
        let Some(track_entity) = track_entity else {
            continue;
        };
        let Ok(track_children) = q_track.get(track_entity) else {
            continue;
        };
        let mut thumb_entity: Option<Entity> = None;
        for c in track_children.iter() {
            if q_thumb.get(c).is_ok() {
                thumb_entity = Some(c);
                break;
            }
        }
        let Some(thumb_entity) = thumb_entity else {
            continue;
        };
        let Ok(mut thumb_node) = q_thumb.get_mut(thumb_entity) else {
            continue;
        };

        // content_size / size 为物理像素，转逻辑像素
        let scale = computed.inverse_scale_factor();
        let content_h = computed.content_size().y * scale;
        let viewport_h = computed.size().y * scale;
        let geo = compute_thumb_geometry(content_h, viewport_h, scroll.y, bar.min_thumb_height);
        thumb_node.top = geo.top;
        thumb_node.height = geo.height;
    }
}

/// 纯函数：根据内容高/视口高/滚动位置计算滑块的 (top, height)。
///
/// 返回 `Val::Px`，便于直接写回 `Node`。若内容不超出视口，滑块占满轨道。
/// 抽出为纯函数便于单元测试边界与比例。
fn compute_thumb_geometry(
    content_h: f32,
    viewport_h: f32,
    scroll_y: f32,
    min_thumb_h: f32,
) -> ThumbGeometry {
    if content_h <= viewport_h || viewport_h <= 0.0 {
        return ThumbGeometry {
            top: Val::Px(0.0),
            height: Val::Percent(100.0),
        };
    }
    let max_scroll = content_h - viewport_h;
    let scroll_y = scroll_y.clamp(0.0, max_scroll);
    let thumb_h = (viewport_h * viewport_h / content_h).max(min_thumb_h);
    let thumb_top = if max_scroll > 0.0 {
        (scroll_y / max_scroll) * (viewport_h - thumb_h)
    } else {
        0.0
    };
    ThumbGeometry {
        top: Val::Px(thumb_top),
        height: Val::Px(thumb_h),
    }
}

/// 滑块几何（top/height，直接写回 `Node`）。
struct ThumbGeometry {
    top: Val,
    height: Val,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(v: Val) -> f32 {
        match v {
            Val::Px(h) => h,
            _ => f32::NAN,
        }
    }

    #[test]
    fn thumb_fills_track_when_content_fits() {
        // 内容不超出视口：滑块占满轨道（Percent(100)）
        let geo = compute_thumb_geometry(100.0, 200.0, 0.0, 24.0);
        assert!(matches!(geo.height, Val::Percent(100.0)));
        assert!(matches!(geo.top, Val::Px(0.0)));
    }

    #[test]
    fn thumb_height_proportional_to_viewport_content_ratio() {
        // 内容 4× 视口：滑块高度 = viewport²/content = viewport/4
        let geo = compute_thumb_geometry(800.0, 200.0, 0.0, 24.0);
        assert_eq!(px(geo.height), 50.0); // 200*200/800 = 50
        assert_eq!(px(geo.top), 0.0); // 顶部
    }

    #[test]
    fn thumb_top_at_bottom_when_scrolled_to_end() {
        // 滚到底：thumb_top = viewport - thumb_h
        let geo = compute_thumb_geometry(800.0, 200.0, 600.0, 24.0);
        // max_scroll = 800-200 = 600，scroll_y=600 = 底
        assert_eq!(px(geo.top), 150.0); // (600/600)*(200-50) = 150
        assert_eq!(px(geo.height), 50.0);
    }

    #[test]
    fn thumb_top_proportional_to_scroll_position() {
        // 滚到一半：thumb_top = 一半可滚范围
        let geo = compute_thumb_geometry(800.0, 200.0, 300.0, 24.0);
        // max_scroll=600, scroll_y=300 → 一半 → top = 0.5*(200-50) = 75
        assert_eq!(px(geo.top), 75.0);
    }

    #[test]
    fn thumb_height_clamped_to_min() {
        // 内容极大：滑块高度被 min_thumb_height 兜底
        let geo = compute_thumb_geometry(100_000.0, 200.0, 0.0, 24.0);
        assert_eq!(px(geo.height), 24.0); // 200*200/100000=0.4 < 24 → 24
    }

    #[test]
    fn thumb_top_clamped_when_scroll_exceeds_max() {
        // scroll_y 超过 max_scroll（不应发生，但系统应容错）：clamp 到底
        let geo = compute_thumb_geometry(800.0, 200.0, 9999.0, 24.0);
        assert_eq!(px(geo.top), 150.0); // 底部
    }
}
