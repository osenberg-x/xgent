//! editor 工具：agent 驱动编辑器动作（UI-only Tier，默认 Approved）。
//!
//! 详见 `doc/design/editor-design.md` 6.4 节。
//!
//! 设计要点：
//! - `EditorTool` 不直接执行 IO，只把请求经 [`EditorCommandSink`] 转发给 UI 侧
//!   （由 `xgent_ui`/`xgent_agent` 注入实现），UI 侧订阅后发 ECS `EditorCommand`。
//! - tier 为 [`ToolTier::UiOnly`]，`resolve_policy` 默认返回 `Approved`，不走确认弹窗。
//! - 工具结果回灌 agent："已打开 src/main.rs:42" 之类的人类可读描述。
//!
//! `EditorCommandSink` 为 trait object 注入：`xgent_tools` 不依赖 Bevy，
//! 跨 ECS/异步桥接由调用方（`xgent_agent` 桥接层）实现。

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use xgent_core::chat::ToolSchema;

use crate::tool::{
    Concurrency, Tool, ToolCtx, ToolError, ToolResult, ToolTier, ToolUpdateCallback,
};

/// agent 驱动编辑器的命令载荷（与 `xgent_ui::editor::command::EditorCommand` 一一对应）。
///
/// 定义在 `xgent_tools` 而非 `xgent_core`：该载荷仅用于工具→UI 的单向传递，
/// 不跨进程；`xgent_core` 只放跨进程共享类型。UI 侧的 `EditorCommand` Event
/// 直接从此枚举构造。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorCommandRequest {
    /// 打开文件并可选跳到某行
    OpenFile {
        /// 相对项目根或绝对路径
        path: PathBuf,
        /// 跳转行号（1-based，None 则不跳）
        line: Option<usize>,
    },
    /// 跳到某行某列
    GoTo {
        /// 行号（1-based）
        line: usize,
        /// 列号（1-based，None 则不指定列）
        col: Option<usize>,
    },
    /// 设置选区（字节偏移区间，半开 [start, end)）
    SetSelection {
        /// 起始字节偏移
        start: usize,
        /// 结束字节偏移（半开）
        end: usize,
    },
    /// 滚动到某行
    ScrollTo {
        /// 行号（1-based）
        line: usize,
    },
    /// 关闭某路径对应的标签
    CloseTab {
        /// 文件路径
        path: PathBuf,
    },
}

/// 命令转发 sink：由 `xgent_agent` 桥接层注入实现，把工具请求转为 ECS Event。
///
/// `xgent_tools` 不依赖 Bevy，故用 trait 抽象。典型实现：`xgent_agent::bridge`
/// 持有一个 `tokio::sync::mpsc::Sender<EditorCommandRequest>`，sink 把请求写入
/// channel，`agent_poll_system` 消费后发 ECS `EditorCommand`。
pub trait EditorCommandSink: Send + Sync {
    /// 转发一条编辑器命令请求。返回 `Err` 表示 UI 侧不可达（如未注入）。
    fn emit(&self, req: EditorCommandRequest) -> Result<(), String>;
}

/// agent 驱动编辑器的工具集，UI-only Tier，默认 Approved。
///
/// 单一工具聚合多个动作（`action` 字段区分），简化 schema 与 sink 注入。
/// 对齐 omp 的"工具即动作集合"模式。
pub struct EditorTool {
    sink: Arc<dyn EditorCommandSink>,
}

impl EditorTool {
    /// 构造，注入命令转发 sink。
    pub fn new(sink: Arc<dyn EditorCommandSink>) -> Self {
        Self { sink }
    }

    /// 工具 id（聚合多个编辑器动作）。
    pub const ID: &'static str = "editor";

    /// 从 LLM 输入解析出 [`EditorCommandRequest`]，并生成人类可读摘要。
    ///
    /// 返回 `(请求, 摘要)`。解析失败返回 `Err(错误描述)`。
    fn parse_input(input: &Value) -> Result<(EditorCommandRequest, String), String> {
        let action = input["action"]
            .as_str()
            .ok_or_else(|| "缺少 action 字段".to_string())?;
        match action {
            "open_file" => {
                let path = input["path"]
                    .as_str()
                    .ok_or_else(|| "open_file 需要 path 字段".to_string())?;
                let line = input["line"].as_u64().map(|l| l as usize);
                let req = EditorCommandRequest::OpenFile {
                    path: PathBuf::from(path),
                    line,
                };
                let summary = match line {
                    Some(l) => format!("打开 {path}:{l}"),
                    None => format!("打开 {path}"),
                };
                Ok((req, summary))
            }
            "goto" => {
                let line = input["line"]
                    .as_u64()
                    .ok_or_else(|| "goto 需要 line 字段".to_string())?
                    as usize;
                let col = input["col"].as_u64().map(|c| c as usize);
                let req = EditorCommandRequest::GoTo { line, col };
                let summary = match col {
                    Some(c) => format!("跳转到 {line}:{c}"),
                    None => format!("跳转到行 {line}"),
                };
                Ok((req, summary))
            }
            "set_selection" => {
                let start = input["start"]
                    .as_u64()
                    .ok_or_else(|| "set_selection 需要 start 字段".to_string())?
                    as usize;
                let end = input["end"]
                    .as_u64()
                    .ok_or_else(|| "set_selection 需要 end 字段".to_string())?
                    as usize;
                let req = EditorCommandRequest::SetSelection { start, end };
                Ok((req, format!("选区 {start}..{end}")))
            }
            "scroll_to" => {
                let line = input["line"]
                    .as_u64()
                    .ok_or_else(|| "scroll_to 需要 line 字段".to_string())?
                    as usize;
                let req = EditorCommandRequest::ScrollTo { line };
                Ok((req, format!("滚动到行 {line}")))
            }
            "close_tab" => {
                let path = input["path"]
                    .as_str()
                    .ok_or_else(|| "close_tab 需要 path 字段".to_string())?;
                let req = EditorCommandRequest::CloseTab {
                    path: PathBuf::from(path),
                };
                Ok((req, format!("关闭标签 {path}")))
            }
            other => Err(format!("未知 action: {other}")),
        }
    }
}

#[async_trait]
impl Tool for EditorTool {
    fn id(&self) -> &str {
        Self::ID
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.id().to_string(),
            description: "驱动内置编辑器：打开文件、跳转、设置选区、滚动、关闭标签。\
                           UI-only 操作，不修改文件（保存请用 write_file 或让用户在编辑器内 Cmd+S）。"
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["open_file", "goto", "set_selection", "scroll_to", "close_tab"],
                        "description": "编辑器动作类型"
                    },
                    "path": {
                        "type": "string",
                        "description": "open_file / close_tab 的文件路径（相对项目根或绝对）"
                    },
                    "line": {
                        "type": "integer",
                        "description": "行号（1-based）。open_file 可选，goto/scroll_to 必填"
                    },
                    "col": {
                        "type": "integer",
                        "description": "列号（1-based）。goto 可选"
                    },
                    "start": {
                        "type": "integer",
                        "description": "set_selection 起始字节偏移"
                    },
                    "end": {
                        "type": "integer",
                        "description": "set_selection 结束字节偏移（半开）"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::UiOnly
    }

    fn concurrency(&self) -> Concurrency {
        // UI-only 命令互不冲突，可并行
        Concurrency::Shared
    }

    fn summarize(&self, input: &Value) -> String {
        match Self::parse_input(input) {
            Ok((_, s)) => s,
            Err(e) => format!("editor: {e}"),
        }
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolCtx,
        _signal: CancellationToken,
        _on_update: Option<&ToolUpdateCallback>,
    ) -> Result<ToolResult, ToolError> {
        let (req, summary) = match Self::parse_input(&input) {
            Ok(v) => v,
            Err(e) => {
                return Ok(ToolResult {
                    output: e,
                    is_error: true,
                    side_effect: None,
                });
            }
        };
        match self.sink.emit(req.clone()) {
            Ok(()) => Ok(ToolResult {
                output: format!("已请求：{summary}"),
                is_error: false,
                side_effect: None,
            }),
            Err(e) => Ok(ToolResult {
                output: format!("编辑器不可达：{e}（{summary}）"),
                is_error: true,
                side_effect: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use tokio_util::sync::CancellationToken;
    #[derive(Default)]
    struct RecordingSink {
        received: Mutex<Vec<EditorCommandRequest>>,
    }

    impl EditorCommandSink for RecordingSink {
        fn emit(&self, req: EditorCommandRequest) -> Result<(), String> {
            self.received.lock().push(req);
            Ok(())
        }
    }

    /// 失败 sink：总是返回错误。
    struct FailingSink;

    impl EditorCommandSink for FailingSink {
        fn emit(&self, _req: EditorCommandRequest) -> Result<(), String> {
            Err("no editor".into())
        }
    }

    fn make_ctx() -> ToolCtx {
        ToolCtx {
            project_root: PathBuf::from("/proj"),
            tool_policy: Default::default(),
        }
    }

    #[test]
    fn tier_is_uionly() {
        let sink = Arc::new(RecordingSink::default());
        let tool = EditorTool::new(sink);
        assert_eq!(tool.tier(), ToolTier::UiOnly);
    }

    #[test]
    fn parse_open_file_with_line() {
        let input = json!({"action": "open_file", "path": "src/main.rs", "line": 42});
        let (req, summary) = EditorTool::parse_input(&input).unwrap();
        assert_eq!(
            req,
            EditorCommandRequest::OpenFile {
                path: PathBuf::from("src/main.rs"),
                line: Some(42),
            }
        );
        assert_eq!(summary, "打开 src/main.rs:42");
    }

    #[test]
    fn parse_goto_without_col() {
        let input = json!({"action": "goto", "line": 10});
        let (req, summary) = EditorTool::parse_input(&input).unwrap();
        assert_eq!(
            req,
            EditorCommandRequest::GoTo {
                line: 10,
                col: None
            }
        );
        assert_eq!(summary, "跳转到行 10");
    }

    #[test]
    fn parse_unknown_action_errors() {
        let input = json!({"action": "frob"});
        let err = EditorTool::parse_input(&input).unwrap_err();
        assert!(err.contains("未知 action"));
    }

    #[tokio::test]
    async fn execute_emits_command_and_returns_summary() {
        let sink = Arc::new(RecordingSink::default());
        let tool = EditorTool::new(sink.clone());
        let input = json!({"action": "open_file", "path": "src/lib.rs", "line": 5});
        let result = tool
            .execute(input, &make_ctx(), CancellationToken::new(), None)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("打开 src/lib.rs:5"));
        let received = sink.received.lock().clone();
        assert_eq!(received.len(), 1);
        assert_eq!(
            received[0],
            EditorCommandRequest::OpenFile {
                path: PathBuf::from("src/lib.rs"),
                line: Some(5),
            }
        );
    }

    #[tokio::test]
    async fn execute_returns_error_when_sink_fails() {
        let sink = Arc::new(FailingSink);
        let tool = EditorTool::new(sink);
        let input = json!({"action": "scroll_to", "line": 100});
        let result = tool
            .execute(input, &make_ctx(), CancellationToken::new(), None)
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("编辑器不可达"));
    }

    #[tokio::test]
    async fn execute_returns_error_on_bad_input() {
        let sink = Arc::new(RecordingSink::default());
        let tool = EditorTool::new(sink.clone());
        let input = json!({"action": "goto"}); // 缺 line
        let result = tool
            .execute(input, &make_ctx(), CancellationToken::new(), None)
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("line"));
        // 不应调用 sink
        assert!(sink.received.lock().is_empty());
    }

    #[test]
    fn resolve_policy_uionly_default_approved() {
        use crate::security::resolve_policy;
        use xgent_settings_core::project::ToolPolicyConfig;
        let sink = Arc::new(RecordingSink::default());
        let tool = EditorTool::new(sink);
        let p = ToolPolicyConfig::default();
        assert_eq!(
            resolve_policy(
                EditorTool::ID,
                tool.tier(),
                &json!({"action": "open_file", "path": "x"}),
                &tool,
                &p,
            ),
            crate::tool::SecurityPolicy::Approved,
        );
    }
}
