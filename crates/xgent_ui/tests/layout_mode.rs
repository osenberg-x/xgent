//! 4.3：`preferences.three_d_home` 驱动的主区布局切换。
//!
//! 宇宙：Center = space，对话常驻 SideDock。
//! 经典：Center = 对话，编辑器/差异/终端回 SideDock，space 不暴露。
//!
//! 断言全部落在**真实布局树**上（父子关系 + 派生 display），不看中间变量。

use bevy::prelude::*;
use xgent_settings::{GlobalConfigRes, Localizer};
use xgent_settings_core::GlobalConfig;
use xgent_ui::layout::{CenterHostMarker, ChatPanelMarker, LayoutPlugin, SideViewMarker};
use xgent_ui::layout_mode::LayoutMode;
use xgent_ui::panels::visibility::VisibilityPlugin;
use xgent_ui::panels::{PanelDescriptor, PanelRegister, PanelRegistry, PanelVisibility, Slot, ids};

fn find_one<M: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    let mut query = world.query_filtered::<Entity, With<M>>();
    let mut found: Vec<Entity> = query.iter(world).collect();
    assert_eq!(found.len(), 1, "应恰有一个 {}", std::any::type_name::<M>());
    found.remove(0)
}

fn parent_of(app: &App, entity: Entity) -> Option<Entity> {
    app.world().get::<ChildOf>(entity).map(ChildOf::parent)
}

fn display_of(app: &App, entity: Entity) -> Display {
    app.world()
        .get::<Node>(entity)
        .map(|node| node.display)
        .unwrap_or_else(|| panic!("实体 {entity:?} 应有 Node"))
}

/// 装配一个五块视图齐全的最小布局 App。
///
/// `editor` / `diff` / `terminal` / `space` 用最简视图实体代替真实模块：本测试
/// 只验证布局模式搬动视图、改槽位、派生可见性这条链，与各面板内容无关。
fn layout_app(three_d_home: bool) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Localizer>()
        .init_resource::<xgent_ui::editor::EditorView>()
        .insert_resource(GlobalConfigRes(GlobalConfig {
            preferences: xgent_settings_core::Preferences {
                three_d_home,
                ..Default::default()
            },
            ..Default::default()
        }))
        .add_plugins(LayoutPlugin);
    if !app.is_plugin_added::<VisibilityPlugin>() {
        app.add_plugins(VisibilityPlugin);
    }
    if !app.is_plugin_added::<xgent_ui::panels::tabs::TabsPlugin>() {
        app.add_plugins(xgent_ui::panels::tabs::TabsPlugin);
    }
    app.update();

    let center = find_one::<CenterHostMarker>(&mut app);
    for (id, title, icon, order) in [
        (ids::SPACE, "panel-space", "panel-right", 10),
        (ids::EDITOR, "panel-editor", "file", 0),
        (ids::DIFF, "panel-diff", "diff", 1),
        (ids::TERMINAL, "panel-terminal", "terminal", 2),
    ] {
        let view = app
            .world_mut()
            .spawn((
                Node {
                    display: Display::None,
                    ..default()
                },
                PanelVisibility::of_panel(id, Slot::Center),
            ))
            .id();
        app.world_mut().entity_mut(center).add_child(view);
        app.world_mut().write_message(PanelRegister {
            desc: PanelDescriptor::new(id, Slot::Center, title, icon, order),
            view: Some(view),
        });
    }
    for _ in 0..4 {
        app.update();
    }
    app
}

fn view_of(app: &App, id: &str) -> Entity {
    app.world()
        .resource::<PanelRegistry>()
        .get(id)
        .and_then(|entry| entry.runtime.view)
        .unwrap_or_else(|| panic!("面板 {id} 应有视图实体"))
}

fn mode_of(app: &App) -> LayoutMode {
    *app.world().resource::<LayoutMode>()
}

/// 默认偏好 = 宇宙：空间坐 Center 并可见，对话常驻 SideDock。
#[test]
fn universe_layout_is_the_default() {
    let mut app = layout_app(true);
    let chat = find_one::<ChatPanelMarker>(&mut app);
    let center = find_one::<CenterHostMarker>(&mut app);
    let side = find_one::<SideViewMarker>(&mut app);
    let space = view_of(&app, ids::SPACE);

    assert_eq!(mode_of(&app), LayoutMode::Universe);
    assert_eq!(
        parent_of(&app, space),
        Some(center),
        "space 应在 CenterHost"
    );
    assert_eq!(parent_of(&app, chat), Some(side), "对话应在 SideDock");
    assert_eq!(display_of(&app, space), Display::Flex, "启动默认显示空间");
    assert_eq!(display_of(&app, chat), Display::Flex, "对话侧栏常驻");
    assert!(
        app.world()
            .resource::<PanelRegistry>()
            .is_visible(ids::SPACE),
        "space 应为默认激活的面板"
    );
}

/// 关掉 `three_d_home`：对话回 Center，space/编辑器等回 SideDock 且不露头。
#[test]
fn classic_layout_restores_chat_as_center() {
    let mut app = layout_app(true);

    app.world_mut()
        .resource_mut::<GlobalConfigRes>()
        .0
        .preferences
        .three_d_home = false;
    for _ in 0..3 {
        app.update();
    }

    let chat = find_one::<ChatPanelMarker>(&mut app);
    let center = find_one::<CenterHostMarker>(&mut app);
    let side = find_one::<SideViewMarker>(&mut app);
    let space = view_of(&app, ids::SPACE);
    let editor = view_of(&app, ids::EDITOR);

    assert_eq!(mode_of(&app), LayoutMode::Classic);
    assert_eq!(parent_of(&app, chat), Some(center), "对话应回 CenterHost");
    assert_eq!(parent_of(&app, space), Some(side), "space 应退到 SideDock");
    assert_eq!(parent_of(&app, editor), Some(side), "编辑器应在 SideDock");
    assert_eq!(display_of(&app, chat), Display::Flex, "对话接管主区");
    assert_eq!(display_of(&app, space), Display::None, "回退下空间不显示");
    assert_eq!(
        display_of(&app, side),
        Display::None,
        "侧栏里没有可见面板时应整块收起"
    );
    assert!(
        app.world()
            .resource::<PanelRegistry>()
            .is_visible(ids::CHAT),
        "对话应为激活面板"
    );

    let mut tabs = app
        .world_mut()
        .query::<&xgent_ui::panels::tabs::SlotTabMarker>();
    let tab_ids: Vec<&str> = tabs.iter(app.world()).map(|tab| tab.id).collect();
    assert!(
        !tab_ids.contains(&ids::SPACE),
        "经典回退不应在侧栏页签暴露空间：{tab_ids:?}"
    );

    // 经典模式下打开编辑器：侧栏出现、对话仍在主区。
    app.world_mut()
        .write_message(xgent_ui::panels::PanelActivate { id: ids::EDITOR });
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        display_of(&app, editor),
        Display::Flex,
        "编辑器接管 SideDock"
    );
    assert_eq!(
        display_of(&app, side),
        Display::Flex,
        "有可见面板后侧栏应出现"
    );
    assert_eq!(
        display_of(&app, chat),
        Display::Flex,
        "经典模式打开编辑器不影响主区对话"
    );
}

/// 再打开偏好：空间回 Center 当首页，对话仍常驻 SideDock。
#[test]
fn re_enabling_three_d_home_returns_space_to_center() {
    let mut app = layout_app(false);
    let chat = find_one::<ChatPanelMarker>(&mut app);
    let center = find_one::<CenterHostMarker>(&mut app);
    let side = find_one::<SideViewMarker>(&mut app);
    let space = view_of(&app, ids::SPACE);
    assert_eq!(mode_of(&app), LayoutMode::Classic);
    assert_eq!(parent_of(&app, chat), Some(center));

    app.world_mut()
        .resource_mut::<GlobalConfigRes>()
        .0
        .preferences
        .three_d_home = true;
    for _ in 0..3 {
        app.update();
    }

    assert_eq!(mode_of(&app), LayoutMode::Universe);
    assert_eq!(parent_of(&app, space), Some(center));
    assert_eq!(parent_of(&app, chat), Some(side));
    assert_eq!(display_of(&app, space), Display::Flex);
    assert_eq!(display_of(&app, chat), Display::Flex);
}
