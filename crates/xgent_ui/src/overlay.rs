//! 可覆盖层（抽屉）统一裁决（v8 M1，修 B2）。
//!
//! 文件抽屉与历史抽屉互斥不再由各入口手工维护：所有入口只发 [`DrawerToggle`]，
//! 由 [`handle_drawer_toggle`] 单一系统决定开谁、关谁，并把结果写回
//! `FileDrawerOpen` / `SessionHistoryState`（派生视图）。

use bevy::prelude::*;

use crate::layout::FileDrawerOpen;
use crate::session_history::SessionHistoryState;

/// 可覆盖的抽屉。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayId {
    /// 文件抽屉。
    FileDrawer,
    /// 历史会话抽屉。
    HistoryDrawer,
}

/// 当前打开的可覆盖层（同一时刻至多一个）。
#[derive(Resource, Debug, Default)]
pub struct OverlayRegistry {
    open: Option<OverlayId>,
}

impl OverlayRegistry {
    /// 当前打开的层。
    pub fn open(&self) -> Option<OverlayId> {
        self.open
    }
}

/// 切换某个抽屉：打开它并关闭其他；再次切换同一个则关闭。
#[derive(Message, Debug, Clone, Copy)]
pub struct DrawerToggle(pub OverlayId);

/// 抽屉裁决插件。
pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OverlayRegistry>()
            // 独立使用时也保证派生资源存在（与 LayoutPlugin/SessionHistoryPlugin 的
            // init_resource 幂等）。
            .init_resource::<FileDrawerOpen>()
            .init_resource::<SessionHistoryState>()
            .add_message::<DrawerToggle>()
            .add_systems(Update, handle_drawer_toggle);
    }
}

fn handle_drawer_toggle(
    mut reader: MessageReader<DrawerToggle>,
    mut registry: ResMut<OverlayRegistry>,
    mut file: ResMut<FileDrawerOpen>,
    mut history: ResMut<SessionHistoryState>,
) {
    for DrawerToggle(id) in reader.read() {
        registry.open = if registry.open == Some(*id) {
            None
        } else {
            Some(*id)
        };
        file.0 = registry.open == Some(OverlayId::FileDrawer);
        history.open = registry.open == Some(OverlayId::HistoryDrawer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_one_drawer_closes_the_other() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<FileDrawerOpen>()
            .init_resource::<SessionHistoryState>()
            .add_plugins(OverlayPlugin);

        app.world_mut().write_message(DrawerToggle(OverlayId::HistoryDrawer));
        app.update();
        assert!(app.world().resource::<SessionHistoryState>().open);
        assert!(!app.world().resource::<FileDrawerOpen>().0);

        app.world_mut().write_message(DrawerToggle(OverlayId::FileDrawer));
        app.update();
        assert!(app.world().resource::<FileDrawerOpen>().0);
        assert!(!app.world().resource::<SessionHistoryState>().open);

        // 再次切换同一个 → 关闭
        app.world_mut().write_message(DrawerToggle(OverlayId::FileDrawer));
        app.update();
        assert!(!app.world().resource::<FileDrawerOpen>().0);
    }
}
