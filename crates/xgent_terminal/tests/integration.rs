//! LocalPtyBackend 集成测试：验证 spawn → write → 输出 → kill 完整链路。
//!
//! 跨平台：Windows 用 powershell，Unix 用 $SHELL。测试用 shell 的 echo 命令
//! 产生可预测输出，验证 PTY 读循环 + channel 桥接 + 解析是否通畅。

use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::timeout;

use xgent_terminal::{
    LocalPtyBackend, ShellSpec, SpawnRequest, TerminalBackend, TerminalEvent,
};

/// 默认 shell（跨平台）。
fn default_shell() -> ShellSpec {
    if cfg!(windows) {
        ShellSpec::Powershell
    } else {
        ShellSpec::FromEnv
    }
}

/// 收集 PTY 输出直到收集到 `needle` 子串、PTY 退出、或超时。
///
/// 返回至今收集的全部字节。`needle` 为 `None` 时收集到首次非空输出即返回
/// （用于等待 shell 启动）。
async fn collect_output(
    rx: &mut tokio::sync::mpsc::Receiver<TerminalEvent>,
    needle: Option<&str>,
    max_wait: Duration,
) -> Vec<u8> {
    let mut buf = Vec::new();
    let deadline = tokio::time::Instant::now() + max_wait;
    while tokio::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(200), rx.recv()).await {
            Ok(Some(TerminalEvent::Output(bytes))) => {
                if !bytes.is_empty() {
                    buf.extend_from_slice(&bytes);
                    match needle {
                        Some(n) if String::from_utf8_lossy(&buf).contains(n) => return buf,
                        None if !buf.is_empty() => return buf,
                        _ => {}
                    }
                }
            }
            Ok(Some(TerminalEvent::Exited(_))) => break,
            Ok(None) => break,
            Err(_) => continue, // 200ms 超时，继续轮询直到 deadline
        }
    }
    buf
}

/// 构造标准 spawn 请求。
fn spawn_request() -> SpawnRequest {
    SpawnRequest {
        shell: default_shell(),
        cwd: std::env::temp_dir(),
        cols: 80,
        rows: 24,
    }
}

#[tokio::test]
async fn spawn_write_echo_kill() {
    let backend = LocalPtyBackend::new();
    let (tx, mut rx) = mpsc::channel::<TerminalEvent>(256);

    let id = backend.spawn(spawn_request(), tx).await.expect("spawn");

    // 等 shell 启动（首次非空输出 = prompt 或 ready）
    let _ = collect_output(&mut rx, None, Duration::from_secs(8)).await;

    // 发 echo 命令（PowerShell 和 sh 都支持 echo）
    let cmd = "echo xgent_test_marker_42\r\n";
    backend
        .write(id, cmd.as_bytes().to_vec())
        .await
        .expect("write");

    // 收输出，找 marker
    let buf = collect_output(&mut rx, Some("xgent_test_marker_42"), Duration::from_secs(8)).await;
    let text = String::from_utf8_lossy(&buf);
    assert!(
        text.contains("xgent_test_marker_42"),
        "应在输出中找到 marker，实际: {text}"
    );

    // kill
    backend.kill(id).await.expect("kill");

    // 等 Exited 事件（可能收到也可能 channel 关闭）
    let _ = timeout(Duration::from_secs(3), rx.recv()).await;
}

#[tokio::test]
async fn kill_releases_session() {
    let backend = LocalPtyBackend::new();
    let (tx, mut rx) = mpsc::channel::<TerminalEvent>(256);

    let id = backend.spawn(spawn_request(), tx).await.expect("spawn");

    // 等 shell 稍稍启动，避免 kill 一个还没起来的进程
    let _ = collect_output(&mut rx, None, Duration::from_secs(3)).await;

    backend.kill(id).await.expect("kill");

    // kill 后再 kill 同一 id 应报 UnknownId
    let err = backend.kill(id).await;
    assert!(err.is_err(), "kill 已销毁的 id 应报错");

    let _ = timeout(Duration::from_secs(3), rx.recv()).await;
}

#[tokio::test]
async fn resize_does_not_error() {
    let backend = LocalPtyBackend::new();
    let (tx, mut rx) = mpsc::channel::<TerminalEvent>(256);

    let id = backend.spawn(spawn_request(), tx).await.expect("spawn");

    // 等 shell 启动
    let _ = collect_output(&mut rx, None, Duration::from_secs(5)).await;

    // resize 应成功
    backend.resize(id, 120, 40).await.expect("resize");

    backend.kill(id).await.expect("kill");
    let _ = timeout(Duration::from_secs(3), rx.recv()).await;
}
