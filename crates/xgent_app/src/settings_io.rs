//! 设置读写与热应用：面板消息 ↔ daemon 权威配置 ↔ 运行时句柄。
//!
//! 三条通路，共享同一套句柄与同一条 apply 判据：
//!
//! 1. **取快照**（[`request_snapshot`] / [`drain_snapshot`]）：面板打开时发
//!    [`RequestSettingsSnapshotMessage`]，异步经 `config.read` 读全局与项目两份
//!    配置，回填 [`GlobalConfigRes`] / [`ProjectConfigRes`] 并让面板重渲染。
//!    面板自己不发 IPC，也不碰磁盘（AGENTS.md §5.2：子系统之间只经 ECS 通信）。
//! 2. **保存**（[`save_settings`] / [`drain_save`]）：只把脏键经 `config.write`
//!    写下去，逐键收集成功与失败，再重读权威值 → 落进 [`LiveSettings`] 句柄 →
//!    把结果如实报回面板（[`SettingsSaveReportMessage`]）。写失败的键**不**进句柄
//!    （daemon 没收，重读也读不回来），运行时用的始终是磁盘真值；显示则停在
//!    编辑态，让被拒的那一格连同原因留在屏幕上，见 [`apply_save_result`]。
//! 3. **对端变更**（[`apply_peer_change`]）：daemon 广播的 `config.changed` 走
//!    同一个 [`apply_snapshot`]，因此另一个窗口改的配置在本窗口同样生效、
//!    同样显示，不需要哪个窗口重启。
//!
//! 生效边界（面板与能力规格都这么写）：
//! - 工具策略 → 下一次工具调用
//! - `[agent]` 三项与上下文窗口 → 下一条用户消息（`max_events_per_frame` 下一帧）
//! - `[tools]` 三项 → 下一次确认 / 下一次 `run_command`
//! - `[context]` 五项 → 下一次检索
//! - provider 限额 → 下一次请求（daemon 侧 provider 池失效）

use std::sync::Arc;

use bevy::prelude::*;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use xgent_agent::bridge::{AgentBridge, RunSettings};
use xgent_core::config::{ConfigReadRequest, ConfigScope, ConfigWriteRequest};
use xgent_core::methods;
use xgent_settings::{GlobalConfigRes, Localizer, ProjectConfigRes};
use xgent_settings_core::{GlobalConfig, ProjectConfig};
use xgent_ui::settings_panel::{
    RequestSettingsSnapshotMessage, SaveLanguageMessage, SaveSettingsMessage,
    SettingsSaveReportMessage, SettingsSnapshotReady,
};

use crate::config_bridge::RefreshSender;
use crate::fs_event_bridge::{ConfigChangedMessage, IpcClientResource};
use xgent_ui::file_panel::ProjectRoot;

/// 运行时可改配置的共享句柄集合。
///
/// 每个句柄都与真正的消费方（执行器 / 检索 provider / agent bridge）共享同一个
/// `Arc`：这里写入即对端读到，不需要重建任何一个对象。形状沿用
/// `retry_config` 与 `loop_bounds`（bridge 已有的运行期可改值）。
#[derive(Resource, Clone)]
pub struct LiveSettings {
    /// 工具策略（`[tool_policy]`）——执行器每次解析策略时读它
    pub tool_policy: Arc<RwLock<xgent_settings_core::ToolPolicyConfig>>,
    /// 工具运行参数（`[tools]`）——执行器与 `run_command` 读它
    pub tool_params: Arc<RwLock<xgent_tools::ToolParams>>,
    /// 检索限额（`[context]`）——内置检索 provider 每次检索读它
    pub retrieval_limits: Arc<RwLock<xgent_context::RetrievalLimits>>,
    /// agent 设置（`[agent]` + 解析后的上下文窗口）
    pub run_settings: Arc<RwLock<RunSettings>>,
}

impl LiveSettings {
    /// 由项目配置与**已解析**的上下文窗口构造初始句柄集。
    ///
    /// 窗口从外面传进来而不是在这里再解析一次：`xgent_app` 在启动时解析并打
    /// "未配置窗口按内置缺省计算"的告警（R3-17），这里重复解析会静默吞掉那条告警。
    pub fn new(project: &ProjectConfig, context_window: u32) -> Self {
        let window = context_window;
        Self {
            tool_policy: Arc::new(RwLock::new(project.tool_policy.clone())),
            tool_params: Arc::new(RwLock::new(xgent_tools::ToolParams::from_tool_config(
                &project.tools,
            ))),
            retrieval_limits: Arc::new(RwLock::new(
                xgent_context::RetrievalLimits::from_context_limits(&project.context),
            )),
            run_settings: Arc::new(RwLock::new(RunSettings {
                retrieval_max_tokens: project.agent.effective_retrieval_max_tokens(),
                max_events_per_frame: project.agent.effective_max_events_per_frame(),
                max_parallel_tools: project.agent.effective_max_parallel_tools(),
                context_window: window,
            })),
        }
    }
}

/// 重算全部运行时句柄（唯一的 apply 通路：本地写入与对端广播都走这里）。
///
/// 逐键增量应用容易漏（写了 `context_window` 却没重算窗口），所以每次配置变化
/// 都从**整份权威快照**重算一遍——配置只有十几项，代价可以忽略。
pub fn apply_snapshot(
    live: &LiveSettings,
    project: &ProjectConfig,
    global: &GlobalConfig,
    provider_id: &str,
    model: &str,
) {
    *live.tool_policy.write() = project.tool_policy.clone();
    *live.tool_params.write() = xgent_tools::ToolParams::from_tool_config(&project.tools);
    *live.retrieval_limits.write() =
        xgent_context::RetrievalLimits::from_context_limits(&project.context);
    let (window, _fell_back) = global.resolve_context_window(provider_id, model);
    let mut rs = live.run_settings.write();
    rs.retrieval_max_tokens = project.agent.effective_retrieval_max_tokens();
    rs.max_events_per_frame = project.agent.effective_max_events_per_frame();
    rs.max_parallel_tools = project.agent.effective_max_parallel_tools();
    // 直接赋值即可：`GlobalConfig::resolve_context_window` 的三级次序最差也回落到
    // 内置 128_000，永不返回 0，所以"值为 0 就别覆盖"是要防一个不存在的输入。
    rs.context_window = window;
}

/// 快照读取结果（异步 task → ECS）。
struct SnapshotResult {
    /// 读到的全局配置（`None` = 读失败，保留现有值）
    global: Option<GlobalConfig>,
    /// 读到的项目配置（`None` = 读失败）
    project: Option<ProjectConfig>,
    /// 失败原因（写进面板顶部，不把"读不到"演成"全是默认值"）
    error: String,
}

/// 保存结果（异步 task → ECS）。
struct SaveTaskResult {
    /// 报告给面板的内容
    report: SettingsSaveReportMessage,
    /// 写完并重读到的权威配置（`None` = 没读到，句柄不动）
    snapshot: Option<SnapshotResult>,
}

#[derive(Resource)]
struct SnapshotRx {
    rx: mpsc::Receiver<SnapshotResult>,
}

#[derive(Resource, Clone)]
struct SnapshotTx(mpsc::Sender<SnapshotResult>);

#[derive(Resource)]
struct SaveRx {
    rx: mpsc::Receiver<SaveTaskResult>,
}

#[derive(Resource, Clone)]
struct SaveTx(mpsc::Sender<SaveTaskResult>);

/// 设置读写插件。
pub struct SettingsIoPlugin;

impl Plugin for SettingsIoPlugin {
    fn build(&self, app: &mut App) {
        let (s_tx, s_rx) = mpsc::channel::<SnapshotResult>(8);
        let (w_tx, w_rx) = mpsc::channel::<SaveTaskResult>(8);
        app.insert_resource(SnapshotTx(s_tx))
            .insert_resource(SnapshotRx { rx: s_rx })
            .insert_resource(SaveTx(w_tx))
            .insert_resource(SaveRx { rx: w_rx })
            .add_systems(
                Update,
                (
                    request_snapshot,
                    drain_snapshot,
                    save_settings,
                    drain_save,
                    refresh_provider_after_save,
                    apply_peer_change,
                    save_language,
                ),
            );
    }
}

/// 面板打开 → 异步读两份配置。
fn request_snapshot(
    mut reader: MessageReader<RequestSettingsSnapshotMessage>,
    ipc: Res<IpcClientResource>,
    bridge: Res<AgentBridge>,
    root: Res<ProjectRoot>,
    tx: Res<SnapshotTx>,
) {
    for _ in reader.read() {
        let ipc = ipc.client.clone();
        let root = root.path.clone();
        let tx = tx.0.clone();
        bridge.runtime.handle().spawn(async move {
            let (global, project, error) = read_both(&ipc, &root).await;
            let _ = tx
                .send(SnapshotResult {
                    global,
                    project,
                    error,
                })
                .await;
        });
    }
}

/// 读全局与项目两份配置；任一失败都带原因回来（另一个仍可用）。
async fn read_both(
    ipc: &Arc<crate::ipc_client::IpcClient>,
    root: &std::path::Path,
) -> (Option<GlobalConfig>, Option<ProjectConfig>, String) {
    let mut error = String::new();
    let global = read_global(ipc).await.map_err(|e| error = e).ok();
    let project = read_project(ipc, root).await.map_err(|e| error = e).ok();
    (global, project, error)
}

/// 组装全局配置（daemon 的 key 是分散的，这里拼回一个完整视图）。
async fn read_global(ipc: &Arc<crate::ipc_client::IpcClient>) -> Result<GlobalConfig, String> {
    let providers = ipc
        .call_ok(
            methods::CONFIG_READ,
            config_read_params(ConfigScope::Global, "providers", None),
        )
        .await
        .map_err(|e| e.to_string())?;
    let mut cfg = GlobalConfig::default();
    if let Ok(map) = serde_json::from_value::<
        std::collections::HashMap<String, xgent_settings_core::ProviderConfig>,
    >(providers)
    {
        cfg.providers = map;
    }
    for key in [
        "default_provider",
        "default_model",
        "preferences.language",
        "preferences.theme",
    ] {
        let v = ipc
            .call_ok(
                methods::CONFIG_READ,
                config_read_params(ConfigScope::Global, key, None),
            )
            .await
            .map_err(|e| e.to_string())?;
        let s = v.as_str().unwrap_or_default().to_string();
        match key {
            "default_provider" => cfg.default_provider = s,
            "default_model" => cfg.default_model = s,
            "preferences.language" => cfg.preferences.language = s,
            _ => cfg.preferences.theme = s,
        }
    }
    Ok(cfg)
}

/// 读项目整份配置。
async fn read_project(
    ipc: &Arc<crate::ipc_client::IpcClient>,
    root: &std::path::Path,
) -> Result<ProjectConfig, String> {
    let v = ipc
        .call_ok(
            methods::CONFIG_READ,
            config_read_params(ConfigScope::Project, "project", Some(root)),
        )
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(v).map_err(|e| e.to_string())
}

/// `config.read` 参数。
fn config_read_params(
    scope: ConfigScope,
    key: &str,
    root: Option<&std::path::Path>,
) -> serde_json::Value {
    let req = ConfigReadRequest {
        scope,
        key: key.to_string(),
        project_root: root.map(|p| p.display().to_string()),
    };
    serde_json::to_value(req).unwrap_or(serde_json::Value::Null)
}

/// 回填两份配置资源并让面板重渲染。
fn drain_snapshot(
    mut rx: ResMut<SnapshotRx>,
    mut global: ResMut<GlobalConfigRes>,
    mut project: ResMut<ProjectConfigRes>,
    mut ready: ResMut<SettingsSnapshotReady>,
    mut live: Option<ResMut<LiveSettings>>,
    provider_info: Res<xgent_agent::ProviderInfo>,
    mut loc: ResMut<Localizer>,
) {
    while let Ok(r) = rx.rx.try_recv() {
        if let Some(g) = r.global.as_ref() {
            let switched = apply_language(&mut loc, &g.preferences.language);
            // 保留插件表等面板不读、也不该被 IPC 视图抹掉的字段
            global.0.providers = g.providers.clone();
            global.0.default_provider = g.default_provider.clone();
            global.0.default_model = g.default_model.clone();
            global.0.preferences.language = g.preferences.language.clone();
            global.0.preferences.theme = g.preferences.theme.clone();
            let _ = switched;
        }
        if let Some(p) = r.project.as_ref() {
            project.0 = p.clone();
        }
        if r.global.is_some() || r.project.is_some() {
            if let Some(live) = live.as_mut() {
                apply_snapshot(
                    &live,
                    &project.0,
                    &global.0,
                    &provider_info.id,
                    &provider_info.model,
                );
            }
        }
        ready.error = r.error.clone();
        if r.global.is_some() || r.project.is_some() {
            ready.ready = true;
        }
        ready.tick += 1;
    }
}

/// 面板保存 → 逐键写 → 重读权威值。
fn save_settings(
    mut reader: MessageReader<SaveSettingsMessage>,
    ipc: Res<IpcClientResource>,
    bridge: Res<AgentBridge>,
    root: Res<ProjectRoot>,
    tx: Res<SaveTx>,
) {
    for msg in reader.read() {
        let ipc = ipc.client.clone();
        let root = root.path.clone();
        let tx = tx.0.clone();
        let project_items = msg.project.clone();
        let global_items = msg.global.clone();
        bridge.runtime.handle().spawn(async move {
            let mut written: Vec<String> = Vec::new();
            let mut failed: Vec<(String, String)> = Vec::new();
            for item in &global_items {
                write_one(
                    &ipc,
                    ConfigScope::Global,
                    item,
                    None,
                    &mut written,
                    &mut failed,
                )
                .await;
            }
            for item in &project_items {
                write_one(
                    &ipc,
                    ConfigScope::Project,
                    item,
                    Some(&root),
                    &mut written,
                    &mut failed,
                )
                .await;
            }
            // 重读一次权威值：句柄只按 daemon 真正接受的内容更新
            let (global, project, error) = read_both(&ipc, &root).await;
            let _ = tx
                .send(SaveTaskResult {
                    report: SettingsSaveReportMessage { written, failed },
                    snapshot: Some(SnapshotResult {
                        global,
                        project,
                        error,
                    }),
                })
                .await;
        });
    }
}

/// 写一个键，把结果归入 written / failed。
async fn write_one(
    ipc: &Arc<crate::ipc_client::IpcClient>,
    scope: ConfigScope,
    item: &xgent_ui::settings_panel::ConfigItem,
    root: Option<&std::path::Path>,
    written: &mut Vec<String>,
    failed: &mut Vec<(String, String)>,
) {
    let req = ConfigWriteRequest {
        scope,
        key: item.key.clone(),
        value: item.value.clone(),
        project_root: root.map(|p| p.display().to_string()),
    };
    let params = serde_json::to_value(&req).unwrap_or(serde_json::Value::Null);
    match ipc.call_ok(methods::CONFIG_WRITE, params).await {
        Ok(_) => written.push(item.key.clone()),
        Err(e) => failed.push((item.key.clone(), e.to_string())),
    }
}

/// 保存结果 → 更新资源与句柄 → 报告面板 → 决定是否让面板重建。
///
/// `tick` 只在这批写入全部落盘时才推进，且**不成功的场合连这份资源都不碰**：
/// `ResMut` 的 `deref_mut` 会无条件标脏，而面板曾有两轮把重建挂在 `is_changed()`
/// 上——被 daemon 拒掉的那个值就是这样在下一帧被磁盘值换掉的。
///
/// 写完重读失败的原因也只在同一支里落到 `ready.error`：那需要一次重建才会显示到
/// 顶部提示行，而有键被拒时不能重建（会抹掉被拒的值），那种场合由逐键报告自己说清楚。
fn drain_save(
    mut rx: ResMut<SaveRx>,
    mut global: ResMut<GlobalConfigRes>,
    mut project: ResMut<ProjectConfigRes>,
    mut live: Option<ResMut<LiveSettings>>,
    provider_info: Res<xgent_agent::ProviderInfo>,
    mut writer: MessageWriter<SettingsSaveReportMessage>,
    mut ready: ResMut<SettingsSnapshotReady>,
    mut loc: ResMut<Localizer>,
) {
    while let Ok(r) = rx.rx.try_recv() {
        if let Some(snap) = r.snapshot.as_ref() {
            apply_save_result(
                &mut global,
                &mut project,
                live.as_deref_mut(),
                &provider_info,
                &mut loc,
                snap,
            );
            if r.report.failed.is_empty() {
                ready.error = snap.error.clone();
                ready.tick += 1;
            }
        }
        writer.write(r.report.clone());
    }
}

/// 这批写入里有没有 provider 字段。
///
/// 有则要重取 [`ProviderInfo`]（模型名与就绪判据按 kind 分，都来自 daemon 侧那份
/// 配置），否则顶栏与状态栏继续显示旧凭据状态。
fn touches_provider(written: &[String]) -> bool {
    written
        .iter()
        .any(|k| k == "providers" || k.starts_with("providers."))
}

/// provider 字段写过 → 异步重取一次 `ProviderInfo`（含重试配置）。
///
/// 与 [`drain_save`] 分开的理由只有一个：`drain_save` 因此不再需要 IPC 与
/// bridge，能被单独装进测试 App 与面板系统一起跑真实帧序。
fn refresh_provider_after_save(
    mut reader: MessageReader<SettingsSaveReportMessage>,
    ipc: Res<IpcClientResource>,
    bridge: Res<AgentBridge>,
    provider_info: Res<xgent_agent::ProviderInfo>,
    refresh_tx: Res<RefreshSender>,
) {
    for ev in reader.read() {
        if !touches_provider(&ev.written) {
            continue;
        }
        let ipc = ipc.client.clone();
        let tx = refresh_tx.0.clone();
        let pid = provider_info.id.clone();
        bridge.runtime.handle().spawn(async move {
            crate::config_bridge::spawn_refresh(&ipc, &tx, pid).await;
        });
    }
}

/// 把写完后重读到的权威快照落到两份配置资源与运行时句柄上。
///
/// 只落值，不碰渲染计数：让不让面板按这份快照重建由 [`drain_save`] 判（有键被拒时
/// 重渲染会用磁盘值重建那一格输入框，把用户刚填、且需要看到的值当场抹掉）。
/// 资源照旧跟磁盘一致，其余消费方读到的是真值。
fn apply_save_result(
    global: &mut GlobalConfigRes,
    project: &mut ProjectConfigRes,
    live: Option<&mut LiveSettings>,
    provider_info: &xgent_agent::ProviderInfo,
    loc: &mut Localizer,
    snap: &SnapshotResult,
) {
    if let Some(g) = snap.global.as_ref() {
        global.0.providers = g.providers.clone();
        global.0.default_provider = g.default_provider.clone();
        global.0.default_model = g.default_model.clone();
        global.0.preferences.language = g.preferences.language.clone();
        global.0.preferences.theme = g.preferences.theme.clone();
        apply_language(loc, &g.preferences.language);
    }
    if let Some(p) = snap.project.as_ref() {
        project.0 = p.clone();
    }
    if let Some(live) = live {
        apply_snapshot(
            live,
            &project.0,
            &global.0,
            &provider_info.id,
            &provider_info.model,
        );
    }
}

/// 对端窗口改配置 → 本窗口同步显示与运行时。
fn apply_peer_change(
    mut reader: MessageReader<ConfigChangedMessage>,
    ipc: Res<IpcClientResource>,
    bridge: Res<AgentBridge>,
    root: Res<ProjectRoot>,
    tx: Res<SnapshotTx>,
) {
    for ev in reader.read() {
        let Ok(changed) = serde_json::from_value::<xgent_core::config::ConfigChanged>(ev.0.clone())
        else {
            continue;
        };
        // 项目作用域只认自己这个根（daemon 已按订阅路由，这里是一层自检）
        if !concerns_root(&changed, &root.path) {
            continue;
        }
        let ipc = ipc.client.clone();
        let root = root.path.clone();
        let tx = tx.0.clone();
        bridge.runtime.handle().spawn(async move {
            let (global, project, error) = read_both(&ipc, &root).await;
            let _ = tx
                .send(SnapshotResult {
                    global,
                    project,
                    error,
                })
                .await;
        });
    }
}

/// 语言选择（面板与命令面板共用一条写入通路）。
fn save_language(
    mut reader: MessageReader<SaveLanguageMessage>,
    ipc: Res<IpcClientResource>,
    bridge: Res<AgentBridge>,
    tx: Res<SnapshotTx>,
    mut loc: ResMut<Localizer>,
) {
    for ev in reader.read() {
        // 先就地切换：语言是"看到即所得"的，等 IPC 往返会让用户以为没生效
        loc.switch(&ev.language);
        let ipc = ipc.client.clone();
        let tx = tx.0.clone();
        let lang = ev.language.clone();
        let root = {
            let r = bridge.project_root.clone();
            r
        };
        bridge.runtime.handle().spawn(async move {
            let req = ConfigWriteRequest {
                scope: ConfigScope::Global,
                key: "preferences.language".into(),
                value: serde_json::Value::String(lang),
                project_root: None,
            };
            let params = serde_json::to_value(&req).unwrap_or(serde_json::Value::Null);
            if let Err(e) = ipc.call_ok(methods::CONFIG_WRITE, params).await {
                tracing::warn!("写入 preferences.language 失败: {e}");
            }
            let (global, project, error) = read_both(&ipc, &root).await;
            let _ = tx
                .send(SnapshotResult {
                    global,
                    project,
                    error,
                })
                .await;
        });
    }
}

/// 把本地语言切到配置值；返回是否真的换了。
fn apply_language(loc: &mut Localizer, lang: &str) -> bool {
    if lang.is_empty() {
        return false;
    }
    xgent_ui::i18n::switch_language(loc, lang)
}

/// 这条配置变更是否与本窗口有利害关系。
///
/// 全局变更一律相关（provider 池、语言都是跨项目共享的）；项目变更只认根路径
/// 完全相等的那一个——同一个 daemon 上可以同时挂着多个项目的窗口，把它们都叫醒
/// 只会让每个窗口去重读与己无关的配置。缺 `project_root` 的项目变更同样不理。
pub fn concerns_root(
    change: &xgent_core::config::ConfigChanged,
    our_root: &std::path::Path,
) -> bool {
    match change.scope {
        ConfigScope::Global => true,
        ConfigScope::Project => {
            change.project_root.as_deref() == Some(our_root.display().to_string().as_str())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xgent_settings_core::{
        AgentConfig, ContextLimits, ToolConfig, ToolPolicyConfig, global::ProviderConfig,
    };

    fn global_with(provider_id: &str, pc: ProviderConfig) -> GlobalConfig {
        let mut g = GlobalConfig::default();
        g.default_provider = provider_id.to_string();
        g.providers.insert(provider_id.to_string(), pc);
        g
    }

    /// 初始句柄集带的是**生效值**，不是原始存储值（`0` 已解析成内置缺省）。
    #[test]
    fn new_handles_carry_effective_values() {
        let live = LiveSettings::new(&ProjectConfig::default(), 128_000);
        let rs = live.run_settings.read();
        assert_eq!(rs.max_parallel_tools, 4, "0 = 未设置 → 缺省 4，不是不限");
        assert_eq!(rs.retrieval_max_tokens, 8_000);
        assert_eq!(rs.max_events_per_frame, 64);
        assert_eq!(rs.context_window, 128_000);
        drop(rs);
        assert_eq!(live.tool_params.read().confirm_timeout_secs, 300);
        assert_eq!(live.retrieval_limits.read().tree_max_depth, 4);
        assert!(live.tool_policy.read().approved.is_empty());
    }

    /// A6/A7/A8/A12 的收口：一次 apply 换掉全部四组句柄。
    ///
    /// 这条是"apply 只认整份权威快照"的守卫：增量应用最容易漏的就是"改了
    /// `context_window` 却没重算窗口"这类半套更新。
    #[test]
    fn apply_snapshot_replaces_every_handle() {
        let live = LiveSettings::new(&ProjectConfig::default(), 128_000);
        let project = ProjectConfig {
            tool_policy: ToolPolicyConfig {
                approved: vec!["read_file".into()],
                denied: vec!["run_command".into()],
            },
            agent: AgentConfig {
                retrieval_max_tokens: 1_200,
                max_events_per_frame: 8,
                max_parallel_tools: 2,
            },
            tools: ToolConfig {
                confirm_timeout_secs: 45,
                command_timeout_secs: 12,
                command_max_output_bytes: 2_048,
            },
            context: ContextLimits {
                tree_max_depth: 1,
                tree_max_entries: 9,
                rg_timeout_secs: 3,
                max_search_files: 5,
                max_keywords: 6,
            },
            ..Default::default()
        };
        let global = global_with(
            "openai",
            ProviderConfig {
                context_window: 32_000,
                ..Default::default()
            },
        );
        apply_snapshot(&live, &project, &global, "openai", "gpt-4o");

        let policy = live.tool_policy.read();
        assert_eq!(policy.approved, vec!["read_file".to_string()]);
        assert_eq!(policy.denied, vec!["run_command".to_string()]);
        drop(policy);

        let rs = live.run_settings.read();
        assert_eq!(rs.retrieval_max_tokens, 1_200);
        assert_eq!(rs.max_events_per_frame, 8);
        assert_eq!(rs.max_parallel_tools, 2);
        assert_eq!(rs.context_window, 32_000, "provider 级窗口应被采纳");
        drop(rs);

        let params = live.tool_params.read();
        assert_eq!(params.confirm_timeout_secs, 45);
        assert_eq!(params.command_timeout_secs, 12);
        assert_eq!(params.command_max_output_bytes, 2_048);
        drop(params);

        let limits = live.retrieval_limits.read();
        assert_eq!(limits.tree_max_depth, 1);
        assert_eq!(limits.tree_max_entries, 9);
        assert_eq!(limits.rg_timeout_secs, 3);
        assert_eq!(limits.max_search_files, 5);
        assert_eq!(limits.max_keywords, 6);
    }

    /// 改回"未设置"要真的回到内置缺省（面板"留空即清除"依赖这条）。
    #[test]
    fn apply_snapshot_returns_to_defaults_when_unset_again() {
        let live = LiveSettings::new(&ProjectConfig::default(), 128_000);
        let global = GlobalConfig::default();
        let configured = ProjectConfig {
            agent: AgentConfig {
                max_parallel_tools: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        apply_snapshot(&live, &configured, &global, "", "");
        assert_eq!(live.run_settings.read().max_parallel_tools, 2);

        apply_snapshot(&live, &ProjectConfig::default(), &global, "", "");
        assert_eq!(
            live.run_settings.read().max_parallel_tools,
            4,
            "写回 0 应回到内置缺省 4"
        );
        assert_eq!(
            live.run_settings.read().context_window,
            128_000,
            "没有任何 provider 配置时按内置窗口"
        );
        assert!(live.tool_policy.read().approved.is_empty());
    }

    /// A9 的整条帧序：真实的 `drain_save` 与真实的面板系统跑在同一个 App 里。
    ///
    /// 前两轮都是"两半各自绿、合起来错"：面板把重建挂在 `is_changed()` 上，而
    /// `drain_save` 每帧都会写资源；两边单测各守一半，谁也没看见整条帧序。这里
    /// 不复刻任何一侧的逻辑——把保存结果投进通道，让真实系统与 Bevy 自己的调度
    /// 走完，再读输入框里的文本。
    #[test]
    fn a_failed_save_leaves_the_rejected_value_in_the_real_panel_frame() {
        let mut app = save_app();
        app.world_mut()
            .resource_mut::<xgent_ui::settings_panel::SettingsPanelState>()
            .open = true;
        app.world_mut()
            .resource_mut::<SettingsSnapshotReady>()
            .ready = true;
        for _ in 0..4 {
            app.update();
        }

        // 真实前置：面板打开后 app 取回一份快照并推进 tick，面板据此按磁盘值重建
        push_snapshot(&mut app, ProjectConfig::default());
        assert_eq!(
            num_field_text(&mut app, "context.tree_max_depth"),
            "4",
            "前置条件：成功快照落地后框里是磁盘/生效值"
        );
        let tick_after_clean = app.world().resource::<SettingsSnapshotReady>().tick;

        // 用户把深度改成 7，daemon 说这个键不认
        set_num_field_text(&mut app, "context.tree_max_depth", "7");
        let mut disk = ProjectConfig::default();
        disk.context.tree_max_depth = 4;
        push_save(
            &mut app,
            vec!["agent.max_parallel_tools".into()],
            vec![("context.tree_max_depth".into(), "未知配置 key".into())],
            Some(disk),
        );
        assert_eq!(
            app.world().resource::<SettingsSnapshotReady>().tick,
            tick_after_clean,
            "有键被拒时不能推进渲染计数"
        );
        assert_eq!(
            num_field_text(&mut app, "context.tree_max_depth"),
            "7",
            "被拒的值要留在框里——这正是 A9 三轮争的那一条"
        );
        assert!(
            app.world()
                .resource::<xgent_ui::settings_panel::SettingsPanelState>()
                .open,
            "有失败时面板保持打开"
        );

        // 随后一次合法的刷新（对端改了配置 → 走取快照那条通路，不经"保存"）
        // 必须把显示拉回磁盘值，不能因为刚失败过一次就永久停更
        let mut peer = ProjectConfig::default();
        peer.context.tree_max_depth = 6;
        push_snapshot(&mut app, peer);
        assert_eq!(
            num_field_text(&mut app, "context.tree_max_depth"),
            "6",
            "刷新不该被上一次失败永久挡住"
        );
    }

    /// `drain_save` 的计数判据：只有全部落盘才推进 `tick`。
    #[test]
    fn drain_save_advances_the_render_counter_only_when_every_key_landed() {
        let mut app = save_app();
        let start = app.world().resource::<SettingsSnapshotReady>().tick;

        push_save(&mut app, vec!["a.b".into()], Vec::new(), None);
        assert_eq!(
            app.world().resource::<SettingsSnapshotReady>().tick,
            start + 1,
            "全部成功推进一格"
        );

        push_save(
            &mut app,
            vec!["a.b".into()],
            vec![("c.d".into(), "未知配置 key".into())],
            None,
        );
        assert_eq!(
            app.world().resource::<SettingsSnapshotReady>().tick,
            start + 1,
            "有键被拒时一格都不许动"
        );

        push_save(
            &mut app,
            Vec::new(),
            vec![("c.d".into(), "boom".into())],
            None,
        );
        assert_eq!(
            app.world().resource::<SettingsSnapshotReady>().tick,
            start + 1,
            "全部失败同样不许动"
        );
    }

    /// 写完重读失败的原因要显示得出来，而"有键被拒"那一支照旧不动这份资源。
    ///
    /// 有键被拒时不能重建面板（会抹掉被拒的值），顶部提示行也就跟着写不进去——
    /// 那种场合由逐键报告自己说清楚，这里把边界钉住而不是假装已显示。
    #[test]
    fn a_clean_save_surfaces_a_re_read_failure_and_a_rejected_one_does_not() {
        let mut app = save_app();
        let tx = app.world().resource::<SaveTx>().0.clone();

        // 有键被拒 + 重读也失败：资源原样不动
        let _ = tx.try_send(SaveTaskResult {
            report: SettingsSaveReportMessage {
                written: Vec::new(),
                failed: vec![("c.d".to_string(), "boom".to_string())],
            },
            snapshot: Some(SnapshotResult {
                global: None,
                project: None,
                error: "第一次的原因".to_string(),
            }),
        });
        for _ in 0..3 {
            app.update();
        }
        {
            let ready = app.world().resource::<SettingsSnapshotReady>();
            assert!(
                ready.error.is_empty(),
                "有键被拒时不碰这份资源：{}",
                ready.error
            );
        }

        // 全成功 + 重读失败：原因落进面板状态，并推进一次计数
        let _ = tx.try_send(SaveTaskResult {
            report: SettingsSaveReportMessage {
                written: vec!["a.b".to_string()],
                failed: Vec::new(),
            },
            snapshot: Some(SnapshotResult {
                global: None,
                project: None,
                error: "解析 /proj/.xgent/config.toml 失败".to_string(),
            }),
        });
        for _ in 0..3 {
            app.update();
        }
        let tick_with_error = {
            let ready = app.world().resource::<SettingsSnapshotReady>();
            assert_eq!(
                ready.error, "解析 /proj/.xgent/config.toml 失败",
                "重读失败要让面板说得出原因，而不是演成“配置都是默认值”"
            );
            ready.tick
        };

        // 之后一次全成功且重读正常：原因撤掉，计数再推进
        let _ = tx.try_send(SaveTaskResult {
            report: SettingsSaveReportMessage {
                written: Vec::new(),
                failed: Vec::new(),
            },
            snapshot: Some(SnapshotResult {
                global: None,
                project: None,
                error: String::new(),
            }),
        });
        for _ in 0..3 {
            app.update();
        }
        let ready = app.world().resource::<SettingsSnapshotReady>();
        assert!(ready.error.is_empty(), "恢复可读后原因要撤掉");
        assert_eq!(ready.tick, tick_with_error + 1);
    }

    /// provider 字段写过才需要重取 ProviderInfo。
    #[test]
    fn provider_writes_are_detected_from_the_report() {
        assert!(touches_provider(&["providers.openai.api_base".into()]));
        assert!(touches_provider(&["providers".into()]));
        assert!(!touches_provider(&[
            "agent.max_parallel_tools".into(),
            "preferences.language".into()
        ]));
        assert!(!touches_provider(&["providers_extra".into()]));
    }

    /// 别的项目的变更不该把本窗口叫醒（多项目共用一个 daemon 时的自检）。
    #[test]
    fn concerns_root_filters_peer_changes_by_scope_and_root() {
        let our = std::path::Path::new("/proj/a");
        let change = |scope: ConfigScope, root: Option<&str>| xgent_core::config::ConfigChanged {
            scope,
            key: "agent.max_parallel_tools".into(),
            value: serde_json::json!(3),
            project_root: root.map(str::to_string),
        };
        assert!(
            concerns_root(&change(ConfigScope::Global, None), our),
            "全局变更与本窗口有关"
        );
        assert!(
            concerns_root(&change(ConfigScope::Project, Some("/proj/a")), our),
            "同根的项目变更应被处理"
        );
        assert!(
            !concerns_root(&change(ConfigScope::Project, Some("/proj/b")), our),
            "别的项目的变更不该叫醒本窗口"
        );
        assert!(
            !concerns_root(&change(ConfigScope::Project, None), our),
            "缺根的项目变更不该被当作自己的"
        );
    }

    // --- 测试装配：把面板与本模块的保存系统装进同一个 App ---------------

    /// 最小但完整的面板装配（与 `xgent_ui/tests/common/mod.rs` 同一套插件），
    /// 外加本模块的通道资源与 `drain_save`——为的是能跑真实的帧序。
    fn save_app() -> App {
        let (s_tx, s_rx) = mpsc::channel::<SnapshotResult>(8);
        let (w_tx, w_rx) = mpsc::channel::<SaveTaskResult>(8);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .add_plugins(bevy::image::ImagePlugin::default())
            .add_plugins(bevy::window::WindowPlugin::default())
            .add_message::<bevy::window::WindowEvent>()
            .add_plugins(bevy::picking::input::PointerInputPlugin)
            .add_plugins(bevy::picking::PickingPlugin)
            .add_plugins(bevy::picking::InteractionPlugin)
            .add_plugins(bevy::input::InputPlugin)
            .add_plugins(bevy::input_focus::InputFocusPlugin)
            .add_plugins(bevy::input_focus::InputDispatchPlugin)
            .add_plugins(bevy::text::TextPlugin)
            .add_plugins(bevy::ui::UiPlugin)
            .add_plugins(bevy::ui_widgets::UiWidgetsPlugins)
            .init_asset::<bevy::image::TextureAtlasLayout>()
            .add_plugins(bevy::camera::visibility::VisibilityPlugin)
            .add_plugins(bevy::mesh::MeshPlugin)
            .init_resource::<xgent_ui::theme::Theme>()
            .init_resource::<Localizer>()
            .insert_resource(xgent_ui::fonts::UiFonts {
                ui: Handle::default(),
                mono: Handle::default(),
            })
            .insert_resource(GlobalConfigRes::default())
            .insert_resource(ProjectConfigRes::default())
            .insert_resource(xgent_agent::ProviderInfo::default())
            .insert_resource(xgent_tools::ToolExecutorResource(std::sync::Arc::new(
                xgent_tools::ToolExecutor::with_defaults(),
            )))
            .add_plugins(xgent_ui::settings_panel::SettingsPanelPlugin)
            .add_message::<xgent_ui::kit::ToastMessage>()
            .insert_resource(SnapshotTx(s_tx))
            .insert_resource(SnapshotRx { rx: s_rx })
            .insert_resource(SaveTx(w_tx))
            .insert_resource(SaveRx { rx: w_rx })
            .add_systems(Update, (drain_snapshot, drain_save));
        app.world_mut().spawn(Camera::default());
        app
    }

    /// 投一份"打开面板后取回的快照"（与保存通路不同，它不写报告、也不会关面板）。
    fn push_snapshot(app: &mut App, project: ProjectConfig) {
        let tx = app.world().resource::<SnapshotTx>().0.clone();
        let _ = tx.try_send(SnapshotResult {
            global: None,
            project: Some(project),
            error: String::new(),
        });
        for _ in 0..4 {
            app.update();
        }
    }

    /// 把一份保存结果（连同写完重读到的权威快照）投进通道并跑几帧。
    fn push_save(
        app: &mut App,
        written: Vec<String>,
        failed: Vec<(String, String)>,
        project: Option<ProjectConfig>,
    ) {
        let tx = app.world().resource::<SaveTx>().0.clone();
        let _ = tx.try_send(SaveTaskResult {
            report: SettingsSaveReportMessage { written, failed },
            snapshot: Some(SnapshotResult {
                global: None,
                project,
                error: String::new(),
            }),
        });
        for _ in 0..4 {
            app.update();
        }
    }

    /// 面板里某个数值框的当前文本。
    fn num_field_text(app: &mut App, key: &str) -> String {
        use bevy::text::EditableText;
        let mut q = app.world_mut().query::<(
            Entity,
            &EditableText,
            &xgent_ui::settings_panel::SettingsNumInput,
        )>();
        let found: Vec<String> = q
            .iter(app.world())
            .filter(|(_, _, n)| n.key == key)
            .map(|(_, e, _)| e.value().to_string())
            .collect();
        assert_eq!(
            found.len(),
            1,
            "键 {key} 应恰有一个输入框，实际 {} 个",
            found.len()
        );
        found[0].clone()
    }

    /// 改某个数值框的文本（等价于用户改框）。
    fn set_num_field_text(app: &mut App, key: &str, text: &str) {
        use bevy::text::EditableText;
        let target = {
            let mut q = app
                .world_mut()
                .query::<(Entity, &xgent_ui::settings_panel::SettingsNumInput)>();
            let found: Vec<Entity> = q
                .iter(app.world())
                .filter(|(_, n)| n.key == key)
                .map(|(e, _)| e)
                .collect();
            assert_eq!(found.len(), 1, "键 {key} 应恰有一个输入框");
            found[0]
        };
        let mut e = app
            .world_mut()
            .get_mut::<EditableText>(target)
            .expect("输入框应有 EditableText");
        *e = EditableText::new(text.to_string());
    }
}
