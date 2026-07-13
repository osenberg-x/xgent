//! xgent_app — UI 进程入口。
//!
//! 职责：解析命令行参数、组装所有 UI 侧插件、探测/拉起 daemon、建立 IPC 连接、
//! 把 IPC 封装为 agent bridge 用的 ProviderClient、打开项目、运行 Bevy App。

mod daemon;
mod fs_event_bridge;
mod ipc_client;
mod provider_client;
mod startup;

use std::sync::Arc;

use bevy::prelude::*;
use clap::Parser;
use xgent_agent::bridge::{AgentBridge, AgentBridgeConfig};
use xgent_context::OnDemandContextProvider;
use xgent_settings::Localizer;
use xgent_settings_core::paths::daemon_socket_path;
use xgent_settings_core::store::ProjectConfigStore;
use xgent_tools::ToolExecutor;
use xui::i18n_bridge::Strings;

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
    // 日志
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    // 验证项目路径存在
    let project_root = match std::fs::canonicalize(&args.project) {
        Ok(p) => p,
        Err(_) => {
            eprintln!(
                "错误：项目路径不存在或无法访问: {}",
                args.project.display()
            );
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
    let executor = Arc::new(ToolExecutor::with_defaults());
    let context = Arc::new(OnDemandContextProvider::new(project_root.clone()))
        as Arc<dyn xgent_context::ContextProvider>;
    let bridge = AgentBridge::new(AgentBridgeConfig {
        provider,
        executor,
        context,
        project_root: project_root.clone(),
    });

    // 加载项目配置
    let project_config = ProjectConfigStore::load(&project_root).unwrap_or_default();

    // 派生当前 provider/model
    let provider_id = args
        .provider
        .clone()
        .or(project_config.provider_override.clone());
    let (provider_id, model) = derive_provider_model(provider_id, args.model.clone());

    // 通知订阅端（fs/config 桥接用）
    let notif_rx = ipc.subscribe();

    // 组装 App
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "XGent".into(),
            ..default()
        }),
        ..default()
    }))
    .add_plugins((
        xui::XuiPlugin,
        xgent_settings::XgentSettingsPlugin,
        xgent_agent::XgentAgentPlugin,
        xgent_ui::XgentUiPlugin,
        crate::fs_event_bridge::FsEventBridgePlugin,
    ))
    .insert_resource(args)
    .insert_resource(Localizer::default())
    .insert_resource(Strings(Box::new(Localizer::default())))
    .insert_resource(bridge)
    .insert_resource(IpcClientResource {
        client: ipc.clone(),
    })
    .insert_resource(NotifPump { rx: notif_rx })
    .insert_resource(xgent_settings::ProjectConfigRes(project_config))
    .insert_resource(xgent_agent::ProviderInfo {
        id: provider_id,
        model,
    })
    .add_systems(Startup, crate::startup::open_project);

    // 清理提示：退出时 daemon 末个客户端退出后自退出
    let socket_path = daemon_socket_path();
    tracing::info!("xgent_app 启动，daemon socket: {}", socket_path.display());

    app.run();
}

/// 从参数与项目配置派生 provider id 与 model。
fn derive_provider_model(provider_id: Option<String>, model: Option<String>) -> (String, String) {
    let pid = provider_id.unwrap_or_else(|| "openai".to_string());
    let m = model.unwrap_or_else(|| "gpt-4o-mini".to_string());
    (pid, m)
}
