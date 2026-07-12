//! xgent_settings — Bevy Resource 包装 + fluent Localizer。
//!
//! 在 [`xgent_settings_core`] 纯类型之上做 Bevy 集成：
//! - [`resources`]：把 core 配置类型包装为 Bevy Resource；
//! - [`localizer`]：fluent 本地化器，实现 [`xui_i18n::StringSource`]。
//!
//! 本 crate 依赖 Bevy（UI 侧使用），daemon/provider 不依赖本 crate，只依赖 core。

pub mod localizer;
pub mod resources;

pub use localizer::{DEFAULT_LANG, Localizer};
pub use resources::{GlobalConfigRes, ProjectConfigRes};

use bevy::prelude::*;
use xgent_settings_core::GlobalConfigStore;

/// XGent 设置插件。
///
/// 注册 `GlobalConfigRes` 与 `Localizer` 资源。`ProjectConfigRes` 由
/// `xgent_app` 在打开项目时 `insert_resource`。
pub struct XgentSettingsPlugin;

impl Plugin for XgentSettingsPlugin {
    fn build(&self, app: &mut App) {
        let global = GlobalConfigStore::load().unwrap_or_default();
        let lang = if global.preferences.language.is_empty() {
            DEFAULT_LANG.to_string()
        } else {
            global.preferences.language.clone()
        };
        app.insert_resource(GlobalConfigRes(global))
            .insert_resource(Localizer::load(&lang));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xui_i18n::StringSource;

    #[test]
    fn plugin_registers_resources() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, XgentSettingsPlugin));
        assert!(app.world().contains_resource::<GlobalConfigRes>());
        assert!(app.world().contains_resource::<Localizer>());
    }

    #[test]
    fn plugin_provides_localizer_with_default_lang() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, XgentSettingsPlugin));
        let loc = app.world().resource::<Localizer>();
        // 未配置时回退到默认中文
        assert_eq!(loc.current_lang(), DEFAULT_LANG);
        assert_eq!(loc.get("welcome", &[]), "欢迎");
    }
}
