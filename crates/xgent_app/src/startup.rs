//! 启动系统：打开项目、订阅 fs.watch。

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
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
    let inter_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/fonts/Inter-Variable.ttf"
    );
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

/// 截图捕获（统一入口）：spawn Screenshot + 落盘观察器。
fn capture(commands: &mut Commands, path: String) {
    tracing::info!("UI 截图 → {path}");
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}

/// UI 截图工具：
/// ① 启动 3 秒后自动截一次（`XGENT_SHOT=<路径>` 时启用，spike/验收取证）；
/// ② F12 随时截图到 `target/snapshots/`（期验收对照用，M2-T6）。
///
/// app 截自己的渲染目标，不依赖系统录屏权限。
pub fn ui_screenshot_tool(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut path: Local<Option<String>>,
    mut done: Local<bool>,
    mut counter: Local<u32>,
) {
    if path.is_none() {
        *path = std::env::var("XGENT_SHOT").ok();
    }
    // ① 环境变量一次性截图
    if let Some(p) = path.as_ref()
        && !*done
        && time.elapsed_secs() >= 3.0
    {
        *done = true;
        capture(&mut commands, p.clone());
    }
    // ② F12 常规截图
    if keys.just_pressed(KeyCode::F12) {
        let _ = std::fs::create_dir_all("target/snapshots");
        let name = format!(
            "target/snapshots/ui-{}-{}.png",
            std::process::id(),
            *counter
        );
        *counter += 1;
        capture(&mut commands, name);
    }
}
