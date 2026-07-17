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

/// 危险命令模式：检测到则 `approval_for` 始终返回 Exec（即使配置 approved
/// 也需确认——MVP 暂无 yolo mode，此 override 逻辑预留）。
const DANGER_PATTERNS: &[&str] = &["rm -rf", "sudo", "mkfs"];

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
        if let Some(cmd) = input["command"].as_str() {
            if DANGER_PATTERNS.iter().any(|p| cmd.contains(p)) {
                return ToolTier::Exec;
            }
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

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    output: format!("启动命令失败: {e}"),
                    is_error: true,
                    side_effect: None,
                });
            }
        };
        // kill_on_drop 保证 task 取消时子进程被清理；显式 kill 仍用于返回正确错误

        // 取 stdout/stderr 句柄（等待完成后读取）
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // 用 select! 同时监听 cancel / 超时 / 子进程完成
        // 将 child 包进 Option：cancel/timeout 分支 take 出 child 调 kill，
        // 避免 child.wait() 的可变借用与 child.kill() 冲突
        let mut child_opt = Some(child);
        let status = tokio::select! {
            biased;
            _ = signal.cancelled() => {
                // 中断：kill 子进程并等待其退出
                if let Some(mut c) = child_opt.take() {
                    let _ = c.start_kill();
                    let _ = c.wait().await;
                }
                return Err(ToolError::Aborted);
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(TIMEOUT_SECS)) => {
                // 超时：kill 子进程
                if let Some(mut c) = child_opt.take() {
                    let _ = c.start_kill();
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
                        side_effect: None,
                    });
                }
            },
        };
        // 读取 stdout/stderr（子进程已结束，管道有数据）
        let stdout_data = match stdout {
            Some(mut s) => {
                use tokio::io::AsyncReadExt;
                let mut buf = Vec::new();
                let _ = s.read_to_end(&mut buf).await;
                buf
            }
            None => Vec::new(),
        };
        let stderr_data = match stderr {
            Some(mut s) => {
                use tokio::io::AsyncReadExt;
                let mut buf = Vec::new();
                let _ = s.read_to_end(&mut buf).await;
                buf
            }
            None => Vec::new(),
        };

        let stdout = String::from_utf8_lossy(&stdout_data).to_string();
        let stderr = String::from_utf8_lossy(&stderr_data).to_string();
        let is_error = !status.success();
        let mut text = stdout;
        if !stderr.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str("[stderr]\n");
            text.push_str(&stderr);
        }
        text.push_str(&format!("\n[exit: {}]", status));
        Ok(ToolResult {
            output: text,
            is_error,
            side_effect: Some(SideEffect::CommandRun(command.to_string())),
        })
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
