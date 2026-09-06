//! 启动系统：打开项目、订阅 fs.watch。

use bevy::prelude::*;
use xgent_settings_core::store::ProjectConfigStore;
use xgent_ui::fonts::UiFonts;

use crate::fs_event_bridge::IpcClientResource;

/// 启动序列：打开项目（订阅文件监听、加载会话）。
pub fn open_project(args: Res<crate::Args>, ipc: Res<IpcClientResource>) {
    let project_root = args.project.clone();
    tracing::info!("打开项目: {}", project_root.display());

    // 重新加载项目配置（Startup 系统里确认）
    if let Ok(cfg) = ProjectConfigStore::load(&project_root) {
        tracing::debug!(
            "项目配置: provider_override={:?}, strategy={:?}",
            cfg.provider_override,
            cfg.context_strategy
        );
    }

    // 订阅 fs.watch（异步 task，不阻塞 Startup）
    let ipc = ipc.client.clone();
    let root = project_root.clone();
    bevy::tasks::block_on(async move {
        let params = serde_json::to_value(&xgent_core::fs::WatchRequest {
            project_root: root.clone(),
        })
        .unwrap();
        if let Err(e) = ipc.call_ok(xgent_core::methods::FS_WATCH, params).await {
            tracing::warn!("订阅 fs.watch 失败: {e}");
        } else {
            tracing::debug!("已订阅项目文件变更: {}", root.display());
        }
    });
}

/// 加载字体：Inter Variable 为全局默认 + Menlo 等宽（macOS）。
///
/// Inter 读自随仓库分发的 `assets/fonts/Inter-Variable.ttf`（OFL 许可），
/// 覆盖 `Assets<Font>` 的 `AssetId::default()`——所有未显式指定 `font` 的
/// [`TextFont`] 自动用 Inter（Linear 视觉基准的排版底座）。Menlo 为 macOS
/// Terminal/Xcode 默认等宽，度量紧凑，经 `fonts.add` 得强句柄存入
/// [`UiFonts::mono`] 供等宽场景（代码/终端）显式引用。
///
/// 非 macOS：Inter 照常加载（随仓库分发），Menlo 跳过（mono 回退默认句柄）。
pub fn load_fonts(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    // Inter Variable —— 全局默认
    let inter_path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/Inter-Variable.ttf");
    match std::fs::read(inter_path) {
        Ok(data) => {
            let _ = fonts.insert(bevy::asset::AssetId::default(), Font::from_bytes(data));
            tracing::info!("已加载 Inter Variable 为全局默认字体");
        }
        Err(e) => {
            tracing::warn!("加载 Inter 失败，回退 Bevy 默认: {inter_path}: {e}");
        }
    }

    // Menlo —— 等宽（macOS 系统字体）
    let mono = if cfg!(target_os = "macos") {
        match std::fs::read("/System/Library/Fonts/Menlo.ttc") {
            Ok(data) => fonts.add(Font::from_bytes(data)),
            Err(e) => {
                tracing::warn!("加载 Menlo 失败，等宽回退默认字体: {e}");
                Handle::default()
            }
        }
    } else {
        Handle::default()
    };

    commands.insert_resource(UiFonts {
        ui: Handle::default(),
        mono,
    });
}
