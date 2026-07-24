//! 终端键盘透传：按键直接发原始字节给 PTY，shell 回显承担唯一显示。
//!
//! 详见 `doc/design/terminal-design.md` §3.3、§2.3。
//!
//! PTY 保持 cooked 模式，shell 自带 readline（行编辑/历史/补全）。
//! UI 不本地镜像字符——避免「输入框 + shell 回显」双显。
//! 键盘事件直接转为字节发 [`TerminalInput`]，由 shell 回显产生可见输入。
//!
//! 控制字符 Ctrl+C / Ctrl+D 即时单字节发送。
//!
//! 焦点：终端视图激活（`SideViewContent::Terminal`）时捕获键盘；否则忽略。

use bevy::input::keyboard::KeyCode;
use bevy::input::ButtonInput;
use bevy::prelude::*;

use crate::editor::SideViewContent;
use crate::terminal::io::TerminalInput;
use crate::terminal::TerminalTabs;

/// 终端键盘透传处理：终端视图激活且无输入框聚焦时捕获 `KeyboardInput` 事件，
/// 把按键转为原始字节直接发 [`TerminalInput`]（经 `handle_terminal_input` 送 PTY）。
///
/// - 字符键 → 发对应 UTF-8 字节
/// - Enter → 发 `\n`
/// - Backspace → 发 `\x7f`（DEL，shell readline 识别为退格）
/// - Ctrl+C/D → 即时发 `\x03`/`\x04`
/// - Ctrl+A/E/U → 发 readline 控制字节（行首/行末/删行，交给 shell 处理）
/// - ←→ Home End → 发对应 ANSI 光标移动转义（shell readline 识别）
///
/// 焦点互斥：当对话输入区/命令面板等 `EditableText` 获得焦点时，
/// `InputFocus` 非空，终端不捕获键盘——避免两个输入区同步输入。
/// 切到终端视图时由 [`apply_terminal_view_visibility`] 清除焦点，
/// 使终端独占键盘；用户点回对话输入区时焦点恢复，终端停止捕获。
pub fn handle_terminal_keyboard(
    mut reader: MessageReader<bevy::input::keyboard::KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    content: Res<SideViewContent>,
    focus: Res<bevy::input_focus::InputFocus>,
    tabs: Res<TerminalTabs>,
    mut input_writer: MessageWriter<TerminalInput>,
) {
    // 仅终端视图激活时捕获
    if *content != SideViewContent::Terminal {
        return;
    }
    // 有输入框聚焦时不捕获（焦点互斥）
    if focus.get().is_some() {
        return;
    }
    let Some(active_tab) = tabs.active_entity() else {
        return;
    };
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);

    for ev in reader.read() {
        if ev.state != bevy::input::ButtonState::Pressed {
            continue;
        }
        use bevy::input::keyboard::KeyCode as K;

        // 控制字符优先（Ctrl+C / Ctrl+D / readline 控制字节）
        if ctrl {
            let bytes: Option<Vec<u8>> = match ev.key_code {
                K::KeyC => Some(vec![0x03]),
                K::KeyD => Some(vec![0x04]),
                K::KeyA => Some(vec![0x01]), // readline 行首
                K::KeyE => Some(vec![0x05]), // readline 行末
                K::KeyU => Some(vec![0x15]), // readline 删行
                K::KeyW => Some(vec![0x17]), // readline 删词
                K::KeyL => Some(vec![0x0c]), // 清屏（shell 侧）
                _ => None,
            };
            if let Some(bytes) = bytes {
                input_writer.write(TerminalInput {
                    tab: active_tab,
                    bytes,
                });
                continue;
            }
            // 其他 Ctrl 组合不透传，避免误发
            continue;
        }

        match ev.key_code {
            K::Enter | K::NumpadEnter => {
                input_writer.write(TerminalInput {
                    tab: active_tab,
                    bytes: vec![b'\n'],
                });
            }
            K::Backspace => {
                // DEL（0x7f）：shell readline 识别为退格删除
                input_writer.write(TerminalInput {
                    tab: active_tab,
                    bytes: vec![0x7f],
                });
            }
            K::Delete => {
                // Delete 键发 ANSI 序列，多数 shell 映射为 forward-delete
                input_writer.write(TerminalInput {
                    tab: active_tab,
                    bytes: b"\x1b[3~".to_vec(),
                });
            }
            K::Home => {
                input_writer.write(TerminalInput {
                    tab: active_tab,
                    bytes: b"\x1b[H".to_vec(),
                });
            }
            K::End => {
                input_writer.write(TerminalInput {
                    tab: active_tab,
                    bytes: b"\x1b[F".to_vec(),
                });
            }
            K::ArrowLeft => {
                input_writer.write(TerminalInput {
                    tab: active_tab,
                    bytes: b"\x1b[D".to_vec(),
                });
            }
            K::ArrowRight => {
                input_writer.write(TerminalInput {
                    tab: active_tab,
                    bytes: b"\x1b[C".to_vec(),
                });
            }
            K::ArrowUp => {
                input_writer.write(TerminalInput {
                    tab: active_tab,
                    bytes: b"\x1b[A".to_vec(),
                });
            }
            K::ArrowDown => {
                input_writer.write(TerminalInput {
                    tab: active_tab,
                    bytes: b"\x1b[B".to_vec(),
                });
            }
            K::Tab => {
                input_writer.write(TerminalInput {
                    tab: active_tab,
                    bytes: vec![b'\t'],
                });
            }
            _ => {
                // 字符输入：优先 ev.text（含 IME/组合输入），fallback logical_key
                let text = ev.text.clone().or_else(|| {
                    if let bevy::input::keyboard::Key::Character(s) = &ev.logical_key {
                        Some(s.clone())
                    } else {
                        None
                    }
                });
                if let Some(text) = text {
                    let bytes = text.as_bytes().to_vec();
                    if !bytes.is_empty() {
                        input_writer.write(TerminalInput {
                            tab: active_tab,
                            bytes,
                        });
                    }
                }
            }
        }
    }
}
