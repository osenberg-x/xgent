//! xgent_app — UI 进程入口。
//!
//! 职责：解析命令行参数、组装所有 UI 侧插件、探测/拉起 daemon、建立 IPC 连接、
//! 把 IPC 封装为 agent bridge 用的 ProviderClient、打开项目、运行 Bevy App。

mod config_bridge;
mod daemon;
mod fs_event_bridge;
mod ipc_client;
mod provider_client;
mod startup;

use std::sync::Arc;

use bevy::prelude::*;
use clap::Parser;
use xgent_agent::bridge::{AgentBridge, AgentBridgeConfig};
use xgent_context::ContextHub;
use xgent_plugin::{PluginHost, PluginHostProxy, WasmHost};
use xgent_plugin_host::{PluginEventRx, PluginHostResource, register_proxy_impls};
use xgent_settings_core::paths::{daemon_socket_path, plugins_dir};
use xgent_settings_core::store::{GlobalConfigStore, ProjectConfigStore};
use xgent_tools::{ToolExecutor, ToolExecutorResource};

use crate::daemon::connect_or_spawn_daemon;
use crate::fs_event_bridge::{IpcClientResource, NotifPump};
use crate::provider_client::IpcProviderClient;

/// 命令行参数。
#[derive(Parser, Resource, Debug, Clone)]
#[command(name = "xgent", version, about = "XGent — AI 代码助手")]
pub struct Args {
    /// 项目根目录
    #[arg(long, default_value = ".")]
    project: std::path::PathBuf,

    /// provider id 覆盖
    #[arg(long)]
    provider: Option<String>,

    /// 模型名覆盖
    #[arg(long)]
    model: Option<String>,
}

fn main() {
    // 初始化日志：tracing-subscriber 默认启用 tracing-log 特性，
    // 自动桥接 log crate → tracing，故 icu_provider 的日志也会被 EnvFilter 过滤。
    // 不使用 Bevy 的 LogPlugin（它会重复设置全局 subscriber），改为手动初始化。
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(
            // wgpu/naga 噪音降级；icu_provider 的 data error warn 降级为 error
            "info,wgpu=error,naga=warn,icu_provider=error",
        )
    });
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let args = Args::parse();

    // 验证项目路径存在
    let project_root = match std::fs::canonicalize(&args.project) {
        Ok(p) => p,
        Err(_) => {
            eprintln!("错误：项目路径不存在或无法访问: {}", args.project.display());
            std::process::exit(1);
        }
    };

    if !project_root.is_dir() {
        eprintln!("错误：项目路径不是目录: {}", project_root.display());
        std::process::exit(1);
    }

    // 用一个临时 tokio runtime 完成 daemon 连接（之后 agent bridge 自带 runtime）
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("错误：无法创建 tokio 运行时: {e}");
            std::process::exit(1);
        }
    };
    let ipc = match rt.block_on(async { connect_or_spawn_daemon().await }) {
        Ok(ipc) => ipc,
        Err(e) => {
            eprintln!("错误：无法连接 daemon: {e:#}");
            eprintln!("提示：可尝试手动启动 daemon: cargo run -p xgent_daemon");
            std::process::exit(1);
        }
    };
    let ipc = Arc::new(ipc);
    // 构造 agent bridge 依赖
    let provider =
        Arc::new(IpcProviderClient::new(ipc.clone())) as Arc<dyn xgent_agent::ProviderClient>;
    // ToolExecutor 作为 Resource（与 AgentBridge 共享同一 Arc）。
    // 插件系统经 ToolExecutorResource 注册工具到同一实例。
    let executor = Arc::new(ToolExecutor::with_defaults());
    // Editor 命令 channel：agent EditorTool → ECS（经 agent_poll_system 桥接）
    let (editor_cmd_tx, editor_cmd_rx) =
        tokio::sync::mpsc::channel::<xgent_tools::EditorCommandRequest>(32);
    let editor_sink = Arc::new(xgent_agent::ChannelEditorCommandSink::new(editor_cmd_tx));
    let editor_tool = xgent_tools::EditorTool::new(editor_sink);
    executor.register(Arc::new(editor_tool));
    // ContextHub 包装内置 provider + 动态插件 provider。
    // 作为 Arc<dyn ContextProvider> 注入 bridge（agent 无感于内置 vs 插件）。
    //
    // 内置 provider 按项目配置的 context_strategy 选择（R2-4）。无实现的
    // 策略显式报错而非静默回退：静默回退会让误配置的项目看起来完全正常。
    let project_config_for_ctx = ProjectConfigStore::load(&project_root).unwrap_or_default();
    let builtin_ctx: Arc<dyn xgent_context::ContextProvider> =
        match xgent_context::build_context_provider(
            project_config_for_ctx.context_strategy,
            project_root.clone(),
        ) {
            xgent_context::BuiltContextProvider::Ready(p) => Arc::from(p),
            xgent_context::BuiltContextProvider::Unsupported(s) => {
                tracing::error!(
                    strategy = ?s,
                    "context_strategy 尚无实现，已中止启动（不静默回退到其他策略）。                     可用策略：on_demand"
                );
                std::process::exit(2);
            }
        };
    let context_hub = Arc::new(ContextHub::default());
    context_hub.set_builtin(vec![builtin_ctx]);
    let context = context_hub.clone() as Arc<dyn xgent_context::ContextProvider>;
    // 加载全局配置（daemon 也持有同一份，此处用于派生默认 provider/model 与重试配置）
    let global_config = GlobalConfigStore::load().unwrap_or_default();
    // 加载项目配置（bridge 需 tool_policy）
    let project_config = ProjectConfigStore::load(&project_root).unwrap_or_default();

    // 派生重试配置：命令行 provider > 项目配置 > 全局配置的 default provider
    let provider_id_for_retry = args
        .provider
        .clone()
        .or(project_config.provider_override.clone())
        .or_else(|| {
            let id = global_config.default_provider.clone();
            if id.is_empty() { None } else { Some(id) }
        });
    let retry_config = provider_id_for_retry
        .as_deref()
        .and_then(|pid| global_config.providers.get(pid))
        .map(xgent_agent::bridge::RetryConfig::from)
        .unwrap_or_default();
    // 派生当前 provider/model：命令行 > 项目配置 > 全局配置
    let provider_id = args
        .provider
        .clone()
        .or(project_config.provider_override.clone())
        .or_else(|| {
            let id = global_config.default_provider.clone();
            if id.is_empty() { None } else { Some(id) }
        });
    let model = args.model.clone().or_else(|| {
        let m = global_config.default_model.clone();
        if m.is_empty() { None } else { Some(m) }
    });
    let (provider_id, model) = derive_provider_model(provider_id, model);
    // Compaction provider：复用 agent 的 ProviderClient，与对话同 provider/model。
    // context_window 用默认 128k（后续可从 ModelInfo 派生，见 D-04）。
    let compactor: Arc<dyn xgent_agent::CompactionProvider> = Arc::new(
        xgent_agent::LlmCompactor::new(provider.clone(), provider_id.clone(), model.clone()),
    );
    let bridge = AgentBridge::new(AgentBridgeConfig {
        provider,
        executor: executor.clone(),
        context,
        project_root: project_root.clone(),
        tool_policy: project_config.tool_policy.clone(),
        retry_config: Arc::new(parking_lot::RwLock::new(retry_config)),
        compaction: Some(compactor),
        context_window: 128_000,
        compaction_settings: xgent_agent::CompactionSettings::default(),
        // 有界执行：防止 LLM 反复请求同一 tool_call 导致无限执行与无限计费（R1-1）
        max_tool_rounds: Some(xgent_agent::loop_limits::MAX_TOOL_ROUNDS),
        max_tokens_per_turn: Some(xgent_agent::loop_limits::MAX_TOKENS_PER_TURN),
    });

    // 通知订阅端（fs/config 桥接用）
    let notif_rx = ipc.subscribe();
    // 终端复用 agent bridge 的 tokio runtime handle（bridge 在下方 insert_resource 移动）
    let terminal_rt_handle = bridge.runtime.handle().clone();
    let plugin_rt_handle = bridge.runtime.handle().clone();
    // 插件系统组装（照设计文档 §13 Step P4）：
    // 1. 创建 PluginHostProxy + WasmHost + PluginHost（event_rx 持有，注入 ECS）
    // 2. PluginHostPlugin build 时注册 proxy impl（发 PluginOp 到 PluginOpQueue）
    // 3. 业务 Plugin（XgentTools/CommandPalette/XgentContext）已 add，proxy 注册时
    //    ToolExecutor/CommandRegistry/ContextHub 已就绪
    // 4. load_builtin_plugins 扫描 assets/plugins/ 预装内建插件
    let proxy = Arc::new(PluginHostProxy::new());
    let wasm_host = WasmHost::new(proxy.clone(), project_root.clone())
        .expect("wasmtime 引擎初始化失败（致命错误）");
    let plugins_root = plugins_dir();
    let _ = std::fs::create_dir_all(&plugins_root);
    let assets_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/plugins");
    let (plugin_host, event_rx) = PluginHost::new(
        proxy.clone(),
        wasm_host,
        plugins_root,
        if assets_dir.exists() {
            Some(assets_dir)
        } else {
            None
        },
        global_config.plugin_settings.clone(),
        global_config.plugin.enabled.clone(),
    );
    // 注册 proxy impl（返回 op_rx，包成 PluginOpRx Resource 注入 ECS，
    let op_rx = register_proxy_impls(&proxy);
    // 启动插件目录文件监听（生产模式，200ms debounce reload，§8.5）
    plugin_host.start_file_watcher(plugin_rt_handle.clone());
    // 组装 App
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "XGent".into(),
                    // 最小窗口尺寸（方案 §8.8）：五列/四列布局的钳制下限
                    resize_constraints: WindowResizeConstraints {
                        min_width: 1024.0,
                        min_height: 640.0,
                        ..default()
                    },
                    ..default()
                }),
                ..default()
            })
            .set(bevy::asset::AssetPlugin {
                // 资产根指向本 crate 的 assets/（对齐内建插件目录的 CARGO_MANIFEST_DIR
                // 取径方式），使 AssetServer 在任意工作目录下可用（v7 字体/图标依赖）。
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").to_string(),
                ..default()
            })
            .disable::<bevy::log::LogPlugin>(),
    )
    .add_plugins((
        xui::XuiPlugin,
        xgent_settings::XgentSettingsPlugin,
        xgent_agent::XgentAgentPlugin,
        xgent_context::XgentContextPlugin,
        xgent_plugin_host::PluginHostPlugin,
        xgent_ui::XgentUiPlugin,
        crate::config_bridge::ConfigBridgePlugin,
        crate::fs_event_bridge::FsEventBridgePlugin,
    ))
    .insert_resource(args)
    .insert_resource(xgent_ui::file_panel::ProjectRoot {
        path: project_root.clone(),
    })
    .insert_resource(xgent_ui::terminal::TerminalIoRuntime::new(
        terminal_rt_handle,
        xgent_terminal::LocalPtyBackend::new(),
    ))
    .insert_resource(bridge)
    .insert_resource(IpcClientResource {
        client: ipc.clone(),
    })
    .insert_resource(NotifPump { rx: notif_rx })
    .insert_resource(xgent_agent::ProviderInfo {
        id: provider_id,
        model,
        ready: false,
        kind: None,
    })
    .insert_resource(ToolExecutorResource(executor.clone()))
    .insert_resource(PluginHostResource(plugin_host.clone()))
    .insert_resource(PluginEventRx(parking_lot::Mutex::new(event_rx)))
    .insert_resource(xgent_plugin_host::PluginOpRx(parking_lot::Mutex::new(
        op_rx,
    )))
    .insert_resource(xgent_agent::EditorCommandRx {
        rx: tokio::sync::Mutex::new(editor_cmd_rx),
    });
    // 在 bridge 的 tokio runtime 上 spawn load_builtin_plugins（async，主线程无法 await）
    {
        let host = plugin_host.clone();
        plugin_rt_handle.spawn(async move {
            if let Err(e) = host.load_builtin_plugins().await {
                tracing::warn!(error = %e, "加载内建插件失败");
            }
        });
    }
    app.add_systems(
        Startup,
        (crate::startup::load_fonts, crate::startup::open_project),
    );
    app.add_systems(Update, crate::startup::ui_screenshot_tool);
    // ===== 临时诊断探测（XGENT_AUTOPLAY=<项目相对路径>）=====
    // 自动开文件 → 模拟持续滚轮 → 阶段性截图 + 每 60 帧打帧耗时 → 自动退出。
    // 仅用于定位预览区滚动卡顿与行号错位，诊断完成后删除。
    if let Ok(rel) = std::env::var("XGENT_AUTOPLAY") {
        tracing::info!("autoplay 诊断启用: {rel}");
        app.insert_resource(AutoplayProbe {
            rel: rel.into(),
            opened: false,
            frame: 0,
        });
        app.add_systems(Update, autoplay_probe);
        app.add_systems(Last, autoplay_cpu_probe);
    }

    // 清理提示：退出时 daemon 末个客户端退出后自退出
    let socket_path = daemon_socket_path();
    tracing::info!("xgent_app 启动，daemon socket: {}", socket_path.display());

    app.run();
}

/// 从参数与配置派生 provider id 与 model。
///
/// 优先级：命令行 > 项目配置 > 全局配置。三者皆空时返回空串，
/// UI 侧据此判断未配置状态并提示用户设置 provider。
fn derive_provider_model(provider_id: Option<String>, model: Option<String>) -> (String, String) {
    (provider_id.unwrap_or_default(), model.unwrap_or_default())
}

// ===== 临时诊断探测（用后即删）=====

/// 探测参数。
#[derive(Resource)]
struct AutoplayProbe {
    rel: std::path::PathBuf,
    opened: bool,
    frame: u32,
}

/// 探测序列（60fps 假设）：
/// - frame 90：打开目标文件（走真实 OpenFileRequest → io → 高亮 → 渲染链路）
/// - frame 330..1000：每帧 `ScrollPosition.y += 8` 模拟持续滚轮（约 5.4k 像素）
/// - frame 300/660/1020：截图（顶部 / 滚动中 / 底部）
/// - 每 60 帧打印帧耗时（定位停滞帧与停滞阶段）
/// - frame 1240 自动退出
fn autoplay_probe(
    mut state: ResMut<AutoplayProbe>,
    mut commands: Commands,
    time: Res<Time>,
    project_root: Res<xgent_ui::file_panel::ProjectRoot>,
    mut open_writer: MessageWriter<xgent_ui::editor::tabs::OpenFileRequest>,
    mut q_scroll: Query<&mut bevy::ui::ScrollPosition, With<xui::TextEditor>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};

    state.frame += 1;
    let f = state.frame;

    if f == 90 && !state.opened {
        state.opened = true;
        open_writer.write(xgent_ui::editor::tabs::OpenFileRequest {
            path: project_root.path.join(&state.rel),
            line: None,
        });
    }

    if (330..850).contains(&f) {
        for mut sp in q_scroll.iter_mut() {
            sp.y += 8.0;
        }
    }

    for (frame, path) in [
        (300u32, "target/autoplay_top.png"),
        (600, "target/autoplay_mid.png"),
        (880, "target/autoplay_end.png"),
    ] {
        if f == frame {
            let p = path.to_string();
            tracing::info!("autoplay: 截图 {p}");
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(p));
        }
    }

    // 每帧打 dt（与各系统 xperf 日志对齐，定位停滞系统）
    tracing::info!(
        target: "xperf",
        "frame {f} dt={:.1}",
        time.delta().as_secs_f64() * 1000.0
    );

    if f >= 880 {
        tracing::info!("autoplay: 诊断结束，自动退出");
        app_exit.write(AppExit::Success);
    }
}

/// 帧 CPU 耗时探针（Last 调度）：上一帧 Last 到本帧 Last 的墙钟间隔。
/// 与 `time.delta()`（帧间隔）对比：CPU ≈ dt → 全程在算；CPU ≪ dt → 在等（渲染/睡眠）。
fn autoplay_cpu_probe(mut last: Local<Option<std::time::Instant>>, mut c: Local<u32>) {
    let now = std::time::Instant::now();
    if let Some(prev) = *last {
        tracing::info!(
            target: "xperf",
            "cpu-frame {} cpu={:.1}ms",
            *c,
            now.duration_since(prev).as_secs_f64() * 1000.0
        );
    }
    *last = Some(now);
    *c += 1;
}
