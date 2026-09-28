//! run_command 工具：运行子进程命令（无沙箱，靠用户确认）。
//!
//! MVP 无沙箱，仅靠用户确认。文档警示用户只运行可信命令。
//! 工作目录固定为项目根，捕获 stdout/stderr。
//! 支持中断：`CancellationToken` cancel 时 kill 子进程并返回 `ToolError::Aborted`。

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use xgent_core::chat::ToolSchema;

use crate::tool::{
    Concurrency, SideEffect, Tool, ToolCtx, ToolError, ToolResult, ToolTier, ToolUpdateCallback,
};

/// 运行子进程命令。
///
/// 工作目录固定为 `project_root`，捕获合并的 stdout/stderr。
/// 超时由内部限制（60s），避免卡死 agent loop。
pub struct RunCommand;

/// 默认命令超时（秒）。
const TIMEOUT_SECS: u64 = 60;

/// 工具输出上限（字节）：超出部分保留头尾 + 截断标记。
///
/// 无上限时命令输出（递归 cat、日志 dump）原样回灌 LLM 上下文，
/// 单次即可撑爆窗口并触发连锁压缩。
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// 危险命令模式：检测到则 `approval_for` 始终返回 Exec（即使配置 approved
/// 也需确认——MVP 暂无 yolo mode，此 override 逻辑预留）。
const DANGER_PATTERNS: &[&str] = &["rm -rf", "sudo", "mkfs"];

/// 把命令输出截断到 `MAX_OUTPUT_BYTES` 内：保留开头与结尾，中间以
/// 截断标记衔接（结尾常是报错位置，比只留头部更有用）。
///
/// 进入截断分支的判据是**字节**数（`s.len()`），而头尾保留按**字符**数算：
/// CJK 输出 65537 字节仅约 2.2 万字符，可能少于保留额（32768 字符），
/// `total - keep` 会下溢（debug panic 会打死 agent 任务）——须双重判据。
fn truncate_output(s: &str) -> String {
    let keep = MAX_OUTPUT_BYTES / 2;
    let total = s.chars().count();
    if s.len() <= MAX_OUTPUT_BYTES || total <= keep {
        return s.to_string();
    }
    // 按字符边界截断，避免切在 UTF-8 中间
    let head: String = s.chars().take(keep).collect();
    let tail: String = s.chars().skip(total - keep).collect();
    format!("{head}\n[... 输出过长，中间部分已截断 ...]\n{tail}")
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

    /// 检测危险命令（`rm -rf` / `sudo` / `mkfs`）始终返回 Exec。
    ///
    /// RunCommand 的 tier 本就是 Exec，此 override 语义是"即使配置 approved
    /// 也需确认"——MVP 暂无 yolo mode，实现检测危险命令返回 Exec（普通也返回
    /// Exec，逻辑等价但保留方法以支持 P1 的 yolo override）。
    fn approval_for(&self, input: &Value) -> ToolTier {
        if let Some(cmd) = input["command"].as_str()
            && DANGER_PATTERNS.iter().any(|p| cmd.contains(p))
        {
            return ToolTier::Exec;
        }
        self.tier()
    }

    fn summarize(&self, input: &Value) -> String {
        let cmd = input["command"].as_str().unwrap_or("?");
        format!("运行命令：{cmd}")
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolCtx,
        signal: CancellationToken,
        _on_update: Option<&ToolUpdateCallback>,
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
            _ = tokio::time::sleep(std::time::Duration::from_secs(TIMEOUT_SECS)) => {
                // 超时：杀整个进程组
                if let Some(c) = child_opt.as_mut() {
                    kill_child(c);
                    let _ = c.wait().await;
                }
                return Err(ToolError::Timeout(TIMEOUT_SECS));
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
        let mut text = truncate_output(&stdout);
        if !stderr.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str("[stderr]\n");
            text.push_str(&truncate_output(&stderr));
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
    child.start_kill();
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
        let r = RunCommand
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
        let r = RunCommand
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
        let r = RunCommand
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
            RunCommand.execute(
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

    #[test]
    fn summarize_includes_command() {
        let s = RunCommand.summarize(&json!({"command": "ls -la"}));
        assert!(s.contains("ls -la"));
    }

    #[test]
    fn approval_for_dangerous_command_returns_exec() {
        // rm -rf / 应返回 Exec（危险命令）
        assert_eq!(
            RunCommand.approval_for(&json!({"command": "rm -rf /"})),
            ToolTier::Exec
        );
        // sudo 应返回 Exec
        assert_eq!(
            RunCommand.approval_for(&json!({"command": "sudo apt update"})),
            ToolTier::Exec
        );
        // mkfs 应返回 Exec
        assert_eq!(
            RunCommand.approval_for(&json!({"command": "mkfs.ext4 /dev/sda"})),
            ToolTier::Exec
        );
    }

    #[test]
    fn approval_for_normal_command_returns_exec() {
        // 普通命令也返回 Exec（tier 本就是 Exec）
        assert_eq!(
            RunCommand.approval_for(&json!({"command": "ls -la"})),
            ToolTier::Exec
        );
    }
}
