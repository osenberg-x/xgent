//! 终端输入：**本地行编辑模式**——字符在输入行本地回显，Enter 提交整行。
//!
//! 模式变更（R2 终端反馈）：原「透传模式」把每键直接发 PTY，shell 回显出现在
//! 输出区——用户看不到「自己在输入什么」，与常规终端体验相悖。改为本地行编辑：
//!
//! - 字符键 → 本地累积进输入行 buffer（光标位置随字符宽度右移），**不发 PTY**
//! - Backspace → 删光标前一字符（UTF-8 字符边界）
//! - Enter → 整行 + `\n` 发 PTY，清空 buffer；shell 回显出现在输出区
//! - Ctrl+C → 发 `\x03` 并清空本地行（对齐 shell 行取消语义）
//! - ↑↓ 历史导航：MVP 不做本地历史（shell readline 在 cooked 模式下不可用，
//!   因字符不再逐键透传）——后续接伪历史（本地缓存已提交行）
//!
//! 妥协说明：本地行编辑下 shell 的 tab 补全 / ↑ 历史不可用（这些依赖逐键
//! 透传）。MVP 优先输入可见性；补全/历史升级路径=输出区解析 + PTY 行模式
//! 切换（标 P2）。
//!
//! 焦点互斥：对话输入区/命令面板等 `EditableText` 聚焦时终端不捕获键盘。

use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;

use crate::editor::SideViewContent;
use crate::terminal::io::TerminalInput;
use crate::terminal::{
    TerminalInputCaretMarker, TerminalInputMarker, TerminalTab, TerminalTabStatus, TerminalTabs,
};

/// 终端输入行本地 buffer（正在输入、尚未 Enter 提交的字符 + 光标位置）。
///
/// 光标为字符索引（非字节）：插入/删除按字符边界操作，回显时按
/// `chars[..pos] + ▊ + chars[pos..]` 渲染，块光标天然随字符宽度右移。
#[derive(Resource, Debug, Default)]
pub struct TerminalLineBuf {
    /// 已输入字符（Unicode 标量）。
    pub chars: Vec<char>,
    /// 光标位置（字符索引，0..=chars.len()）。
    pub pos: usize,
}

impl TerminalLineBuf {
    /// 插入字符（光标处）。
    fn insert(&mut self, c: char) {
        self.chars.insert(self.pos.min(self.chars.len()), c);
        self.pos = (self.pos + 1).min(self.chars.len());
    }

    /// 删除光标前一字符（Backspace）。
    fn backspace(&mut self) {
        if self.pos > 0 {
            self.pos -= 1;
            self.chars.remove(self.pos);
        }
    }

    /// 清空（Ctrl+C 行取消 / Enter 提交后 / 切 tab）。
    pub fn clear(&mut self) {
        self.chars.clear();
        self.pos = 0;
    }
}

/// 终端键盘处理（本地行编辑模式）。
///
/// 终端视图激活且无 `EditableText` 聚焦时捕获 `KeyboardInput`：
/// - 可打印字符 → 本地 buffer 插入（光标随字符宽度右移）
/// - Enter → 整行 + `\n` 发 PTY（shell 在输出区回显该命令 + 输出）
/// - Backspace/Delete/←→/Home/End → 本地行内编辑
/// - Ctrl+C → 发 `\x03`（中断前台进程）+ 清空本地行
/// - Ctrl+D → 发 `\x04`（EOF，exit shell）
/// - Tab → 本地补全 MVP 不做，忽略（避免 PTY 侧补全输出错位）
pub fn handle_terminal_keyboard(
    mut reader: MessageReader<bevy::input::keyboard::KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    content: Res<SideViewContent>,
    focus: Res<bevy::input_focus::InputFocus>,
    tabs: Res<TerminalTabs>,
    mut buf: ResMut<TerminalLineBuf>,
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

        // 控制字符（本地行取消 / EOF / 中断即时发 PTY）
        if ctrl {
            let bytes: Option<Vec<u8>> = match ev.key_code {
                K::KeyC => {
                    // Ctrl+C：中断前台进程 + 本地行取消（对齐 shell ^C 语义）
                    buf.clear();
                    Some(vec![0x03])
                }
                K::KeyD => {
                    // Ctrl+D：EOF（空行时 exit shell）
                    if buf.chars.is_empty() {
                        Some(vec![0x04])
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(bytes) = bytes {
                input_writer.write(TerminalInput {
                    tab: active_tab,
                    bytes,
                });
            }
            continue;
        }

        match ev.key_code {
            K::Enter | K::NumpadEnter => {
                // 整行提交：行内容 + \n（shell 收到后回显整行到输出区）
                let mut bytes: Vec<u8> = buf.chars.iter().collect::<String>().into_bytes();
                bytes.push(b'\r');
                input_writer.write(TerminalInput {
                    tab: active_tab,
                    bytes,
                });
                buf.clear();
            }
            K::Backspace => buf.backspace(),
            K::Delete => {
                // Delete：删光标处字符
                if buf.pos < buf.chars.len() {
                    let pos = buf.pos;
                    buf.chars.remove(pos);
                }
            }
            K::Home => buf.pos = 0,
            K::End => buf.pos = buf.chars.len(),
            K::ArrowLeft => buf.pos = buf.pos.saturating_sub(1),
            K::ArrowRight => buf.pos = (buf.pos + 1).min(buf.chars.len()),
            // ↑↓ 历史 / Tab 补全：本地模式 MVP 不做（见模块头说明）
            K::ArrowUp | K::ArrowDown | K::Tab => {}
            _ => {
                // 字符输入：优先 ev.text（含 IME/组合输入），fallback logical_key
                let text = ev.text.clone().or_else(|| {
                    if let bevy::input::keyboard::Key::Character(s) = &ev.logical_key {
                        Some(s.clone())
                    } else {
                        None
                    }
                });
                // 过滤控制字符（\r/\n/\t 已在上方分支处理）
                if let Some(text) = text.filter(|t| !t.chars().any(char::is_control)) {
                    for c in text.chars() {
                        buf.insert(c);
                    }
                }
            }
        }
    }
}

/// 输入行渲染：prompt「❯」之后的文本节点内容 = `chars[..pos]`，其后为光标
/// 位置占位（光标块由 [`TerminalInputCaretMarker`] 节点承担，渲染为反色块
/// 字符）——顺序：已输入文本 + 光标块 + 光标后文本。
///
/// 光标随字符宽度自然右移：`chars[..pos]` 先渲染，光标节点紧随其后，
/// 文本节点自身只存 `chars[..pos]`，`chars[pos..]` 由独立的「光标后」节点存。
pub fn update_input_line(
    buf: Res<TerminalLineBuf>,
    content: Res<SideViewContent>,
    mut q: Query<&mut Text, With<TerminalInputMarker>>,
) {
    if !content.is_changed() && !buf.is_changed() {
        return;
    }
    let pos = buf.pos.min(buf.chars.len());
    let before: String = buf.chars[..pos].iter().collect();
    for mut text in q.iter_mut() {
        if text.0 != before {
            text.0 = before.clone();
        }
    }
}

/// 光标后文本（`chars[pos..]`）渲染节点标记。
#[derive(Component, Default)]
pub struct TerminalInputAfterMarker;

/// 光标后文本渲染。
pub fn update_input_after(
    buf: Res<TerminalLineBuf>,
    content: Res<SideViewContent>,
    mut q: Query<&mut Text, With<TerminalInputAfterMarker>>,
) {
    if !content.is_changed() && !buf.is_changed() {
        return;
    }
    let pos = buf.pos.min(buf.chars.len());
    let after: String = buf.chars[pos..].iter().collect();
    for mut text in q.iter_mut() {
        if text.0 != after {
            text.0 = after.clone();
        }
    }
}

/// 块状光标闪烁（1s 周期：0.5s 显 / 0.5s 隐）。
///
/// 终端视图激活且激活 tab 处于 Running 态时显示闪烁；非激活视图 / tab 退出
/// （shell 结束，无输入意义）时隐藏。光标字符始终存在（占位稳定），
/// 闪烁用透明度切换。
pub fn update_input_caret(
    content: Res<SideViewContent>,
    tabs: Res<TerminalTabs>,
    q_tabs: Query<&TerminalTab>,
    time: Res<Time>,
    theme: Res<crate::theme::Theme>,
    mut q_caret: Query<&mut TextColor, With<TerminalInputCaretMarker>>,
) {
    let active = *content == SideViewContent::Terminal
        && tabs
            .active_entity()
            .and_then(|e| q_tabs.get(e).ok())
            .is_some_and(|t| t.status == TerminalTabStatus::Running);
    let visible = active && (time.elapsed().as_secs_f64() % 1.0) < 0.5;
    let want = if visible {
        theme.accent_interactive
    } else {
        bevy::color::Color::NONE
    };
    for mut color in q_caret.iter_mut() {
        if color.0 != want {
            color.0 = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本地行编辑：插入/退格/光标移动按字符边界操作。
    #[test]
    fn line_buf_edit_operations() {
        let mut buf = TerminalLineBuf::default();
        for c in "echo hello".chars() {
            buf.insert(c);
        }
        assert_eq!(buf.chars.iter().collect::<String>(), "echo hello");
        assert_eq!(buf.pos, 10);

        // 光标移到行中 + 插入
        buf.pos = 5;
        buf.insert('X');
        assert_eq!(buf.chars.iter().collect::<String>(), "echo Xhello");
        assert_eq!(buf.pos, 6);

        // Backspace 删光标前一字符（'X'）
        buf.backspace();
        assert_eq!(buf.chars.iter().collect::<String>(), "echo hello");

        // 多字节字符按字符边界（'中' 3 字节但 1 字符）
        let mut buf2 = TerminalLineBuf::default();
        buf2.insert('中');
        buf2.insert('文');
        buf2.backspace();
        assert_eq!(buf2.chars.iter().collect::<String>(), "中");
        assert_eq!(buf2.pos, 1);
    }

    /// Ctrl+C 语义验证所需的行为：clear 清空字符与光标。
    #[test]
    fn line_buf_clear() {
        let mut buf = TerminalLineBuf::default();
        for c in "abc".chars() {
            buf.insert(c);
        }
        buf.clear();
        assert!(buf.chars.is_empty());
        assert_eq!(buf.pos, 0);
    }

    /// 渲染切分契约：光标前后文本按字符索引切分（光标随字符宽度右移）。
    #[test]
    fn render_split_at_cursor() {
        let mut buf = TerminalLineBuf::default();
        for c in "abc中".chars() {
            buf.insert(c);
        }
        buf.pos = 3; // '中' 之前
        let pos = buf.pos.min(buf.chars.len());
        let before: String = buf.chars[..pos].iter().collect();
        let after: String = buf.chars[pos..].iter().collect();
        assert_eq!(before, "abc");
        assert_eq!(after, "中");
    }
}
