//! OpenAI compatible 接口适配器。
//!
//! 适用于 OpenAI、DeepSeek、Ollama 兼容模式等遵循 OpenAI `/v1/chat/completions`
//! 协议的 provider。支持流式输出与工具调用（按 index 聚合）。

use async_trait::async_trait;
use futures::Stream;
use reqwest::Client;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_stream::StreamExt;
use xgent_core::chat::{ChatEvent, ChatMessage, ChatRequest, TokenUsage};
use xgent_core::ids::StreamId;

use crate::provider::{ChatStream, LlmProvider, ModelInfo, ProviderError};
use crate::sse::parse_response_stream;

/// 等待首个 SSE 事件的最大秒数。
///
/// send() 返回后，若首事件迟迟不到，通常意味着服务端已接收请求但卡在排队/推理，
/// 超过此阈值视为网络异常，触发 `ChatEvent::Error{kind: Network}`。
const FIRST_EVENT_TIMEOUT_SECS: u64 = 30;

/// 流式消费中相邻两个事件之间的最大空闲秒数。
///
/// 超过此阈值未收到下一事件，视为流卡死（服务端 hang 住），触发
/// `ChatEvent::Error{kind: Network}`，终止本次流。
const IDLE_TIMEOUT_SECS: u64 = 60;

/// OpenAI compatible 适配器。
pub struct OpenAiCompatProvider {
    /// provider id，如 "openai" / "deepseek" / "ollama"
    id: String,
    /// API 基础 URL，如 "https://api.openai.com/v1"
    api_base: String,
    /// API Key
    api_key: String,
    /// 复用的 HTTP 客户端（自带连接池）
    client: Client,
}

impl OpenAiCompatProvider {
    /// 构造适配器。
    ///
    /// `api_base` 不含尾部 `/`，方法内部拼接路径。
    pub fn new(id: String, api_base: String, api_key: String) -> Self {
        let client = Client::new();
        Self {
            id,
            api_base,
            api_key,
            client,
        }
    }

    /// 用已有 Client 构造（便于测试与连接复用）。
    pub fn with_client(id: String, api_base: String, api_key: String, client: Client) -> Self {
        Self {
            id,
            api_base,
            api_key,
            client,
        }
    }

    /// 构造 chat completions 请求体。
    fn build_chat_body(&self, req: &ChatRequest) -> Value {
        let messages: Vec<Value> = req.messages.iter().map(message_to_json).collect();
        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        if let Some(tools) = &req.tools
            && !tools.is_empty()
        {
            let tools_json: Vec<Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.input_schema,
                        }
                    })
                })
                .collect();
            body["tools"] = json!(tools_json);
        }
        body
    }

    /// chat completions 端点 URL。
    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.api_base.trim_end_matches('/'))
    }

    /// models 端点 URL。
    fn models_url(&self) -> String {
        format!("{}/models", self.api_base.trim_end_matches('/'))
    }
}

/// 把结构化 [`ChatMessage`] 转为 OpenAI API 的 message JSON。
///
/// 按 role 展开（见 ADR-0005）：
/// - System/User：content 为 Text 块拼接的字符串
/// - Assistant：Text 块拼接为 content；ToolCall 块展开为顶层 `tool_calls` 字段
/// - Tool：从 ToolResult 块取 content + tool_call_id（修复旧版缺 tool_call_id 的 bug）
fn message_to_json(m: &ChatMessage) -> Value {
    use xgent_core::chat::{ContentBlock, Role};
    match m.role {
        Role::System | Role::User => {
            let text = blocks_to_text(&m.content);
            json!({
                "role": role_str(m.role),
                "content": text,
            })
        }
        Role::Assistant => {
            let text = blocks_to_text(&m.content);
            let tool_calls: Vec<Value> = m
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolCall { id, name, args } => Some(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": args.to_string(),
                        }
                    })),
                    _ => None,
                })
                .collect();
            let mut v = json!({
                "role": "assistant",
                "content": text,
            });
            if !tool_calls.is_empty() {
                v["tool_calls"] = json!(tool_calls);
            }
            v
        }
        Role::Tool => {
            // OpenAI 协议：tool role 消息必须带 tool_call_id
            let (tool_call_id, content, is_error) = m
                .content
                .iter()
                .find_map(|b| match b {
                    ContentBlock::ToolResult {
                        tool_call_id,
                        content,
                        is_error,
                    } => Some((tool_call_id.clone(), content.clone(), *is_error)),
                    _ => None,
                })
                .unwrap_or_default();
            let _ = is_error; // OpenAI 协议无 is_error 字段，忽略
            json!({
                "role": "tool",
                "content": content,
                "tool_call_id": tool_call_id,
            })
        }
    }
}

/// 从 content blocks 提取所有 Text 块拼接为字符串。
fn blocks_to_text(content: &[xgent_core::chat::ContentBlock]) -> String {
    use xgent_core::chat::ContentBlock;
    content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// role 枚举转 OpenAI 字符串。
fn role_str(role: xgent_core::chat::Role) -> &'static str {
    use xgent_core::chat::Role;
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let resp = self
            .client
            .get(self.models_url())
            .bearer_auth(&self.api_key)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                status: status.as_u16(),
                body,
            });
        }
        let v: Value = resp.json().await?;
        let models = v["data"]
            .as_array()
            .ok_or_else(|| ProviderError::Stream("missing 'data' array".into()))?;
        let result = models
            .iter()
            .filter_map(|m| {
                let id = m["id"].as_str()?.to_string();
                Some(ModelInfo {
                    name: id.clone(),
                    id,
                    context_window: m["context_window"].as_u64().map(|n| n as u32),
                })
            })
            .collect();
        Ok(result)
    }

    async fn chat(&self, req: ChatRequest) -> Result<(StreamId, ChatStream), ProviderError> {
        if self.api_base.is_empty() {
            return Err(ProviderError::Config("api_base 未配置".into()));
        }
        let body = self.build_chat_body(&req);
        let resp = self
            .client
            .post(self.chat_url())
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                status: status.as_u16(),
                body,
            });
        }

        let stream = parse_response_stream(resp);

        // StreamId 用伪随机（时间戳），daemon 侧会用自己的生成器覆盖
        let stream_id = next_stream_id();
        let (tx, rx) = mpsc::channel::<ChatEvent>(64);

        let model = req.model.clone();
        tokio::spawn(async move {
            run_stream(stream, model, tx).await;
        });

        Ok((stream_id, rx))
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        // list_models 成功即健康
        self.list_models().await.map(|_| ())
    }
}

/// 消费 SSE JSON Value 流，转换为细粒度 [`ChatEvent`] 并发送到 `tx`。
///
/// 由 [`OpenAiCompatProvider::chat`] 在 spawn 的任务中调用。独立为函数便于
/// 用 mock SSE 流做超时单测（见 `tests::run_stream_*_timeout`）。
///
/// 超时保护（O8）：
/// - 首事件超时：`Start` 发出后，首个事件等待不超过
///   [`FIRST_EVENT_TIMEOUT_SECS`]，超时发 `Error{kind: Network}` 并终止。
/// - idle 超时：首事件后，相邻两事件之间等待不超过
///   [`IDLE_TIMEOUT_SECS`]，超时发 `Error{kind: Network}` 并终止。
///
/// `S` 取 `Stream<Item = Result<Value, ProviderError>>`，兼容真实 SSE 与 mock 流。
async fn run_stream<S>(stream: S, model: String, tx: mpsc::Sender<ChatEvent>)
where
    S: Stream<Item = Result<Value, ProviderError>> + Send + 'static,
{
    run_stream_with_timeout(
        stream,
        model,
        tx,
        Duration::from_secs(FIRST_EVENT_TIMEOUT_SECS),
        Duration::from_secs(IDLE_TIMEOUT_SECS),
    )
    .await;
}

/// [`run_stream`] 的可配置超时版本，供单测注入短超时，避免等待 30s/60s。
///
/// 语义与 [`run_stream`] 一致：`first_timeout` 限定首事件等待，`idle_timeout`
/// 限定后续相邻事件等待。两者超时均发 `Error{kind: Network}` 并终止流。
async fn run_stream_with_timeout<S>(
    stream: S,
    model: String,
    tx: mpsc::Sender<ChatEvent>,
    first_timeout: Duration,
    idle_timeout: Duration,
) where
    S: Stream<Item = Result<Value, ProviderError>> + Send + 'static,
{
    use xgent_core::chat::ErrorKind;

    let mut s = Box::pin(stream);
    // 工具调用按 index 聚合（OpenAI 分块到达）
    let mut tool_calls: Vec<ToolCallAccum> = Vec::new();
    // 跟踪是否已发 TextStart（跨 chunk 状态）
    let mut text_started = false;
    // 标记是否已发过 finish（避免重复 Done）
    let mut finished = false;

    // 流开始
    let _ = tx.send(ChatEvent::Start { model }).await;

    // 首事件单独用 first_timeout 等待；后续用 idle_timeout
    let first = match timeout(first_timeout, s.next()).await {
        Ok(Some(item)) => item,
        // 流在首事件前就结束：当作正常空流，走收尾逻辑
        Ok(None) => {
            finish_stream(&tx, &mut text_started, finished).await;
            return;
        }
        Err(_) => {
            let _ = tx
                .send(ChatEvent::Error {
                    kind: ErrorKind::Network,
                    message: "stream first event timeout".into(),
                })
                .await;
            return;
        }
    };

    // 处理首事件
    if !handle_item(
        first,
        &tx,
        &mut tool_calls,
        &mut text_started,
        &mut finished,
    )
    .await
    {
        return;
    }

    // 后续事件用 idle_timeout 逐个等待
    loop {
        match timeout(idle_timeout, s.next()).await {
            Ok(Some(item)) => {
                if !handle_item(item, &tx, &mut tool_calls, &mut text_started, &mut finished).await
                {
                    return;
                }
            }
            // 流自然结束
            Ok(None) => break,
            Err(_) => {
                let _ = tx
                    .send(ChatEvent::Error {
                        kind: ErrorKind::Network,
                        message: "stream idle timeout".into(),
                    })
                    .await;
                return;
            }
        }
    }

    // 流自然结束，补 Done（若未发过）
    finish_stream(&tx, &mut text_started, finished).await;
}

/// 处理单个流 item，返回 false 表示应终止流（已发 Error）。
async fn handle_item(
    item: Result<Value, ProviderError>,
    tx: &mpsc::Sender<ChatEvent>,
    tool_calls: &mut Vec<ToolCallAccum>,
    text_started: &mut bool,
    finished: &mut bool,
) -> bool {
    match item {
        Ok(v) => {
            if let Err(e) = handle_chunk(&v, tx, tool_calls, text_started).await {
                let _ = tx
                    .send(ChatEvent::Error {
                        kind: e.to_error_kind(),
                        message: e.to_string(),
                    })
                    .await;
                return false;
            }
            // 检测是否已发 Done（finish_reason 处理过）
            if !*finished
                && v["choices"]
                    .as_array()
                    .and_then(|c| c.first())
                    .and_then(|c| c["finish_reason"].as_str())
                    .is_some()
            {
                *finished = true;
            }
            true
        }
        Err(e) => {
            let _ = tx
                .send(ChatEvent::Error {
                    kind: e.to_error_kind(),
                    message: e.to_string(),
                })
                .await;
            false
        }
    }
}

/// 流收尾：若文本块未结束补 TextEnd；若未发 Done 补 Done{Stop}。
async fn finish_stream(tx: &mpsc::Sender<ChatEvent>, text_started: &mut bool, finished: bool) {
    if !finished {
        if *text_started {
            let _ = tx.send(ChatEvent::TextEnd).await;
        }
        let _ = tx
            .send(ChatEvent::Done {
                reason: xgent_core::chat::StopReason::Stop,
                usage: TokenUsage::default(),
            })
            .await;
    }
}

/// 工具调用累积器（按 index 聚合分块，发射 ToolCallStart/Delta/End）。
#[derive(Default, Clone)]
struct ToolCallAccum {
    /// 是否已发射 ToolCallStart
    started: bool,
    id: String,
    name: String,
    args: String,
}

/// 把 OpenAI finish_reason 映射为 StopReason。
fn map_stop_reason(reason: &str) -> xgent_core::chat::StopReason {
    use xgent_core::chat::StopReason;
    match reason {
        "stop" => StopReason::Stop,
        "tool_calls" => StopReason::ToolUse,
        "length" => StopReason::Length,
        _ => StopReason::Stop,
    }
}

/// 处理单个 SSE chunk，转换为细粒度 [`ChatEvent`] 发送。
///
/// 事件发射规则（对齐 ADR-0006）：
/// - 文本：首个非空 content 发 `TextStart`，后续 `TextDelta`，finish 时 `TextEnd`
/// - 工具调用：按 index 首次见发 `ToolCallStart`，参数片段发 `ToolCallDelta`，finish 时 `ToolCallEnd`（全量 args）
/// - finish_reason：映射 StopReason 后发 `Done{reason, usage}`
///
/// `text_started` 跟踪是否已发 TextStart（跨 chunk 状态）。
///
/// 返回 `Err` 表示致命解析错误，应终止流。
async fn handle_chunk(
    v: &Value,
    tx: &mpsc::Sender<ChatEvent>,
    tool_calls: &mut Vec<ToolCallAccum>,
    text_started: &mut bool,
) -> Result<(), ProviderError> {
    let usage = extract_usage(v);

    let choices: &[Value] = match v["choices"].as_array() {
        Some(c) if !c.is_empty() => c,
        _ => {
            // 无 choices 或空 choices（纯 usage chunk）——usage 留到 finish 时发
            return Ok(());
        }
    };

    for choice in choices {
        // 工具调用 delta
        if let Some(tc_arr) = choice["delta"]["tool_calls"].as_array() {
            for tc in tc_arr {
                let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                if idx >= tool_calls.len() {
                    tool_calls.resize(idx + 1, ToolCallAccum::default());
                }
                let accum = &mut tool_calls[idx];

                // 首次见该 index：提取 id/name，发 ToolCallStart
                if !accum.started {
                    if let Some(id) = tc["id"].as_str() {
                        accum.id = id.to_string();
                    }
                    if let Some(name) = tc["function"]["name"].as_str() {
                        accum.name = name.to_string();
                    }
                    accum.started = true;
                    let _ = tx
                        .send(ChatEvent::ToolCallStart {
                            index: idx as u32,
                            id: accum.id.clone(),
                            name: accum.name.clone(),
                        })
                        .await;
                }

                // 参数片段：发 ToolCallDelta（原始 partial_json）
                if let Some(args) = tc["function"]["arguments"].as_str()
                    && !args.is_empty()
                {
                    accum.args.push_str(args);
                    let _ = tx
                        .send(ChatEvent::ToolCallDelta {
                            index: idx as u32,
                            partial_json: args.to_string(),
                        })
                        .await;
                }
            }
        }

        // 文本 delta
        if let Some(content) = choice["delta"]["content"].as_str()
            && !content.is_empty()
        {
            if !*text_started {
                *text_started = true;
                let _ = tx.send(ChatEvent::TextStart).await;
            }
            let _ = tx
                .send(ChatEvent::TextDelta {
                    text: content.to_string(),
                })
                .await;
        }

        // finish_reason
        if let Some(reason) = choice["finish_reason"].as_str() {
            // 文本块结束（若已开始）
            if *text_started {
                *text_started = false;
                let _ = tx.send(ChatEvent::TextEnd).await;
            }

            // 工具调用结束：发 ToolCallEnd（聚合 args 解析为 JSON）
            for (idx, accum) in tool_calls.drain(..).enumerate() {
                if accum.started {
                    let args_val: Value = if accum.args.is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&accum.args).unwrap_or(json!({}))
                    };
                    let _ = tx
                        .send(ChatEvent::ToolCallEnd {
                            index: idx as u32,
                            args: args_val,
                        })
                        .await;
                }
            }

            // 流结束
            let stop_reason = map_stop_reason(reason);
            let _ = tx
                .send(ChatEvent::Done {
                    reason: stop_reason,
                    usage: usage.clone().unwrap_or_default(),
                })
                .await;
        }
    }

    Ok(())
}

/// 从 chunk 提取 usage（OpenAI 在最后一个 chunk 带 usage）。
fn extract_usage(v: &Value) -> Option<TokenUsage> {
    let u = &v["usage"];
    if u.is_null() {
        return None;
    }
    Some(TokenUsage {
        prompt: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
        completion: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
    })
}

/// 简单的 StreamId 生成器（基于全局原子计数）。
///
/// daemon 侧会用自己的生成器覆盖；这里仅供本地直调。
fn next_stream_id() -> StreamId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    StreamId(COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use xgent_core::chat::{ChatMessage, Role};

    /// 构造一个伪 SSE chunk Value。
    fn chunk(json_str: &str) -> Value {
        serde_json::from_str(json_str).unwrap()
    }

    /// 带超时保护的 recv，防止未来回归导致测试死等挂起。
    async fn recv(rx: &mut mpsc::Receiver<ChatEvent>) -> ChatEvent {
        match tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await {
            Ok(Some(ev)) => ev,
            Ok(None) => panic!("channel closed before receiving event"),
            Err(_) => panic!("recv timed out (2s): handle_chunk 未发出预期事件"),
        }
    }

    /// 带超时保护的可选 recv，返回 Option<Event>。超时视为 None（流结束）。
    async fn recv_opt(rx: &mut mpsc::Receiver<ChatEvent>) -> Option<ChatEvent> {
        match tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await {
            Ok(Some(ev)) => Some(ev),
            Ok(None) | Err(_) => None,
        }
    }

    #[tokio::test]
    async fn handle_chunk_text_emits_text_start_delta_end() {
        let (tx, mut rx) = mpsc::channel::<ChatEvent>(16);
        let mut tool_calls = Vec::new();
        let mut text_started = false;
        // 文本 chunk
        let v1 = chunk(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#);
        handle_chunk(&v1, &tx, &mut tool_calls, &mut text_started)
            .await
            .unwrap();
        // finish chunk
        let v2 = chunk(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#);
        handle_chunk(&v2, &tx, &mut tool_calls, &mut text_started)
            .await
            .unwrap();
        drop(tx);

        // 期望序列：TextStart, TextDelta("Hello"), TextEnd, Done{Stop}
        let mut seq = Vec::new();
        while let Some(ev) = recv_opt(&mut rx).await {
            seq.push(ev);
        }
        assert!(
            matches!(seq[0], ChatEvent::TextStart),
            "第 1 个应为 TextStart"
        );
        assert!(
            matches!(&seq[1], ChatEvent::TextDelta { text } if text == "Hello"),
            "第 2 个应为 TextDelta"
        );
        assert!(matches!(seq[2], ChatEvent::TextEnd), "第 3 个应为 TextEnd");
        assert!(
            matches!(
                &seq[3],
                ChatEvent::Done {
                    reason: xgent_core::chat::StopReason::Stop,
                    ..
                }
            ),
            "第 4 个应为 Done{{Stop}}"
        );
    }

    #[test]
    fn build_chat_body_basic() {
        let p = OpenAiCompatProvider::new(
            "openai".into(),
            "https://api.openai.com/v1".into(),
            "sk-x".into(),
        );
        let req = ChatRequest {
            provider: "openai".into(),
            model: "gpt-4".into(),
            messages: vec![ChatMessage::text(Role::User, "hi")],
            tools: None,
        };
        let body = p.build_chat_body(&req);
        assert_eq!(body["model"], "gpt-4");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "hi");
        // 无 tools 时不写 tools 字段
        assert!(body.get("tools").is_none());
    }
    #[test]
    fn message_to_json_assistant_with_tool_call() {
        // 验证 assistant 含 ToolCall 块时展开为顶层 tool_calls 字段
        let m = ChatMessage {
            role: Role::Assistant,
            content: vec![
                xgent_core::chat::ContentBlock::Text {
                    text: "let me read".into(),
                },
                xgent_core::chat::ContentBlock::ToolCall {
                    id: "call_1".into(),
                    name: "read_file".into(),
                    args: json!({"path": "/x"}),
                },
            ],
        };
        let v = message_to_json(&m);
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["content"], "let me read");
        // tool_calls 是顶层字段（非 content 内）
        assert_eq!(v["tool_calls"][0]["id"], "call_1");
        assert_eq!(v["tool_calls"][0]["type"], "function");
        assert_eq!(v["tool_calls"][0]["function"]["name"], "read_file");
        assert_eq!(
            v["tool_calls"][0]["function"]["arguments"],
            r#"{"path":"/x"}"#
        );
    }

    #[test]
    fn message_to_json_tool_role_has_tool_call_id() {
        // 验证 Tool role 消息带 tool_call_id（修复旧版 bug）
        let m = ChatMessage {
            role: Role::Tool,
            content: vec![xgent_core::chat::ContentBlock::ToolResult {
                tool_call_id: "call_1".into(),
                content: "file content".into(),
                is_error: false,
            }],
        };
        let v = message_to_json(&m);
        assert_eq!(v["role"], "tool");
        assert_eq!(v["content"], "file content");
        assert_eq!(v["tool_call_id"], "call_1");
    }

    #[tokio::test]
    async fn handle_chunk_finish_stop_sends_done() {
        let (tx, mut rx) = mpsc::channel::<ChatEvent>(8);
        let mut tool_calls = Vec::new();
        let mut text_started = false;
        let v = chunk(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#);
        handle_chunk(&v, &tx, &mut tool_calls, &mut text_started)
            .await
            .unwrap();
        let ev = recv(&mut rx).await;
        assert!(matches!(
            ev,
            ChatEvent::Done {
                reason: xgent_core::chat::StopReason::Stop,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn handle_chunk_finish_length_maps_stop_reason() {
        let (tx, mut rx) = mpsc::channel::<ChatEvent>(8);
        let mut tool_calls = Vec::new();
        let mut text_started = false;
        let v = chunk(r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#);
        handle_chunk(&v, &tx, &mut tool_calls, &mut text_started)
            .await
            .unwrap();
        let ev = recv(&mut rx).await;
        assert!(matches!(
            ev,
            ChatEvent::Done {
                reason: xgent_core::chat::StopReason::Length,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn handle_chunk_tool_call_emits_start_delta_end() {
        let (tx, mut rx) = mpsc::channel::<ChatEvent>(32);
        let mut tool_calls = Vec::new();
        let mut text_started = false;
        // 第一块：tool_call 开始 + 参数片段
        let v1 = chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}"#,
        );
        handle_chunk(&v1, &tx, &mut tool_calls, &mut text_started)
            .await
            .unwrap();
        // 第二块：arguments 继续
        let v2 = chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"/x\"}"}}]}}]}"#,
        );
        handle_chunk(&v2, &tx, &mut tool_calls, &mut text_started)
            .await
            .unwrap();
        // 第三块：finish
        let v3 = chunk(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#);
        handle_chunk(&v3, &tx, &mut tool_calls, &mut text_started)
            .await
            .unwrap();
        drop(tx);

        // 期望序列：ToolCallStart{0,"call_1","read_file"},
        //           ToolCallDelta{0,"{\"pa"},
        //           ToolCallDelta{0,"th\":\"/x\"}"},
        //           ToolCallEnd{0,{"path":"/x"}},
        //           Done{ToolUse}
        let mut seq = Vec::new();
        while let Some(ev) = recv_opt(&mut rx).await {
            seq.push(ev);
        }
        assert!(
            matches!(&seq[0], ChatEvent::ToolCallStart { index: 0, id, name } if id == "call_1" && name == "read_file"),
            "第 1 个应为 ToolCallStart"
        );
        assert!(
            matches!(&seq[1], ChatEvent::ToolCallDelta { index: 0, partial_json } if partial_json == "{\"pa"),
            "第 2 个应为 ToolCallDelta"
        );
        assert!(
            matches!(&seq[2], ChatEvent::ToolCallDelta { index: 0, .. }),
            "第 3 个应为 ToolCallDelta"
        );
        assert!(
            matches!(&seq[3], ChatEvent::ToolCallEnd { index: 0, args } if args == &json!({"path": "/x"})),
            "第 4 个应为 ToolCallEnd 含全量 args"
        );
        assert!(
            matches!(
                &seq[4],
                ChatEvent::Done {
                    reason: xgent_core::chat::StopReason::ToolUse,
                    ..
                }
            ),
            "第 5 个应为 Done{{ToolUse}}"
        );
    }

    #[test]
    fn build_chat_body_with_tools() {
        use xgent_core::chat::ToolSchema;
        let p = OpenAiCompatProvider::new("o".into(), "https://x/v1".into(), "k".into());
        let req = ChatRequest {
            provider: "o".into(),
            model: "m".into(),
            messages: vec![],
            tools: Some(vec![ToolSchema {
                name: "read_file".into(),
                description: "read a file".into(),
                input_schema: json!({"type": "object"}),
            }]),
        };
        let body = p.build_chat_body(&req);
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "read_file");
    }

    #[test]
    fn urls_trim_trailing_slash() {
        let p = OpenAiCompatProvider::new("o".into(), "https://x/v1/".into(), "k".into());
        assert_eq!(p.chat_url(), "https://x/v1/chat/completions");
        assert_eq!(p.models_url(), "https://x/v1/models");
    }

    #[tokio::test]
    async fn chat_missing_api_base_returns_config_error() {
        let p = OpenAiCompatProvider::new("o".into(), "".into(), "k".into());
        let req = ChatRequest {
            provider: "o".into(),
            model: "m".into(),
            messages: vec![],
            tools: None,
        };
        let err = p.chat(req).await.unwrap_err();
        assert!(matches!(err, ProviderError::Config(_)));
    }

    #[test]
    fn stream_id_next_increments() {
        let a = next_stream_id();
        let b = next_stream_id();
        assert_ne!(a, b);
        assert!(b.0 > a.0);
    }

    // —— O8 流式超时单测 ——

    /// 构造一个 mock SSE Value 流：按指定延迟依次产出 item。
    ///
    /// 每个 item 产出前先 `sleep(delay)`，用以模拟慢速 / 卡顿的 SSE 流。
    /// item 为 `Ok(Value)`（正常 chunk）或 `Err(ProviderError)`（流错误）。
    fn delayed_stream<I>(items: I) -> impl futures::Stream<Item = Result<Value, ProviderError>>
    where
        I: IntoIterator<Item = (std::time::Duration, Result<Value, ProviderError>)>
            + Send
            + 'static,
        I::IntoIter: Send,
    {
        use futures::stream;
        // 用完全限定调用，避免 tokio_stream::StreamExt::then 的歧义
        futures::StreamExt::then(
            stream::iter(items.into_iter().collect::<Vec<_>>()),
            |(delay, item)| async move {
                tokio::time::sleep(delay).await;
                item
            },
        )
    }

    /// 首事件超时：mock 流的首个事件延迟 300ms，first_timeout=100ms 必触发。
    #[tokio::test]
    async fn run_stream_first_event_timeout() {
        let (tx, mut rx) = mpsc::channel::<ChatEvent>(16);
        // 唯一一个 chunk 延迟 300ms 才产出
        let stream = delayed_stream(vec![(
            std::time::Duration::from_millis(300),
            Ok(chunk(r#"{"choices":[{"delta":{"content":"hi"}}]}"#)),
        )]);
        // run_stream_with_timeout 用 100ms 首事件超时（生产常量 30s 太长）
        run_stream_with_timeout(
            stream,
            "m".into(),
            tx,
            std::time::Duration::from_millis(100),
            std::time::Duration::from_secs(60),
        )
        .await;

        // 首条应为 Start{model}
        let ev0 = recv(&mut rx).await;
        assert!(
            matches!(ev0, ChatEvent::Start { ref model } if model == "m"),
            "首条应为 Start{{model}}, 实际 {ev0:?}"
        );
        // 第二条应为 Error{Network, "stream first event timeout"}
        let ev1 = recv(&mut rx).await;
        match ev1 {
            ChatEvent::Error { kind, message } => {
                assert_eq!(
                    kind,
                    xgent_core::chat::ErrorKind::Network,
                    "首事件超时 kind 应为 Network"
                );
                assert!(
                    message.contains("first event timeout"),
                    "首事件超时 message 应含 'first event timeout', 实际 {message}"
                );
            }
            other => panic!("期望 Error{{Network}}, 实际 {other:?}"),
        }
        // 之后通道应关闭（无更多事件）
        assert!(rx.recv().await.is_none(), "超时后不应有更多事件");
    }

    /// idle 超时：首事件立即到达，第二事件延迟 300ms，idle_timeout=100ms 必触发。
    #[tokio::test]
    async fn run_stream_idle_timeout() {
        let (tx, mut rx) = mpsc::channel::<ChatEvent>(16);
        // 首个 chunk 立即产出；第二个 chunk 延迟 300ms
        let stream = delayed_stream(vec![
            (
                std::time::Duration::ZERO,
                Ok(chunk(r#"{"choices":[{"delta":{"content":"hi"}}]}"#)),
            ),
            (
                std::time::Duration::from_millis(300),
                Ok(chunk(
                    r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
                )),
            ),
        ]);
        run_stream_with_timeout(
            stream,
            "m".into(),
            tx,
            std::time::Duration::from_secs(30),
            std::time::Duration::from_millis(100),
        )
        .await;

        // Start
        let ev0 = recv(&mut rx).await;
        assert!(
            matches!(ev0, ChatEvent::Start { .. }),
            "首条应为 Start, 实际 {ev0:?}"
        );
        // 首个 chunk：TextStart + TextDelta("hi")
        let ev1 = recv(&mut rx).await;
        assert!(
            matches!(ev1, ChatEvent::TextStart),
            "应为 TextStart, 实际 {ev1:?}"
        );
        let ev2 = recv(&mut rx).await;
        assert!(
            matches!(ev2, ChatEvent::TextDelta { ref text } if text == "hi"),
            "应为 TextDelta{{hi}}, 实际 {ev2:?}"
        );
        // 之后应因 idle 超时发 Error{Network, "stream idle timeout"}
        let ev3 = recv(&mut rx).await;
        match ev3 {
            ChatEvent::Error { kind, message } => {
                assert_eq!(
                    kind,
                    xgent_core::chat::ErrorKind::Network,
                    "idle 超时 kind 应为 Network"
                );
                assert!(
                    message.contains("idle timeout"),
                    "idle 超时 message 应含 'idle timeout', 实际 {message}"
                );
            }
            other => panic!("期望 Error{{Network}}, 实际 {other:?}"),
        }
        assert!(rx.recv().await.is_none(), "idle 超时后不应有更多事件");
    }

    /// 回归：正常快流不受超时影响，应正常发完事件并 Done。
    #[tokio::test]
    async fn run_stream_normal_flow_not_interrupted() {
        let (tx, mut rx) = mpsc::channel::<ChatEvent>(16);
        // 两个 chunk 都立即产出
        let stream = delayed_stream(vec![
            (
                std::time::Duration::ZERO,
                Ok(chunk(r#"{"choices":[{"delta":{"content":"hi"}}]}"#)),
            ),
            (
                std::time::Duration::ZERO,
                Ok(chunk(
                    r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
                )),
            ),
        ]);
        run_stream_with_timeout(
            stream,
            "m".into(),
            tx,
            std::time::Duration::from_secs(30),
            std::time::Duration::from_secs(60),
        )
        .await;

        let mut seq = Vec::new();
        while let Some(ev) = recv_opt(&mut rx).await {
            seq.push(ev);
        }
        // 期望：Start, TextStart, TextDelta("hi"), TextEnd, Done{Stop}
        assert!(
            matches!(seq[0], ChatEvent::Start { .. }),
            "第 1 个应为 Start"
        );
        assert!(
            matches!(seq[1], ChatEvent::TextStart),
            "第 2 个应为 TextStart"
        );
        assert!(
            matches!(&seq[2], ChatEvent::TextDelta { text } if text == "hi"),
            "第 3 个应为 TextDelta{{hi}}"
        );
        assert!(matches!(seq[3], ChatEvent::TextEnd), "第 4 个应为 TextEnd");
        assert!(
            matches!(
                &seq[4],
                ChatEvent::Done {
                    reason: xgent_core::chat::StopReason::Stop,
                    ..
                }
            ),
            "第 5 个应为 Done{{Stop}}"
        );
        assert_eq!(seq.len(), 5, "正常流应恰好 5 个事件, 实际 {}", seq.len());
    }
}
