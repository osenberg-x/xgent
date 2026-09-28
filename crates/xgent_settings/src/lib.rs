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
        // daemon 对损坏配置拒启，UI 侧不能静默吞掉同一错误——那会表现为
        // "配置全丢"假象（providers 为空、语言回退），用户无从知道根因是
        // TOML 损坏。仍回退默认启动，但必须把文件路径打到日志。
        let global = match GlobalConfigStore::load() {
            Ok(g) => g,
            Err(e) => {
                let path = xgent_settings_core::paths::global_config_file();
                tracing::error!(
                    "全局配置加载失败（使用默认配置启动，请检查文件）: {}: {e}",
                    path.display()
                );
                Default::default()
            }
        };
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
    fn plugin_provides_localizer() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, XgentSettingsPlugin));
        let loc = app.world().resource::<Localizer>();
        // Localizer 已注册且能取到本地化串（语言由全局配置或默认决定）
        assert!(!loc.current_lang().is_empty());
        let welcome = loc.get("welcome", &[]);
        assert!(!welcome.is_empty());
    }
}
