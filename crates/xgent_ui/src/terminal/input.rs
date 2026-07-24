//! 终端行编辑器：UI 侧维护输入缓冲，回车提交整行送 PTY。
//!
//! 详见 `doc/design/terminal-design.md` §3.3、§2.3。
//!
//! MVP 简化（见 `mod.rs` 文档注释）：PTY 保持 cooked 模式，shell 回显用户输入。
//! 故行编辑器的「即时显示」由 shell 回显承担——本输入框仅作命令草稿，回车后
//! 整行送 PTY，输入框清空。shell 回显产生 tv-body 中的可见命令行。
//!
//! 控制字符 Ctrl+C / Ctrl+D 即时单字节发送（不等回车）。
//!
//! 焦点：终端视图激活（`SideViewContent::Terminal`）时捕获键盘；否则忽略。

use bevy::input::keyboard::KeyCode;
use bevy::input::ButtonInput;
use bevy::prelude::*;

use crate::editor::SideViewContent;
use crate::terminal::io::TerminalInput;
use crate::terminal::{TerminalInputMarker, TerminalTabs};

/// 行编辑器状态（全局，对应当前激活 tab 的输入草稿）。
#[derive(Resource, Debug, Default)]
pub struct TerminalInputState {
    /// 当前输入文本。
    pub buffer: String,
    /// 光标字节位置（0..=buffer.len()）。
    pub cursor: usize,
}

impl TerminalInputState {
    /// 在光标处插入字符。
    fn insert(&mut self, ch: char) {
        self.buffer.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    /// 光标左移一个字符。
    fn left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor = prev_char_boundary(&self.buffer, self.cursor);
    }

    /// 光标右移一个字符。
    fn right(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        self.cursor = next_char_boundary(&self.buffer, self.cursor);
    }

    /// 删除光标前一个字符（Backspace）。
    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = prev_char_boundary(&self.buffer, self.cursor);
        self.buffer.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// 删除光标处字符（Delete）。
    fn delete(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let end = next_char_boundary(&self.buffer, self.cursor);
        self.buffer.replace_range(self.cursor..end, "");
    }

    /// 光标到行首。
    fn home(&mut self) {
        self.cursor = 0;
    }

    /// 光标到行末。
    fn end(&mut self) {
        self.cursor = self.buffer.len();
    }

    /// 清空缓冲 + 光标归零。
    fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
    }
}

/// 返回 `idx` 左侧最近的 char 边界（不含 `idx`）。
fn prev_char_boundary(s: &str, idx: usize) -> usize {
    if idx == 0 {
        return 0;
    }
    let mut i = idx - 1;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// 返回 `idx` 右侧最近的 char 边界（不含 `idx`）。
fn next_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx + 1;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}


/// 行提交事件（回车）：把 buffer 内容 + `\n` 发给 PTY。
#[derive(Message, Debug, Clone)]
pub struct TerminalLineSubmitted {
    pub tab: Entity,
    pub line: String,
}

/// 处理回车提交：把行 + `\n` 发 [`TerminalInput`]。
pub fn handle_line_submit(
    mut reader: MessageReader<TerminalLineSubmitted>,
    mut writer: MessageWriter<TerminalInput>,
    mut state: ResMut<TerminalInputState>,
) {
    for ev in reader.read() {
        let mut bytes = ev.line.clone().into_bytes();
        bytes.push(b'\n');
        writer.write(TerminalInput {
            tab: ev.tab,
            bytes,
        });
        state.clear();
    }
}

/// 终端键盘处理：终端视图激活且无输入框聚焦时捕获 `KeyboardInput` 事件。
///
/// 字符 → 插入缓冲；Enter → 提交；Ctrl+C/D → 即时发控制字节；
/// Backspace/Delete/Home/End/Ctrl+A/E/U → 行编辑。
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
    mut state: ResMut<TerminalInputState>,
    mut input_writer: MessageWriter<TerminalInput>,
    mut line_writer: MessageWriter<TerminalLineSubmitted>,
    mut q_input_text: Query<&mut Text, With<TerminalInputMarker>>,
) {
    // 仅终端视图激活时捕获
    if *content != SideViewContent::Terminal {
        return;
    }
    // 有输入框聚焦时不捕获（焦点互斥）
    if focus.get().is_some() {
        return;
    }
    // 仅终端视图激活时捕获
    if *content != SideViewContent::Terminal {
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

        // 控制字符优先（Ctrl+C / Ctrl+D）
        if ctrl {
            match ev.key_code {
                K::KeyC => {
                    input_writer.write(TerminalInput {
                        tab: active_tab,
                        bytes: vec![0x03],
                    });
                    continue;
                }
                K::KeyD => {
                    input_writer.write(TerminalInput {
                        tab: active_tab,
                        bytes: vec![0x04],
                    });
                    continue;
                }
                K::KeyA => {
                    state.home();
                    continue;
                }
                K::KeyE => {
                    state.end();
                    continue;
                }
                K::KeyU => {
                    state.clear();
                    continue;
                }
                _ => {}
            }
        }

        match ev.key_code {
            K::Enter | K::NumpadEnter => {
                let line = state.buffer.clone();
                line_writer.write(TerminalLineSubmitted {
                    tab: active_tab,
                    line,
                });
            }
            K::Backspace => {
                state.backspace();
            }
            K::Delete => {
                state.delete();
            }
            K::ArrowLeft => {
                state.left();
            }
            K::ArrowRight => {
                state.right();
            }
            K::Home => {
                state.home();
            }
            K::End => {
                state.end();
            }
            _ => {
                // 字符输入：从 logical_key / text 取
                if let Some(text) = &ev.text {
                    for ch in text.chars() {
                        if ch.is_control() {
                            continue;
                        }
                        state.insert(ch);
                    }
                } else if let bevy::input::keyboard::Key::Character(s) = &ev.logical_key {
                    for ch in s.chars() {
                        if ch.is_control() {
                            continue;
                        }
                        state.insert(ch);
                    }
                }
            }
        }
    }

    // 更新输入框显示文本（buffer 内容 + 光标占位）
    if let Ok(mut input_text) = q_input_text.single_mut() {
        let display = if state.buffer.is_empty() {
            String::new()
        } else {
            state.buffer.clone()
        };
        if input_text.0 != display {
            input_text.0 = display;
        }
    }
}
