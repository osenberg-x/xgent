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
