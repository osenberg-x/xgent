//! run_command 工具：运行子进程命令（无沙箱，靠用户确认）。
//!
//! MVP 无沙箱，仅靠用户确认。文档警示用户只运行可信命令。
//! 工作目录固定为项目根，捕获 stdout/stderr。

use async_trait::async_trait;
use serde_json::{Value, json};
use xgent_core::chat::ToolSchema;

use crate::tool::{SideEffect, Tool, ToolCtx, ToolResult};

/// 运行子进程命令。
///
/// 工作目录固定为 `project_root`，捕获合并的 stdout/stderr。
/// 超时由内部限制（60s），避免卡死 agent loop。
pub struct RunCommand;

/// 默认命令超时（秒）。
const TIMEOUT_SECS: u64 = 60;

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

    fn summarize(&self, input: &Value) -> String {
        let cmd = input["command"].as_str().unwrap_or("?");
        format!("运行命令：{cmd}")
    }

    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolResult {
        let Some(command) = input["command"].as_str() else {
            return ToolResult {
                output: "缺少参数 command".into(),
                success: false,
                side_effect: None,
            };
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

        let output =
            match tokio::time::timeout(std::time::Duration::from_secs(TIMEOUT_SECS), cmd.output())
                .await
            {
                Ok(Ok(o)) => o,
                Ok(Err(e)) => {
                    return ToolResult {
                        output: format!("启动命令失败: {e}"),
                        success: false,
                        side_effect: None,
                    };
                }
                Err(_) => {
                    return ToolResult {
                        output: format!("命令超时（{TIMEOUT_SECS}s）"),
                        success: false,
                        side_effect: None,
                    };
                }
            };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let success = output.status.success();
        let mut text = stdout;
        if !stderr.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str("[stderr]\n");
            text.push_str(&stderr);
        }
        text.push_str(&format!("\n[exit: {}]", output.status));
        ToolResult {
            output: text,
            success,
            side_effect: Some(SideEffect::CommandRun(command.to_string())),
        }
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
            .execute(json!({"command": "echo hello"}), &ctx)
            .await;
        assert!(r.success);
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
        let r = RunCommand.execute(json!({"command": "exit 7"}), &ctx).await;
        assert!(!r.success);
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
            .execute(json!({"command": "pwd > out.txt"}), &ctx)
            .await;
        assert!(r.success);
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
}
