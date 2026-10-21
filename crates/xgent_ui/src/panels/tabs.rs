//! 槽位页签条（修 D3）：**唯一**的侧区顶部页签条，由 [`SlotLayout`] 拥有并渲染。
//!
//! 布局架构 §3.2 / §12 风险预案：
//!
//! - 侧区顶部的页签条是**槽位页签**（面板切换），由本模块从注册表的
//!   `Slot::SideDock` 面板生成，标题取描述符的 `title_key`（i18n 单源）。
//! - 编辑器内部的 tab 条是**文档列表**（打开的代码文件），与槽位页签**不是
//!   一回事、不合并**（见 `editor/tabs.rs`）。
//! - 侧区顶部**只有一个返回入口**（[`EditorBackButtonMarker`]，全仓唯一定义）。

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;

use xgent_settings::Localizer;

use crate::i18n::tr;
use crate::panels::{PanelActivate, PanelRegistry, Slot, SlotLayout, ids};
use crate::theme::{Theme, space, type_scale};

/// 槽位页签按钮标记。
#[derive(Component, Debug, Clone)]
pub struct SlotTabMarker {
    /// 目标面板 id。
    pub id: &'static str,
}

/// 侧区返回入口标记（收起侧区）。
///
/// 全仓**唯一定义**（原 `editor/mod.rs` 有两套：`EditorViewMarker` 顶栏的 ✕ 与
/// 槽位页签条的 ✕；M5-T4 合一，编辑器顶栏的 ✕ 随「文档列表」语义一并移除）。
#[derive(Component, Default, Debug, Clone)]
pub struct EditorBackButtonMarker;

/// 槽位页签条容器标记。
#[derive(Component, Default, Debug, Clone)]
pub struct SlotTabBarMarker;

/// 启动时在右侧分屏容器顶部 spawn 槽位页签条。
pub fn spawn_slot_tab_bar(
    mut commands: Commands,
    q_side: Query<Entity, With<crate::layout::SideViewMarker>>,
    theme: Res<Theme>,
) {
    let Ok(side) = q_side.single() else {
        return;
    };
    let bar = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: px(crate::theme::size::CONTEXT_TABS_H),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(space::XS)),
                border: UiRect::bottom(px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme.surface),
            BorderColor::all(theme.line),
            SlotTabBarMarker,
        ))
        .id();
    commands.entity(side).insert_children(0, &[bar]);
}

/// 注册表变化时重建页签条目（槽位页签 = `Slot::SideDock` 面板）。
pub fn sync_slot_tab_bar(
    registry: Res<PanelRegistry>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    mut commands: Commands,
    q_bar: Query<Entity, With<SlotTabBarMarker>>,
    q_tabs: Query<Entity, With<SlotTabMarker>>,
    mut signature: Local<Vec<(&'static str, i16)>>,
) {
    let wanted: Vec<(&'static str, i16)> = registry
        .iter_slot(Slot::SideDock)
        .map(|e| (e.desc.id, e.desc.order))
        .collect();
    if wanted == *signature {
        return;
    }
    *signature = wanted.clone();
    let Ok(bar) = q_bar.single() else {
        return;
    };
    for entity in q_tabs.iter() {
        commands.entity(entity).despawn();
    }
    for (id, _) in wanted {
        let title = registry
            .get(id)
            .map(|e| tr(&loc, e.desc.title_key))
            .unwrap_or_else(|| id.to_string());
        let tab = commands
            .spawn((
                Button,
                Hovered::default(),
                Node {
                    height: Val::Percent(100.0),
                    padding: UiRect::horizontal(px(space::MD)),
                    align_items: AlignItems::Center,
                    border: UiRect::bottom(px(2.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                crate::fonts::ui_text(
                    title,
                    type_scale::SMALL,
                    510,
                    theme.text_muted,
                    type_scale::line_height::UI,
                ),
                SlotTabMarker { id },
            ))
            .id();
        commands.entity(bar).add_child(tab);
    }
}

/// 页签点击：激活对应面板（消息）。
pub fn handle_slot_tab_click(
    q: Query<&SlotTabMarker, (Added<Pressed>, With<Button>)>,
    mut activate: MessageWriter<PanelActivate>,
) {
    for tab in q.iter() {
        activate.write(PanelActivate { id: tab.id });
    }
}

/// 页签指示器：当前可见的面板页签用 `accent_interactive` 文字 + 底部 2px 线。
pub fn update_slot_tab_indicators(
    registry: Res<PanelRegistry>,
    theme: Res<Theme>,
    mut q: Query<(&SlotTabMarker, &mut TextColor, &mut Node, &mut BorderColor), With<Button>>,
) {
    if !registry.is_changed() && !theme.is_changed() {
        return;
    }
    for (tab, mut color, mut node, mut border) in q.iter_mut() {
        let is_active = registry.is_visible(tab.id);
        let want_color = if is_active {
            theme.accent_interactive
        } else {
            theme.text_muted
        };
        if color.0 != want_color {
            color.0 = want_color;
        }
        node.border = UiRect {
            bottom: px(if is_active { 2.0 } else { 0.0 }),
            ..default()
        };
        border.set_all(if is_active {
            theme.accent_interactive
        } else {
            Color::NONE
        });
    }
}

/// 侧区返回入口：收起侧区（关闭 `Slot::SideDock`）并回到主区对话。
///
/// 全仓唯一的返回入口（`EditorBackButtonMarker` 只定义一次）。
pub fn handle_back_button_click(
    q_btn: Query<(), (With<EditorBackButtonMarker>, Added<Pressed>)>,
    registry: Res<PanelRegistry>,
    mut deactivate: MessageWriter<crate::panels::PanelDeactivate>,
    mut activate: MessageWriter<PanelActivate>,
    mut layout: ResMut<SlotLayout>,
    mut view: ResMut<crate::editor::EditorView>,
) {
    for _ in q_btn.iter() {
        // 关闭侧区里的所有面板
        for entry in registry.iter_slot(Slot::SideDock) {
            deactivate.write(crate::panels::PanelDeactivate { id: entry.desc.id });
        }
        layout.set_open(Slot::SideDock, false);
        // 回到主区对话
        activate.write(PanelActivate { id: ids::CHAT });
        *view = crate::editor::EditorView::Chat;
    }
}

/// 槽位页签条插件（v8 M5-T4）。
pub struct TabsPlugin;

impl Plugin for TabsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            spawn_slot_tab_bar.after(crate::layout::spawn_layout),
        )
        .add_systems(
            Update,
            (
                handle_slot_tab_click.in_set(crate::panels::PanelSet::Requests),
                handle_back_button_click.in_set(crate::panels::PanelSet::Requests),
                sync_slot_tab_bar,
                update_slot_tab_indicators,
            )
                .in_set(crate::panels::PanelSet::Visibility),
        );
    }
}
