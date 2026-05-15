# xgent_tools — 详细编码指导

## 前置依赖

- xgent_settings ✅
- xgent_provider ✅（仅需 ProviderId 类型）

## 模块职责

定义 Agent 可调用的工具枚举、实现安全策略检查、提供工具执行器。

---

## 目标文件结构

```
crates/xgent_tools/
├── Cargo.toml
└── src/
    ├── lib.rs            # Plugin + 导出
    ├── definition.rs     # Tool enum, ToolCallRequest/Result
    ├── security.rs       # SecurityPolicy, Verdict
    ├── executor.rs       # ToolExecutor (异步执行工具)
    └── builtins/
        ├── mod.rs
        ├── file_ops.rs   # ReadFile, WriteFile, SearchFiles
        └── command.rs    # RunCommand (Git* 暂缓)
```

---

## Cargo.toml

```toml
[package]
name = "xgent_tools"
version = "0.1.0"
edition = "2024"

[dependencies]
bevy = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
```

---

## definition.rs — 工具定义

```rust
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 工具标识（用于 LLM function calling 的 name 字段）
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolId(pub String);

/// Agent 可调用的工具
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Tool {
    #[serde(rename = "read_file")]
    ReadFile {
        path: PathBuf,
        #[serde(skip_serializing_if = "Option::is_none")]
        range: Option<(usize, usize)>,
    },

    #[serde(rename = "write_file")]
    WriteFile {
        path: PathBuf,
        content: String,
    },

    #[serde(rename = "search_files")]
    SearchFiles {
        pattern: String,
        query: String,
    },

    #[serde(rename = "run_command")]
    RunCommand {
        cmd: String,
        cwd: PathBuf,
        #[serde(default = "default_timeout")]
        timeout_secs: u64,
    },

    #[serde(rename = "mcp_tool")]
    McpTool {
        server_id: String,
        name: String,
        arguments: serde_json::Value,
    },
}

fn default_timeout() -> u64 { 30 }

/// 工具调用请求（Bevy Message）
#[derive(Debug, Clone, Event)]
pub struct ToolCallRequest {
    pub call_id: String,
    pub tool: Tool,
}

/// 工具调用结果（Bevy Message）
#[derive(Debug, Clone, Event)]
pub struct ToolCallResult {
    pub call_id: String,
    pub output: Result<serde_json::Value, ToolError>,
    pub duration_secs: f64,
}

/// 工具执行错误
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolError {
    /// 权限不足
    PermissionDenied(String),
    /// 执行失败
    ExecutionFailed(String),
    /// 超时
    Timeout,
    /// 工具未找到
    NotFound(String),
}

/// 将 Tool 转换为 OpenAI function calling 格式
impl Tool {
    /// 生成 LLM function calling 的工具定义列表
    pub fn tool_definitions() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "Read the contents of a file. Returns the file content as a string.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Absolute path to the file" },
                            "range": {
                                "type": "array",
                                "items": { "type": "integer" },
                                "description": "Optional [start_line, end_line] range to read"
                            }
                        },
                        "required": ["path"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "write_file",
                    "description": "Write content to a file. Creates the file if it doesn't exist.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Absolute path to the file" },
                            "content": { "type": "string", "description": "Content to write" }
                        },
                        "required": ["path", "content"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "search_files",
                    "description": "Search for files matching a pattern and containing a query string.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "pattern": { "type": "string", "description": "Glob pattern, e.g. '**/*.rs'" },
                            "query": { "type": "string", "description": "Text to search for within matching files" }
                        },
                        "required": ["pattern", "query"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "run_command",
                    "description": "Run a shell command and return its output.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "cmd": { "type": "string", "description": "The command to run" },
                            "cwd": { "type": "string", "description": "Working directory" },
                            "timeout_secs": { "type": "integer", "description": "Timeout in seconds (default 30)" }
                        },
                        "required": ["cmd", "cwd"]
                    }
                }
            }),
        ]
    }

    /// 从 LLM 的 tool_call 响应解析出 Tool 实例
    pub fn from_tool_call(name: &str, arguments: &str) -> Result<Self, ToolError> {
        let args: serde_json::Value = serde_json::from_str(arguments)
            .map_err(|e| ToolError::ExecutionFailed(format!("Invalid JSON arguments: {}", e)))?;

        match name {
            "read_file" => {
                let path = args["path"].as_str()
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'path'".into()))?;
                let range = args.get("range").and_then(|r| r.as_array())
                    .and_then(|a| {
                        let start = a.first()?.as_u64()? as usize;
                        let end = a.get(1)?.as_u64()? as usize;
                        Some((start, end))
                    });
                Ok(Tool::ReadFile { path: path.into(), range })
            }
            "write_file" => {
                let path = args["path"].as_str()
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'path'".into()))?;
                let content = args["content"].as_str()
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'content'".into()))?;
                Ok(Tool::WriteFile { path: path.into(), content: content.into() })
            }
            "search_files" => {
                let pattern = args["pattern"].as_str()
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'pattern'".into()))?;
                let query = args["query"].as_str()
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'query'".into()))?;
                Ok(Tool::SearchFiles { pattern: pattern.into(), query: query.into() })
            }
            "run_command" => {
                let cmd = args["cmd"].as_str()
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'cmd'".into()))?;
                let cwd = args["cwd"].as_str()
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'cwd'".into()))?;
                let timeout_secs = args["timeout_secs"].as_u64().unwrap_or(30);
                Ok(Tool::RunCommand { cmd: cmd.into(), cwd: cwd.into(), timeout_secs })
            }
            _ => Err(ToolError::NotFound(name.into())),
        }
    }
}
```

---

## security.rs — 安全策略

```rust
use crate::definition::Tool;
use bevy::prelude::*;
use std::path::PathBuf;

/// 安全策略检查结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 自动允许
    Approved,
    /// 需要用户确认
    NeedsConfirmation,
    /// 硬性拒绝
    Denied,
}

/// 安全策略（ECS Resource）
#[derive(Resource, Debug, Clone)]
pub struct SecurityPolicy {
    /// 项目根目录（沙箱边界）
    pub sandbox_root: PathBuf,
    /// 禁止执行的命令前缀
    pub blocked_commands: Vec<String>,
}

impl SecurityPolicy {
    pub fn new(sandbox_root: PathBuf) -> Self {
        Self {
            sandbox_root,
            blocked_commands: vec![
                "rm -rf".into(),
                "mkfs".into(),
                "dd ".into(),
                "format ".into(),
                "shutdown".into(),
                "reboot".into(),
            ],
        }
    }

    /// 检查工具是否被允许执行
    pub fn check(&self, tool: &Tool) -> Verdict {
        match tool {
            // 只读操作 → 自动允许
            Tool::ReadFile { .. } | Tool::SearchFiles { .. } => Verdict::Approved,

            // 写文件 → 需确认（但必须在沙箱内）
            Tool::WriteFile { path, .. } => {
                if !path.starts_with(&self.sandbox_root) {
                    Verdict::Denied  // 不允许写沙箱外的文件
                } else {
                    Verdict::NeedsConfirmation
                }
            }

            // 执行命令 → 检查黑名单
            Tool::RunCommand { cmd, .. } => {
                if self.blocked_commands.iter().any(|b| cmd.starts_with(b)) {
                    Verdict::Denied
                } else {
                    Verdict::NeedsConfirmation
                }
            }

            // MCP 工具 → 取决于 MCP Server 的 trust_level
            Tool::McpTool { .. } => Verdict::NeedsConfirmation,
        }
    }
}
```

---

## executor.rs — 工具执行器

```rust
use crate::definition::*;
use std::time::Instant;

/// 工具执行器
///
/// 在 tokio 异步运行时中执行工具，返回结果。
/// 不依赖 Bevy ECS，可以独立测试。
pub struct ToolExecutor;

impl ToolExecutor {
    /// 执行一个工具
    pub async fn execute(tool: &Tool) -> Result<serde_json::Value, ToolError> {
        let start = Instant::now();
        let result = match tool {
            Tool::ReadFile { path, range } => Self::read_file(path, range).await,
            Tool::WriteFile { path, content } => Self::write_file(path, content).await,
            Tool::SearchFiles { pattern, query } => Self::search_files(pattern, query).await,
            Tool::RunCommand { cmd, cwd, timeout_secs } => {
                Self::run_command(cmd, cwd, *timeout_secs).await
            }
            Tool::McpTool { .. } => {
                Err(ToolError::NotFound("MCP tools are handled by xgent_mcp".into()))
            }
        };
        result
    }

    async fn read_file(path: &std::path::Path, range: &Option<(usize, usize)>) -> Result<serde_json::Value, ToolError> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read {}: {}", path.display(), e)))?;

        let content = match range {
            Some((start, end)) => {
                let lines: Vec<&str> = content.lines().collect();
                let selected: Vec<&str> = lines.iter()
                    .skip(start.saturating_sub(1))
                    .take(end.saturating_sub(*start).max(1))
                    .copied()
                    .collect();
                selected.join("\n")
            }
            None => content,
        };

        Ok(serde_json::json!({
            "path": path.to_string_lossy(),
            "content": content,
        }))
    }

    async fn write_file(path: &std::path::Path, content: &str) -> Result<serde_json::Value, ToolError> {
        // 确保父目录存在
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create directory: {}", e)))?;
        }

        tokio::fs::write(path, content)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write {}: {}", path.display(), e)))?;

        Ok(serde_json::json!({
            "path": path.to_string_lossy(),
            "written": true,
        }))
    }

    async fn search_files(pattern: &str, query: &str) -> Result<serde_json::Value, ToolError> {
        // MVP-1: 简单实现 — 使用 grep 命令
        // 后续替换为 Rust 原生实现
        let output = tokio::process::Command::new("grep")
            .args(["-rn", "--include", pattern, query, "."])
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("grep failed: {}", e)))?;

        let result = String::from_utf8_lossy(&output.stdout).to_string();
        let matches: Vec<&str> = result.lines().take(50).collect(); // 限制结果数量

        Ok(serde_json::json!({
            "pattern": pattern,
            "query": query,
            "matches": matches,
            "total": result.lines().count(),
        }))
    }

    async fn run_command(cmd: &str, cwd: &std::path::Path, timeout_secs: u64) -> Result<serde_json::Value, ToolError> {
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(cwd)
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Command failed: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(serde_json::json!({
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
        }))
    }
}
```

---

## lib.rs

```rust
mod definition;
mod security;
mod executor;
pub mod builtins;

pub use definition::*;
pub use security::*;
pub use executor::ToolExecutor;

use bevy::prelude::*;

pub struct XgentToolsPlugin;

impl Plugin for XgentToolsPlugin {
    fn build(&self, app: &mut App) {
        // SecurityPolicy 需要由 xgent_app 在 Startup 时设置 sandbox_root
        // 这里先不 init_resource，让上层设置
    }
}
```

---

## 验证方法

1. `cargo check -p xgent_tools`
2. 单元测试 Tool::from_tool_call 解析
3. 单元测试 SecurityPolicy::check 各场景
4. 集成测试 ToolExecutor::execute 真实文件读写

---

## 完成后下一步

→ **xgent_agent**（对话循环 + ECS 桥接）
