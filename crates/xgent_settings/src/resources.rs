//! Bevy Resource 包装层。
//!
//! 用 newtype 包装 [`xgent_settings_core`] 的纯类型，加 `Resource` 派生供 UI 侧使用。
//! core 类型不依赖 Bevy，故由本层包装。
//!
//! 注意：本层不给 newtype 派生 `Reflect`，因为 core 类型未派生 `Reflect`
//! （core 保持与 Bevy 无关）。如需反射能力，后续可在本层手写 `Reflect` impl
//! 或仅对 UI 专用字段单独建模。
//!
//! `Deref`/`DerefMut` 使调用方可像用 core 类型一样访问字段。

use bevy::prelude::*;
use xgent_settings_core::{GlobalConfig, ProjectConfig};

/// 全局配置 Bevy Resource（newtype 包装 core 类型）。
#[derive(Resource, Deref, DerefMut, Debug, Clone, Default)]
pub struct GlobalConfigRes(pub GlobalConfig);

/// 项目配置 Bevy Resource（newtype 包装 core 类型）。
///
/// 由 `xgent_app` 在打开项目时 `insert_resource`。
#[derive(Resource, Deref, DerefMut, Debug, Clone, Default)]
pub struct ProjectConfigRes(pub ProjectConfig);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_config_res_deref() {
        let mut res = GlobalConfigRes(GlobalConfig {
            default_provider: "openai".into(),
            ..Default::default()
        });
        // Deref：直接访问 core 字段
        assert_eq!(res.default_provider, "openai");
        // DerefMut：可变修改
        res.default_model = "gpt-4".into();
        assert_eq!(res.default_model, "gpt-4");
        // 内部值
        assert_eq!(res.0.default_provider, "openai");
    }

    #[test]
    fn project_config_res_default() {
        let res = ProjectConfigRes::default();
        assert_eq!(
            res.context_strategy,
            xgent_settings_core::ContextStrategy::OnDemand
        );
    }
}
