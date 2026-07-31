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
        .write_message(UserInputMessage { text: "hi".into(), editor_queries: Vec::new() });

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
        .write_message(UserInputMessage { text: "hi".into(), editor_queries: Vec::new() });
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
    app.world_mut().write_message(UserInputMessage { text: "first".into(), editor_queries: Vec::new() });
    app.update();
    // 在 Thinking/Streaming 时发第二条
    app.world_mut().write_message(UserInputMessage { text: "second".into(), editor_queries: Vec::new() });
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
    app.world_mut().write_message(UserInputMessage { text: "do echo".into(), editor_queries: Vec::new() });
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
    app.world_mut().write_message(UserInputMessage { text: "hello".into(), editor_queries: Vec::new() });
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
        .write_message(UserInputMessage { text: "hi".into(), editor_queries: Vec::new() });
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
        .write_message(UserInputMessage { text: "hi".into(), editor_queries: Vec::new() });
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
        .write_message(UserInputMessage { text: "hi".into(), editor_queries: Vec::new() });
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
        .write_message(UserInputMessage { text: "hi".into(), editor_queries: Vec::new() });
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
        .write_message(UserInputMessage { text: "hi".into(), editor_queries: Vec::new() });
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


/// 验证 compaction 后 system prompt 仍保留在 req.messages 中。
///
/// 修复前：maybe_compact 把 system 消息映射为 User 并混入摘要，
/// 压缩后 system prompt 丢失，后续请求缺失系统指令。
/// 修复后：system 在压缩前分离，压缩后重新前置。
///
/// 通过发 FollowUp 触发第二次 chat 调用（压缩后的请求），断言首条仍为 system。
#[test]
fn compaction_preserves_system_prompt_in_req() {
    use parking_lot::Mutex;
    struct CapturingProvider {
        captured: Arc<Mutex<Vec<ChatRequest>>>,
        /// 第二次及以后返回无 tool_calls 的短回复，避免无限 compaction
        call_count: std::sync::atomic::AtomicU32,
    }
    #[async_trait]
    impl ProviderClient for CapturingProvider {
        async fn chat(
            &self,
            req: ChatRequest,
        ) -> Result<(StreamId, mpsc::Receiver<ChatEvent>), (xgent_core::chat::ErrorKind, String)> {
            self.captured.lock().push(req.clone());
            let (tx, rx) = mpsc::channel(8);
            let n = self
                .call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // 首次 prompt=9999 触发 compaction；后续 prompt=1 不触发
            let usage = if n == 0 {
                TokenUsage { prompt: 9999, completion: 1 }
            } else {
                TokenUsage { prompt: 1, completion: 1 }
            };
            let events = vec![
                ChatEvent::TextDelta { text: "hi".into() },
                ChatEvent::Done {
                    reason: xgent_core::chat::StopReason::Stop,
                    usage,
                },
            ];
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async move {
                    for ev in events {
                        if tx.send(ev).await.is_err() { break; }
                    }
                });
            });
            Ok((StreamId(1), rx))
        }
    }
    let captured: Arc<Mutex<Vec<ChatRequest>>> = Arc::new(Mutex::new(vec![]));
    let provider = Arc::new(CapturingProvider {
        captured: captured.clone(),
        call_count: std::sync::atomic::AtomicU32::new(0),
    });
    let mut app = test_app_with_compaction(provider as Arc<dyn crate::bridge::ProviderClient>);
    app.insert_resource(CompactedCollected::default())
        .add_systems(Update, collect_compacted);
    app.world_mut()
        .write_message(UserInputMessage { text: "hi".into(), editor_queries: Vec::new() });
    // 跑帧让首次对话完成（含 compaction 触发）→ 回到 Idle
    for _ in 0..60 {
        app.update();
    }
    // 确认 compaction 已触发
    {
        let collected = app.world().resource::<CompactedCollected>();
        assert!(collected.count >= 1, "compaction 应触发");
    }
    // 发 FollowUp 触发第二次 chat 调用（压缩后的 req）
    app.world_mut()
        .write_message(FollowUpMessage { text: "more".into() });
    for _ in 0..80 {
        app.update();
    }
    // 第二次 chat 调用的首条消息应为 system
    let reqs = captured.lock();
    assert!(
        reqs.len() >= 2,
        "应至少有两次 chat 调用（首次 + FollowUp 后），实际 {}",
        reqs.len()
    );
    let post_compact = &reqs[1];
    assert_eq!(
        post_compact.messages[0].role,
        xgent_core::chat::Role::System,
        "压缩后 req 首条消息必须是 system（修复前会丢失）"
    );
    // system 文本不应为空
    let system_text = post_compact.messages[0]
        .content
        .iter()
        .find_map(|b| match b {
            xgent_core::chat::ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default();
    assert!(!system_text.is_empty(), "system prompt 文本不应为空");
}

/// 验证 compaction 后 conv.messages 与 req 同步（修复前下次 StartLoop 会丢失压缩）。
#[test]
fn compaction_syncs_conv_messages() {
    let provider = Arc::new(MockProvider::new(vec![
        ChatEvent::TextDelta { text: "hi".into() },
        ChatEvent::Done {
            reason: xgent_core::chat::StopReason::Stop,
            usage: TokenUsage { prompt: 9999, completion: 1 },
        },
    ]));
    let mut app = test_app_with_compaction(provider as Arc<dyn crate::bridge::ProviderClient>);
    app.insert_resource(CompactedCollected::default())
        .add_systems(Update, collect_compacted);
    app.world_mut()
        .write_message(UserInputMessage { text: "hi".into(), editor_queries: Vec::new() });
    for _ in 0..60 {
        app.update();
    }
    let collected = app.world().resource::<CompactedCollected>();
    assert!(collected.count >= 1, "compaction 应触发");
    // 压缩后 conv.messages 应被替换为压缩后的消息（含 summary 前置）
    let conv = app.world().resource::<crate::conversation::Conversation>();
    let has_summary = conv.messages.iter().any(|m| match m {
        xgent_core::chat::AgentMessage::User(u) => {
            u.content.iter().any(|b| matches!(b, xgent_core::chat::ContentBlock::Text { text } if text.contains("前序对话摘要")))
        }
        _ => false,
    });
    assert!(
        has_summary,
        "conv.messages 应含压缩后的 summary 前置消息（修复前 conv 未同步）"
    );
}

/// 验证 Length 截断时 conv.messages 中 tool_call 与 tool_result 配对完整。
///
/// 修复前：Length 路径只发 ToolResult 事件不发 ToolCall 事件，
/// conv.messages 得到孤儿 tool_result，下次 StartLoop 时 convert_to_llm
/// 生成无配对 tool_call 的 tool 消息，OpenAI 会 400 拒绝。
/// 修复后：Length 路径先发 ToolCall 再发 ToolResult，conv.messages 配对完整。
#[test]
fn length_truncation_pairs_tool_call_and_result_in_conv() {
    let provider = Arc::new(RetryMockProvider::new(vec![
        vec![
            ChatEvent::ToolCallStart {
                index: 0,
                id: "call_len".into(),
                name: "echo".into(),
            },
            ChatEvent::ToolCallEnd {
                index: 0,
                args: json!({"msg": "partial"}),
            },
            ChatEvent::Done {
                reason: xgent_core::chat::StopReason::Length,
                usage: TokenUsage::default(),
            },
        ],
        vec![
            ChatEvent::TextDelta { text: "done".into() },
            ChatEvent::Done {
                reason: xgent_core::chat::StopReason::Stop,
                usage: TokenUsage::default(),
            },
        ],
    ]));
    let executor = Arc::new(ToolExecutor::new(vec![Arc::new(EchoTool)]));
    let policy = ToolPolicyConfig {
        approved: vec!["echo".to_string()],
        denied: vec![],
    };
    let (mut app, _root) = test_app_with_retry_provider(
        provider as Arc<dyn crate::bridge::ProviderClient>,
        executor,
        policy,
        crate::bridge::RetryConfig::default(),
    );
    app.insert_resource(Collected::default())
        .add_systems(Update, collect_messages);
    app.world_mut()
        .write_message(UserInputMessage { text: "do echo".into(), editor_queries: Vec::new() });
    for _ in 0..80 {
        app.update();
    }
    let conv = app.world().resource::<crate::conversation::Conversation>();
    assert_eq!(conv.status, ConversationStatus::Idle, "应回到 Idle");
    // conv.messages 中 assistant ToolCall 与 ToolResult 应配对
    let mut tool_calls = 0;
    let mut tool_results = 0;
    for m in &conv.messages {
        match m {
            xgent_core::chat::AgentMessage::Assistant(a) => {
                if a.content.iter().any(|b| matches!(b, xgent_core::chat::ContentBlock::ToolCall { .. })) {
                    tool_calls += 1;
                }
            }
            xgent_core::chat::AgentMessage::ToolResult(_) => tool_results += 1,
            _ => {}
        }
    }
    assert_eq!(
        tool_calls, tool_results,
        "conv.messages 中 tool_call({}) 与 tool_result({}) 应配对（修复前 Length 路径孤儿 tool_result）",
        tool_calls,
        tool_results
    );
    assert!(tool_calls >= 1, "应至少有一对 tool_call/tool_result");
}

/// 验证 LLM 同时返回文本+tool_calls 时，assistant 文本回灌到 req.messages。
///
/// 修复前：tool 执行分支回灌的 assistant 消息只含 ToolCall 块，
/// LLM 同时生成的推理文本丢失，下次调用看不到本轮推理上下文。
/// 修复后：首个 tool_call 的 assistant 消息携带文本块。
#[test]
fn tool_call_with_text_preserves_assistant_text_in_req() {
    use parking_lot::Mutex;
    struct CapturingProvider {
        captured: Arc<Mutex<Vec<ChatRequest>>>,
    }
    #[async_trait]
    impl ProviderClient for CapturingProvider {
        async fn chat(
            &self,
            req: ChatRequest,
        ) -> Result<(StreamId, mpsc::Receiver<ChatEvent>), (xgent_core::chat::ErrorKind, String)> {
            self.captured.lock().push(req.clone());
            let (tx, rx) = mpsc::channel(8);
            // 首次：文本 + tool_call；后续：无 tool_call 正常停止
            let n = self.captured.lock().len() - 1;
            let events = if n == 0 {
                vec![
                    ChatEvent::TextDelta { text: "Let me read that file.".into() },
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
                ]
            } else {
                vec![
                    ChatEvent::TextDelta { text: "done".into() },
                    ChatEvent::Done {
                        reason: xgent_core::chat::StopReason::Stop,
                        usage: TokenUsage::default(),
                    },
                ]
            };
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async move {
                    for ev in events {
                        if tx.send(ev).await.is_err() { break; }
                    }
                });
            });
            Ok((StreamId(1), rx))
        }
    }
    let captured: Arc<Mutex<Vec<ChatRequest>>> = Arc::new(Mutex::new(vec![]));
    let provider = Arc::new(CapturingProvider { captured: captured.clone() });
    let executor = Arc::new(ToolExecutor::new(vec![Arc::new(EchoTool)]));
    let policy = ToolPolicyConfig {
        approved: vec!["echo".to_string()],
        denied: vec![],
    };
    let (mut app, _root) = test_app_with_retry_provider(
        provider as Arc<dyn crate::bridge::ProviderClient>,
        executor,
        policy,
        crate::bridge::RetryConfig::default(),
    );
    app.world_mut()
        .write_message(UserInputMessage { text: "do echo".into(), editor_queries: Vec::new() });
    for _ in 0..80 {
        app.update();
    }
    let conv = app.world().resource::<crate::conversation::Conversation>();
    assert_eq!(conv.status, ConversationStatus::Idle, "应回到 Idle");
    // 第二次 chat 调用的 req.messages 应含 assistant 文本（"Let me read that file."）
    let reqs = captured.lock();
    assert!(
        reqs.len() >= 2,
        "应至少两次 chat 调用，实际 {}",
        reqs.len()
    );
    let second_req = &reqs[1];
    // 在第二次请求的消息历史中查找 assistant 消息含该文本
    let has_text = second_req.messages.iter().any(|m| {
        m.role == xgent_core::chat::Role::Assistant
            && m.content.iter().any(|b| {
                matches!(b, xgent_core::chat::ContentBlock::Text { text }
                    if text.contains("Let me read that file."))
            })
    });
    assert!(
        has_text,
        "第二次请求应包含首轮 assistant 文本（修复前 tool 执行分支丢失文本）"
    );
    // 同时验证保留原始 args（非 Null）
    let has_real_args = second_req.messages.iter().any(|m| {
        m.role == xgent_core::chat::Role::Assistant
            && m.content.iter().any(|b| match b {
                xgent_core::chat::ContentBlock::ToolCall { args, .. } => !args.is_null(),
                _ => false,
            })
    });
    assert!(
        has_real_args,
        "第二次请求的 tool_call 应保留原始 args（修复前用 Null）"
    );
}

/// 验证 Abort 后同一 App 上的新对话仍能正常流式（cancel_token 不复用）。
///
/// 修复前：cancel_token 在 agent_loop_task 顶层创建一次，Abort 后该 token
/// 永久处于已取消态。第二次 StartLoop 传入同一 token，stream_llm_response
/// 的 select! 立即命中 cancel_token.cancelled() 分支，导致 agent 永久无法流式。
/// 修复后：每次 StartLoop 创建独立 cancel_token。
#[test]
fn abort_then_new_conversation_still_streams() {
    // 首次对话发一个 delta 然后用户 abort；第二次对话发 delta + Done
    //
    // 用专用 mock：第一次发 delta 后保持 channel 开启（sleep），让 abort 有机会
    // 经 cancel_token 中断 stream_llm_response 的 select!，而非依赖 stream 结束。
    // 修复前：cancel_token 在 agent_loop_task 顶层创建一次，Abort 后该 token
    // 永久处于已取消态。第二次 StartLoop 传入同一 token，stream_llm_response
    // 的 select! 立即命中 cancel_token.cancelled() 分支，导致 agent 永久无法流式。
    // 修复后：每次 StartLoop 创建独立 cancel_token。

    struct AbortTestProvider {
        sequences: Vec<Vec<ChatEvent>>,
        call_count: std::sync::atomic::AtomicU32,
    }
    impl AbortTestProvider {
        fn new(sequences: Vec<Vec<ChatEvent>>) -> Self {
            Self {
                sequences,
                call_count: std::sync::atomic::AtomicU32::new(0),
            }
        }
    }
    #[async_trait]
    impl ProviderClient for AbortTestProvider {
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
                    for ev in &events {
                        if tx.send(ev.clone()).await.is_err() {
                            break;
                        }
                    }
                    // 首次对话：发完 delta 后保持 channel 开启 1s，
                    // 让 abort 有机会 cancel_token 中断 select!（而非 stream None）
                    if n == 0 {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                    // drop tx：若未被 abort，stream None 到达触发 StreamParse；
                    // 若已被 abort，cancel_token 分支已 return，None 无影响。
                });
            });
            Ok((StreamId(1), rx))
        }
    }

    let provider = Arc::new(AbortTestProvider::new(vec![
        // 首次：只发 delta（模拟流式中被中断），无 Done
        vec![ChatEvent::TextDelta { text: "partial".into() }],
        // 第二次：正常文本 + Done
        vec![
            ChatEvent::TextDelta { text: "second reply".into() },
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
        crate::bridge::RetryConfig::default(),
    );

    // 第一次对话
    app.world_mut()
        .write_message(UserInputMessage { text: "first".into(), editor_queries: Vec::new() });
    // 让流式开始（delta 到达）
    for _ in 0..10 {
        app.update();
    }
    // 发 abort
    app.world_mut().write_message(AbortMessage);
    for _ in 0..30 {
        app.update();
    }
    // 第一次对话应被中断（回到 Idle）
    {
        let conv = app.world().resource::<crate::conversation::Conversation>();
        assert_ne!(
            conv.status,
            ConversationStatus::Streaming,
            "首次对话应被 abort 中断"
        );
    }

    // 第二次对话（同一 App）
    app.world_mut()
        .write_message(UserInputMessage { text: "second".into(), editor_queries: Vec::new() });
    for _ in 0..80 {
        app.update();
    }

    let conv = app.world().resource::<crate::conversation::Conversation>();
    // 修复前：cancel_token 复用导致第二次对话立即 abort，status 不是 Idle
    // 而是 Aborting 或卡在中间态；assistant_text 为空（流式被立即中断）
    assert_eq!(
        conv.status,
        ConversationStatus::Idle,
        "第二次对话应正常完成回到 Idle（修复前 cancel_token 复用导致永久无法流式）"
    );
    // 第二次对话应固化 "second reply" 文本
    let assistant_text = conv
        .messages
        .iter()
        .rev()
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
    assert_eq!(
        assistant_text, "second reply",
        "第二次对话应固化 \"second reply\"（修复前流式被立即中断，文本为空）"
    );
}

/// 验证 ConfirmDecision 后状态为 ToolRunning（修复前为 Streaming）。
///
/// 修复前：agent_poll_system 第 131 行在 ConfirmDecision 后设 status=Streaming，
/// 但实际工具仍在执行（executor.execute 刚从 confirm 恢复），UI 误显示「生成中」
/// 而非「执行工具中」。修复后改为 ToolRunning。
#[test]
fn confirm_decision_sets_tool_running_status() {
    // 用未批准的 echo 工具（默认 NeedsConfirmation）触发确认流程
    let executor = Arc::new(ToolExecutor::new(vec![Arc::new(EchoTool)]));
    let policy = ToolPolicyConfig::default(); // 无 approved → NeedsConfirmation
    let (mut app, _project_root) = test_app_with_executor(
        vec![
            ChatEvent::ToolCallStart {
                index: 0,
                id: "call_c".into(),
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
            // 第二次（工具执行后）：无 tool_calls，正常停止
            ChatEvent::TextDelta { text: "done".into() },
            ChatEvent::Done {
                reason: xgent_core::chat::StopReason::Stop,
                usage: TokenUsage::default(),
            },
        ],
        executor,
        policy,
    );
    app.insert_resource(Collected::default())
        .add_systems(Update, collect_messages);
    app.world_mut()
        .write_message(UserInputMessage { text: "do echo".into(), editor_queries: Vec::new() });

    // 跑帧让 tool_call 到达 + ConfirmRequest 弹出
    for _ in 0..30 {
        app.update();
    }

    // 此时状态应为 Confirming（等待用户确认）
    {
        let conv = app.world().resource::<crate::conversation::Conversation>();
        assert_eq!(
            conv.status,
            ConversationStatus::Confirming,
            "应处于 Confirming 态等待确认"
        );
    }

    // 用户批准
    app.world_mut().write_message(ConfirmDecisionMessage {
        decision: xgent_tools::confirm::ConfirmDecision::Allow,
    });
    app.update();

    // 批准后状态应为 ToolRunning（修复前为 Streaming）
    {
        let conv = app.world().resource::<crate::conversation::Conversation>();
        assert_eq!(
            conv.status,
            ConversationStatus::ToolRunning,
            "确认后应为 ToolRunning（修复前误为 Streaming）"
        );
    }

    // 跑完剩余帧让对话完成
    for _ in 0..80 {
        app.update();
    }
    let conv = app.world().resource::<crate::conversation::Conversation>();
    assert_eq!(conv.status, ConversationStatus::Idle, "最终应回到 Idle");
    let collected = app.world().resource::<Collected>();
    assert_eq!(collected.tool_calls, vec!["echo".to_string()]);
    assert_eq!(collected.tool_results.len(), 1);
    assert_eq!(collected.tool_results[0], ("echo".to_string(), false));
}

/// 验证一轮多个 tool_calls 回灌为单条 assistant 消息（修复 OpenAI 协议破坏）。
///
/// 修复前：每个 tool_call 拆成独立 assistant ChatMessage，产生
/// `assistant(tc1) → tool(r1) → assistant(tc2) → tool(r2)`，
/// 严格 OpenAI endpoint 会 400（assistant 后未紧跟对应 tool 消息）。
/// 修复后：`assistant(text + tc1 + tc2) → tool(r1) → tool(r2)`。
#[test]
fn multi_tool_calls_single_assistant_message() {
    use parking_lot::Mutex;
    struct CapturingProvider {
        captured: Arc<Mutex<Vec<ChatRequest>>>,
    }
    #[async_trait]
    impl ProviderClient for CapturingProvider {
        async fn chat(
            &self,
            req: ChatRequest,
        ) -> Result<(StreamId, mpsc::Receiver<ChatEvent>), (xgent_core::chat::ErrorKind, String)> {
            self.captured.lock().push(req.clone());
            let (tx, rx) = mpsc::channel(8);
            let n = self.captured.lock().len() - 1;
            // 首次：两个并行 tool_calls（index 0 + 1）；后续：正常停止
            let events = if n == 0 {
                vec![
                    ChatEvent::TextDelta { text: "Let me run two tools.".into() },
                    ChatEvent::ToolCallStart {
                        index: 0,
                        id: "call_a".into(),
                        name: "echo".into(),
                    },
                    ChatEvent::ToolCallEnd {
                        index: 0,
                        args: json!({"msg": "first"}),
                    },
                    ChatEvent::ToolCallStart {
                        index: 1,
                        id: "call_b".into(),
                        name: "echo".into(),
                    },
                    ChatEvent::ToolCallEnd {
                        index: 1,
                        args: json!({"msg": "second"}),
                    },
                    ChatEvent::Done {
                        reason: xgent_core::chat::StopReason::ToolUse,
                        usage: TokenUsage::default(),
                    },
                ]
            } else {
                vec![
                    ChatEvent::TextDelta { text: "done".into() },
                    ChatEvent::Done {
                        reason: xgent_core::chat::StopReason::Stop,
                        usage: TokenUsage::default(),
                    },
                ]
            };
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async move {
                    for ev in events {
                        if tx.send(ev).await.is_err() { break; }
                    }
                });
            });
            Ok((StreamId(1), rx))
        }
    }
    let captured: Arc<Mutex<Vec<ChatRequest>>> = Arc::new(Mutex::new(vec![]));
    let provider = Arc::new(CapturingProvider { captured: captured.clone() });
    let executor = Arc::new(ToolExecutor::new(vec![Arc::new(EchoTool)]));
    let policy = ToolPolicyConfig {
        approved: vec!["echo".to_string()],
        denied: vec![],
    };
    let (mut app, _root) = test_app_with_retry_provider(
        provider as Arc<dyn crate::bridge::ProviderClient>,
        executor,
        policy,
        crate::bridge::RetryConfig::default(),
    );
    app.insert_resource(Collected::default())
        .add_systems(Update, collect_messages);
    app.world_mut()
        .write_message(UserInputMessage { text: "run two".into(), editor_queries: Vec::new() });
    for _ in 0..80 {
        app.update();
    }

    let conv = app.world().resource::<crate::conversation::Conversation>();
    assert_eq!(conv.status, ConversationStatus::Idle, "应回到 Idle");

    // conv 侧：两个 tool_call 应在一条 assistant 消息中
    let assistant_tool_call_msgs: Vec<_> = conv
        .messages
        .iter()
        .filter_map(|m| match m {
            xgent_core::chat::AgentMessage::Assistant(a) => {
                let tcs: Vec<_> = a
                    .content
                    .iter()
                    .filter(|b| matches!(b, xgent_core::chat::ContentBlock::ToolCall { .. }))
                    .collect();
                if tcs.is_empty() { None } else { Some((a, tcs)) }
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        assistant_tool_call_msgs.len(),
        1,
        "conv 侧应只有一条含 tool_call 的 assistant 消息（修复前拆成两条）"
    );
    let (asst, tcs) = &assistant_tool_call_msgs[0];
    assert_eq!(tcs.len(), 2, "该 assistant 消息应含 2 个 ToolCall 块");
    // 应含文本块（首个）
    assert!(
        asst.content.iter().any(|b| matches!(
            b,
            xgent_core::chat::ContentBlock::Text { text } if text == "Let me run two tools."
        )),
        "assistant 消息应含本轮流式文本块"
    );

    // req 侧（第二次请求）：验证回灌的消息序列符合 OpenAI 协议
    let reqs = captured.lock();
    assert!(reqs.len() >= 2, "应至少两次 chat 调用");
    let second_req = &reqs[1];
    // 找到 assistant 消息含 tool_call，应只有一条含 2 个 ToolCall 块
    let asst_tc_msgs: Vec<_> = second_req
        .messages
        .iter()
        .filter(|m| {
            m.role == xgent_core::chat::Role::Assistant
                && m.content
                    .iter()
                    .any(|b| matches!(b, xgent_core::chat::ContentBlock::ToolCall { .. }))
        })
        .collect();
    assert_eq!(
        asst_tc_msgs.len(),
        1,
        "req 侧应只有一条含 tool_call 的 assistant 消息（修复前拆成两条破坏协议）"
    );
    let tc_count = asst_tc_msgs[0]
        .content
        .iter()
        .filter(|b| matches!(b, xgent_core::chat::ContentBlock::ToolCall { .. }))
        .count();
    assert_eq!(tc_count, 2, "该 assistant 消息应含 2 个 ToolCall 块");
    // 紧跟 2 条 tool role 消息
    let tool_msgs: Vec<_> = second_req
        .messages
        .iter()
        .filter(|m| m.role == xgent_core::chat::Role::Tool)
        .collect();
    assert_eq!(tool_msgs.len(), 2, "应有 2 条 tool result 消息");

    // UI 侧收到 2 个 ToolCall + 2 个 ToolResult
    let collected = app.world().resource::<Collected>();
    assert_eq!(collected.tool_calls.len(), 2, "UI 应收到 2 个 ToolCall 事件");
    assert_eq!(collected.tool_results.len(), 2, "UI 应收到 2 个 ToolResult 事件");
}

/// 验证 provider 流提前断开（未发 Done）触发可重试的 StreamParse 错误，
/// 而非返回 Ok 让上层误执行可能不完整的 tool_calls。
///
/// 修复前：stream None 返回 Ok(Stop)，若已收集部分 tool_calls 会误执行。
/// 修复后：stream None 返回 Err(StreamParse)，经 stream_with_retry 重试。
#[test]
fn stream_none_triggers_retryable_error() {
    // 首次：发 delta + ToolCallStart（无 ToolCallEnd、无 Done）后流断开
    // → stream None → StreamParse 错误 → 重试
    // 第二次：正常文本 + Done
    let provider = Arc::new(RetryMockProvider::new(vec![
        vec![
            ChatEvent::TextDelta { text: "partial".into() },
            ChatEvent::ToolCallStart {
                index: 0,
                id: "call_x".into(),
                name: "echo".into(),
            },
            // 无 ToolCallEnd，无 Done：流断开
        ],
        vec![
            ChatEvent::TextDelta { text: "recovered".into() },
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
        crate::bridge::RetryConfig {
            max_retries: Some(2),
            mode: xgent_settings_core::global::RetryMode::Fixed,
            initial_delay_ms: 10,
            max_delay_ms: 100,
            backoff_factor: 2.0,
        },
    );
    app.world_mut()
        .write_message(UserInputMessage { text: "hi".into(), editor_queries: Vec::new() });
    for _ in 0..150 {
        app.update();
    }

    let conv = app.world().resource::<crate::conversation::Conversation>();
    // 重试成功后应回到 Idle（修复前会误执行不完整 tool_call）
    assert_eq!(
        conv.status,
        ConversationStatus::Idle,
        "stream None 触发重试后应成功回到 Idle"
    );
    // 最终 assistant 文本应为重试后的 "recovered"
    let assistant_text = conv
        .messages
        .iter()
        .rev()
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
    assert_eq!(
        assistant_text, "recovered",
        "重试成功后应固化 \"recovered\"（而非误执行不完整 tool_call）"
    );
}

/// 验证 steering 中断后，被中断的 assistant 文本回灌到 req.messages
/// （与 conv 侧 finalize_assistant 对称），LLM 下一轮能看到自己半截话。
///
/// 修复前：bridge 只 push steering User 文本到 req，漏 push 被中断 assistant，
/// 导致 LLM 下一轮看不到自己刚说的半截话，可能重复/矛盾。
#[test]
fn steering_interrupt_preserves_assistant_text_in_req() {
    use parking_lot::Mutex;
    struct CapturingSteerProvider {
        captured: Arc<Mutex<Vec<ChatRequest>>>,
        steer_seen: Arc<Mutex<bool>>,
    }
    #[async_trait]
    impl ProviderClient for CapturingSteerProvider {
        async fn chat(
            &self,
            req: ChatRequest,
        ) -> Result<(StreamId, mpsc::Receiver<ChatEvent>), (xgent_core::chat::ErrorKind, String)> {
            self.captured.lock().push(req.clone());
            let (tx, rx) = mpsc::channel(8);
            let steer_seen = self.steer_seen.clone();
            let n = self.captured.lock().len() - 1;
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async move {
                    if n == 0 {
                        // 首次：发 delta 后等 steering 信号，再发 Done
                        let _ = tx
                            .send(ChatEvent::TextDelta { text: "partial answer".into() })
                            .await;
                        for _ in 0..200 {
                            { let s = steer_seen.lock(); if *s { break; } }
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        }
                        let _ = tx
                            .send(ChatEvent::Done {
                                reason: xgent_core::chat::StopReason::Stop,
                                usage: TokenUsage { prompt: 5, completion: 1 },
                            })
                            .await;
                    } else {
                        // 第二次（steering 后重新流式）：正常文本 + Done
                        let _ = tx
                            .send(ChatEvent::TextDelta { text: "final".into() })
                            .await;
                        let _ = tx
                            .send(ChatEvent::Done {
                                reason: xgent_core::chat::StopReason::Stop,
                                usage: TokenUsage::default(),
                            })
                            .await;
                    }
                });
            });
            Ok((StreamId(1), rx))
        }
    }
    let steer_seen = Arc::new(Mutex::new(false));
    let captured: Arc<Mutex<Vec<ChatRequest>>> = Arc::new(Mutex::new(vec![]));
    let provider = Arc::new(CapturingSteerProvider {
        captured: captured.clone(),
        steer_seen: steer_seen.clone(),
    });
    let (mut app, _root) = test_app_with_retry_provider(
        provider as Arc<dyn crate::bridge::ProviderClient>,
        Arc::new(ToolExecutor::with_defaults()),
        ToolPolicyConfig::default(),
        crate::bridge::RetryConfig::default(),
    );
    app.world_mut()
        .write_message(UserInputMessage { text: "hi".into(), editor_queries: Vec::new() });
    for _ in 0..5 {
        app.update();
    }
    app.world_mut().write_message(SteeringMessage { text: "wait".into() });
    *steer_seen.lock() = true;
    for _ in 0..80 {
        app.update();
    }

    let conv = app.world().resource::<crate::conversation::Conversation>();
    assert_eq!(conv.status, ConversationStatus::Idle, "steering 后应回到 Idle");

    // 第二次 chat 调用的 req 应含被中断的 assistant 文本 "partial answer"
    let reqs = captured.lock();
    assert!(reqs.len() >= 2, "应至少两次 chat 调用");
    let second_req = &reqs[1];
    let has_partial = second_req.messages.iter().any(|m| {
        m.role == xgent_core::chat::Role::Assistant
            && m.content.iter().any(|b| matches!(
                b,
                xgent_core::chat::ContentBlock::Text { text } if text == "partial answer"
            ))
    });
    assert!(
        has_partial,
        "第二次 req 应含被中断的 assistant 文本（修复前 req 侧丢失半截话）"
    );
}

/// 验证重试时半截 assistant 文本回灌到 req.messages
/// （与 conv 侧 RetryAttempt 的 finalize_assistant 对称）。
///
/// 修复前：stream_with_retry 用 &ChatRequest 不可变引用，无法回灌 partial_text，
/// 导致重试后 LLM 看不到重试前自己说的半截话。
#[test]
fn retry_preserves_partial_assistant_text_in_req() {
    use parking_lot::Mutex;
    struct CapturingProvider {
        captured: Arc<Mutex<Vec<ChatRequest>>>,
    }
    #[async_trait]
    impl ProviderClient for CapturingProvider {
        async fn chat(
            &self,
            req: ChatRequest,
        ) -> Result<(StreamId, mpsc::Receiver<ChatEvent>), (xgent_core::chat::ErrorKind, String)> {
            self.captured.lock().push(req.clone());
            let (tx, rx) = mpsc::channel(8);
            let n = self.captured.lock().len() - 1;
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async move {
                    if n == 0 {
                        // 首次：发 delta 后流断开（stream None）→ StreamParse → 重试
                        let _ = tx
                            .send(ChatEvent::TextDelta { text: "half said".into() })
                            .await;
                        // 无 Done，tx drop → stream None
                    } else {
                        // 重试后：正常文本 + Done
                        let _ = tx
                            .send(ChatEvent::TextDelta { text: "recovered".into() })
                            .await;
                        let _ = tx
                            .send(ChatEvent::Done {
                                reason: xgent_core::chat::StopReason::Stop,
                                usage: TokenUsage::default(),
                            })
                            .await;
                    }
                });
            });
            Ok((StreamId(1), rx))
        }
    }
    let captured: Arc<Mutex<Vec<ChatRequest>>> = Arc::new(Mutex::new(vec![]));
    let provider = Arc::new(CapturingProvider { captured: captured.clone() });
    let (mut app, _root) = test_app_with_retry_provider(
        provider as Arc<dyn crate::bridge::ProviderClient>,
        Arc::new(ToolExecutor::with_defaults()),
        ToolPolicyConfig::default(),
        fast_retry_config(Some(2)),
    );
    app.world_mut()
        .write_message(UserInputMessage { text: "hi".into(), editor_queries: Vec::new() });
    for _ in 0..150 {
        app.update();
    }

    let conv = app.world().resource::<crate::conversation::Conversation>();
    assert_eq!(conv.status, ConversationStatus::Idle, "重试后应回到 Idle");

    // 第二次 chat 调用的 req 应含重试前的半截 assistant 文本 "half said"
    let reqs = captured.lock();
    assert!(reqs.len() >= 2, "应至少两次 chat 调用");
    let second_req = &reqs[1];
    let has_half = second_req.messages.iter().any(|m| {
        m.role == xgent_core::chat::Role::Assistant
            && m.content.iter().any(|b| matches!(
                b,
                xgent_core::chat::ContentBlock::Text { text } if text == "half said"
            ))
    });
    assert!(
        has_half,
        "重试后的 req 应含重试前半截 assistant 文本（修复前 req 侧丢失）"
    );
}

/// 验证 Confirming 态下 Abort 能中断工具确认等待。
///
/// 修复前：executor.execute 的 rx.await（等待用户决策）不监听 cancel_token，
/// 用户在确认弹窗等待时按"停止"无法中断，对话卡住直到用户做决策或 300s 超时。
/// 修复后：rx.await 外包 tokio::select! 监听 cancel_token，Abort 返回 Aborted。
#[test]
fn abort_interrupts_confirming_state() {
    // 用未批准的 echo 工具（默认 NeedsConfirmation）触发确认流程
    let executor = Arc::new(ToolExecutor::new(vec![Arc::new(EchoTool)]));
    let policy = ToolPolicyConfig::default(); // 无 approved → NeedsConfirmation
    let (mut app, _project_root) = test_app_with_executor(
        vec![
            ChatEvent::ToolCallStart {
                index: 0,
                id: "call_c".into(),
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
            // 第二次（若未被 abort）：正常停止
            ChatEvent::TextDelta { text: "done".into() },
            ChatEvent::Done {
                reason: xgent_core::chat::StopReason::Stop,
                usage: TokenUsage::default(),
            },
        ],
        executor,
        policy,
    );
    app.insert_resource(Collected::default())
        .add_systems(Update, collect_messages);
    app.world_mut()
        .write_message(UserInputMessage { text: "do echo".into(), editor_queries: Vec::new() });

    // 跑帧让 tool_call 到达 + ConfirmRequest 弹出
    for _ in 0..30 {
        app.update();
    }

    // 此时状态应为 Confirming（等待用户确认）
    {
        let conv = app.world().resource::<crate::conversation::Conversation>();
        assert_eq!(
            conv.status,
            ConversationStatus::Confirming,
            "应处于 Confirming 态等待确认"
        );
    }

    // 用户按"停止"（Abort），而非做确认决策
    app.world_mut().write_message(AbortMessage);
    for _ in 0..50 {
        app.update();
    }

    // 修复前：卡在 Confirming/Aborting 态；修复后：被 abort 中断回 Idle
    let conv = app.world().resource::<crate::conversation::Conversation>();
    assert_ne!(
        conv.status,
        ConversationStatus::Confirming,
        "Abort 应中断确认等待（修复前卡在 Confirming）"
    );
}

/// 验证正常完成（无 tool_calls）后，本轮 assistant 文本回灌到 req.messages，
/// 后续 FollowUp 时 LLM 能看到上一轮 assistant 回复。
///
/// 修复前：`tool_calls.is_empty()` 分支不回灌 `partial_text`，导致对话内
/// FollowUp/steering 时 req.messages 缺最后一轮 assistant 回复，LLM 上下文断裂
/// （看不到自己刚说的话）。conv 侧 finalize_assistant 固化了，但 req/conv 不同步。
/// 修复后：该分支把 `outcome.partial_text` 作为 assistant 消息 push 到 req.messages。
#[test]
fn normal_completion_backfills_assistant_text_to_req() {
    use parking_lot::Mutex;
    struct CapturingProvider {
        captured: Arc<Mutex<Vec<ChatRequest>>>,
    }
    #[async_trait]
    impl ProviderClient for CapturingProvider {
        async fn chat(
            &self,
            req: ChatRequest,
        ) -> Result<(StreamId, mpsc::Receiver<ChatEvent>), (xgent_core::chat::ErrorKind, String)> {
            self.captured.lock().push(req.clone());
            let (tx, rx) = mpsc::channel(8);
            let n = self.captured.lock().len() - 1;
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async move {
                    if n == 0 {
                        // 首次：文本 + 正常停止（无 tool_calls）
                        let _ = tx
                            .send(ChatEvent::TextDelta { text: "hello back".into() })
                            .await;
                        let _ = tx
                            .send(ChatEvent::Done {
                                reason: xgent_core::chat::StopReason::Stop,
                                usage: TokenUsage::default(),
                            })
                            .await;
                    } else {
                        // FollowUp 后：空回复停止（仅用于捕获 req）
                        let _ = tx
                            .send(ChatEvent::Done {
                                reason: xgent_core::chat::StopReason::Stop,
                                usage: TokenUsage::default(),
                            })
                            .await;
                    }
                });
            });
            Ok((StreamId(1), rx))
        }
    }
    let captured: Arc<Mutex<Vec<ChatRequest>>> = Arc::new(Mutex::new(vec![]));
    let provider = Arc::new(CapturingProvider { captured: captured.clone() });
    let (mut app, _root) = test_app_with_retry_provider(
        provider as Arc<dyn crate::bridge::ProviderClient>,
        Arc::new(ToolExecutor::with_defaults()),
        ToolPolicyConfig::default(),
        fast_retry_config(Some(0)),
    );
    app.world_mut()
        .write_message(UserInputMessage { text: "hi".into(), editor_queries: Vec::new() });
    // 跑到第一轮完成（Idle）。轮询直到 Idle，上限 300 帧避免 flaky。
    for _ in 0..300 {
        app.update();
        let conv = app.world().resource::<crate::conversation::Conversation>();
        if conv.status == ConversationStatus::Idle {
            break;
        }
    }
    {
        let conv = app.world().resource::<crate::conversation::Conversation>();
        assert_eq!(conv.status, ConversationStatus::Idle, "第一轮应完成回 Idle");
    }

    // 发 FollowUp，触发第二次 chat 调用。轮询直到第二次 req 被捕获，上限 300 帧。
    app.world_mut()
        .write_message(FollowUpMessage { text: "again".into() });
    for _ in 0..300 {
        app.update();
        if captured.lock().len() >= 2 {
            break;
        }
    }

    // 第二次 chat 调用的 req 应含第一轮的 assistant 文本 "hello back"
    let reqs = captured.lock();
    assert!(reqs.len() >= 2, "FollowUp 应触发第二次 chat 调用");
    let second_req = &reqs[1];
    let has_assistant = second_req.messages.iter().any(|m| {
        m.role == xgent_core::chat::Role::Assistant
            && m.content.iter().any(|b| matches!(
                b,
                xgent_core::chat::ContentBlock::Text { text } if text == "hello back"
            ))
    });
    assert!(
        has_assistant,
        "FollowUp 后的 req 应含上一轮 assistant 文本（修复前 req 侧丢失，LLM 看不到自己刚说的话）reqs={}",
        reqs.len()
    );
}