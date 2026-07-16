//! 配置桥接：UI 消息 → daemon IPC 配置读写 → 刷新 ProviderInfo。
//!
//! - [`save_provider_config`]：读 [`SaveProviderConfigMessage`]，经 IPC `config.write`
//!   写 daemon 全局配置（provider 各字段 + default_provider 联动）。
//! - [`drain_pending_refresh`]：每帧从异步刷新 channel 拉取结果，刷新
//!   [`ProviderInfo`] Resource（daemon 侧权威，UI 仅缓存投影）。
//!
//! 权威源在 daemon 侧（ADR 0001）。UI 的 `ProviderInfo` 经 daemon 广播的
//! `CONFIG_CHANGED` 通知触发重读，多开窗口一致。
//!
//! 刷新流程是异步的（IPC `config.read`），结果经 mpsc channel 回 ECS。
//! [`PendingRefresh`] 持有 receiver，每帧 drain。

use bevy::prelude::*;
use tokio::sync::mpsc;
use xgent_agent::bridge::AgentBridge;
use xgent_agent::ProviderInfo;
use xgent_core::config::{ConfigReadRequest, ConfigScope, ConfigWriteRequest};
use xgent_core::methods;
use xgent_settings_core::global::{ProviderConfig, ProviderKind};
use xgent_ui::settings_panel::SaveProviderConfigMessage;

use crate::fs_event_bridge::{ConfigChangedMessage, IpcClientResource};

/// 刷新结果（从 async task 经 channel 传回 ECS）。
struct RefreshResult {
    provider_id: String,
    model: String,
    kind: Option<ProviderKind>,
    ready: bool,
}

/// 异步刷新结果的接收端（每帧 drain）。
#[derive(Resource)]
pub struct PendingRefresh {
    rx: mpsc::Receiver<RefreshResult>,
}

/// 异步刷新结果的发送端（clone 给各 spawn task）。
#[derive(Resource, Clone)]
struct RefreshSender(mpsc::Sender<RefreshResult>);

/// 配置桥接插件：注册系统与资源。
pub struct ConfigBridgePlugin;

impl Plugin for ConfigBridgePlugin {
    fn build(&self, app: &mut App) {
        let (tx, rx) = mpsc::channel::<RefreshResult>(16);
        app.insert_resource(PendingRefresh { rx })
            .insert_resource(RefreshSender(tx))
            .add_systems(
                Update,
                (save_provider_config, drain_pending_refresh, refresh_on_startup),
            );
    }
}

/// 处理保存 provider 配置：经 IPC 写 daemon 全局配置。
///
/// 写入：
/// 1. `providers.<id>.kind` / `api_base` / `api_key` / `model_overrides`
/// 2. `default_provider`（联动设为当前保存的 provider id，保存即生效）
///
/// 写完后 spawn 一次刷新 task，结果经 channel 回 ECS。
fn save_provider_config(
    mut reader: MessageReader<SaveProviderConfigMessage>,
    ipc: Res<IpcClientResource>,
    bridge: Res<AgentBridge>,
    refresh_tx: Res<RefreshSender>,
) {
    for ev in reader.read() {
        let ipc = ipc.client.clone();
        let pid = ev.provider_id.clone();
        let kind = ev.kind;
        let api_base = ev.api_base.clone();
        let api_key = ev.api_key.clone();
        let model = ev.model.clone();
        let tx = refresh_tx.0.clone();

        bridge.runtime.handle().spawn(async move {
            // 写 provider 各字段
            let fields: [(&str, serde_json::Value); 4] = [
                ("kind", serde_json::to_value(kind).unwrap_or(serde_json::Value::Null)),
                ("api_base", serde_json::Value::String(api_base)),
                ("api_key", serde_json::Value::String(api_key)),
                ("model_overrides", serde_json::Value::String(model)),
            ];
            for (field, value) in fields {
                let key = format!("providers.{pid}.{field}");
                let req = ConfigWriteRequest {
                    scope: ConfigScope::Global,
                    key,
                    value,
                };
                let params = serde_json::to_value(&req).unwrap_or(serde_json::Value::Null);
                if let Err(e) = ipc.call_ok(methods::CONFIG_WRITE, params).await {
                    tracing::error!("写入配置 {pid}.{field} 失败: {e}");
                }
            }
            // default_provider 联动：保存即生效
            let req = ConfigWriteRequest {
                scope: ConfigScope::Global,
                key: "default_provider".to_string(),
                value: serde_json::Value::String(pid.clone()),
            };
            let params = serde_json::to_value(&req).unwrap_or(serde_json::Value::Null);
            if let Err(e) = ipc.call_ok(methods::CONFIG_WRITE, params).await {
                tracing::error!("写入 default_provider 失败: {e}");
            }
            // 触发刷新
            spawn_refresh(&ipc, &tx, pid).await;
        });
    }
}

/// 每帧 drain 异步刷新结果 + 处理 daemon 广播的配置变更。
fn drain_pending_refresh(
    mut pending: ResMut<PendingRefresh>,
    mut provider_info: ResMut<ProviderInfo>,
    ipc: Res<IpcClientResource>,
    bridge: Res<AgentBridge>,
    mut config_changed: MessageReader<ConfigChangedMessage>,
    refresh_tx: Res<RefreshSender>,
) {
    // 1. drain 已完成的刷新结果（tokio mpsc::Receiver::try_recv 取 &self）
    while let Ok(r) = pending.rx.try_recv() {
        provider_info.id = r.provider_id;
        provider_info.model = r.model;
        provider_info.kind = r.kind;
        provider_info.ready = r.ready;
    }

    // 2. 处理 daemon 广播的配置变更（多开对端修改等）
    for _ev in config_changed.read() {
        let ipc = ipc.client.clone();
        let tx = refresh_tx.0.clone();
        bridge.runtime.handle().spawn(async move {
            // 读 default_provider
            let read_req = ConfigReadRequest {
                scope: ConfigScope::Global,
                key: "default_provider".to_string(),
            };
            let params = serde_json::to_value(&read_req).unwrap_or(serde_json::Value::Null);
            let dp = match ipc.call_ok(methods::CONFIG_READ, params).await {
                Ok(v) => v.as_str().map(|s| s.to_string()).unwrap_or_default(),
                Err(_) => String::new(),
            };
            if dp.is_empty() {
                let _ = tx
                    .send(RefreshResult {
                        provider_id: String::new(),
                        model: String::new(),
                        kind: None,
                        ready: false,
                    })
                    .await;
            } else {
                spawn_refresh(&ipc, &tx, dp).await;
            }
        });
    }
}

/// 启动时主动刷新一次 [`ProviderInfo`]。
///
/// `main` 注入的 `ProviderInfo` 派生自本地全局配置快照，其 `model` 仅取
/// `default_model`（常为空），且 `ready` 固定为 `false`——并不反映 daemon 侧
/// 权威状态。若不在启动后触发一次刷新，重启时即便磁盘配置完整，UI 也会显示
/// model 为空、provider 未就绪，让用户误以为配置丢失而重新填写。
///
/// 用 `Local<bool>` 去重，整个进程生命周期只触发一次：`id` 非空即 spawn
/// [`spawn_refresh`]，结果经 channel 由 `drain_pending_refresh` 回填。
fn refresh_on_startup(
    mut triggered: Local<bool>,
    provider_info: Res<ProviderInfo>,
    ipc: Res<IpcClientResource>,
    bridge: Res<AgentBridge>,
    refresh_tx: Res<RefreshSender>,
) {
    if *triggered {
        return;
    }
    if provider_info.id.is_empty() {
        return;
    }
    *triggered = true;
    let ipc = ipc.client.clone();
    let tx = refresh_tx.0.clone();
    let dp = provider_info.id.clone();
    bridge.runtime.handle().spawn(async move {
        spawn_refresh(&ipc, &tx, dp).await;
    });
}

/// 异步重读 default provider 配置，判定就绪，发 RefreshResult。
async fn spawn_refresh(ipc: &std::sync::Arc<crate::ipc_client::IpcClient>, tx: &mpsc::Sender<RefreshResult>, dp: String) {
    // 读整个 provider 配置
    let key = format!("providers.{dp}");
    let read_req = ConfigReadRequest {
        scope: ConfigScope::Global,
        key,
    };
    let params = serde_json::to_value(&read_req).unwrap_or(serde_json::Value::Null);
    let result = match ipc.call_ok(methods::CONFIG_READ, params).await {
        Ok(v) => v,
        Err(_) => serde_json::Value::Null,
    };
    // 反序列化为 ProviderConfig
    let pc: ProviderConfig = serde_json::from_value(result.clone()).unwrap_or_default();
    // 取 model_overrides["default"]
    let model = pc
        .model_overrides
        .get("default")
        .cloned()
        .unwrap_or_default();
    // 就绪判据按 kind 分
    let ready = is_provider_ready(&pc);
    let _ = tx
        .send(RefreshResult {
            provider_id: dp,
            model,
            kind: Some(pc.kind),
            ready,
        })
        .await;
}

/// 就绪判据：按 ProviderKind 分（见 CONTEXT.md「Provider 就绪」）。
///
/// - Ollama：api_base 非空即就绪（本地部署通常无 key）
/// - 其余：api_base 与 api_key 均非空
fn is_provider_ready(pc: &ProviderConfig) -> bool {
    let base_ok = !pc.api_base.is_empty();
    match pc.kind {
        ProviderKind::Ollama => base_ok,
        _ => base_ok && !pc.api_key.is_empty(),
    }
}
