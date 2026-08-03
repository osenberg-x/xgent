//! xgent_plugin_git — Git 集成参考插件。
//!
//! 照设计文档 §11.3 + Step P5。验证完整链路：工具经 `host.run_command("git", ...)`
//! 执行 git 子命令，返回 JSON 序列化的 ToolResult。
//!
//! 提供：git_diff / git_commit / git_log / git_status 工具 + diff/commit/log/status 命令 + git_history ContextProvider。

use xgent_plugin_api::{Extension, WitCommandDef, WitToolDef, WitToolError, WitToolTier};

struct GitPlugin;

impl Extension for GitPlugin {
    fn new() -> Self {
        GitPlugin
    }

    fn register_tools(&mut self) -> Vec<WitToolDef> {
        vec![
            WitToolDef {
                id: "git_diff".into(),
                description: "查看 Git 工作区 diff".into(),
                schema: r#"{"type":"object","properties":{"cached":{"type":"boolean","description":"是否只看已暂存(cached)的 diff"}}}}"#.into(),
                tier: WitToolTier::Read,
            },
            WitToolDef {
                id: "git_log".into(),
                description: "查看 Git 提交历史（最近 20 条）".into(),
                schema: r#"{"type":"object","properties":{"limit":{"type":"number","description":"返回条数，默认 20"}}}"#.into(),
                tier: WitToolTier::Read,
            },
            WitToolDef {
                id: "git_status".into(),
                description: "查看 Git 工作区状态".into(),
                schema: r#"{"type":"object"}"#.into(),
                tier: WitToolTier::Read,
            },
            WitToolDef {
                id: "git_commit".into(),
                description: "提交暂存区更改".into(),
                schema: r#"{"type":"object","properties":{"message":{"type":"string","description":"提交信息"}},"required":["message"]}"#.into(),
                tier: WitToolTier::Write,
            },
        ]
    }

    fn register_commands(&mut self) -> Vec<WitCommandDef> {
        vec![
            WitCommandDef { id: "diff".into(), label: "Git: 查看 Diff".into() },
            WitCommandDef { id: "log".into(), label: "Git: 提交历史".into() },
            WitCommandDef { id: "status".into(), label: "Git: 状态".into() },
            WitCommandDef { id: "commit".into(), label: "Git: 提交".into() },
        ]
    }

    fn register_context_providers(&mut self) -> Vec<xgent_plugin_api::WitProviderDef> {
        vec![xgent_plugin_api::WitProviderDef {
            id: "git_history".into(),
            description: "Git 提交历史上下文".into(),
        }]
    }

    fn execute(&mut self, tool_id: &str, input: &str) -> Result<String, WitToolError> {
        // 解析 input JSON 取参数（忽略解析失败用默认）
        let input_val: serde_json::Value = serde_json::from_str(input).unwrap_or_default();
        match tool_id {
            "git_diff" => {
                let cached = input_val
                    .get("cached")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let args: Vec<&str> = if cached {
                    vec!["diff", "--cached", "--stat"]
                } else {
                    vec!["diff", "--stat"]
                };
                run_git_via_host_command(tool_id, args)
            }
            "git_log" => {
                let limit = input_val
                    .get("limit")
                    .and_then(|v| v.as_f64())
                    .map(|n| n as u32)
                    .unwrap_or(20);
                let n_str = limit.to_string();
                run_git_via_host_command(tool_id, vec!["log", &format!("-n{n_str}"), "--oneline"])
            }
            "git_status" => {
                run_git_via_host_command(tool_id, vec!["status", "--short"])
            }
            "git_commit" => {
                let message = input_val
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if message.is_empty() {
                    return Err(WitToolError::Failed("提交信息不能为空".into()));
                }
                run_git_via_host_command(tool_id, vec!["commit", "-m", message])
            }
            _ => Err(WitToolError::Failed(format!("未知工具: {tool_id}"))),
        }
    }

    fn run_command(&mut self, command_id: &str) -> Result<String, String> {
        // 命令面板命令：复用 execute 的 git 调用，返回 output
        let tool_id = match command_id {
            "diff" => "git_diff",
            "log" => "git_log",
            "status" => "git_status",
            "commit" => "git_commit",
            _ => return Err(format!("未知命令: {command_id}")),
        };
        self.execute(tool_id, "{}").map_err(|e| match e {
            WitToolError::Failed(msg) => msg,
            WitToolError::Aborted => "被中断".into(),
        })
    }

    fn retrieve(
        &mut self,
        provider_id: &str,
        _query: &xgent_plugin_api::WitContextQuery,
    ) -> Result<xgent_plugin_api::WitContextResult, String> {
        if provider_id != "git_history" {
            return Err(format!("未知 provider: {provider_id}"));
        }
        // 取最近 20 条提交历史作为上下文
        let req = xgent_plugin_api::WitCommandReq {
            program: "git".into(),
            args: vec!["log".into(), "-n20".into(), "--oneline".into()],
            cwd: None,
        };
        let out = xgent_plugin_api::host::run_command(&req)
            .map_err(|e| format!("git log 失败: {:?}", e))?;
        let content = if !out.stdout.is_empty() { out.stdout } else { out.stderr };
        let chunk = xgent_plugin_api::WitContextChunk {
            path: "git_history".into(),
            content,
            relevance: "high".into(),
            token_estimate: 0,
        };
        Ok(xgent_plugin_api::WitContextResult {
            chunks: vec![chunk],
            tree_summary: None,
            total_tokens: 0,
        })
    }
}

fn run_git_via_host_command(tool_id: &str, args: Vec<&str>) -> Result<String, WitToolError> {
    let req = xgent_plugin_api::WitCommandReq {
        program: "git".into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        cwd: None,
    };
    match xgent_plugin_api::host::run_command(&req) {
        Ok(out) => {
            // git 命令成功执行（exit_code 可能非 0，如 diff 无变更返回 1）
            let output = if !out.stdout.is_empty() {
                out.stdout
            } else if !out.stderr.is_empty() {
                out.stderr
            } else {
                format!("git {tool_id} 完成（exit {}）", out.exit_code)
            };
            let result = serde_json::json!({
                "output": output,
                "is_error": false,
                "denied": false,
                "side_effect": null,
            });
            serde_json::to_string(&result).map_err(|e| WitToolError::Failed(e.to_string()))
        }
        Err(e) => {
            // command-error：cancelled / permission-denied / spawn-failed / io
            let msg = match e {
                xgent_plugin_api::WitCommandError::Cancelled => {
                    return Err(WitToolError::Aborted);
                }
                xgent_plugin_api::WitCommandError::PermissionDenied => {
                    "git 命令被权限拒绝".to_string()
                }
                xgent_plugin_api::WitCommandError::SpawnFailed(s) => {
                    format!("git 启动失败: {s}")
                }
                xgent_plugin_api::WitCommandError::Io(s) => {
                    format!("git IO 错误: {s}")
                }
            };
            let result = serde_json::json!({
                "output": msg,
                "is_error": true,
                "denied": false,
                "side_effect": null,
            });
            serde_json::to_string(&result).map_err(|e| WitToolError::Failed(e.to_string()))
        }
    }
}

xgent_plugin_api::register_plugin!(GitPlugin);
