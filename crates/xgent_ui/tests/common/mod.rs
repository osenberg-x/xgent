//! 设置面板测试共用的 headless 装配。
//!
//! 每个集成测试二进制各自 `mod common;` 编译一份，因此这里允许出现
//! 某个二进制没用到的辅助函数。
#![allow(dead_code)]

use bevy::prelude::*;
use bevy::text::EditableText;
use xgent_ui::fonts::UiFonts;
use xgent_ui::settings_panel::{SettingsPanelPlugin, SettingsPanelState, SettingsSnapshotReady};
use xgent_ui::theme::Theme;

/// 装配一个能跑设置面板的最小 App。
///
/// 面板现在绑真实配置（打开即回填），所以配置资源、provider 状态与执行器资源
/// 都必须齐备；缺任何一个，`toggle_panel` 就 panic。
pub fn panel_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(bevy::image::ImagePlugin::default())
        .add_plugins(bevy::window::WindowPlugin::default())
        .add_message::<bevy::window::WindowEvent>()
        .add_plugins(bevy::picking::input::PointerInputPlugin)
        .add_plugins(bevy::picking::PickingPlugin)
        .add_plugins(bevy::picking::InteractionPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::input_focus::InputFocusPlugin)
        .add_plugins(bevy::input_focus::InputDispatchPlugin)
        .add_plugins(bevy::text::TextPlugin)
        .add_plugins(bevy::ui::UiPlugin)
        .add_plugins(bevy::ui_widgets::UiWidgetsPlugins)
        .init_asset::<bevy::image::TextureAtlasLayout>()
        .add_plugins(bevy::camera::visibility::VisibilityPlugin)
        .add_plugins(bevy::mesh::MeshPlugin)
        .init_resource::<Theme>()
        .init_resource::<xgent_settings::Localizer>()
        .insert_resource(UiFonts {
            ui: Handle::default(),
            mono: Handle::default(),
        })
        .insert_resource(xgent_settings::GlobalConfigRes::default())
        .insert_resource(xgent_settings::ProjectConfigRes::default())
        .insert_resource(xgent_agent::ProviderInfo {
            id: String::new(),
            model: String::new(),
            ready: false,
            kind: None,
        })
        .insert_resource(xgent_tools::ToolExecutorResource(std::sync::Arc::new(
            xgent_tools::ToolExecutor::with_defaults(),
        )))
        .add_plugins(SettingsPanelPlugin)
        .add_message::<xgent_ui::kit::ToastMessage>();
    app.world_mut().spawn(Camera::default());
    app
}

/// 覆盖面板看到的全局配置。
pub fn set_global(app: &mut App, cfg: xgent_settings_core::GlobalConfig) {
    app.world_mut()
        .insert_resource(xgent_settings::GlobalConfigRes(cfg));
}

/// 覆盖面板看到的项目配置。
pub fn set_project(app: &mut App, cfg: xgent_settings_core::ProjectConfig) {
    app.world_mut()
        .insert_resource(xgent_settings::ProjectConfigRes(cfg));
}

/// 替换面板使用的执行器（决定三态行列出哪些工具）。
pub fn set_executor(app: &mut App, executor: std::sync::Arc<xgent_tools::ToolExecutor>) {
    app.world_mut()
        .insert_resource(xgent_tools::ToolExecutorResource(executor));
}

/// 打开面板并渲染表单。
pub fn open_panel(app: &mut App) {
    app.world_mut().resource_mut::<SettingsPanelState>().open = true;
    app.world_mut()
        .resource_mut::<SettingsSnapshotReady>()
        .ready = true;
    for _ in 0..4 {
        app.update();
    }
}

/// 关闭面板（模拟点关闭 / Esc 之类的收起路径）。
pub fn close_panel(app: &mut App) {
    app.world_mut().resource_mut::<SettingsPanelState>().open = false;
    for _ in 0..2 {
        app.update();
    }
}

/// 面板重开一次（关掉再打开），用于验证"重开不丢显示值"。
pub fn reopen_panel(app: &mut App) {
    close_panel(app);
    open_panel(app);
}

/// 按标记取实体（必须恰好一个）。
pub fn one_entity<M: bevy::ecs::component::Component>(app: &mut App) -> Entity {
    let mut q = app
        .world_mut()
        .query_filtered::<Entity, With<M>>()
        .iter(app.world())
        .collect::<Vec<_>>();
    q.sort_by_key(|e| e.index());
    let Some(e) = q.first().copied() else {
        panic!("面板里应有一个带 {} 的节点", std::any::type_name::<M>());
    };
    e
}

/// 某个数值字段输入框的当前文本。
pub fn num_field_text(app: &mut App, key: &str) -> String {
    let mut q = app.world_mut().query::<(
        Entity,
        &EditableText,
        &xgent_ui::settings_panel::SettingsNumInput,
    )>();
    let world = app.world();
    let found: Vec<String> = q
        .iter(world)
        .filter(|(_, _, n)| n.key == key)
        .map(|(_, e, _)| e.value().to_string())
        .collect();
    assert_eq!(
        found.len(),
        1,
        "键 {key} 应恰有一个输入框，实际 {} 个",
        found.len()
    );
    found[0].clone()
}

/// 把某个数值字段的输入框文本改成指定值（等价于用户改框）。
pub fn set_num_field_text(app: &mut App, key: &str, text: &str) {
    let target = {
        let mut q = app
            .world_mut()
            .query::<(Entity, &xgent_ui::settings_panel::SettingsNumInput)>();
        let found: Vec<Entity> = q
            .iter(app.world())
            .filter(|(_, n)| n.key == key)
            .map(|(e, _)| e)
            .collect();
        assert_eq!(found.len(), 1, "键 {key} 应恰有一个输入框");
        found[0]
    };
    let mut e = app
        .world_mut()
        .get_mut::<EditableText>(target)
        .expect("输入框应有 EditableText");
    *e = EditableText::new(text.to_string());
}

/// 把带标记的按钮置为"刚被按下"（等价于交互系统给出的 `Added<Pressed>`）。
pub fn press<M: bevy::ecs::component::Component>(app: &mut App, pick: impl Fn(&M) -> bool) {
    let candidates: Vec<Entity> = {
        let mut q = app.world_mut().query::<(Entity, &M)>();
        q.iter(app.world())
            .filter(|(_, m)| pick(m))
            .map(|(e, _)| e)
            .collect()
    };
    let entity = match candidates.as_slice() {
        [one] => *one,
        other => panic!("应恰好命中一个按钮，实际 {} 个", other.len()),
    };
    app.world_mut().entity_mut(entity).insert(bevy::ui::Pressed);
    app.update();
}

/// 按下"保存"按钮。
pub fn press_save(app: &mut App) {
    let e = one_entity::<xgent_ui::settings_panel::SettingsSaveButtonMarker>(app);
    app.world_mut().entity_mut(e).insert(bevy::ui::Pressed);
    app.update();
}

/// 面板状态行的当前文案。
pub fn status_text(app: &mut App) -> String {
    let mut q = app
        .world_mut()
        .query_filtered::<&Text, With<xgent_ui::settings_panel::SettingsStatusMarker>>();
    let Ok(t) = q.single(app.world()) else {
        return String::new();
    };
    t.0.clone()
}
