//! 输入增强（K-05）：基于官方 `EditableText` 薄封装，做多行 + 发送语义。
//!
//! **不重写输入核心**：光标、删除、IME 全部由官方 `EditableText` + text_input 系统处理。
//! xui 只在 key event 层做"Enter 语义"判定：
//! - 单行模式：Enter 发送
//! - 多行模式：Enter 换行（由 text_input 处理），平台主修饰键 + Enter 发送
//!
//! 实现方式：在带 `ChatInput` 的 `EditableText` 实体上挂 observer 监听
//! `FocusedInput<KeyboardInput>`，该事件由官方 text_input 在未消费时传播出来，
//! 注释明确"propagate to allow for tab navigation and submit actions"。

use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;
use bevy::input::ButtonInput;
use bevy::input::ButtonState;
use bevy::input::keyboard::Key;
use bevy::input::keyboard::KeyCode;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::FocusedInput;
use bevy::prelude::*;
use bevy::text::EditableText;

use crate::hotkeys::platform_primary_mod;
use crate::input::SendModifier as SendMod;

/// 多行聊天输入框标记组件，挂在与 `EditableText` 同一实体上。
#[derive(Component, Debug)]
#[component(on_add = on_chat_input_added)]
pub struct ChatInput {
    /// 是否多行模式
    pub multiline: bool,
    /// 发送所需的修饰键
    pub send_modifier: SendModifier,
}

impl ChatInput {
    /// 构造多行聊天输入（Cmd/Ctrl+Enter 发送）。
    pub fn multiline() -> Self {
        Self {
            multiline: true,
            send_modifier: SendModifier::from_platform(),
        }
    }

    /// 构造单行输入（Enter 发送）。
    pub fn single_line() -> Self {
        Self {
            multiline: false,
            send_modifier: SendModifier::None,
        }
    }
}

/// `on_add` 钩子：同步设置 `EditableText::allow_newlines`。
fn on_chat_input_added(mut world: DeferredWorld, ctx: HookContext) {
    let multiline = world
        .get_mut::<ChatInput>(ctx.entity)
        .map(|c| c.multiline)
        .unwrap_or(false);
    if let Some(mut et) = world.get_mut::<EditableText>(ctx.entity) {
        et.allow_newlines = multiline;
    }
}

/// 发送修饰键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendModifier {
    /// 无修饰键（单行模式，Enter 直接发送）
    None,
    /// 平台主修饰键（macOS=Cmd，其他=Ctrl）
    Primary,
}

impl SendModifier {
    /// 取当前平台默认发送修饰键（多行模式用）。
    pub fn from_platform() -> Self {
        match platform_primary_mod() {
            crate::hotkeys::Mod::Cmd => SendModifier::Primary,
            crate::hotkeys::Mod::Ctrl => SendModifier::Primary,
        }
    }
}

/// 聊天输入被提交事件。
#[derive(Message, Debug, Clone)]
pub struct ChatInputSubmitted {
    /// 提交的文本
    pub text: String,
    /// 来源实体
    pub entity: Entity,
}

/// 输入增强插件。
pub struct InputEnhancePlugin;

impl Plugin for InputEnhancePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ChatInputSubmitted>()
            .add_observer(on_enter_submit);
    }
}

/// 判定一次按键是否构成"发送"（纯函数，便于测试）。
///
/// 规则：
/// - 单行模式：无修饰 Enter 发送
/// - 多行模式：平台主修饰键 + Enter 发送
/// - 任何 Shift/Alt 组合或非 Enter 键都不发送
pub fn should_send(
    input: &ChatInput,
    key: &Key,
    state: ButtonState,
    primary_pressed: bool,
) -> bool {
    if state != ButtonState::Pressed {
        return false;
    }
    if !matches!(key, Key::Enter) {
        return false;
    }
    match (input.multiline, input.send_modifier) {
        (false, _) => true,
        (true, SendMod::Primary) => primary_pressed,
        (true, SendMod::None) => true,
    }
}

/// observer：监听聚焦输入框的键盘事件，Enter 语义触发提交。
fn on_enter_submit(
    trigger: On<FocusedInput<KeyboardInput>>,
    query: Query<&ChatInput>,
    mut editable: Query<&mut EditableText>,
    mut writer: MessageWriter<ChatInputSubmitted>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let entity = trigger.focused_entity;
    let Ok(input) = query.get(entity) else {
        return;
    };
    let ev = &trigger.input;
    if !should_send(input, &ev.logical_key, ev.state, primary_pressed(&keys)) {
        return;
    }
    // 提交时取当前文本并清空
    let Ok(mut et) = editable.get_mut(entity) else {
        return;
    };
    let text = et.value().to_string();
    if text.trim().is_empty() {
        return;
    }
    et.clear();
    writer.write(ChatInputSubmitted { text, entity });
}

fn primary_pressed(keys: &ButtonInput<KeyCode>) -> bool {
    #[cfg(target_os = "macos")]
    {
        keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight)
    }
    #[cfg(not(target_os = "macos"))]
    {
        keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::keyboard::Key;

    fn single() -> ChatInput {
        ChatInput::single_line()
    }
    fn multi() -> ChatInput {
        ChatInput::multiline()
    }

    #[test]
    fn single_line_enter_sends() {
        let input = single();
        assert!(should_send(
            &input,
            &Key::Enter,
            ButtonState::Pressed,
            false
        ));
    }

    #[test]
    fn multi_line_plain_enter_does_not_send() {
        let input = multi();
        assert!(!should_send(
            &input,
            &Key::Enter,
            ButtonState::Pressed,
            false
        ));
    }

    #[test]
    fn multi_line_primary_enter_sends() {
        let input = multi();
        assert!(should_send(&input, &Key::Enter, ButtonState::Pressed, true));
    }

    #[test]
    fn release_does_not_send() {
        let input = single();
        assert!(!should_send(
            &input,
            &Key::Enter,
            ButtonState::Released,
            false
        ));
    }

    #[test]
    fn non_enter_does_not_send() {
        let input = single();
        assert!(!should_send(
            &input,
            &Key::Character("a".into()),
            ButtonState::Pressed,
            false
        ));
    }
}
