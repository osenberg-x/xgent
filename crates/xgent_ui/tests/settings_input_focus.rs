//! 设置面板输入框回归：打开面板 → 输入框持有焦点 → 敲字符 → 文本进入 `EditableText`。
//!
//! 覆盖「配置 AI provider 窗口无法输入内容」那条历史缺陷的后半段链路：
//! `InputFocus` 指向带 `TabIndex` 的输入框 → `FocusedInput<KeyboardInput>` 派发 →
//! `TextInput` 落字。缺 `TabIndex` 时敲键石沉大海，本用例当场失败。
//!
//! **为什么不走真实点击**：headless 装配里没有 `UiRoot`，Bevy 的 UI 布局在这种
//! 装配下只算尺寸、不产出子节点坐标——实测面板里每个节点的 `border_box().min`
//! 全部相同（一起重叠在原点），picking 命中谁取决于 spawn 顺序而不是位置。
//! `PointerPress → AcquireFocus` 那半段只能在真实窗口里验；本用例因此把焦点
//! 直接设到目标输入框，断言"焦点到位后键盘确实能落字"这一段。

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input_focus::{FocusCause, InputDispatchPlugin, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::window::{PrimaryWindow, WindowEvent};
use xgent_settings::Localizer;
use xgent_ui::fonts::UiFonts;
use xgent_ui::settings_panel::{
    ApiBaseInput, SettingsPanelPlugin, SettingsPanelState, SettingsSnapshotReady,
};
use xgent_ui::theme::Theme;

/// headless 装配 + 设置面板插件 + 面板所绑的那套配置资源。
///
/// 面板现在打开即回填真实配置，所以 `GlobalConfigRes` / `ProjectConfigRes` /
/// `ProviderInfo` / `ToolExecutorResource` 都必须存在（缺一个系统就 panic）。
pub(crate) fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(bevy::image::ImagePlugin::default())
        .add_plugins(bevy::window::WindowPlugin::default())
        .add_message::<WindowEvent>()
        .add_plugins(bevy::picking::input::PointerInputPlugin)
        .add_plugins(bevy::picking::PickingPlugin)
        .add_plugins(bevy::picking::InteractionPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::input_focus::InputFocusPlugin)
        .add_plugins(InputDispatchPlugin)
        .add_plugins(bevy::text::TextPlugin)
        .add_plugins(bevy::ui::UiPlugin)
        .add_plugins(bevy::ui_widgets::UiWidgetsPlugins)
        .init_asset::<bevy::image::TextureAtlasLayout>()
        .add_plugins(bevy::camera::visibility::VisibilityPlugin)
        .add_plugins(bevy::mesh::MeshPlugin)
        .init_resource::<Theme>()
        .init_resource::<Localizer>()
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

/// 主窗口实体。
pub(crate) fn primary_window(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(app.world())
        .expect("primary window")
}

/// 面板里每个输入框都必须带 `TabIndex`。
///
/// 真机的点击聚焦链路是 `PointerPress` → 冒泡 `AcquireFocus` → 按 `TabIndex`
/// 解析出 `InputFocus`，缺它时点击会把焦点清到窗口、敲字全无反应（历史缺陷）。
/// headless 装配拿不到可用的 picking 几何（文件顶部已测量说明），所以这里锁组件
/// 契约：删掉 `text_input_node` 里的 `TabIndex(0)` 会直接让本用例失败，而键盘派发
/// 那一段由上一个用例覆盖，点击机制本身由 `tests/interaction_model.rs` 覆盖。
#[test]
fn every_settings_input_carries_tab_index() {
    use bevy::input_focus::tab_navigation::TabIndex;
    use bevy::ui_widgets::TextInput;
    let mut app = test_app();
    open_panel(&mut app);
    let mut q = app
        .world_mut()
        .query::<(Entity, &EditableText, &TabIndex)>()
        .iter(&app.world())
        .map(|(e, _, _)| e)
        .collect::<Vec<_>>();
    let with_input = {
        let mut tq = app
            .world_mut()
            .query_filtered::<Entity, With<TextInput>>()
            .iter(app.world())
            .collect::<Vec<_>>();
        tq.sort();
        tq
    };
    assert!(
        with_input.len() >= 10,
        "面板应有一批输入框，实测 {} 个",
        with_input.len()
    );
    q.sort();
    let missing: Vec<Entity> = with_input
        .iter()
        .filter(|e| !q.contains(e))
        .cloned()
        .collect();
    assert!(
        missing.is_empty(),
        "这些输入框缺 TabIndex，点击将无法聚焦: {missing:?}"
    );
}

/// 打开面板并等它渲染出表单（快照就绪后才渲染字段）。
pub(crate) fn open_panel(app: &mut App) {
    app.world_mut().resource_mut::<SettingsPanelState>().open = true;
    app.world_mut()
        .resource_mut::<SettingsSnapshotReady>()
        .ready = true;
    for _ in 0..4 {
        app.update();
    }
}

/// 向窗口写一次按键（按下 + 释放）。
pub(crate) fn type_char(app: &mut App, ch: &str, key_code: KeyCode) {
    let window = primary_window(app);
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            window,
            key_code,
            logical_key: Key::Character(ch.into()),
            state,
            text: (state == ButtonState::Pressed).then(|| ch.into()),
            repeat: false,
        });
        app.update();
    }
}

/// 焦点在 API Base 输入框上时敲字，字符须进入该框。
#[test]
fn settings_panel_input_accepts_typing() {
    let mut app = test_app();
    open_panel(&mut app);

    let input = app
        .world_mut()
        .query_filtered::<Entity, With<ApiBaseInput>>()
        .single(app.world())
        .expect("面板应已 spawn API Base 输入框");
    let size = app
        .world()
        .get::<ComputedNode>(input)
        .expect("输入框应有 ComputedNode")
        .size();
    assert!(
        size.x > 1.0 && size.y > 1.0,
        "输入框须有非零尺寸（实测 {size:?}）"
    );

    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(input, FocusCause::Pressed);
    app.update();

    type_char(&mut app, "o", KeyCode::KeyO);
    type_char(&mut app, "k", KeyCode::KeyK);
    let value = app
        .world()
        .get::<EditableText>(input)
        .expect("输入框应有 EditableText")
        .value()
        .to_string();
    assert_eq!(value, "ok", "敲入的字符应进入输入框");
}
