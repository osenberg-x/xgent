//! IPC 客户端连接与读写集成测试。
//!
//! 用 tokio UnixListener 起 mock daemon，验证 JSON-RPC 请求-响应往返
//! 与通知推送。不依赖真实 xgent_daemon 进程。

#![cfg(unix)]

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use xgent_core::notifications;
use xgent_core::proto::{Notification, Request, Response};

/// mock daemon：接受一个连接，读一行请求，回一行 Response。
async fn mock_daemon_respond(listener: UnixListener) {
    let (stream, _) = listener.accept().await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();

    // 解析请求 id，回 echo 响应
    let req: Request = serde_json::from_str(line.trim()).unwrap();
    let resp = Response::ok(req.id, serde_json::json!({"echo": req.method}));
    let resp_line = serde_json::to_string(&resp).unwrap();
    reader.get_mut().write_all(resp_line.as_bytes()).await.unwrap();
    reader.get_mut().write_all(b"\n").await.unwrap();
}

/// mock daemon：接受连接，先回 Response，再推一条 Notification。
async fn mock_daemon_notif(listener: UnixListener) {
    let (stream, _) = listener.accept().await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();

    let req: Request = serde_json::from_str(line.trim()).unwrap();
    let resp = Response::ok(req.id, serde_json::json!({"stream_id": 42}));
    let resp_line = serde_json::to_string(&resp).unwrap();
    reader.get_mut().write_all(resp_line.as_bytes()).await.unwrap();
    reader.get_mut().write_all(b"\n").await.unwrap();

    // 推送一条 fs.changed 通知
    let notif = Notification::new(
        notifications::FS_CHANGED,
        serde_json::json!({
            "project_root": "/tmp/proj",
            "path": "src/main.rs",
            "kind": "modified"
        }),
    );
    let notif_line = serde_json::to_string(&notif).unwrap();
    reader.get_mut().write_all(notif_line.as_bytes()).await.unwrap();
    reader.get_mut().write_all(b"\n").await.unwrap();
}

/// 验证 IpcClient 的 call 请求-响应往返。
///
/// 但 IpcClient 是 xgent_app 内部模块，集成测试无法直接引用。
/// 此测试验证 mock daemon 的协议行为与 xgent_core 序列化兼容性，
/// 确保两端协议对称。
#[tokio::test]
async fn mock_daemon_echo_response() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("test.sock");
    let listener = UnixListener::bind(&sock).unwrap();

    // 起 mock daemon
    let server_task = tokio::spawn(async move {
        mock_daemon_respond(listener).await;
    });

    // 模拟客户端：连接、发请求、收响应
    let stream = tokio::net::UnixStream::connect(&sock).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let req = Request::new(1, "ping", serde_json::json!({}));
    let line = serde_json::to_string(&req).unwrap();
    write_half.write_all(line.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();

    let mut reader = BufReader::new(read_half);
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line).await.unwrap();
    let resp: Response = serde_json::from_str(resp_line.trim()).unwrap();

    assert_eq!(resp.id, 1);
    assert_eq!(resp.result.unwrap()["echo"], "ping");

    server_task.await.unwrap();
}

/// 验证 mock daemon 先回 Response 再推 Notification，客户端能分别解析。
#[tokio::test]
async fn mock_daemon_response_then_notification() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("test2.sock");
    let listener = UnixListener::bind(&sock).unwrap();

    let server_task = tokio::spawn(async move {
        mock_daemon_notif(listener).await;
    });

    let stream = tokio::net::UnixStream::connect(&sock).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();

    // 发 provider.chat 请求
    let req = Request::new(
        1,
        "provider.chat",
        serde_json::json!({"model": "gpt-4o-mini"}),
    );
    let line = serde_json::to_string(&req).unwrap();
    write_half.write_all(line.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();

    let mut reader = BufReader::new(read_half);
    let mut line1 = String::new();
    reader.read_line(&mut line1).await.unwrap();
    let resp: Response = serde_json::from_str(line1.trim()).unwrap();
    assert_eq!(resp.id, 1);
    assert_eq!(resp.result.unwrap()["stream_id"], 42);

    // 读第二条行：通知（无 id）
    let mut line2 = String::new();
    reader.read_line(&mut line2).await.unwrap();
    let notif: Notification = serde_json::from_str(line2.trim()).unwrap();
    assert_eq!(notif.method, notifications::FS_CHANGED);
    assert_eq!(notif.params["path"], "src/main.rs");

    server_task.await.unwrap();
}

/// 验证 config.read 请求-响应往返。
#[tokio::test]
async fn config_read_request_response() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("config.sock");
    let listener = UnixListener::bind(&sock).unwrap();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();

        let req: Request = serde_json::from_str(line.trim()).unwrap();
        let resp = Response::ok(req.id, serde_json::json!({"value": "openai"}));
        let resp_line = serde_json::to_string(&resp).unwrap();
        reader.get_mut().write_all(resp_line.as_bytes()).await.unwrap();
        reader.get_mut().write_all(b"\n").await.unwrap();
    });

    // 短暂等待 server 准备
    tokio::time::sleep(Duration::from_millis(10)).await;

    let stream = tokio::net::UnixStream::connect(&sock).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let req = Request::new(
        5,
        "config.read",
        serde_json::json!({"key": "default_provider"}),
    );
    let line = serde_json::to_string(&req).unwrap();
    write_half.write_all(line.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();

    let mut reader = BufReader::new(read_half);
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line).await.unwrap();
    let resp: Response = serde_json::from_str(resp_line.trim()).unwrap();
    assert_eq!(resp.id, 5);
    assert_eq!(resp.result.unwrap()["value"], "openai");
}

/// 验证 fs.watch 请求序列化与响应解析。
#[tokio::test]
async fn fs_watch_request_response() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("fs.sock");
    let listener = UnixListener::bind(&sock).unwrap();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();

        let req: Request = serde_json::from_str(line.trim()).unwrap();
        // 验证请求参数包含 project_root
        let project_root = req.params["project_root"].as_str().unwrap();
        assert!(project_root.starts_with("/tmp"));

        let resp = Response::ok(req.id, serde_json::json!({"ok": true}));
        let resp_line = serde_json::to_string(&resp).unwrap();
        reader.get_mut().write_all(resp_line.as_bytes()).await.unwrap();
        reader.get_mut().write_all(b"\n").await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(10)).await;

    let stream = tokio::net::UnixStream::connect(&sock).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();

    let watch_req = xgent_core::fs::WatchRequest {
        project_root: std::path::PathBuf::from("/tmp/test-proj"),
    };
    let params = serde_json::to_value(&watch_req).unwrap();
    let req = Request::new(3, "fs.watch", params);
    let line = serde_json::to_string(&req).unwrap();
    write_half.write_all(line.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();

    let mut reader = BufReader::new(read_half);
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line).await.unwrap();
    let resp: Response = serde_json::from_str(resp_line.trim()).unwrap();
    assert_eq!(resp.id, 3);
    assert_eq!(resp.result.unwrap()["ok"], true);
}

/// 验证错误响应解析。
#[tokio::test]
async fn error_response_from_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("err.sock");
    let listener = UnixListener::bind(&sock).unwrap();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();

        let req: Request = serde_json::from_str(line.trim()).unwrap();
        let resp = Response::err(
            req.id,
            xgent_core::proto::RpcError::new(
                xgent_core::proto::METHOD_NOT_FOUND,
                "未知方法",
                None,
            ),
        );
        let resp_line = serde_json::to_string(&resp).unwrap();
        reader.get_mut().write_all(resp_line.as_bytes()).await.unwrap();
        reader.get_mut().write_all(b"\n").await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(10)).await;

    let stream = tokio::net::UnixStream::connect(&sock).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();

    let req = Request::new(9, "unknown.method", serde_json::json!({}));
    let line = serde_json::to_string(&req).unwrap();
    write_half.write_all(line.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();

    let mut reader = BufReader::new(read_half);
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line).await.unwrap();
    let resp: Response = serde_json::from_str(resp_line.trim()).unwrap();
    assert_eq!(resp.id, 9);
    assert!(resp.result.is_none());
    let err = resp.error.unwrap();
    assert_eq!(err.code, xgent_core::proto::METHOD_NOT_FOUND);
    assert_eq!(err.message, "未知方法");
}
