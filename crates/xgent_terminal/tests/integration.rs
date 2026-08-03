//! LocalPtyBackend 集成测试：验证 spawn → write → 输出 → kill 完整链路。
//!
//! 跨平台：Windows 用 powershell，Unix 用 $SHELL。测试用 shell 的 echo 命令
//! 产生可预测输出，验证 PTY 读循环 + channel 桥接 + 解析是否通畅。

use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::timeout;

use xgent_terminal::{LocalPtyBackend, ShellSpec, SpawnRequest, TerminalBackend, TerminalEvent};

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
    let buf = collect_output(
        &mut rx,
        Some("xgent_test_marker_42"),
        Duration::from_secs(8),
    )
    .await;
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

/// 验证写入 `ls\n`（仅 LF，对齐 xgent_ui handle_line_submit）时 shell 的回显。
///
/// 复现用户报告：终端输入 `ls` 回车后，回显显示 `lls`（首字母重复）。
/// 本测试捕获 PTY 实际回显字节流，断言是否出现重复 `l`。
#[tokio::test]
async fn write_lf_only_echo_no_duplicate_first_char() {
    let backend = LocalPtyBackend::new();
    let (tx, mut rx) = mpsc::channel::<TerminalEvent>(256);
    let id = backend.spawn(spawn_request(), tx).await.expect("spawn");
    // 等 shell 启动 + prompt
    let _ = collect_output(&mut rx, None, Duration::from_secs(8)).await;

    // 对齐 handle_line_submit：只 push b'\n'（无 \r）
    backend.write(id, b"ls\n".to_vec()).await.expect("write");

    // 收集足够输出（回显 + 命令结果 + 新 prompt）
    let buf = collect_output(&mut rx, None, Duration::from_secs(3)).await;
    let text = String::from_utf8_lossy(&buf);
    eprintln!("=== write_lf_only 回显 ===\n文本: {text}\n===");

    backend.kill(id).await.expect("kill");
    let _ = timeout(Duration::from_secs(3), rx.recv()).await;

    // 断言：不应出现 "lls"（首字母重复）
    assert!(
        !text.contains("lls"),
        "回显不应出现首字母重复的 'lls'，实际: {text}"
    );
}

/// 回归测试：shell（真实 zsh + 用户配置）回显 `ls` 不应出现 `lls`。
///
/// 根因：shell 的 ZLE/readline 在 cooked PTY 模式回显用户输入时，先回显首字符
/// （`l`），再 BS（退格 0x08）删除 + 重绘整行（`ls`）。xgent 的 vte `execute`
/// 未实现 BS 时，`l` 残留 + `ls` 追加 = `lls`。实现 BS 后应为 `ls`。
#[tokio::test]
async fn shell_echo_ls_not_lls() {
    use xgent_terminal::TerminalParser;
    let backend = LocalPtyBackend::new();
    let (tx, mut rx) = mpsc::channel::<TerminalEvent>(256);
    let id = backend.spawn(spawn_request(), tx).await.expect("spawn");
    // 等 shell 完全就绪（bracketed paste 启用 = ZLE 初始化完成）
    let _ = collect_output(&mut rx, Some("\x1b[?2004h"), Duration::from_secs(10)).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    while let Ok(Some(_)) = timeout(Duration::from_millis(50), rx.recv()).await {}

    backend.write(id, b"ls\n".to_vec()).await.expect("write");

    // 逐块 feed，累积所有完成行
    let mut parser = TerminalParser::new();
    let mut all_text = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while tokio::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(TerminalEvent::Output(bytes))) => {
                for line in parser.feed(&bytes) {
                    all_text.push_str(&line.plain_text());
                    all_text.push('\n');
                }
            }
            Ok(Some(TerminalEvent::Exited(_))) | Ok(None) => break,
            Err(_) => continue,
        }
    }
    backend.kill(id).await.expect("kill");
    let _ = timeout(Duration::from_secs(3), rx.recv()).await;

    assert!(
        !all_text.contains("lls"),
        "shell 回显 ls 不应出现首字母重复的 lls，实际解析行:\n{all_text}"
    );
}

/// 回归测试：backend 被 drop（不调 kill）时应清理所有 PTY 子进程。
///
/// 复现：LocalPtyBackend 无 Drop 实现时，drop 后子进程变孤儿、读循环
/// 线程泄漏。实现 Drop 后，drop 应 kill 所有未显式 kill 的会话。
#[tokio::test]
async fn drop_kills_unterminated_sessions() {
    let backend = LocalPtyBackend::new();
    let (tx, mut rx) = mpsc::channel::<TerminalEvent>(256);

    let id = backend.spawn(spawn_request(), tx).await.expect("spawn");
    // 等 shell 启动
    let _ = collect_output(&mut rx, None, Duration::from_secs(5)).await;

    // 不调 kill，直接 drop backend——Drop 应 kill 子进程。
    // receiver 持续收集，期望收到 Exited（子进程被 kill → reader EOF → Exited）。
    drop(backend);

    let mut got_exited = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(TerminalEvent::Exited(_))) => {
                got_exited = true;
                break;
            }
            Ok(Some(TerminalEvent::Output(_))) => continue,
            Ok(None) => break,
            Err(_) => continue,
        }
    }
    assert!(got_exited, "drop backend 后应收到 Exited（子进程被 kill）");

    // 避免未使用变量警告
    let _ = id;
}
