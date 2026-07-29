//! daemon 通知 → Bevy 消息桥接：把 IPC 通知转成 ECS 消息喂入 UI 侧。
//!
//! - `fs.changed` / `peer.fileChanged` → [`FileChangedMessage`] → [`FileChangedEvent`]
//! - `config.changed` → [`ConfigChangedMessage`]
//! - `BufferSavedEvent` → IPC `fs.notify`（通知 daemon 广播给其他客户端）
//!
//! provider.* 通知不在此处理（由 [`IpcProviderClient`] 直接消费）。

use bevy::prelude::*;
use tokio::sync::broadcast;
use xgent_core::fs::FileChanged;
use xgent_core::notifications;
use xgent_core::proto::Notification;

use crate::ipc_client::IpcClient;

/// 文件变更消息（本机或对端客户端触发）。
#[allow(dead_code)]
#[derive(Message, Debug, Clone)]
pub struct FileChangedMessage(pub FileChanged);

/// 配置变更消息。
#[allow(dead_code)]
#[derive(Message, Debug, Clone)]
pub struct ConfigChangedMessage(pub serde_json::Value);

/// IPC 客户端 Resource（供桥接系统与 ProviderClient 共享）。
#[derive(Resource, Clone)]
pub struct IpcClientResource {
    pub client: std::sync::Arc<IpcClient>,
}

/// 通知桥接插件：注册消息类型与轮询系统。
pub struct FsEventBridgePlugin;
impl Plugin for FsEventBridgePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<FileChangedMessage>()
            .add_message::<ConfigChangedMessage>()
            .add_message::<xgent_ui::editor::conflict::FileChangedEvent>()
            .add_message::<xgent_ui::editor::io::BufferSavedEvent>()
            .add_systems(
                Update,
                (pump_notifications, bridge_file_changed, forward_save_event).chain(),
            );
    }
}

/// 持有通知订阅端的 Resource（由启动序列注入）。
#[derive(Resource)]
pub struct NotifPump {
    pub rx: broadcast::Receiver<Notification>,
}

/// 每帧非阻塞从通知订阅端拉取，分发到对应 Bevy 消息。
fn pump_notifications(
    pump: Option<ResMut<NotifPump>>,
    mut file_writer: MessageWriter<FileChangedMessage>,
    mut config_writer: MessageWriter<ConfigChangedMessage>,
) {
    let Some(mut pump) = pump else {
        return;
    };
    // 每帧最多处理 64 条，避免单帧过长
    for _ in 0..64 {
        match pump.rx.try_recv() {
            Ok(notif) => {
                route_notification(&notif, &mut file_writer, &mut config_writer);
            }
            Err(broadcast::error::TryRecvError::Empty) => break,
            Err(broadcast::error::TryRecvError::Closed) => break,
            Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
        }
    }
}

fn route_notification(
    notif: &Notification,
    file_writer: &mut MessageWriter<FileChangedMessage>,
    config_writer: &mut MessageWriter<ConfigChangedMessage>,
) {
    match notif.method.as_str() {
        notifications::FS_CHANGED | notifications::PEER_FILE_CHANGED => {
            if let Ok(fc) = serde_json::from_value::<FileChanged>(notif.params.clone()) {
                file_writer.write(FileChangedMessage(fc));
            }
        }
        notifications::CONFIG_CHANGED => {
            config_writer.write(ConfigChangedMessage(notif.params.clone()));
        }
        _ => {} // provider.* 由 ProviderClient 直接消费
    }
}

/// 把 [`FileChangedMessage`]（相对路径）转成 [`FileChangedEvent`]（绝对路径），
/// 供编辑器 `handle_file_changed` 消费。
///
/// `FileChanged.path` 相对 `project_root`，拼绝对路径后发出。
fn bridge_file_changed(
    mut reader: MessageReader<FileChangedMessage>,
    project_root: Option<Res<xgent_ui::file_panel::ProjectRoot>>,
    mut writer: MessageWriter<xgent_ui::editor::conflict::FileChangedEvent>,
) {
    let Some(root) = project_root else {
        return;
    };
    for msg in reader.read() {
        let abs = if msg.0.path.is_absolute() {
            msg.0.path.clone()
        } else {
            root.path.join(&msg.0.path)
        };
        writer.write(xgent_ui::editor::conflict::FileChangedEvent { path: abs });
    }
}

/// 用户保存文件后通知 daemon，daemon 广播 `peer.fileChanged` 给同项目其他客户端。
///
/// 走 IPC `fs.notify` 方法（daemon 排除来源客户端广播）。
fn forward_save_event(
    mut reader: MessageReader<xgent_ui::editor::io::BufferSavedEvent>,
    ipc: Res<IpcClientResource>,
    bridge: Res<xgent_agent::AgentBridge>,
) {
    for ev in reader.read() {
        let path = ev.path.clone();
        let client = ipc.client.clone();
        bridge.runtime.spawn(async move {
            let params = serde_json::json!({ "path": path });
            if let Err(e) = client.call_ok(xgent_core::methods::FS_NOTIFY, params).await {
                tracing::warn!("fs.notify 失败: {e}");
            }
        });
    }
}
