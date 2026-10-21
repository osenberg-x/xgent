//! 可覆盖层（抽屉）统一入口（v8 M5：面板注册表裁决）。
//!
//! 文件抽屉与历史抽屉互斥不再由各入口手工维护：所有入口只发 [`DrawerToggle`]，
//! 由本模块转成面板激活 / 失活消息，由**面板注册表**（`Slot::Overlay` 单例
//! 槽位）裁决互斥——二者不再争层级。

use bevy::prelude::*;

use crate::panels::{PanelActivate, PanelDeactivate, PanelRegistry, PanelSet, ids};

/// 可覆盖的抽屉。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayId {
    /// 文件抽屉。
    FileDrawer,
    /// 历史会话抽屉。
    HistoryDrawer,
}

impl OverlayId {
    /// 对应的注册表面板 id。
    pub fn panel_id(&self) -> &'static str {
        match self {
            OverlayId::FileDrawer => ids::FILE_DRAWER,
            OverlayId::HistoryDrawer => ids::SESSION_HISTORY,
        }
    }
}

/// 切换某个抽屉：打开它并关闭其他；再次切换同一个则关闭。
#[derive(Message, Debug, Clone, Copy)]
pub struct DrawerToggle(pub OverlayId);

/// 抽屉裁决插件。
pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        // 面板注册表（幂等：宿主可能已装配）
        if !app.is_plugin_added::<crate::panels::PanelsPlugin>() {
            app.add_plugins(crate::panels::PanelsPlugin);
        }
        app
            // 独立使用时也保证派生资源存在（与 LayoutPlugin/SessionHistoryPlugin 的
            // init_resource 幂等）。
            .init_resource::<crate::layout::FileDrawerOpen>()
            .init_resource::<crate::session_history::SessionHistoryState>()
            .add_message::<DrawerToggle>()
            .add_systems(Update, handle_drawer_toggle.in_set(PanelSet::Requests));
    }
}

/// 抽屉开关 → 面板激活 / 失活消息。
///
/// 互斥由注册表的 `Slot::Overlay` 单例语义保证（激活一个自动让另一个失活）。
fn handle_drawer_toggle(
    mut reader: MessageReader<DrawerToggle>,
    registry: Res<PanelRegistry>,
    mut activate: MessageWriter<PanelActivate>,
    mut deactivate: MessageWriter<PanelDeactivate>,
) {
    for DrawerToggle(id) in reader.read() {
        let panel = id.panel_id();
        if registry.is_visible(panel) {
            deactivate.write(PanelDeactivate { id: panel });
        } else {
            activate.write(PanelActivate { id: panel });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::{PanelDescriptor, Slot};

    /// 测试用：注册两个抽屉面板。
    fn register_drawers(app: &mut App) {
        let mut registry = app.world_mut().resource_mut::<PanelRegistry>();
        registry.register(
            PanelDescriptor::new(ids::FILE_DRAWER, Slot::Overlay, "t", "folder", 0),
            None,
        );
        registry.register(
            PanelDescriptor::new(ids::SESSION_HISTORY, Slot::Overlay, "t", "clock", 1),
            None,
        );
    }

    #[test]
    fn opening_one_drawer_closes_the_other() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(OverlayPlugin);
        register_drawers(&mut app);

        app.world_mut()
            .write_message(DrawerToggle(OverlayId::HistoryDrawer));
        app.update();
        assert!(
            app.world()
                .resource::<PanelRegistry>()
                .is_visible(ids::SESSION_HISTORY)
        );
        assert!(
            !app.world()
                .resource::<PanelRegistry>()
                .is_visible(ids::FILE_DRAWER)
        );

        app.world_mut()
            .write_message(DrawerToggle(OverlayId::FileDrawer));
        app.update();
        assert!(
            app.world()
                .resource::<PanelRegistry>()
                .is_visible(ids::FILE_DRAWER)
        );
        assert!(
            !app.world()
                .resource::<PanelRegistry>()
                .is_visible(ids::SESSION_HISTORY)
        );

        // 再次切换同一个 → 关闭
        app.world_mut()
            .write_message(DrawerToggle(OverlayId::FileDrawer));
        app.update();
        assert!(
            !app.world()
                .resource::<PanelRegistry>()
                .is_visible(ids::FILE_DRAWER)
        );
    }
}
