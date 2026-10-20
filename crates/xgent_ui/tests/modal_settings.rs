//! v8 M1：设置面板经模态栈 dismiss 的集成断言（A3/A6 的面板侧）。

#![cfg(test)]

mod common;

use bevy::input::ButtonState;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::prelude::*;
use bevy::ui::GlobalZIndex;

use xgent_ui::modal::{ModalPlugin, ModalStack};
use xgent_ui::settings_panel::{SettingsPanelMarker, SettingsPanelState, SettingsSnapshotReady};
use xgent_ui::theme::z;

fn panel_count(app: &mut App) -> usize {
    let mut query = app
        .world_mut()
        .query_filtered::<Entity, With<SettingsPanelMarker>>();
    query.iter(app.world()).count()
}

#[test]
fn esc_closes_settings_through_modal_stack() {
    let mut app = common::panel_app();
    app.add_plugins(ModalPlugin)
        .add_message::<xgent_agent::AbortMessage>();

    let opener = app.world_mut().spawn_empty().id();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(opener, FocusCause::Navigated);

    common::open_panel(&mut app);
    assert_eq!(panel_count(&mut app), 1, "面板应已打开");
    assert_eq!(
        app.world().resource::<ModalStack>().top().map(|l| l.id),
        Some("settings"),
        "设置面板应在模态栈顶"
    );

    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Escape,
        logical_key: Key::Escape,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
    // 第一帧：Esc → dismiss 请求 → state.open=false；第二帧：despawn + 出栈。
    app.update();
    app.update();

    assert_eq!(panel_count(&mut app), 0, "Esc 应关闭设置面板");
    assert!(
        app.world().resource::<ModalStack>().is_empty(),
        "关闭后模态栈应为空"
    );
    assert!(
        !app.world().resource::<SettingsPanelState>().open,
        "面板状态应回到关闭"
    );
    assert_eq!(
        app.world().resource::<InputFocus>().get(),
        Some(opener),
        "关闭后焦点应回到打开前的实体"
    );

    let messages = app.world().resource::<Messages<xgent_agent::AbortMessage>>();
    let mut cursor = messages.get_cursor();
    assert_eq!(
        cursor.read(messages).count(),
        0,
        "Esc 关面板不得穿透成 chat.abort"
    );
}

#[test]
fn settings_close_button_pops_modal_stack() {
    let mut app = common::panel_app();
    app.add_plugins(ModalPlugin);

    common::open_panel(&mut app);
    assert!(!app.world().resource::<ModalStack>().is_empty());

    common::close_panel(&mut app);
    assert_eq!(panel_count(&mut app), 0);
    assert!(app.world().resource::<ModalStack>().is_empty());
}

/// 快照 tick 推进会触发面板重建：重建后必须仍在栈顶且拿到模态 z。
#[test]
fn rerender_keeps_settings_registered_in_modal_stack() {
    let mut app = common::panel_app();
    app.add_plugins(ModalPlugin);

    common::open_panel(&mut app);
    assert_eq!(panel_count(&mut app), 1);

    // settings_io 每次快照响应都会 tick += 1，面板随之 despawn + respawn。
    app.world_mut()
        .resource_mut::<SettingsSnapshotReady>()
        .tick += 1;
    for _ in 0..3 {
        app.update();
    }

    assert_eq!(panel_count(&mut app), 1, "重建后应只有一个面板");
    let stack = app.world().resource::<ModalStack>();
    let top = stack.top().expect("重建后仍在模态栈内");
    assert_eq!(top.id, "settings");
    assert_eq!(
        app.world().get::<GlobalZIndex>(top.entity).map(|index| index.0),
        Some(z::MODAL_BASE),
        "重建后的面板应重新拿到模态 z"
    );
}
