//! 启动系统：打开项目、订阅 fs.watch。

use bevy::prelude::*;
use xgent_settings_core::store::ProjectConfigStore;

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

/// 加载 macOS 系统等宽字体（Menlo）作为全局默认，替代 Bevy 内置的 FiraMono。
///
/// Bevy 默认字体是 FiraMono Medium，x-height 偏高、字宽偏宽，在 14px 下视觉
/// 显得比 zed/VSCode 的 JetBrains Mono 大且「糊」。Menlo 是 macOS Terminal/
/// Xcode 的默认等宽字体，度量紧凑、抗锯齿清晰，与 zed 视觉一致。
///
/// 用系统字体而非内嵌字体文件：零打包体积、跟随系统更新、与原生应用一致。
/// 覆盖 `Assets<Font>` 的 `AssetId::default()`（对齐 Bevy `TextPlugin` 注册
/// 默认字体的方式），所有未显式指定 `font` 的 `TextFont` 自动用此字体。
///
/// 非 macOS 平台静默跳过（保留 FiraMono 兜底）。
pub fn load_system_font(mut fonts: ResMut<Assets<Font>>) {
    let path = if cfg!(target_os = "macos") {
        std::path::Path::new("/System/Library/Fonts/Menlo.ttc").to_path_buf()
    } else {
        // 非 macOS：保留 Bevy 默认 FiraMono
        return;
    };
    match std::fs::read(&path) {
        Ok(data) => {
            let font = Font::from_bytes(data);
            fonts.insert(bevy::asset::AssetId::default(), font);
            tracing::info!("已加载系统字体: {}", path.display());
        }
        Err(e) => {
            let p = path.display();
            tracing::warn!("加载系统字体失败，回退 Bevy 默认: {p}: {e}");
        }
    }
}
