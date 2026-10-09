//! 单个 UI 客户端连接的会话。
//!
//! 注册客户端、循环读取 JSON-RPC 消息（按行）、分发到对应 handler。
//! 所有输出（Response 与 Notification）经统一 writer task 写回 socket，
//! 避免读写半边竞争。连接断开时注销并触发退出计时。

use std::pin::Pin;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use xgent_core::config::{ConfigReadRequest, ConfigWriteRequest};
use xgent_core::fs::{FileChangeKind, FileChanged, WatchRequest};
use xgent_core::methods;
use xgent_core::notifications;
use xgent_core::proto::{Notification, Request, Response, RpcError};

use crate::server::Shared;

/// 已连接的 IPC 流的读写两半（trait object，跨平台抽象）。
///
/// - Unix：`tokio::net::UnixStream::into_split()` 得到 `OwnedReadHalf` / `OwnedWriteHalf`
/// - Windows：`tokio::net::windows::NamedPipeServer` 经 `tokio::io::split()` 得到两半
///
/// 统一装箱为 trait object，`Session` 不关心底层具体类型。
pub struct ConnStream {
    pub read: Pin<Box<dyn AsyncRead + Send>>,
    pub write: Pin<Box<dyn AsyncWrite + Send>>,
}

/// 写回客户端的一行消息（Response 或 Notification）。
enum Outgoing {
    Response(Response),
    Notification(Notification),
}

impl Outgoing {
    /// 序列化为 JSON 行文本（含尾部换行）。
    fn to_json_line(&self) -> String {
        let s = match self {
            Outgoing::Response(r) => serde_json::to_string(r).unwrap_or_default(),
            Outgoing::Notification(n) => serde_json::to_string(n).unwrap_or_default(),
        };
        format!("{s}\n")
    }
}

/// 单个客户端会话。
pub struct Session {
    stream: ConnStream,
    shared: Shared,
}

impl Session {
    pub fn new(stream: ConnStream, shared: Shared) -> Self {
        Self { stream, shared }
    }

    /// 处理整个连接生命周期。
    pub async fn handle(self) {
        let mut reader = BufReader::new(self.stream.read);
        let mut writer = self.stream.write;

        // 统一输出 channel：所有 Response/Notification 经此发送给 writer task
        let (out_tx, mut out_rx) = mpsc::channel::<Outgoing>(128);

        // 注册客户端，把其通知推送端绑到 out_tx（经 Notification 转发）
        let client_id = {
            let mut reg = self.shared.registry.write().await;
            // 把 out_tx 克隆一份作为该客户端的通知 sender
            reg.register(out_tx.clone().into_notification_sender())
        };
        self.shared.lifecycle.on_connect().await;

        // writer task：消费 out_rx，逐行写回 socket
        let writer_task = tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                let line = msg.to_json_line();
                if writer.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
        });

        // 按行读取请求/通知
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(req) = serde_json::from_str::<Request>(trimmed) {
                        let resp = dispatch_request(req, &self.shared, client_id).await;
                        let _ = out_tx.send(Outgoing::Response(resp)).await;
                    } else if let Ok(notif) = serde_json::from_str::<Notification>(trimmed) {
                        // 通知（无 id）MVP 暂不处理
                        let _ = notif;
                    } else {
                        // 非法行（版本偏差/坏客户端）：不回错误响应的话，调用方
                        // 的请求会无响应挂死且无任何痕迹
                        tracing::warn!("无法解析 IPC 行: {}", &trimmed[..trimmed.len().min(200)]);
                        let id = serde_json::from_str::<serde_json::Value>(trimmed)
                            .ok()
                            .and_then(|v| v.get("id").and_then(|i| i.as_u64()));
                        let err = Response::err(
                            id.unwrap_or(0),
                            RpcError::new(
                                if id.is_some() {
                                    xgent_core::proto::INVALID_REQUEST
                                } else {
                                    xgent_core::proto::PARSE_ERROR
                                },
                                "无法解析的 JSON-RPC 行".to_string(),
                                None,
                            ),
                        );
                        let _ = out_tx.send(Outgoing::Response(err)).await;
                    }
                }
                Err(e) => {
                    tracing::warn!("读取失败: {e}");
                    break;
                }
            }
        }

        // 注销
        {
            let mut reg = self.shared.registry.write().await;
            reg.unregister(client_id);
        }
        self.shared.watcher.unwatch_client(client_id).await;
        self.shared.lifecycle.on_disconnect().await;
        // 关闭输出 channel，让 writer task 自然结束
        drop(out_tx);
        let _ = writer_task.await;
    }
}

/// 适配：把 `mpsc::Sender<Outgoing>` 转成 `mpsc::Sender<Notification>`。
///
/// provider_pool/chat 通过 registry 的 `sender_for` 拿到此 sender 推送
/// 通知；这里实际是同一个 out_tx 的克隆，通知会被 writer task 写回。
/// 由于 `Outgoing` 与 `Notification` 不同类型，用一个轻量适配器转发。
trait IntoNotificationSender {
    fn into_notification_sender(self) -> mpsc::Sender<Notification>;
}

impl IntoNotificationSender for mpsc::Sender<Outgoing> {
    fn into_notification_sender(self) -> mpsc::Sender<Notification> {
        // 创建新 channel，spawn task 把 Notification 转 Outgoing 转发。
        let (n_tx, mut n_rx) = mpsc::channel::<Notification>(128);
        let out_tx = self;
        tokio::spawn(async move {
            while let Some(n) = n_rx.recv().await {
                if out_tx.send(Outgoing::Notification(n)).await.is_err() {
                    break;
                }
            }
        });
        n_tx
    }
}

/// 分发请求到对应 handler。
async fn dispatch_request(
    req: Request,
    shared: &Shared,
    client_id: xgent_core::ids::ClientId,
) -> Response {
    match req.method.as_str() {
        methods::CONFIG_READ => config_read(req, shared).await,
        methods::CONFIG_WRITE => config_write(req, shared, client_id).await,
        methods::FS_WATCH => fs_watch(req, shared, client_id).await,
        methods::FS_NOTIFY => fs_notify(req, shared, client_id).await,
        methods::PROVIDER_LIST_MODELS => provider_list_models(req, shared).await,
        methods::PROVIDER_CHAT => provider_chat(req, shared, client_id).await,
        methods::PROVIDER_CANCEL => provider_cancel(req, shared).await,
        _ => Response::err(
            req.id,
            RpcError::new(
                xgent_core::proto::METHOD_NOT_FOUND,
                format!("未知方法: {}", req.method),
                None,
            ),
        ),
    }
}

/// config.read
async fn config_read(req: Request, shared: &Shared) -> Response {
    let params: ConfigReadRequest = match serde_json::from_value(req.params.clone()) {
        Ok(p) => p,
        Err(e) => {
            return Response::err(
                req.id,
                RpcError::new(xgent_core::proto::INVALID_PARAMS, e.to_string(), None),
            );
        }
    };
    // Project 作用域未实现：daemon 的 ConfigCoordinator 只有全局权威副本。
    // 显式拒绝而非静默按 Global 处理——静默降级会让调用方误以为写到了项目
    // 配置（字段语义两侧不一致）。
    if params.scope == xgent_core::config::ConfigScope::Project {
        return Response::err(
            req.id,
            RpcError::new(
                xgent_core::proto::INVALID_PARAMS,
                "config scope=project 未实现，当前仅支持 global".to_string(),
                None,
            ),
        );
    }
    let cfg = shared.config.read().await;
    let value = cfg.read(&params.key);
    Response::ok(req.id, value)
}

/// config.write：写入并广播 config.changed
async fn config_write(
    req: Request,
    shared: &Shared,
    client_id: xgent_core::ids::ClientId,
) -> Response {
    let params: ConfigWriteRequest = match serde_json::from_value(req.params.clone()) {
        Ok(p) => p,
        Err(e) => {
            return Response::err(
                req.id,
                RpcError::new(xgent_core::proto::INVALID_PARAMS, e.to_string(), None),
            );
        }
    };
    // Project 作用域未实现：同 config_read，显式拒绝
    if params.scope == xgent_core::config::ConfigScope::Project {
        return Response::err(
            req.id,
            RpcError::new(
                xgent_core::proto::INVALID_PARAMS,
                "config scope=project 未实现，当前仅支持 global".to_string(),
                None,
            ),
        );
    }
    let changed = {
        let mut cfg = shared.config.write().await;
        match cfg.write(&params.key, params.value) {
            Ok(c) => c,
            Err(e) => {
                return Response::err(
                    req.id,
                    RpcError::new(xgent_core::proto::INTERNAL_ERROR, e.to_string(), None),
                );
            }
        }
    };
    // 触碰 providers.* 时使 provider 池缓存失效：否则改 api_key/api_base 后
    // 旧实例（旧凭据）继续被 chat/list_models 使用，直到 daemon 重启。
    if params.key == "providers" || params.key.starts_with("providers.") {
        shared.pool.invalidate().await;
    }
    // 广播给所有客户端（排除来源）
    let notif = Notification::new(
        notifications::CONFIG_CHANGED,
        serde_json::to_value(&changed).unwrap_or_default(),
    );
    let reg = shared.registry.read().await;
    reg.broadcast_all(notif, Some(client_id));
    Response::ok(req.id, serde_json::json!({"ok": true}))
}

/// fs.watch：订阅项目
async fn fs_watch(req: Request, shared: &Shared, client_id: xgent_core::ids::ClientId) -> Response {
    let params: WatchRequest = match serde_json::from_value(req.params.clone()) {
        Ok(p) => p,
        Err(e) => {
            return Response::err(
                req.id,
                RpcError::new(xgent_core::proto::INVALID_PARAMS, e.to_string(), None),
            );
        }
    };
    {
        let mut reg = shared.registry.write().await;
        reg.subscribe(client_id, params.project_root.clone());
    }
    if let Err(e) = shared
        .watcher
        .watch(params.project_root.clone(), client_id)
        .await
    {
        // watcher 注册失败：仅回滚本次项目的 registry 订阅（不能 clear 全部——
        // 客户端此前成功订阅的其他项目会被误伤，收不到任何 fs.changed）
        let mut reg = shared.registry.write().await;
        reg.unsubscribe_project(client_id, &params.project_root);
        return Response::err(
            req.id,
            RpcError::new(xgent_core::proto::INTERNAL_ERROR, e.to_string(), None),
        );
    }
    Response::ok(req.id, serde_json::json!({"ok": true}))
}

/// fs.notify：UI 通知文件已保存，广播 `peer.fileChanged` 给同项目其他客户端。
///
/// 参数：`{"path": "/abs/path"}`。需要反查客户端所属项目来广播。
async fn fs_notify(
    req: Request,
    shared: &Shared,
    client_id: xgent_core::ids::ClientId,
) -> Response {
    #[derive(serde::Deserialize)]
    struct FsNotifyParams {
        path: std::path::PathBuf,
    }
    let params: FsNotifyParams = match serde_json::from_value(req.params.clone()) {
        Ok(p) => p,
        Err(e) => {
            return Response::err(
                req.id,
                RpcError::new(xgent_core::proto::INVALID_PARAMS, e.to_string(), None),
            );
        }
    };
    // 查找来源客户端订阅的项目，广播给同项目其他客户端（排除来源）。
    // 通知参数必须是完整 FileChanged：UI 侧统一按 FileChanged 反序列化
    // （fs.changed 与 peer.fileChanged 同一路由），缺 project_root/kind 会被
    // 静默丢弃，多客户端文件同步整体失效。
    {
        let reg = shared.registry.read().await;
        let subscribed = reg.subscribed(client_id);
        for project in &subscribed {
            let fc = FileChanged {
                project_root: project.clone(),
                path: params.path.clone(),
                kind: FileChangeKind::Modified,
            };
            let notif = Notification::new(
                notifications::PEER_FILE_CHANGED,
                serde_json::to_value(&fc).unwrap_or_default(),
            );
            reg.broadcast_to_project(project, notif, Some(client_id));
        }
    }
    Response::ok(req.id, serde_json::json!({"ok": true}))
}

/// provider.listModels
///
/// 支持可选的 `kind`/`api_base`/`api_key` override：UI 设置面板用**编辑中的
/// 草稿凭据**探测模型列表，daemon 据此临时构造 provider 实例直连调用，
/// 不写配置、不进池（写配置会把草稿永久持久化并广播 config.changed）。
async fn provider_list_models(req: Request, shared: &Shared) -> Response {
    #[derive(serde::Deserialize)]
    struct Params {
        provider: String,
        #[serde(default)]
        kind: Option<xgent_settings_core::ProviderKind>,
        #[serde(default)]
        api_base: Option<String>,
        #[serde(default)]
        api_key: Option<String>,
    }
    let params: Params = match serde_json::from_value(req.params.clone()) {
        Ok(p) => p,
        Err(e) => {
            return Response::err(
                req.id,
                RpcError::new(xgent_core::proto::INVALID_PARAMS, e.to_string(), None),
            );
        }
    };
    // 有任一 override → 临时实例直连（不缓存）
    if params.kind.is_some() || params.api_base.is_some() || params.api_key.is_some() {
        let cfg = shared.config.read().await;
        let existing = cfg.config().providers.get(&params.provider).cloned();
        drop(cfg);
        let base = existing.unwrap_or_default();
        let cfg = xgent_settings_core::ProviderConfig {
            kind: params.kind.unwrap_or(base.kind),
            api_base: params.api_base.unwrap_or(base.api_base),
            api_key: params.api_key.unwrap_or(base.api_key),
            ..base
        };
        let provider = xgent_provider::build_provider(&params.provider, &cfg);
        return match provider.list_models().await {
            Ok(models) => Response::ok(req.id, serde_json::to_value(models).unwrap_or_default()),
            Err(e) => Response::err(
                req.id,
                RpcError::new(xgent_core::proto::INTERNAL_ERROR, e.to_string(), None),
            ),
        };
    }
    match shared.pool.get(&params.provider).await {
        Ok(p) => match p.list_models().await {
            Ok(models) => Response::ok(req.id, serde_json::to_value(models).unwrap_or_default()),
            Err(e) => Response::err(
                req.id,
                RpcError::new(xgent_core::proto::INTERNAL_ERROR, e.to_string(), None),
            ),
        },
        Err(e) => Response::err(
            req.id,
            RpcError::new(xgent_core::proto::INVALID_PARAMS, e, None),
        ),
    }
}

/// provider.chat：发起流式对话
async fn provider_chat(
    req: Request,
    shared: &Shared,
    client_id: xgent_core::ids::ClientId,
) -> Response {
    let chat_req: xgent_core::chat::ChatRequest = match serde_json::from_value(req.params.clone()) {
        Ok(r) => r,
        Err(e) => {
            return Response::err(
                req.id,
                RpcError::new(xgent_core::proto::INVALID_PARAMS, e.to_string(), None),
            );
        }
    };
    let sender = {
        let reg = shared.registry.read().await;
        reg.sender_for(client_id)
    };
    let Some(sender) = sender else {
        return Response::err(
            req.id,
            RpcError::new(
                xgent_core::proto::INTERNAL_ERROR,
                "客户端未注册".to_string(),
                None,
            ),
        );
    };
    match shared.pool.chat(chat_req, client_id, sender).await {
        Ok(stream_id) => Response::ok(req.id, serde_json::json!({"stream_id": stream_id.0})),
        Err(e) => {
            // 把 ErrorKind 与 Retry-After 一起编码进 data，供 UI 侧恢复：
            // 只传 message 会让 UI 误判为 Network 触发无意义重试；
            // 只传 kind 会丢掉 Retry-After，使限流退避失效（R1-7 / A3）。
            Response::err(
                req.id,
                RpcError::new(
                    xgent_core::proto::INTERNAL_ERROR,
                    e.message,
                    Some(serde_json::json!({
                        "kind": e.kind,
                        "retry_after_secs": e.retry_after_secs,
                    })),
                ),
            )
        }
    }
}

/// provider.cancel：取消指定流。
///
/// 取消是幂等的：流不存在（已结束或已被取消）也返回成功——用户重复按
/// 中断、或流恰好自然结束时，都不该看到错误。
async fn provider_cancel(req: Request, shared: &Shared) -> Response {
    let sid = match req.params.get("stream_id").and_then(|v| v.as_u64()) {
        Some(v) => xgent_core::ids::StreamId(v),
        None => {
            return Response::err(
                req.id,
                RpcError::new(
                    xgent_core::proto::INVALID_PARAMS,
                    "缺少参数 stream_id".to_string(),
                    None,
                ),
            );
        }
    };
    let was_active = shared.pool.cancel(sid).await;
    Response::ok(req.id, serde_json::json!({"cancelled": was_active}))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// provider.cancel：取消活跃流后该流不再登记。
    #[tokio::test]
    async fn provider_cancel_removes_active_stream() {
        let cfg = crate::config_store::ConfigCoordinator::with_config(Default::default());
        let pool = std::sync::Arc::new(crate::provider_pool::ProviderPool::new(
            std::sync::Arc::new(tokio::sync::RwLock::new(cfg)),
        ));
        // 直接往登记表塞一个活跃流，验证 cancel 路径
        let sid = xgent_core::ids::StreamId(9999);
        let token = tokio_util::sync::CancellationToken::new();
        pool.register_stream_for_test(sid, token.clone()).await;
        assert_eq!(pool.active_stream_count().await, 1);
        assert!(pool.cancel(sid).await, "取消活跃流应返回 true");
        assert!(token.is_cancelled(), "取消令牌应被 cancel");
        assert_eq!(pool.active_stream_count().await, 0);
    }

    /// 取消不存在的流是幂等的（返回 false，不报错）。
    #[tokio::test]
    async fn provider_cancel_unknown_stream_is_noop() {
        let cfg = crate::config_store::ConfigCoordinator::with_config(Default::default());
        let pool = crate::provider_pool::ProviderPool::new(std::sync::Arc::new(
            tokio::sync::RwLock::new(cfg),
        ));
        assert!(
            !pool.cancel(xgent_core::ids::StreamId(4242)).await,
            "取消未知流应返回 false"
        );
    }

    /// A3/R1-7：建流失败的错误必须把 ErrorKind 与 Retry-After **一起**
    /// 编进 IPC error.data——只传其一都会让限流退避在跨进程后失效。
    #[test]
    fn provider_chat_error_carries_kind_and_retry_after() {
        use crate::provider_pool::ProviderCallError;
        use xgent_core::chat::ErrorKind;

        let e = ProviderCallError {
            kind: ErrorKind::RateLimited,
            message: "429 rate limited".into(),
            retry_after_secs: Some(7),
        };
        let r = Response::err(
            1,
            RpcError::new(
                xgent_core::proto::INTERNAL_ERROR,
                e.message,
                Some(serde_json::json!({
                    "kind": e.kind,
                    "retry_after_secs": e.retry_after_secs,
                })),
            ),
        );
        let data = r.error.expect("应有 error");
        assert_eq!(data.data.as_ref().unwrap()["retry_after_secs"], 7);
        // kind 序列化后应能被 UI 侧反序列化回同一枚举
        let raw = data.data.as_ref().unwrap()["kind"].clone();
        let back: ErrorKind = serde_json::from_value(raw).expect("kind 应可反序列化");
        assert_eq!(back, ErrorKind::RateLimited);
    }

    #[test]
    fn provider_chat_error_without_retry_after_is_null() {
        use crate::provider_pool::ProviderCallError;
        use xgent_core::chat::ErrorKind;
        let e = ProviderCallError {
            kind: ErrorKind::AuthFailed,
            message: "401".into(),
            retry_after_secs: None,
        };
        let r = Response::err(
            1,
            RpcError::new(
                xgent_core::proto::INTERNAL_ERROR,
                e.message,
                Some(serde_json::json!({
                    "kind": e.kind,
                    "retry_after_secs": e.retry_after_secs,
                })),
            ),
        );
        assert!(
            r.error.unwrap().data.as_ref().unwrap()["retry_after_secs"].is_null(),
            "无 Retry-After 时应编码为 null 而非缺字段（UI 侧按 as_u64() 取值）"
        );
    }

    #[test]
    fn outgoing_response_to_json_line() {
        let r = Outgoing::Response(Response::ok(1, serde_json::json!({"ok": true})));
        let line = r.to_json_line();
        assert!(line.ends_with('\n'));
        assert!(line.contains(r#""id":1"#));
        assert!(line.contains(r#""result""#));
    }

    #[test]
    fn outgoing_notification_to_json_line() {
        let n = Outgoing::Notification(Notification::new(
            notifications::FS_CHANGED,
            serde_json::json!({}),
        ));
        let line = n.to_json_line();
        assert!(line.ends_with('\n'));
        // 通知无 id 字段
        assert!(!line.contains(r#""id""#));
        assert!(line.contains(r#""fs.changed""#));
    }
}
