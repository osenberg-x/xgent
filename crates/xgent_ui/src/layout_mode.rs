//! MVP-2 主区布局模式：宇宙（3D 首页）/ 经典 2D 回退。
//!
//! 两种模式只差**谁坐 Center、谁坐 SideDock**：
//!
//! | 模式 | Center | SideDock |
//! |:---|:---|:---|
//! | `Universe`（MVP-2 默认） | `space` / 编辑器 / 差异 / 终端 | 对话（常驻） |
//! | `Classic`（MVP-1 回退） | 对话 | 编辑器 / 差异 / 终端 / `space`（不暴露） |
//!
//! 权威始终是 [`PanelRegistry`] 的槽位：本模块只把「偏好 → 模式 → 注册表槽位 +
//! 视图父子关系」这条链走完，可见性仍由 `derive_panel_visibility` 派生。
//! `space` 在 `Classic` 下也挪到 SideDock 且不激活，避免命令面板把它叫到主区顶掉对话。

use bevy::prelude::*;
use xgent_settings::GlobalConfigRes;

use crate::layout::{CenterHostMarker, SideViewMarker};
use crate::panels::{
    PanelActivate, PanelDeactivate, PanelRegistry, PanelRetargetSlot, PanelSet, PanelVisibility,
    Slot, ids,
};

/// 主区布局模式。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutMode {
    /// 宇宙：Center 是 3D 空间，对话常驻右侧（MVP-2 默认）。
    #[default]
    Universe,
    /// 经典：Center 是对话，编辑器/终端在右侧（MVP-1）。
    Classic,
}

impl LayoutMode {
    /// 由 `preferences.three_d_home` 推出模式。
    pub fn from_preference(enabled: bool) -> Self {
        if enabled {
            Self::Universe
        } else {
            Self::Classic
        }
    }

    /// 对话所在槽位。
    pub fn chat_slot(self) -> Slot {
        match self {
            Self::Universe => Slot::SideDock,
            Self::Classic => Slot::Center,
        }
    }

    /// 工作区面板（3D / 编辑器 / 差异 / 终端）所在槽位。
    pub fn work_slot(self) -> Slot {
        match self {
            Self::Universe => Slot::Center,
            Self::Classic => Slot::SideDock,
        }
    }

    /// 主区首页面板：Esc / 关闭工作区面板后回到这里。
    pub fn home_panel(self) -> &'static str {
        match self {
            Self::Universe => ids::SPACE,
            Self::Classic => ids::CHAT,
        }
    }
}

/// 已经完成一次完整应用的模式（`None` = 还没凑齐五块视图）。
#[derive(Resource, Default)]
struct AppliedLayoutMode(Option<LayoutMode>);

/// 布局模式插件。
pub struct LayoutModePlugin;

impl Plugin for LayoutModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LayoutMode>()
            .init_resource::<AppliedLayoutMode>()
            .add_systems(Update, apply_layout_mode.in_set(PanelSet::Requests));
    }
}

/// 五块视图全部注册后，一次性把槽位 / 父子关系 / 首页面板切到位。
///
/// 缺视图时直接返回，下一帧重试——启动期各模块的 `PostStartup` 与注册表消息
/// 处理存在一帧时差，强行在半套视图上应用会把先到的那几块切错。
fn apply_layout_mode(
    global: Option<Res<GlobalConfigRes>>,
    mut mode: ResMut<LayoutMode>,
    mut applied: ResMut<AppliedLayoutMode>,
    registry: Option<Res<PanelRegistry>>,
    q_center: Query<Entity, With<CenterHostMarker>>,
    q_side: Query<Entity, With<SideViewMarker>>,
    q_parent: Query<&ChildOf>,
    mut vis: Query<&mut PanelVisibility>,
    mut commands: Commands,
    mut retarget: MessageWriter<PanelRetargetSlot>,
    mut activate: MessageWriter<PanelActivate>,
    mut deactivate: MessageWriter<PanelDeactivate>,
) {
    let want = LayoutMode::from_preference(
        global
            .as_deref()
            .is_none_or(|config| config.0.preferences.three_d_home),
    );
    if *mode != want {
        *mode = want;
    }
    if applied.0 == Some(want) {
        return;
    }
    let Some(registry) = registry.as_deref() else {
        return;
    };
    let (Ok(center), Ok(side)) = (q_center.single(), q_side.single()) else {
        return;
    };

    let required = [ids::CHAT, ids::SPACE, ids::EDITOR, ids::DIFF, ids::TERMINAL];
    if required.iter().any(|id| {
        registry
            .get(id)
            .and_then(|entry| entry.runtime.view)
            .is_none()
    }) {
        return;
    }

    for id in required {
        let slot = if id == ids::CHAT {
            want.chat_slot()
        } else {
            want.work_slot()
        };
        if registry.get(id).is_none_or(|entry| entry.desc.slot != slot) {
            retarget.write(PanelRetargetSlot { id, slot });
        }
        let Some(view) = registry.get(id).and_then(|entry| entry.runtime.view) else {
            continue;
        };
        // 视图自己的可见性跟随同一槽位，否则容器搬了、display 还按旧槽位算。
        if let Ok(mut visibility) = vis.get_mut(view)
            && visibility.slot != slot
        {
            visibility.slot = slot;
        }
        let parent = if slot == Slot::Center { center } else { side };
        if q_parent.get(view).map(ChildOf::parent) != Ok(parent) {
            commands.entity(parent).add_child(view);
        }
    }

    match want {
        // 宇宙：空间回 Center 当首页；对话一直在 SideDock，不动。
        LayoutMode::Universe => {
            activate.write(PanelActivate { id: ids::SPACE });
        }
        // 经典：对话回 Center；space 留在 SideDock 且失活，免得它顶掉对话。
        LayoutMode::Classic => {
            deactivate.write(PanelDeactivate { id: ids::SPACE });
            activate.write(PanelActivate { id: ids::CHAT });
        }
    }
    applied.0 = Some(want);
}
