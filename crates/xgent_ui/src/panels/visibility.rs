//! 派生可见性（修 I3 / D2）：**唯一**写「面板 / 区域」`Node.display` 的系统。
//!
//! 布局架构 §1 P-1「状态是权威，呈现是派生」：可见性一律由本模块从
//! [`PanelRegistry`] + [`SlotLayout`] 算出，业务代码只发面板消息，不直接写
//! `Node.display`。
//!
//! 受管的节点分两类（见 [`PanelVisibility`]）：
//!
//! - **跟随面板**：面板内容根、抽屉遮罩等——面板可见 **且** 所属槽位开放。
//! - **跟随槽位**：区域容器（右侧分屏）、分割手柄、rail 展开钮等——只跟随
//!   槽位开合。
//!
//! **不在本模块口径内**的是*内容态*可见性（欢迎空态、最近会话标题、回到底部
//! 按钮、编辑器文档页签、陪伴激活环）——它们描述内容状态而非面板 / 区域。

use bevy::prelude::*;

use crate::panels::{PanelRegistry, PanelVisibility, SlotLayout};

/// 唯一写「面板 / 区域」`Node.display` 的系统。
///
/// 仅在期望值与当前值不同时才写（避免每帧 mutation 触发 change detection）。
pub fn derive_panel_visibility(
    registry: Res<PanelRegistry>,
    layout: Res<SlotLayout>,
    mut q: Query<(&PanelVisibility, &mut Node)>,
) {
    for (vis, mut node) in q.iter_mut() {
        let want = match vis.panel {
            Some(id) => layout.is_panel_visible(&registry, id),
            None if vis.slot_panels => {
                layout.is_open(vis.slot)
                    && registry
                        .iter_slot(vis.slot)
                        .any(|entry| layout.is_panel_visible(&registry, entry.desc.id))
            }
            None => {
                let open = layout.is_open(vis.slot);
                // 反向跟随：槽位**关闭**时可见（如 rail 的「展开面板」钮）。
                if vis.invert_slot { !open } else { open }
            }
        };
        let display = if want { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
}

/// 派生可见性插件（v8 M5-T3）。
pub struct VisibilityPlugin;

impl Plugin for VisibilityPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            derive_panel_visibility.in_set(crate::panels::PanelSet::Visibility),
        );
    }
}
