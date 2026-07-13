//! 快捷键派发系统（K-06）：读键盘输入，匹配注册表，触发 [`HotkeyTriggered`] 事件。
//!
//! xui 只做匹配与触发，**不执行快捷键动作**（动作由调用方业务层订阅事件实现）。

use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;

use crate::hotkeys::HotkeyRegistry;

/// 快捷键被触发事件。调用方订阅此事件执行业务动作。
#[derive(Message, Debug, Clone)]
pub struct HotkeyTriggered {
    /// 命中的快捷键 id
    pub id: String,
}

/// 快捷键派发插件。
pub struct ShortcutsPlugin;

impl Plugin for ShortcutsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<HotkeyTriggered>()
            .add_systems(Update, dispatch_hotkeys);
    }
}

/// 平台主修饰键是否按下。
fn primary_mod_pressed(keys: &ButtonInput<KeyCode>) -> bool {
    #[cfg(target_os = "macos")]
    {
        keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight)
    }
    #[cfg(not(target_os = "macos"))]
    {
        keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)
    }
}

/// Shift 是否按下。
fn shift_pressed(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)
}

/// 每帧检测刚按下的键，匹配注册表，触发事件。
///
/// 仅对"刚按下"的键触发（非持续按住），避免重复派发。
fn dispatch_hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    registry: Res<HotkeyRegistry>,
    mut triggered: MessageWriter<HotkeyTriggered>,
) {
    let shift = shift_pressed(&keys);
    let primary = primary_mod_pressed(&keys);
    for key in keys.get_just_pressed() {
        if let Some(h) = registry.match_input(*key, shift, primary) {
            triggered.write(HotkeyTriggered { id: h.id.clone() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 不依赖 Bevy 运行时的最小验证：primary_mod_pressed / shift_pressed 不会 panic。
    #[test]
    fn mod_pressed_helpers_run() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::ShiftLeft);
        assert!(shift_pressed(&keys));
        assert!(!primary_mod_pressed(&keys));
        keys.release(KeyCode::ShiftLeft);
        assert!(!shift_pressed(&keys));
    }
}
