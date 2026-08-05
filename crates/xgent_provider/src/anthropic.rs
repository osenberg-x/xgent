//! Anthropic 原生适配器。
//!
//! 实现 Anthropic Messages API（`/v1/messages`）的流式对话与工具调用。
//! 使用 `x-api-key` 头鉴权 + `anthropic-version` 头指定协议版本。
//!
//! Anthropic SSE 事件类型：
//! - `message_start`：会话开始，含 model
//! - `content_block_start`：内容块开始（text 或 tool_use）
//! - `content_block_delta`：内容块增量（text_delta 或 input_json_delta）
//! - `content_block_stop`：内容块结束
//! - `message_delta`：消息级增量（含 stop_reason）
//! - `message_stop`：消息结束

use async_trait::async_trait;
use eventsource_stream::Eventsource;
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
use crate::sse::parse_sse_events;

/// Anthropic API 版本头。
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// 等待首个 SSE 事件的最大秒数。
const FIRST_EVENT_TIMEOUT_SECS: u64 = 30;

/// 流式消费中相邻两个事件之间的最大空闲秒数。
const IDLE_TIMEOUT_SECS: u64 = 60;

/// Anthropic 原生适配器。
pub struct AnthropicProvider {
    /// provider id，如 "anthropic"
    id: String,
    /// API 基础 URL，如 "https://api.anthropic.com"
    api_base: String,
    /// API Key
    api_key: String,
    /// 复用的 HTTP 客户端
    client: Client,
}

impl AnthropicProvider {
    /// 构造适配器。
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

    /// messages 端点 URL。
    fn messages_url(&self) -> String {
        format!("{}/v1/messages", self.api_base.trim_end_matches('/'))
    }

    /// models 端点 URL。
    fn models_url(&self) -> String {
        format!("{}/v1/models", self.api_base.trim_end_matches('/'))
    }

    /// 构造 Anthropic Messages API 请求体。
    ///
    /// Anthropic 与 OpenAI 的主要区别：
    /// - system 消息提取到顶层 `system` 字段
    /// - tools 用 `input_schema` 而非 `parameters`
    /// - tool result 作为 `user` role 消息的 content block
    fn build_messages_body(&self, req: &ChatRequest) -> Value {
        let mut system_text = String::new();
        let mut messages: Vec<Value> = Vec::new();

        for m in &req.messages {
            match m.role {
                xgent_core::chat::Role::System => {
                    if !system_text.is_empty() {
                        system_text.push('\n');
                    }
                    system_text.push_str(&blocks_to_text(&m.content));
                }
                xgent_core::chat::Role::User => {
                    let content = message_content_to_anthropic(&m.content);
                    messages.push(json!({"role": "user", "content": content}));
                }
                xgent_core::chat::Role::Assistant => {
                    let content = message_content_to_anthropic(&m.content);
                    messages.push(json!({"role": "assistant", "content": content}));
                }
                xgent_core::chat::Role::Tool => {
                    // Anthropic: tool result 作为 user 消息的 content block
                    let tool_result = m.content.iter().find_map(|b| match b {
                        xgent_core::chat::ContentBlock::ToolResult {
                            tool_call_id,
                            content,
                            is_error,
                        } => Some(json!({
                            "type": "tool_result",
                            "tool_use_id": tool_call_id,
                            "content": content,
                            "is_error": is_error,
                        })),
                        _ => None,
                    });
                    if let Some(tr) = tool_result {
                        // 合并到上一条 user 消息（Anthropic 要求连续 tool_result 在同一 user 消息）
                        if let Some(last) = messages.last_mut()
                            && last["role"] == "user"
                        {
                            if let Some(arr) = last["content"].as_array_mut() {
                                arr.push(tr);
                                continue;
                            }
                        }
                        // 否则新建 user 消息
                        messages.push(json!({"role": "user", "content": [tr]}));
                    }
                }
            }
        }

        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "max_tokens": 8192,
            "stream": true,
        });
        if !system_text.is_empty() {
            body["system"] = json!(system_text);
        }
        if let Some(tools) = &req.tools
            && !tools.is_empty()
        {
            let tools_json: Vec<Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.input_schema,
                    })
                })
                .collect();
            body["tools"] = json!(tools_json);
        }
        body
    }
}

/// 把 ContentBlock 列表转为 Anthropic 格式的 content array。
///
/// Anthropic content block 格式：
/// - Text → `{"type":"text","text":"..."}`
/// - ToolCall → `{"type":"tool_use","id":"...","name":"...","input":{...}}`
/// - ToolResult → `{"type":"tool_result","tool_use_id":"...","content":"..."}`
fn message_content_to_anthropic(content: &[xgent_core::chat::ContentBlock]) -> Vec<Value> {
    content
        .iter()
        .filter_map(|b| match b {
            xgent_core::chat::ContentBlock::Text { text } => {
                Some(json!({"type": "text", "text": text}))
            }
            xgent_core::chat::ContentBlock::ToolCall { id, name, args } => Some(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": args,
            })),
            xgent_core::chat::ContentBlock::ToolResult {
                tool_call_id,
                content,
                is_error,
            } => Some(json!({
                "type": "tool_result",
                "tool_use_id": tool_call_id,
                "content": content,
                "is_error": is_error,
            })),
            _ => None,
        })
        .collect()
}

/// 从 content blocks 提取所有 Text 块拼接为字符串。
fn blocks_to_text(content: &[xgent_core::chat::ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|b| match b {
            xgent_core::chat::ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let resp = self
            .client
            .get(self.models_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
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
                let name = m["display_name"].as_str().unwrap_or(&id).to_string();
                Some(ModelInfo {
                    name,
                    id,
                    context_window: m["context_window"]
                        .as_u64()
                        .map(|n| n as u32),
                })
            })
            .collect();
        Ok(result)
    }

    async fn chat(&self, req: ChatRequest) -> Result<(StreamId, ChatStream), ProviderError> {
        if self.api_base.is_empty() {
            return Err(ProviderError::Config("api_base 未配置".into()));
        }
        if self.api_key.is_empty() {
            return Err(ProviderError::Config("api_key 未配置".into()));
        }

        let body = self.build_messages_body(&req);
        let resp = self
            .client
            .post(self.messages_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
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

        let stream = parse_sse_events(resp.bytes_stream().eventsource());

        let stream_id = next_stream_id();
        let (tx, rx) = mpsc::channel::<ChatEvent>(64);

        let model = req.model.clone();
        tokio::spawn(async move {
            run_anthropic_stream(stream, model, tx).await;
        });

        Ok((stream_id, rx))
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        // 简单检查：发一个最小请求看是否返回 401（有 key 但无效）或 200
        if self.api_key.is_empty() {
            return Err(ProviderError::Config("api_key 未配置".into()));
        }
        Ok(())
    }
}

/// 消费 Anthropic SSE 流，转换为 ChatEvent。
///
/// Anthropic SSE 事件类型：
/// - `message_start`：含 model 信息 → 发 `Start{model}`
/// - `content_block_start`：内容块开始
///   - type=text → 发 `TextStart`
///   - type=tool_use → 发 `ToolCallStart{id, name}`
/// - `content_block_delta`：内容块增量
///   - type=text_delta → 发 `TextDelta{text}`
///   - type=input_json_delta → 发 `ToolCallDelta{partial_json}`
/// - `content_block_stop`：内容块结束
///   - 若为文本块 → 发 `TextEnd`
///   - 若为 tool_use 块 → 发 `ToolCallEnd{args}`
/// - `message_delta`：含 `stop_reason` 与 `usage`
///   - stop_reason → 映射 StopReason，发 `Done{reason, usage}`
/// - `message_stop`：消息结束（已由 message_delta 处理 Done）
/// - `ping`：心跳，跳过
async fn run_anthropic_stream<S>(stream: S, model: String, tx: mpsc::Sender<ChatEvent>)
where
    S: Stream<Item = Result<Value, ProviderError>> + Send + 'static,
{
    run_anthropic_stream_with_timeout(
        stream,
        model,
        tx,
        Duration::from_secs(FIRST_EVENT_TIMEOUT_SECS),
        Duration::from_secs(IDLE_TIMEOUT_SECS),
    )
    .await;
}

async fn run_anthropic_stream_with_timeout<S>(
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
    let mut text_started = false;
    let mut finished = false;
    // 当前 tool_use 块的累积器（Anthropic 一次只有一个 tool_use 块在流中）
    let mut tool_accum: Option<ToolUseAccum> = None;

    // 流开始：先发 Start（model 可能从 message_start 更新）
    let _ = tx.send(ChatEvent::Start { model }).await;

    // 首事件
    let first = match timeout(first_timeout, s.next()).await {
        Ok(Some(item)) => item,
        Ok(None) => {
            finish_anthropic_stream(&tx, &mut text_started, finished).await;
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

    if !handle_anthropic_item(first, &tx, &mut text_started, &mut finished, &mut tool_accum).await {
        return;
    }

    loop {
        match timeout(idle_timeout, s.next()).await {
            Ok(Some(item)) => {
                if !handle_anthropic_item(
                    item,
                    &tx,
                    &mut text_started,
                    &mut finished,
                    &mut tool_accum,
                )
                .await
                {
                    return;
                }
            }
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

    finish_anthropic_stream(&tx, &mut text_started, finished).await;
}

/// 处理单个 Anthropic SSE 事件。
async fn handle_anthropic_item(
    item: Result<Value, ProviderError>,
    tx: &mpsc::Sender<ChatEvent>,
    text_started: &mut bool,
    finished: &mut bool,
    tool_accum: &mut Option<ToolUseAccum>,
) -> bool {
    match item {
        Ok(v) => {
            if let Err(e) =
                handle_anthropic_chunk(&v, tx, text_started, finished, tool_accum).await
            {
                let _ = tx
                    .send(ChatEvent::Error {
                        kind: e.to_error_kind(),
                        message: e.to_string(),
                    })
                    .await;
                return false;
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

/// Anthropic tool_use 块累积器。
struct ToolUseAccum {
    index: u32,
    id: String,
    name: String,
    args: String,
}

/// 处理单个 Anthropic SSE 事件 JSON。
async fn handle_anthropic_chunk(
    v: &Value,
    tx: &mpsc::Sender<ChatEvent>,
    text_started: &mut bool,
    finished: &mut bool,
    tool_accum: &mut Option<ToolUseAccum>,
) -> Result<(), ProviderError> {
    let event_type = v["type"].as_str().unwrap_or("");

    match event_type {
        "message_start" => {
            // 模型信息可在此更新（暂忽略，Start 已发）
        }
        "content_block_start" => {
            let block_type = v["content_block"]["type"].as_str().unwrap_or("");
            let index = v["index"].as_u64().unwrap_or(0) as u32;
            match block_type {
                "text" => {
                    if !*text_started {
                        *text_started = true;
                        let _ = tx.send(ChatEvent::TextStart).await;
                    }
                }
                "tool_use" => {
                    let id = v["content_block"]["id"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    let name = v["content_block"]["name"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    *tool_accum = Some(ToolUseAccum {
                        index,
                        id,
                        name,
                        args: String::new(),
                    });
                    let _ = tx
                        .send(ChatEvent::ToolCallStart {
                            index,
                            id: v["content_block"]["id"].as_str().unwrap_or("").to_string(),
                            name: v["content_block"]["name"].as_str().unwrap_or("").to_string(),
                        })
                        .await;
                }
                _ => {}
            }
        }
        "content_block_delta" => {
            let delta_type = v["delta"]["type"].as_str().unwrap_or("");
            match delta_type {
                "text_delta" => {
                    if let Some(text) = v["delta"]["text"].as_str() {
                        if !*text_started {
                            *text_started = true;
                            let _ = tx.send(ChatEvent::TextStart).await;
                        }
                        let _ = tx
                            .send(ChatEvent::TextDelta {
                                text: text.to_string(),
                            })
                            .await;
                    }
                }
                "input_json_delta" => {
                    if let Some(partial) = v["delta"]["partial_json"].as_str() {
                        if let Some(accum) = tool_accum {
                            accum.args.push_str(partial);
                            let _ = tx
                                .send(ChatEvent::ToolCallDelta {
                                    index: accum.index,
                                    partial_json: partial.to_string(),
                                })
                                .await;
                        }
                    }
                }
                _ => {}
            }
        }
        "content_block_stop" => {
            let index = v["index"].as_u64().unwrap_or(0) as u32;
            // 判断是文本块还是 tool_use 块结束
            if let Some(accum) = tool_accum.take() {
                if accum.index == index {
                    let args_val: Value = if accum.args.is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&accum.args).unwrap_or(json!({}))
                    };
                    let _ = tx
                        .send(ChatEvent::ToolCallEnd {
                            index: accum.index,
                            args: args_val,
                        })
                        .await;
                } else {
                    // index 不匹配，放回
                    *tool_accum = Some(accum);
                }
            } else if *text_started {
                *text_started = false;
                let _ = tx.send(ChatEvent::TextEnd).await;
            }
        }
        "message_delta" => {
            if !*finished {
                let stop_reason = v["delta"]["stop_reason"]
                    .as_str()
                    .map(map_anthropic_stop_reason)
                    .unwrap_or(xgent_core::chat::StopReason::Stop);
                let usage = extract_anthropic_usage(v);
                // 文本块结束（若已开始）
                if *text_started {
                    *text_started = false;
                    let _ = tx.send(ChatEvent::TextEnd).await;
                }
                // tool_use 块结束（若有未关闭的）
                if let Some(accum) = tool_accum.take() {
                    let args_val: Value = if accum.args.is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&accum.args).unwrap_or(json!({}))
                    };
                    let _ = tx
                        .send(ChatEvent::ToolCallEnd {
                            index: accum.index,
                            args: args_val,
                        })
                        .await;
                }
                let _ = tx
                    .send(ChatEvent::Done {
                        reason: stop_reason,
                        usage: usage.unwrap_or_default(),
                    })
                    .await;
                *finished = true;
            }
        }
        "message_stop" => {
            // 已由 message_delta 处理
        }
        "ping" | "error" => {
            // ping: 心跳跳过
            // error: Anthropic 流中错误事件
            if event_type == "error" {
                let msg = v["error"]["message"]
                    .as_str()
                    .unwrap_or("unknown anthropic stream error")
                    .to_string();
                return Err(ProviderError::Stream(msg));
            }
        }
        _ => {}
    }

    Ok(())
}

/// 把 Anthropic stop_reason 映射为 StopReason。
fn map_anthropic_stop_reason(reason: &str) -> xgent_core::chat::StopReason {
    use xgent_core::chat::StopReason;
    match reason {
        "end_turn" => StopReason::Stop,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::Length,
        "stop_sequence" => StopReason::Stop,
        _ => StopReason::Stop,
    }
}

/// 从 message_delta 事件提取 usage。
fn extract_anthropic_usage(v: &Value) -> Option<TokenUsage> {
    let u = &v["usage"];
    if u.is_null() {
        return None;
    }
    Some(TokenUsage {
        prompt: u["input_tokens"].as_u64().unwrap_or(0) as u32,
        completion: u["output_tokens"].as_u64().unwrap_or(0) as u32,
    })
}

/// 流收尾：若未发 Done 则补发。
async fn finish_anthropic_stream(
    tx: &mpsc::Sender<ChatEvent>,
    text_started: &mut bool,
    finished: bool,
) {
    if !finished {
        if *text_started {
            *text_started = false;
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

/// 生成伪随机 StreamId（时间戳低 32 位）。
fn next_stream_id() -> StreamId {
    use std::time::SystemTime;
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    StreamId(ts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eventsource_stream::Event;
    use futures::stream;

    fn ev(data: &str) -> Result<Event, std::io::Error> {
        Ok(Event {
            event: String::new(),
            data: data.to_string(),
            id: String::new(),
            retry: None,
        })
    }

    #[tokio::test]
    async fn anthropic_text_stream() {
        let events = stream::iter(vec![
            ev(r#"{"type":"message_start","message":{"model":"claude-3"}}"#),
            ev(r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#),
            ev(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#),
            ev(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}"#),
            ev(r#"{"type":"content_block_stop","index":0}"#),
            ev(r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"input_tokens":10,"output_tokens":5}}"#),
            ev(r#"{"type":"message_stop"}"#),
        ]);
        let stream = parse_sse_events(events);
        let (tx, mut rx) = mpsc::channel(64);
        run_anthropic_stream(stream, "claude-3".into(), tx).await;

        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }

        // 应有：Start, TextStart, TextDelta(Hello), TextDelta(world), TextEnd, Done
        assert!(events.len() >= 6);
        assert!(matches!(&events[0], ChatEvent::Start { model } if model == "claude-3"));
        assert!(matches!(&events[1], ChatEvent::TextStart));
        assert!(matches!(
            &events[2],
            ChatEvent::TextDelta { text } if text == "Hello"
        ));
        assert!(matches!(
            &events[3],
            ChatEvent::TextDelta { text } if text == " world"
        ));
        assert!(matches!(&events[4], ChatEvent::TextEnd));
        assert!(matches!(
            &events[5],
            ChatEvent::Done {
                reason: xgent_core::chat::StopReason::Stop,
                usage
            } if usage.prompt == 10 && usage.completion == 5
        ));
    }

    #[tokio::test]
    async fn anthropic_tool_use_stream() {
        let events = stream::iter(vec![
            ev(r#"{"type":"message_start","message":{"model":"claude-3"}}"#),
            ev(r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"tool_1","name":"read_file"}}"#),
            ev(r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}"#),
            ev(r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"test.rs\"}"}}"#),
            ev(r#"{"type":"content_block_stop","index":0}"#),
            ev(r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":10,"output_tokens":5}}"#),
            ev(r#"{"type":"message_stop"}"#),
        ]);
        let stream = parse_sse_events(events);
        let (tx, mut rx) = mpsc::channel(64);
        run_anthropic_stream(stream, "claude-3".into(), tx).await;

        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }

        // 应有：Start, ToolCallStart, ToolCallDelta x2, ToolCallEnd, Done(ToolUse)
        assert!(events.len() >= 5);
        assert!(matches!(
            &events[1],
            ChatEvent::ToolCallStart { id, name, .. } if id == "tool_1" && name == "read_file"
        ));
        assert!(matches!(
            &events[4],
            ChatEvent::ToolCallEnd { args, .. } if args["path"] == "test.rs"
        ));
        assert!(matches!(
            &events[5],
            ChatEvent::Done {
                reason: xgent_core::chat::StopReason::ToolUse,
                ..
            }
        ));
    }

    #[test]
    fn build_messages_body_extracts_system() {
        let provider = AnthropicProvider::new(
            "test".into(),
            "https://api.anthropic.com".into(),
            "key".into(),
        );
        let req = ChatRequest {
            provider: "test".into(),
            model: "claude-3".into(),
            messages: vec![
                ChatMessage::text(xgent_core::chat::Role::System, "You are helpful"),
                ChatMessage::text(xgent_core::chat::Role::User, "Hello"),
            ],
            tools: None,
        };
        let body = provider.build_messages_body(&req);
        assert_eq!(body["system"], "You are helpful");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
    }
}
