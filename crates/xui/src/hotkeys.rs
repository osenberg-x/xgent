//! 快捷键注册表与平台修饰键抽象（K-06）。
//!
//! 平台无关的 [`Mod`] 抽象（Cmd=macOS，Ctrl=其他），运行期据 `cfg(target_os)` 选择主修饰键。
//! [`HotkeyRegistry`] 在注册时即检测冲突，避免静默覆盖。

use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use std::collections::HashMap;

/// 平台无关的主修饰键抽象。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mod {
    /// macOS 上为 ⌘ Command，其他平台为 Ctrl
    Cmd,
    /// 始终为 Ctrl
    Ctrl,
}

/// 当前平台的主修饰键：macOS 返回 [`Mod::Cmd`]，其他返回 [`Mod::Ctrl`]。
pub fn platform_primary_mod() -> Mod {
    #[cfg(target_os = "macos")]
    {
        Mod::Cmd
    }
    #[cfg(not(target_os = "macos"))]
    {
        Mod::Ctrl
    }
}

/// 快捷键定义。
#[derive(Debug, Clone)]
pub struct Hotkey {
    /// 唯一 id，如 `"command_palette.open"`
    pub id: String,
    /// 主键
    pub key: KeyCode,
    /// 是否需要 Shift
    pub mod_shift: bool,
    /// 是否需要平台主修饰键（Cmd/Ctrl）
    pub mod_primary: bool,
    /// 已本地化的展示标签（由调用方提供，xui 不依赖 i18n）
    pub label: String,
}

impl Hotkey {
    /// 简便构造。
    pub fn new(id: impl Into<String>, key: KeyCode, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            key,
            mod_shift: false,
            mod_primary: false,
            label: label.into(),
        }
    }

    /// 标记需要 Shift。
    pub fn with_shift(mut self) -> Self {
        self.mod_shift = true;
        self
    }

    /// 标记需要平台主修饰键。
    pub fn with_primary(mut self) -> Self {
        self.mod_primary = true;
        self
    }
}

/// 快捷键冲突错误。
#[derive(Debug, thiserror::Error)]
#[error("快捷键冲突：与已注册的 `{existing}` 重复（key={key:?}, shift={shift}, primary={primary})")]
pub struct HotkeyConflict {
    pub key: KeyCode,
    pub shift: bool,
    pub primary: bool,
    pub existing: String,
}

/// 快捷键注册表 Resource。
#[derive(Resource, Default, Debug)]
pub struct HotkeyRegistry {
    /// id -> Hotkey
    pub bindings: HashMap<String, Hotkey>,
}

impl HotkeyRegistry {
    /// 注册一个快捷键。若 key+mods 组合已被占用则返回冲突错误。
    pub fn register(&mut self, h: Hotkey) -> Result<(), HotkeyConflict> {
        if let Some(existing) = self.conflict(&h) {
            return Err(HotkeyConflict {
                key: h.key,
                shift: h.mod_shift,
                primary: h.mod_primary,
                existing,
            });
        }
        self.bindings.insert(h.id.clone(), h);
        Ok(())
    }

    /// 检测给定按键+修饰状态是否命中某快捷键，返回其引用。
    pub fn match_input(&self, key: KeyCode, shift: bool, primary: bool) -> Option<&Hotkey> {
        self.bindings
            .values()
            .find(|h| h.key == key && h.mod_shift == shift && h.mod_primary == primary)
    }

    fn conflict(&self, h: &Hotkey) -> Option<String> {
        self.bindings.values().find_map(|existing| {
            if existing.key == h.key
                && existing.mod_shift == h.mod_shift
                && existing.mod_primary == h.mod_primary
            {
                Some(existing.id.clone())
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_match() {
        let mut reg = HotkeyRegistry::default();
        reg.register(Hotkey::new("palette.open", KeyCode::KeyP, "打开命令面板").with_primary())
            .unwrap();

        let h = reg.match_input(KeyCode::KeyP, false, true).unwrap();
        assert_eq!(h.id, "palette.open");

        // 修饰键不匹配
        assert!(reg.match_input(KeyCode::KeyP, false, false).is_none());
        // 主键不匹配
        assert!(reg.match_input(KeyCode::KeyA, false, true).is_none());
    }

    #[test]
    fn duplicate_binding_conflicts() {
        let mut reg = HotkeyRegistry::default();
        reg.register(Hotkey::new("a", KeyCode::KeyP, "p").with_primary())
            .unwrap();
        let err = reg
            .register(Hotkey::new("b", KeyCode::KeyP, "p2").with_primary())
            .unwrap_err();
        assert_eq!(err.existing, "a");
    }

    #[test]
    fn same_key_different_mods_ok() {
        let mut reg = HotkeyRegistry::default();
        reg.register(Hotkey::new("a", KeyCode::KeyP, "p").with_primary())
            .unwrap();
        // 同键但加 Shift，不冲突
        reg.register(
            Hotkey::new("b", KeyCode::KeyP, "p2")
                .with_primary()
                .with_shift(),
        )
        .unwrap();
        assert_eq!(reg.bindings.len(), 2);
    }
}
