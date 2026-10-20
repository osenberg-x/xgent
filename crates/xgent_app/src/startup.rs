//! 启动系统：打开项目、订阅 fs.watch。

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::text::{FontCx, FontSource, GenericFontFamily};
use xgent_agent::AgentBridge;
use xgent_settings_core::store::ProjectConfigStore;
use xgent_ui::file_panel::ProjectRoot;
use xgent_ui::fonts::{
    EMBEDDED_MONO_PATH, FontCandidate, FontOrigin, FontSlot, UiFonts, resolve_slot,
};

use crate::fs_event_bridge::IpcClientResource;

/// 启动序列：打开项目（订阅文件监听）。
///
/// daemon 仅向 `fs.watch` 订阅者推送 `fs.changed`，漏订阅则编辑器的
/// 文件冲突检测收不到任何变更通知。IPC 是 tokio IO，不能在 Bevy 系统
/// 线程上下文直接 await，故经 bridge 的 tokio runtime 异步发起。
pub fn open_project(
    project_root: Res<ProjectRoot>,
    bridge: Res<AgentBridge>,
    ipc: Res<IpcClientResource>,
) {
    tracing::info!("打开项目: {}", project_root.path.display());

    // 重新加载项目配置（Startup 系统里确认）
    if let Ok(cfg) = ProjectConfigStore::load(&project_root.path) {
        tracing::debug!(
            "项目配置: provider_override={:?}, strategy={:?}",
            cfg.provider_override,
            cfg.context_strategy
        );
    }

    // 订阅 fs.watch（异步 task，不阻塞 Startup）
    let ipc = ipc.client.clone();
    let root = project_root.path.clone();
    bridge.runtime.spawn(async move {
        let params = match serde_json::to_value(&xgent_core::fs::WatchRequest {
            project_root: root.clone(),
        }) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("序列化 WatchRequest 失败: {e}");
                return;
            }
        };
        if let Err(e) = ipc.call_ok(xgent_core::methods::FS_WATCH, params).await {
            tracing::warn!("订阅 fs.watch 失败: {e}");
        } else {
            tracing::debug!("已订阅项目文件变更: {}", root.display());
        }
    });
}

/// 加载字体：Inter Variable（界面拉丁）+ JetBrains Mono（等宽兜底），并解析四个槽位。
///
/// **Inter Variable** 为全局默认：读自随仓库分发的 `assets/fonts/Inter-Variable.ttf`
/// （OFL 许可），覆盖 `Assets<Font>` 的 `AssetId::default()`——所有未显式指定
/// `font` 的 [`TextFont`] 自动用 Inter（Linear 视觉基准的排版底座）。
///
/// **JetBrains Mono**（OFL 1.1）是 `mono` 槽位的**内嵌兜底**（v8 M4-T1）：系统里
/// 装了等宽字体就用系统的（Cascadia Mono / SF Mono / JetBrains Mono …），全缺则用
/// 内嵌的——**永不**回退到 `ui` 槽位的比例字体（设计系统 C-FONT-1）。
///
/// 四个槽位按**候选栈**解析（平台差异只在候选顺序数据里，代码不分叉），结果写
/// `tracing::info!` 便于三平台核对（C-FONT-2）：槽位 → 家族名 → 来源。
pub fn load_fonts(
    mut commands: Commands,
    mut fonts: ResMut<Assets<Font>>,
    mut font_cx: ResMut<FontCx>,
) {
    // ---- Inter Variable —— 全局默认（界面拉丁）----
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

    // ---- JetBrains Mono —— mono 槽位的内嵌兜底 ----
    let mono_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/fonts/JetBrainsMono-Regular.ttf"
    );
    let embedded_mono: Option<Handle<Font>> = match std::fs::read(mono_path) {
        Ok(data) => {
            let handle = fonts.add(Font::from_bytes(data));
            tracing::info!("已加载内嵌 JetBrains Mono（等宽兜底）");
            Some(handle)
        }
        Err(e) => {
            tracing::warn!("加载内嵌 JetBrains Mono 失败: {mono_path}: {e}");
            None
        }
    };

    // ---- 槽位解析 ----
    // 系统字体集合：parley 的 collection 默认已发现系统字体（parley/system 特性）。
    // 先取一份家族名快照（`family_id` 需要可变借用），再逐槽位解析。
    let families: Vec<String> = font_cx
        .context
        .collection
        .family_names()
        .map(str::to_string)
        .collect();
    let available = |family: &str| families.iter().any(|f| f == family);
    let embedded = |candidate: &FontCandidate| -> Option<Handle<Font>> {
        match candidate.embedded {
            Some(path) if path == EMBEDDED_MONO_PATH => embedded_mono.clone(),
            _ => None,
        }
    };

    let specs = xgent_ui::fonts::font_specs();
    let mut sources: Vec<(FontSlot, FontSource)> = Vec::new();
    for spec in specs {
        let r = resolve_slot(spec, &available, &embedded);
        match r.origin {
            FontOrigin::System => tracing::info!(
                "字体槽位 {} → {}（来源 system）",
                spec.slot.as_str(),
                r.family
            ),
            FontOrigin::Embedded => tracing::info!(
                "字体槽位 {} → {}（来源 embedded）",
                spec.slot.as_str(),
                r.family
            ),
            FontOrigin::Failed => tracing::warn!(
                "字体槽位 {} 无可用候选，回退通用族 {}（来源 failed）",
                spec.slot.as_str(),
                r.family
            ),
        }
        sources.push((spec.slot, r.source));
    }
    let slot_source = |slot: FontSlot, fallback: FontSource| -> FontSource {
        sources
            .iter()
            .find(|(s, _)| *s == slot)
            .map(|(_, src)| src.clone())
            .unwrap_or(fallback)
    };

    commands.insert_resource(UiFonts {
        ui: Handle::default(),
        mono: slot_source(
            FontSlot::Mono,
            FontSource::Generic(GenericFontFamily::Monospace),
        ),
        ui_cjk: slot_source(
            FontSlot::UiCjk,
            FontSource::Generic(GenericFontFamily::SansSerif),
        ),
        mono_cjk: slot_source(
            FontSlot::MonoCjk,
            FontSource::Generic(GenericFontFamily::Monospace),
        ),
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
