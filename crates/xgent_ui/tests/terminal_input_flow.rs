//! 终端键盘→行编辑→PTY 字节消息流测试。
//!
//! 复现用户报告：输入 `ls` 回车后，PTY 回显 `lls`（首字母重复）。
//! 本测试不跑真实 PTY，只验证 `handle_terminal_keyboard` + `handle_line_submit`
//! 把 `ls<Enter>` 键序列翻译成的 `TerminalInput` 字节恰好是 `ls\n`，
//! 不含首字母重复。

use bevy::ecs::message::MessageReader;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input::ButtonInput;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;

use xgent_ui::editor::SideViewContent;
use xgent_ui::terminal::input::{
    handle_line_submit, handle_terminal_keyboard, TerminalInputState, TerminalLineSubmitted,
};
use xgent_ui::terminal::io::TerminalInput;
use xgent_ui::terminal::{TerminalTab, TerminalTabStatus, TerminalTabs};

/// 收集系统：把所有 `TerminalInput` 消息字节拼进全局 `CapturedBytes` Resource。
#[derive(Resource, Default)]
struct CapturedBytes(Vec<u8>);

fn capture_terminal_input(
    mut reader: MessageReader<TerminalInput>,
    mut captured: ResMut<CapturedBytes>,
) {
    for msg in reader.read() {
        captured.0.extend_from_slice(&msg.bytes);
    }
}

/// 构造最小 App：注册消息系统 + 必要 Resource，注入键盘事件。
fn make_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<KeyboardInput>()
        .add_message::<TerminalInput>()
        .add_message::<TerminalLineSubmitted>()
        .init_resource::<TerminalInputState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputFocus>()
        .init_resource::<CapturedBytes>()
        .insert_resource(SideViewContent::Terminal)
        .init_resource::<TerminalTabs>();
    app.add_systems(
        Update,
        (
            handle_line_submit,
            handle_terminal_keyboard,
            capture_terminal_input,
        )
            .chain(),
    );
    app
}

/// 构造一个字符键的 `KeyboardInput`（Pressed，logical_key=Character，text=Some）。
fn char_key(ch: char) -> KeyboardInput {
    KeyboardInput {
        key_code: KeyCode::KeyL, // 仅占位，字符分支走 _ => 不看 key_code
        logical_key: Key::Character(ch.to_string().into()),
        state: bevy::input::ButtonState::Pressed,
        text: Some(ch.to_string().into()),
        repeat: false,
        window: Entity::PLACEHOLDER,
    }
}

/// 构造 Enter 键事件。
fn enter_key() -> KeyboardInput {
    KeyboardInput {
        key_code: KeyCode::Enter,
        logical_key: Key::Enter,
        state: bevy::input::ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    }
}

/// spawn 一个激活的 terminal tab，返回其 Entity。
fn spawn_active_tab(app: &mut App) -> Entity {
    let tab = app
        .world_mut()
        .spawn((
            TerminalTab {
                pty_id: None,
                title: "shell #1".into(),
                status: TerminalTabStatus::Running,
                shell: xgent_terminal::ShellSpec::FromEnv,
                cwd: std::env::temp_dir(), exit_code: None,
            },
        ))
        .id();
    app.world_mut().resource_mut::<TerminalTabs>().open(tab);
    tab
}

/// `ls<Enter>` 应产生 `ls\n`，首字母不重复。
#[test]
fn ls_enter_produces_exact_bytes() {
    let mut app = make_app();
    spawn_active_tab(&mut app);

    // 注入键盘事件：l, s, Enter（每个一帧，对齐用户逐键输入）
    app.world_mut().write_message(char_key('l'));
    app.update();
    app.world_mut().write_message(char_key('s'));
    app.update();
    app.world_mut().write_message(enter_key());
    app.update();
    // Enter 写入的 TerminalLineSubmitted 在下一帧才被 handle_line_submit 消费
    app.update();

    let captured = app.world().resource::<CapturedBytes>();
    let text = String::from_utf8_lossy(&captured.0);
    assert_eq!(
        text, "ls\n",
        "ls<Enter> 应产生恰好 ls\\n，实际: {text:?}"
    );
}

/// 验证 `ev.text` 与 `logical_key` 都命中时只 insert 一次（if/else-if 互斥）。
#[test]
fn single_char_key_inserts_once_even_if_text_and_logical_both_set() {
    let mut app = make_app();
    spawn_active_tab(&mut app);

    app.world_mut().write_message(char_key('a'));
    app.update();

    let state = app.world().resource::<TerminalInputState>();
    assert_eq!(state.buffer, "a", "单次按键应只 insert 一次");
}

/// 验证 `ev.text = None` 时 fallback 到 `logical_key`，也只 insert 一次。
#[test]
fn char_key_without_text_falls_back_to_logical_key() {
    let mut app = make_app();
    spawn_active_tab(&mut app);

    // text = None, logical_key = Character("b")
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::KeyB,
        logical_key: Key::Character("b".into()),
        state: bevy::input::ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
    app.update();

    let state = app.world().resource::<TerminalInputState>();
    assert_eq!(state.buffer, "b", "text=None 时应 fallback logical_key");
}

/// 验证同一帧注入 l, s, Enter（最快输入路径）仍产生 `ls\n`。
#[test]
fn ls_enter_single_frame_produces_exact_bytes() {
    let mut app = make_app();
    spawn_active_tab(&mut app);

    app.world_mut().write_message(char_key('l'));
    app.world_mut().write_message(char_key('s'));
    app.world_mut().write_message(enter_key());
    app.update();
    // Enter 的 TerminalLineSubmitted 需下一帧消费
    app.update();

    let captured = app.world().resource::<CapturedBytes>();
    let text = String::from_utf8_lossy(&captured.0);
    assert_eq!(text, "ls\n", "单帧 ls<Enter> 应产生 ls\\n，实际: {text:?}");
}

/// 复现用户报告：输入 `ls` 回车后回显 `lls`（首字母重复）。
///
/// 假设根因：macOS 键重复（`KeyboardInput.repeat == true`）未被过滤，
/// 导致 `l` 键的 repeat 事件二次 insert，产生 `lls`。
/// 本测试注入 `l`(repeat=false) + `l`(repeat=true) + `s` + Enter，
/// 断言修复后只产生 `ls\n`。
#[test]
fn repeat_key_does_not_duplicate_char() {
    let mut app = make_app();
    spawn_active_tab(&mut app);

    // l（首次按下）
    let mut l_press = char_key('l');
    l_press.repeat = false;
    app.world_mut().write_message(l_press);
    app.update();

    // l（系统 key repeat 事件）
    let mut l_repeat = char_key('l');
    l_repeat.repeat = true;
    app.world_mut().write_message(l_repeat);
    app.update();

    // s
    app.world_mut().write_message(char_key('s'));
    app.update();

    // Enter
    app.world_mut().write_message(enter_key());
    app.update();
    app.update();

    let captured = app.world().resource::<CapturedBytes>();
    let text = String::from_utf8_lossy(&captured.0);
    assert_eq!(
        text, "ls\n",
        "key repeat 不应导致首字母重复，期望 ls\\n，实际: {text:?}"
    );
}
