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
use xgent_tools::tool::{SecurityPolicy, Tool, ToolCtx, ToolResult};

use crate::XgentAgentPlugin;
use crate::bridge::{AgentBridge, AgentBridgeConfig, ProviderClient};
use crate::conversation::ConversationStatus;
use crate::events::*;
use xgent_settings_core::project::ToolPolicyConfig;

/// mock provider：按预设事件序列输出。
struct MockProvider {
    events: Vec<ChatEvent>,
}

#[async_trait]
impl ProviderClient for MockProvider {
    async fn chat(
        &self,
        _req: ChatRequest,
    ) -> Result<(StreamId, mpsc::Receiver<ChatEvent>), String> {
        let (tx, rx) = mpsc::channel(8);
        let events = self.events.clone();
        tokio::spawn(async move {
            for ev in events {
                if tx.send(ev).await.is_err() {
                    break;
                }
            }
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

/// 构造测试用 App，注入 mock provider/executor/context。
fn test_app(mock_events: Vec<ChatEvent>) -> App {
    test_app_with_executor(mock_events, Arc::new(ToolExecutor::with_defaults()))
}

/// 构造测试用 App，使用自定义 executor。
fn test_app_with_executor(mock_events: Vec<ChatEvent>, executor: Arc<ToolExecutor>) -> App {
    let mut app = App::new();
    let provider = Arc::new(MockProvider {
        events: mock_events,
    });
    let context = Arc::new(MockContext);
    let cfg = AgentBridgeConfig {
        provider,
        executor,
        context,
        project_root: std::path::PathBuf::from("."),
    };
    let bridge = AgentBridge::new(cfg);
    app.add_plugins(MinimalPlugins)
        .add_plugins(XgentAgentPlugin)
        .insert_resource(bridge)
        .insert_resource(crate::provider_state::ProviderInfo {
            id: "mock".into(),
            model: "mock-model".into(),
        });
    // 显式设默认 tool_policy 避免确认阻塞
    app
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
        out.tool_results.push((m.tool_id.clone(), m.success));
    }
}

/// 测试用 Approved 策略的 echo 工具：原样返回输入。
struct EchoTool;

#[async_trait]
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
    fn policy(&self) -> SecurityPolicy {
        SecurityPolicy::Approved
    }
    fn summarize(&self, _input: &Value) -> String {
        "echo".into()
    }
    async fn execute(&self, input: Value, _ctx: &ToolCtx) -> ToolResult {
        ToolResult {
            output: input.to_string(),
            success: true,
            side_effect: None,
        }
    }
}

#[test]
fn delta_then_done_message_sequence() {
    let mut app = test_app(vec![
        ChatEvent::Delta {
            text: "Hello".into(),
        },
        ChatEvent::Delta {
            text: " world".into(),
        },
        ChatEvent::Done {
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
    assert_eq!(conv.current_assistant_text, "");
    // conv.messages 只存 user/assistant 轮次（system 在 build_request 时动态注入）
    assert_eq!(conv.messages.len(), 2);
    assert_eq!(conv.messages[0].role, xgent_core::chat::Role::User);
    assert_eq!(conv.messages[1].content, "Hello world");
    assert_eq!(conv.messages[1].role, xgent_core::chat::Role::Assistant);
}

#[test]
fn error_message_propagates() {
    let mut app = test_app(vec![ChatEvent::Error {
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
        ChatEvent::Delta { text: "x".into() },
        ChatEvent::Done {
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
        .filter(|m| m.role == xgent_core::chat::Role::User)
        .collect();
    assert_eq!(user_msgs.len(), 1);
    assert_eq!(user_msgs[0].content, "first");
}

// 避免 unused 警告
#[test]
fn tool_policy_imports() {
    let _ = ToolPolicyConfig::default();
}

#[test]
fn tool_call_executes_approved_tool() {
    // 用自定义 echo 工具（Approved 策略），跳过确认流程
    let executor = Arc::new(ToolExecutor::new(vec![Arc::new(EchoTool)]));
    let mut app = test_app_with_executor(
        vec![
            ChatEvent::ToolCall {
                id: "call_1".into(),
                name: "echo".into(),
                args: json!({"msg": "hi"}),
            },
            ChatEvent::Done {
                usage: TokenUsage::default(),
            },
        ],
        executor,
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
    assert_eq!(collected.tool_results[0], ("echo".to_string(), true));
    // 对话最终回到 Idle
    let conv = app.world().resource::<crate::conversation::Conversation>();
    assert_eq!(conv.status, ConversationStatus::Idle);
}
