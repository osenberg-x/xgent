//! agent 桥接与 loop 的集成测试。
//!
//! 用 mock ProviderClient（本地假流式输出）驱动 agent loop，
//! 断言消息序列与状态流转。

#![cfg(test)]

use std::sync::Arc;

use async_trait::async_trait;
use bevy::prelude::*;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use xgent_context::provider::{ContextProvider, ContextQuery, ContextResult};
use xgent_core::chat::{ChatEvent, ChatRequest, TokenUsage, ToolSchema};
use xgent_core::ids::StreamId;
use xgent_tools::ToolExecutor;
use xgent_tools::ToolUpdateCallback;
use xgent_tools::tool::{Concurrency, Tool, ToolCtx, ToolError, ToolResult, ToolTier};

use crate::XgentAgentPlugin;
use crate::bridge::{AgentBridge, AgentBridgeConfig, ProviderClient};
use crate::conversation::ConversationStatus;
use crate::events::*;
use xgent_settings_core::project::ToolPolicyConfig;

/// mock provider：第一次返回预设事件序列，后续返回空 Done{Stop}（模拟 LLM 收到工具结果后停止）。
struct MockProvider {
    events: Vec<ChatEvent>,
    call_count: std::sync::atomic::AtomicU32,
}

impl MockProvider {
    fn new(events: Vec<ChatEvent>) -> Self {
        Self {
            events,
            call_count: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

#[async_trait]
impl ProviderClient for MockProvider {
    async fn chat(
        &self,
        _req: ChatRequest,
    ) -> Result<(StreamId, mpsc::Receiver<ChatEvent>), (xgent_core::chat::ErrorKind, String)> {
        let (tx, rx) = mpsc::channel(8);
        let n = self
            .call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let events = if n == 0 {
            self.events.clone()
        } else {
            // 后续调用：无 tool_calls，Done{Stop}
            vec![ChatEvent::Done {
                reason: xgent_core::chat::StopReason::Stop,
                usage: TokenUsage::default(),
            }]
        };
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                for ev in events {
                    if tx.send(ev).await.is_err() {
                        break;
                    }
                }
            });
        });
        Ok((StreamId(1), rx))
    }
}

/// mock context provider：返回空结果。
struct MockContext;

#[async_trait]
impl ContextProvider for MockContext {
    async fn retrieve(&self, _q: &ContextQuery) -> ContextResult {
        ContextResult::default()
    }
}

fn test_app(mock_events: Vec<ChatEvent>) -> App {
    test_app_with_executor(
        mock_events,
        Arc::new(ToolExecutor::with_defaults()),
        ToolPolicyConfig::default(),
    )
    .0
}
/// 构造测试用 App，使用自定义 executor 与 tool 策略。
/// 返回 (App, project_root)，project_root 供会话持久化断言读取 JSONL。
fn test_app_with_executor(
    mock_events: Vec<ChatEvent>,
    executor: Arc<ToolExecutor>,
    tool_policy: ToolPolicyConfig,
) -> (App, std::path::PathBuf) {
    let mut app = App::new();
    let provider = Arc::new(MockProvider::new(mock_events));
    let context = Arc::new(MockContext);
    // 用独立临时目录作为项目根，避免会话 JSONL（ADR-0008）污染仓库。
    // ManuallyDrop 阻止 TempDir 析构删目录（测试进程退出后由 OS 清理）。
    let project_root = std::mem::ManuallyDrop::new(tempfile::tempdir().expect("tempdir"))
        .path()
        .to_path_buf();
    let cfg = AgentBridgeConfig {
        provider,
        executor,
        context,
        project_root: project_root.clone(),
        tool_policy,
    };
    let bridge = AgentBridge::new(cfg);
    app.add_plugins(MinimalPlugins)
        .add_plugins(XgentAgentPlugin)
        .insert_resource(bridge)
        .insert_resource(crate::provider_state::ProviderInfo {
            id: "mock".into(),
            model: "mock-model".into(),
            ready: true,
            kind: None,
        });
    (app, project_root)
}

/// 收集 ToolCall/ToolResult 消息到 Resource，供测试断言。
#[derive(Resource, Default, Debug)]
struct Collected {
    tool_calls: Vec<String>,
    tool_results: Vec<(String, bool)>,
}

/// 收集系统：读缓冲消息存入 Collected。
fn collect_messages(
    mut tc: MessageReader<ToolCallMessage>,
    mut tr: MessageReader<ToolResultMessage>,
    mut out: ResMut<Collected>,
) {
    for m in tc.read() {
        out.tool_calls.push(m.tool_id.clone());
    }
    for m in tr.read() {
        out.tool_results.push((m.tool_id.clone(), m.is_error));
    }
}

/// 测试用 echo 工具：原样返回输入。tier=Read，配置 approved 后跳过确认。
struct EchoTool;

#[async_trait::async_trait]
impl Tool for EchoTool {
    fn id(&self) -> &str {
        "echo"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "echo".into(),
            description: "回显输入".into(),
            input_schema: json!({"type":"object"}),
        }
    }
    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }
    fn concurrency(&self) -> Concurrency {
        Concurrency::Shared
    }
    fn summarize(&self, _input: &Value) -> String {
        "echo".into()
    }
    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolCtx,
        _signal: tokio_util::sync::CancellationToken,
        _on_update: Option<&ToolUpdateCallback>,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult {
            output: input.to_string(),
            is_error: false,
            side_effect: None,
        })
    }
}

#[test]
fn delta_then_done_message_sequence() {
    let mut app = test_app(vec![
        ChatEvent::TextDelta {
            text: "Hello".into(),
        },
        ChatEvent::TextDelta {
            text: " world".into(),
        },
        ChatEvent::Done {
            reason: xgent_core::chat::StopReason::Stop,
            usage: TokenUsage::default(),
        },
    ]);
    // 发起用户输入
    app.world_mut()
        .write_message(UserInputMessage { text: "hi".into() });

    // 跑若干帧让事件流转
    for _ in 0..50 {
        app.update();
    }

    let conv = app.world().resource::<crate::conversation::Conversation>();
    // 应回到 Idle，助手文本已固化
    assert_eq!(conv.status, ConversationStatus::Idle);
    // conv.messages 只存 user/assistant 轮次（system 在 build_request 时动态注入）
    assert_eq!(conv.messages.len(), 2);
    assert!(matches!(
        &conv.messages[0],
        xgent_core::chat::AgentMessage::User(u) if u.content.len() == 1
    ));
    match &conv.messages[1] {
        xgent_core::chat::AgentMessage::Assistant(a) => {
            assert_eq!(a.content.len(), 1);
            assert!(
                matches!(&a.content[0], xgent_core::chat::ContentBlock::Text { text } if text == "Hello world")
            );
        }
        _ => panic!("expected Assistant"),
    }
}

#[test]
fn error_message_propagates() {
    let mut app = test_app(vec![ChatEvent::Error {
        kind: xgent_core::chat::ErrorKind::ProviderError,
        message: "boom".into(),
    }]);
    app.world_mut()
        .write_message(UserInputMessage { text: "hi".into() });
    for _ in 0..20 {
        app.update();
    }
    let conv = app.world().resource::<crate::conversation::Conversation>();
    assert_eq!(conv.status, ConversationStatus::Error);
}

#[test]
fn busy_state_ignores_new_input() {
    let mut app = test_app(vec![
        ChatEvent::TextDelta { text: "x".into() },
        ChatEvent::Done {
            reason: xgent_core::chat::StopReason::Stop,
            usage: TokenUsage::default(),
        },
    ]);
    // 第一条输入
    app.world_mut().write_message(UserInputMessage {
        text: "first".into(),
    });
    app.update();
    // 在 Thinking/Streaming 时发第二条
    app.world_mut().write_message(UserInputMessage {
        text: "second".into(),
    });
    for _ in 0..20 {
        app.update();
    }
    let conv = app.world().resource::<crate::conversation::Conversation>();
    // 只应有一条 user 消息（第二条被忽略）
    let user_msgs: Vec<_> = conv
        .messages
        .iter()
        .filter(|m| matches!(m, xgent_core::chat::AgentMessage::User(_)))
        .collect();
    assert_eq!(user_msgs.len(), 1);
    match &user_msgs[0] {
        xgent_core::chat::AgentMessage::User(u) => {
            assert!(
                matches!(&u.content[0], xgent_core::chat::ContentBlock::Text { text } if text == "first")
            );
        }
        _ => panic!("expected User"),
    }
}

// 避免 unused 警告
#[test]
fn tool_policy_imports() {
    let _ = ToolPolicyConfig::default();
}

#[test]
fn tool_call_executes_approved_tool() {
    // 用自定义 echo 工具，配置 approved 列表含 "echo"，跳过确认流程
    let executor = Arc::new(ToolExecutor::new(vec![Arc::new(EchoTool)]));
    let policy = ToolPolicyConfig {
        approved: vec!["echo".to_string()],
        denied: vec![],
    };
    let (mut app, _project_root) = test_app_with_executor(
        vec![
            ChatEvent::ToolCallStart {
                index: 0,
                id: "call_1".into(),
                name: "echo".into(),
            },
            ChatEvent::ToolCallEnd {
                index: 0,
                args: json!({"msg": "hi"}),
            },
            ChatEvent::Done {
                reason: xgent_core::chat::StopReason::ToolUse,
                usage: TokenUsage::default(),
            },
        ],
        executor,
        policy,
    );
    app.insert_resource(Collected::default())
        .add_systems(Update, collect_messages);
    app.world_mut().write_message(UserInputMessage {
        text: "do echo".into(),
    });
    for _ in 0..50 {
        app.update();
    }
    let collected = app.world().resource::<Collected>();
    assert_eq!(collected.tool_calls, vec!["echo".to_string()]);
    assert_eq!(collected.tool_results.len(), 1);
    assert_eq!(collected.tool_results[0], ("echo".to_string(), false));
    // 对话最终回到 Idle
    let conv = app.world().resource::<crate::conversation::Conversation>();
    assert_eq!(conv.status, ConversationStatus::Idle);
}

#[test]
fn session_jsonl_persists_header_and_assistant_message() {
    // ADR-0008：会话开始 append Header，assistant Done 时 append Message entry。
    let (mut app, project_root) = test_app_with_executor(
        vec![
            ChatEvent::TextDelta { text: "hi".into() },
            ChatEvent::Done {
                reason: xgent_core::chat::StopReason::Stop,
                usage: TokenUsage::default(),
            },
        ],
        Arc::new(ToolExecutor::with_defaults()),
        ToolPolicyConfig::default(),
    );
    app.world_mut().write_message(UserInputMessage {
        text: "hello".into(),
    });
    for _ in 0..50 {
        app.update();
    }

    let conv = app.world().resource::<crate::conversation::Conversation>();
    assert_eq!(conv.status, ConversationStatus::Idle);

    // 读取 JSONL，断言包含 1 条 Header + 1 条 Assistant Message
    let path = crate::session_store::session_file_path(&project_root, &conv.id.to_string());
    assert!(path.exists(), "会话 JSONL 应存在: {:?}", path);
    let store = crate::session_store::SessionStore::open(path).expect("open");
    let entries = store.load_all().expect("load_all");
    assert_eq!(entries.len(), 2, "应包含 1 Header + 1 Message entry");

    use xgent_core::session::SessionEntry;
    assert!(
        matches!(entries[0], SessionEntry::Header(_)),
        "首条应为 Header"
    );
    match &entries[1] {
        SessionEntry::Message(m) => {
            assert!(
                matches!(m.message, xgent_core::chat::AgentMessage::Assistant(_)),
                "Message entry 应承载 Assistant 消息"
            );
            assert!(m.parent_id.is_none(), "MVP parent_id 为 None");
        }
        _ => panic!("第二条应为 Message entry"),
    }
}
