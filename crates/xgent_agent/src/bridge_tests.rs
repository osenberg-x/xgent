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
    test_app_with_retry_provider(
        Arc::new(MockProvider::new(mock_events)),
        Arc::new(ToolExecutor::with_defaults()),
        ToolPolicyConfig::default(),
        crate::bridge::RetryConfig::default(),
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
    test_app_with_retry_provider(
        Arc::new(MockProvider::new(mock_events)),
        executor,
        tool_policy,
        crate::bridge::RetryConfig::default(),
    )
}

/// 串行化测试中对 XGENT_AGENT_DIR 的设置（首次只设一次，所有测试共享同一临时目录）。
///
/// session_id 全局唯一，各测试的会话文件名不冲突，故共享目录安全，
/// 无需每测试独立 env（避免并发 set_var 互相覆盖）。
static ENV_ONCE: parking_lot::Mutex<()> = parking_lot::const_mutex(());

/// 构造测试用 App，使用自定义 provider（支持按调用返回不同事件序列）与重试配置。
fn test_app_with_retry_provider(
    provider: Arc<dyn crate::bridge::ProviderClient>,
    executor: Arc<ToolExecutor>,
    tool_policy: ToolPolicyConfig,
    retry_config: crate::bridge::RetryConfig,
) -> (App, std::path::PathBuf) {
    let mut app = App::new();
    let context = Arc::new(MockContext);
    // 用独立临时目录作为项目根。ManuallyDrop 阻止 TempDir 析构删目录
    // （测试进程退出后由 OS 清理）。
    let project_root = std::mem::ManuallyDrop::new(tempfile::tempdir().expect("tempdir"))
        .path()
        .to_path_buf();
    // 会话 JSONL 现存全局 agent_dir，测试需隔离避免污染用户全局。
    // 首次调用设 XGENT_AGENT_DIR 到进程级固定临时目录；后续调用复用。
    // session_id 唯一保证各测试文件不冲突。
    {
        let _g = ENV_ONCE.lock();
        if std::env::var("XGENT_AGENT_DIR").is_err() {
            let dir = std::mem::ManuallyDrop::new(tempfile::tempdir().expect("tempdir"))
                .path()
                .to_path_buf();
            // SAFETY: 持锁串行化；设一次后只读
            unsafe { std::env::set_var("XGENT_AGENT_DIR", dir) };
        }
    }
    let cfg = AgentBridgeConfig {
        provider,
        executor,
        context,
        project_root: project_root.clone(),
        tool_policy,
        retry_config: Arc::new(parking_lot::RwLock::new(retry_config)),
        compaction: None,
        context_window: 128_000,
        compaction_settings: crate::compaction::CompactionSettings::default(),
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
    // 设唯一 session id：全局 sessions 目录下按 id 命名，默认 id=1 会导致并发测试文件冲突
    use std::sync::atomic::{AtomicU64, Ordering};
    static SESSION_SEQ: AtomicU64 = AtomicU64::new(1);
    let unique_id = SESSION_SEQ.fetch_add(1, Ordering::SeqCst);
    {
        let mut conv = app
            .world_mut()
            .resource_mut::<crate::conversation::Conversation>();
        conv.id = xgent_core::ids::SessionId(unique_id);
    }
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
        Ok(ToolResult { output: input.to_string(), is_error: false, denied: false, side_effect: None })
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
    let (mut app, _project_root) = test_app_with_executor(
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
    // 用 SessionStore 固化的 path（避免并发测试 env 覆盖导致路径错乱）
    let path = conv
        .session_store
        .as_ref()
        .map(|s| s.path().to_path_buf())
        .expect("session_store 应已打开");
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

// —— 重试测试 ——

/// mock provider（重试版）：按调用索引返回不同事件序列。
///
/// 第 N 次调用返回 `sequences[N]`；超出则返回空 Done{Stop}。
struct RetryMockProvider {
    sequences: Vec<Vec<ChatEvent>>,
    call_count: std::sync::atomic::AtomicU32,
}

impl RetryMockProvider {
    fn new(sequences: Vec<Vec<ChatEvent>>) -> Self {
        Self {
            sequences,
            call_count: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

#[async_trait]
impl ProviderClient for RetryMockProvider {
    async fn chat(
        &self,
        _req: ChatRequest,
    ) -> Result<(StreamId, mpsc::Receiver<ChatEvent>), (xgent_core::chat::ErrorKind, String)> {
        let (tx, rx) = mpsc::channel(8);
        let n = self
            .call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let events = if (n as usize) < self.sequences.len() {
            self.sequences[n as usize].clone()
        } else {
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

/// 收集 Retry/Error/Done 消息，供重试测试断言。
#[derive(Resource, Default, Debug)]
struct RetryCollected {
    retries: Vec<u32>,
    errors: Vec<xgent_core::chat::ErrorKind>,
    done: bool,
}

fn collect_retry(
    mut r: MessageReader<RetryMessage>,
    mut e: MessageReader<ErrorMessage>,
    mut d: MessageReader<DoneMessage>,
    mut out: ResMut<RetryCollected>,
) {
    for m in r.read() {
        out.retries.push(m.attempt);
    }
    for m in e.read() {
        out.errors.push(m.kind);
    }
    for _ in d.read() {
        out.done = true;
    }
}

/// 极小 delay 的重试配置，避免测试拖慢。
fn fast_retry_config(max_retries: Option<u32>) -> crate::bridge::RetryConfig {
    crate::bridge::RetryConfig {
        max_retries,
        mode: xgent_settings_core::global::RetryMode::Fixed,
        initial_delay_ms: 1,
        max_delay_ms: 10,
        backoff_factor: 2.0,
    }
}

#[test]
fn retryable_error_retries_then_succeeds() {
    // 第 0 次：Network 错误（可重试）；第 1 次：成功文本
    let provider = Arc::new(RetryMockProvider::new(vec![
        vec![ChatEvent::Error {
            kind: xgent_core::chat::ErrorKind::Network,
            message: "conn reset".into(),
        }],
        vec![
            ChatEvent::TextDelta {
                text: "recovered".into(),
            },
            ChatEvent::Done {
                reason: xgent_core::chat::StopReason::Stop,
                usage: TokenUsage::default(),
            },
        ],
    ]));
    let (mut app, _root) = test_app_with_retry_provider(
        provider as Arc<dyn crate::bridge::ProviderClient>,
        Arc::new(ToolExecutor::with_defaults()),
        ToolPolicyConfig::default(),
        fast_retry_config(Some(3)),
    );
    app.insert_resource(RetryCollected::default())
        .add_systems(Update, collect_retry);
    app.world_mut()
        .write_message(UserInputMessage { text: "hi".into() });
    for _ in 0..80 {
        app.update();
    }

    let conv = app.world().resource::<crate::conversation::Conversation>();
    assert_eq!(
        conv.status,
        ConversationStatus::Idle,
        "重试成功后应回到 Idle"
    );
    // 助手文本已固化进 messages（current_assistant_text 被 finalize 清空）
    let assistant_text = conv
        .messages
        .iter()
        .find_map(|m| match m {
            xgent_core::chat::AgentMessage::Assistant(a) => {
                a.content.iter().find_map(|b| match b {
                    xgent_core::chat::ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
            }
            _ => None,
        })
        .unwrap_or_default();
    assert_eq!(assistant_text, "recovered", "助手最终文本应为成功内容");
    let collected = app.world().resource::<RetryCollected>();
    assert_eq!(collected.retries, vec![1], "应有一次重试通知");
    assert!(collected.errors.is_empty(), "不应有最终错误");
    assert!(collected.done, "应有 Done");
}

#[test]
fn non_retryable_error_fails_immediately() {
    // AuthFailed 不可重试：立即失败，无重试通知
    let provider = Arc::new(RetryMockProvider::new(vec![vec![ChatEvent::Error {
        kind: xgent_core::chat::ErrorKind::AuthFailed,
        message: "bad key".into(),
    }]]));
    let (mut app, _root) = test_app_with_retry_provider(
        provider as Arc<dyn crate::bridge::ProviderClient>,
        Arc::new(ToolExecutor::with_defaults()),
        ToolPolicyConfig::default(),
        fast_retry_config(Some(3)),
    );
    app.insert_resource(RetryCollected::default())
        .add_systems(Update, collect_retry);
    app.world_mut()
        .write_message(UserInputMessage { text: "hi".into() });
    for _ in 0..40 {
        app.update();
    }

    let conv = app.world().resource::<crate::conversation::Conversation>();
    assert_eq!(conv.status, ConversationStatus::Error, "应处于 Error 态");
    let collected = app.world().resource::<RetryCollected>();
    assert!(collected.retries.is_empty(), "不可重试错误不应触发重试");
    assert_eq!(collected.errors.len(), 1, "应有一条错误");
    assert_eq!(collected.errors[0], xgent_core::chat::ErrorKind::AuthFailed);
}

#[test]
fn infinite_retry_can_be_aborted() {
    // 无限重试：每次都 Network 错误，直到用户中断
    let provider = Arc::new(RetryMockProvider::new(vec![vec![ChatEvent::Error {
        kind: xgent_core::chat::ErrorKind::Network,
        message: "always fail".into(),
    }]]));
    let (mut app, _root) = test_app_with_retry_provider(
        provider as Arc<dyn crate::bridge::ProviderClient>,
        Arc::new(ToolExecutor::with_defaults()),
        ToolPolicyConfig::default(),
        fast_retry_config(None), // None = 无限重试
    );
    app.insert_resource(RetryCollected::default())
        .add_systems(Update, collect_retry);
    app.world_mut()
        .write_message(UserInputMessage { text: "hi".into() });
    // 让首次失败 + 若干次重试发生
    for _ in 0..30 {
        app.update();
    }
    // 断言确实在重试（有重试通知，无最终错误）
    {
        let collected = app.world().resource::<RetryCollected>();
        assert!(!collected.retries.is_empty(), "应已触发重试");
        assert!(collected.errors.is_empty(), "无限重试不应产生最终错误");
    }
    // 发中断
    app.world_mut().write_message(AbortMessage);
    for _ in 0..80 {
        app.update();
    }

    let conv = app.world().resource::<crate::conversation::Conversation>();
    // 中断后不应停留在 Error（被 abort 终止，返回空 Done → Idle）
    assert_ne!(
        conv.status,
        ConversationStatus::Error,
        "中断后不应是 Error 态"
    );
    let collected = app.world().resource::<RetryCollected>();
    assert!(collected.done, "中断应产生 Done");
    assert!(collected.errors.is_empty(), "中断不应产生错误");
}

// ===========================================================================
// Compaction 与 Steering 集成测试
// ===========================================================================

/// mock compactor：不调 LLM，直接把前半消息摘要为固定文本。
struct MockCompactor;

#[async_trait]
impl crate::compaction::CompactionProvider for MockCompactor {
    fn should_compact(&self, _messages: &[xgent_core::chat::AgentMessage], _model: &str) -> bool {
        true
    }
    async fn compact(
        &self,
        messages: &[xgent_core::chat::AgentMessage],
        _model: &str,
    ) -> Result<crate::compaction::CompactionResult, crate::compaction::CompactionError> {
        // 保留后半，前半摘要为固定文本
        let cut = messages.len() / 2;
        let kept = messages[cut..].to_vec();
        Ok(crate::compaction::CompactionResult {
            summary: "[mock summary]".into(),
            kept_messages: kept,
            tokens_before: 9999,
        })
    }
}

/// 构造带 compaction 的测试 App（小 context_window 触发压缩）。
fn test_app_with_compaction(provider: Arc<dyn crate::bridge::ProviderClient>) -> App {
    let mut app = App::new();
    let context = Arc::new(MockContext);
    let project_root = std::mem::ManuallyDrop::new(tempfile::tempdir().expect("tempdir"))
        .path()
        .to_path_buf();
    {
        let _g = ENV_ONCE.lock();
        if std::env::var("XGENT_AGENT_DIR").is_err() {
            let dir = std::mem::ManuallyDrop::new(tempfile::tempdir().expect("tempdir"))
                .path()
                .to_path_buf();
            unsafe { std::env::set_var("XGENT_AGENT_DIR", dir) };
        }
    }
    let compactor: Arc<dyn crate::compaction::CompactionProvider> = Arc::new(MockCompactor);
    let cfg = AgentBridgeConfig {
        provider,
        executor: Arc::new(ToolExecutor::with_defaults()),
        context,
        project_root: project_root.clone(),
        tool_policy: ToolPolicyConfig::default(),
        retry_config: Arc::new(parking_lot::RwLock::new(
            crate::bridge::RetryConfig::default(),
        )),
        compaction: Some(compactor),
        // 极小窗口 + 默认 80% 阈值 → 8 token 即触发
        context_window: 10,
        compaction_settings: crate::compaction::CompactionSettings::default(),
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
    use std::sync::atomic::{AtomicU64, Ordering};
    static SESSION_SEQ_COMPACT: AtomicU64 = AtomicU64::new(1000);
    let unique_id = SESSION_SEQ_COMPACT.fetch_add(1, Ordering::SeqCst);
    {
        let mut conv = app
            .world_mut()
            .resource_mut::<crate::conversation::Conversation>();
        conv.id = xgent_core::ids::SessionId(unique_id);
    }
    app
}

/// 收集 CompactedMessage 的 Resource。
#[derive(Resource, Default, Debug)]
struct CompactedCollected {
    count: u32,
    last_before: u32,
    last_after: u32,
}

fn collect_compacted(mut c: MessageReader<CompactedMessage>, mut out: ResMut<CompactedCollected>) {
    for m in c.read() {
        out.count += 1;
        out.last_before = m.tokens_before;
        out.last_after = m.tokens_after;
    }
}

#[test]
fn compaction_triggers_when_over_threshold() {
    // 用量 prompt=9999（远超窗口 10 的 80% 阈值=8）→ 必触发
    let provider = Arc::new(MockProvider::new(vec![
        ChatEvent::TextDelta {
            text: "hello".into(),
        },
        ChatEvent::Done {
            reason: xgent_core::chat::StopReason::Stop,
            usage: TokenUsage {
                prompt: 9999,
                completion: 1,
            },
        },
    ]));
    let mut app = test_app_with_compaction(provider as Arc<dyn crate::bridge::ProviderClient>);
    app.insert_resource(CompactedCollected::default())
        .add_systems(Update, collect_compacted);
    app.world_mut()
        .write_message(UserInputMessage { text: "hi".into() });
    for _ in 0..50 {
        app.update();
    }
    let collected = app.world().resource::<CompactedCollected>();
    assert!(collected.count >= 1, "compaction 应至少触发一次");
    assert_eq!(collected.last_before, 9999);
}

/// 流式 mock provider：发首个 delta 后暂停等待 steering 信号，再继续。
/// 用于测试流式期间 steering 即时中断。
struct StreamingSteerMockProvider {
    /// 收到 steering 后发 Done 的信号
    steer_seen: Arc<parking_lot::Mutex<bool>>,
}

#[async_trait]
impl ProviderClient for StreamingSteerMockProvider {
    async fn chat(
        &self,
        _req: ChatRequest,
    ) -> Result<(StreamId, mpsc::Receiver<ChatEvent>), (xgent_core::chat::ErrorKind, String)> {
        let (tx, rx) = mpsc::channel(8);
        let steer_seen = self.steer_seen.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                // 发首个 delta
                let _ = tx
                    .send(ChatEvent::TextDelta {
                        text: "partial".into(),
                    })
                    .await;
                // 轮询等待 steering 信号（最多 2 秒）
                for _ in 0..200 {
                    {
                        let s = steer_seen.lock();
                        if *s {
                            break;
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                // 发 Done（无论是否中断，流都会被 steering 中断）
                let _ = tx
                    .send(ChatEvent::Done {
                        reason: xgent_core::chat::StopReason::Stop,
                        usage: TokenUsage {
                            prompt: 5,
                            completion: 1,
                        },
                    })
                    .await;
            });
        });
        Ok((StreamId(1), rx))
    }
}

#[test]
fn steering_interrupts_streaming_and_continues() {
    let steer_seen = Arc::new(parking_lot::Mutex::new(false));
    let provider = Arc::new(StreamingSteerMockProvider {
        steer_seen: steer_seen.clone(),
    });
    let mut app = test_app_with_retry_provider(
        provider as Arc<dyn crate::bridge::ProviderClient>,
        Arc::new(ToolExecutor::with_defaults()),
        ToolPolicyConfig::default(),
        crate::bridge::RetryConfig::default(),
    )
    .0;
    app.world_mut()
        .write_message(UserInputMessage { text: "hi".into() });
    // 跑几帧让流式开始
    for _ in 0..5 {
        app.update();
    }
    // 发 steering：应即时中断当前流
    app.world_mut().write_message(SteeringMessage {
        text: "wait stop".into(),
    });
    *steer_seen.lock() = true;
    // 继续跑帧让对话完成
    for _ in 0..80 {
        app.update();
    }
    let conv = app.world().resource::<crate::conversation::Conversation>();
    // 对话应回到 Idle（steering 中断后重新流式，最终 Done）
    assert_eq!(
        conv.status,
        ConversationStatus::Idle,
        "steering 中断后对话应正常完成"
    );
    // messages 应含 steering 文本（注入后重新流式）
    let has_steer = conv.messages.iter().any(|m| match m {
        xgent_core::chat::AgentMessage::User(u) => {
            u.content.iter().any(|b| matches!(b, xgent_core::chat::ContentBlock::Text { text } if text.contains("wait stop")))
        }
        _ => false,
    });
    assert!(has_steer, "steering 文本应注入到对话历史");
}
