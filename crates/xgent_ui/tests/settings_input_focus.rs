//! 设置面板输入框端到端回归：打开面板 → 点击输入框 → 敲字符 → 文本进入 `EditableText`。
//!
//! 覆盖「配置 AI provider 窗口无法输入内容」的完整链路：
//! picking 命中 → `PointerPress` → 冒泡 `AcquireFocus` → `TabIndex` 解析出
//! `InputFocus` → `FocusedInput<KeyboardInput>` 派发 → `TextInput` 落字。
//! 任一环节缺失（历史上是 `TabIndex`），敲键都会石沉大海。
//!
//! headless 无渲染器，scale factor = 1，逻辑像素即物理像素。

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input_focus::{InputDispatchPlugin, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::window::{CursorMoved, PrimaryWindow, WindowEvent};
use xgent_settings::Localizer;
use xgent_ui::fonts::UiFonts;
use xgent_ui::settings_panel::{ApiBaseInput, SettingsPanelPlugin, SettingsPanelState};
use xgent_ui::theme::Theme;

/// 与 `tests/interaction_model.rs` 同款 headless 装配 + 设置面板插件。
///
/// 额外需要 [`InputDispatchPlugin`]：把窗口键盘消息派发给 `InputFocus` 实体，
/// 真机由 `DefaultPlugins` 挂载。
fn test_app() -> App {
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
        .add_plugins(SettingsPanelPlugin);

    app.world_mut().spawn(Camera::default());
    app
}

/// 主窗口实体。
fn primary_window(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(app.world())
        .expect("primary window")
}

/// 把光标移到 `pos` 并按下左键（走真实 picking 链路）。
fn click_left(app: &mut App, pos: Vec2) {
    let window = primary_window(app);
    app.world_mut()
        .resource_mut::<Messages<WindowEvent>>()
        .write(WindowEvent::CursorMoved(CursorMoved {
            window,
            position: pos,
            delta: None,
        }));
    app.update();
    app.update();
    app.world_mut()
        .resource_mut::<Messages<WindowEvent>>()
        .write(WindowEvent::MouseButtonInput(
            bevy::input::mouse::MouseButtonInput {
                window,
                button: bevy::input::mouse::MouseButton::Left,
                state: ButtonState::Pressed,
            },
        ));
    app.update();
}

/// 向窗口写一次按键（按下 + 释放）。
fn type_char(app: &mut App, ch: &str, key_code: KeyCode) {
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

/// 端到端：点 API Base 输入框并敲字，文本须出现在框里。
#[test]
fn settings_panel_input_accepts_typing() {
    let mut app = test_app();

    // 打开设置面板（真实路径：顶栏齿轮 / 命令面板置位）
    app.world_mut().resource_mut::<SettingsPanelState>().open = true;
    for _ in 0..4 {
        app.update();
    }

    let (input, center) = {
        let mut q = app
            .world_mut()
            .query_filtered::<(Entity, &ComputedNode), With<ApiBaseInput>>();
        let (entity, node) = q.single(app.world()).expect("面板应已 spawn 输入框");
        (entity, node.border_box().center())
    };
    let size = app.world().get::<ComputedNode>(input).unwrap().size();
    assert!(
        size.x > 1.0 && size.y > 1.0,
        "headless 下输入框须有非零尺寸，否则命中测试无意义（实测 {size:?}）"
    );

    click_left(&mut app, center);
    assert_eq!(
        app.world().resource::<InputFocus>().get(),
        Some(input),
        "点击后焦点应落在该输入框"
    );

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
