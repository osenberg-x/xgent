//! Response API 风格适配器（OpenAI Responses API）。
//!
//! 走 `/v1/responses`：请求用 `input` 项数组，响应以 `response.*` 语义化
//! SSE 事件返回（`response.output_text.delta` / `response.output_item.done`
//! / `response.completed`）。本适配器把这些事件翻译成项目统一的
//! [`ChatEvent`] 序列，与 OpenAI chat-completions 适配器对下游等价。
//!
//! 保留 OpenAI 兼容适配器的既有时序：首事件/空闲超时、流中 `error` 帧检测、
//! 按 index 聚合工具调用、usage 映射。

use async_trait::async_trait;
use futures::Stream;
use reqwest::Client;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_stream::StreamExt;
use xgent_core::chat::{ChatEvent, ChatMessage, ChatRequest, StopReason, TokenUsage};
use xgent_core::ids::StreamId;

use crate::provider::{ChatStream, LlmProvider, ModelInfo, ProviderError};
use crate::sse::parse_response_stream;

/// 流式消费中相邻事件的空闲上限。
///
/// Responses API 无独立的"首事件"超时：首个事件同样受本阈值约束
/// （建流阶段已有 `request_timeout` 覆盖响应头等待，见 `chat()`）。
const IDLE_TIMEOUT_SECS: u64 = 60;

/// Response API 适配器。
pub struct ResponseApiProvider {
    /// provider id
    id: String,
    /// API 基础 URL（如 `https://api.openai.com/v1`）
    api_base: String,
    /// API Key
    api_key: String,
    /// 非流式请求超时
    request_timeout: Duration,
    /// 复用的 HTTP 客户端
    client: Client,
}

impl ResponseApiProvider {
    /// 用配置与已解析的 API Key 构造。
    pub fn with_config(
        id: String,
        cfg: &xgent_settings_core::global::ProviderConfig,
        api_key: String,
    ) -> Self {
        Self::with_timeout(id, cfg.api_base.clone(), api_key, cfg.timeout_secs)
    }

    /// 构造（api_base 为空时用 OpenAI 官方地址）。
    pub fn with_timeout(id: String, api_base: String, api_key: String, timeout_secs: u64) -> Self {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Self {
            id,
            api_base,
            api_key,
            client,
            request_timeout: Duration::from_secs(timeout_secs),
        }
    }

    /// 保留旧构造签名（无配置时用官方 base）。
    pub fn new(id: String) -> Self {
        Self::with_timeout(
            id,
            "https://api.openai.com/v1".to_string(),
            String::new(),
            60,
        )
    }

    fn base(&self) -> String {
        if self.api_base.trim().is_empty() {
            "https://api.openai.com/v1".to_string()
        } else {
            self.api_base.trim_end_matches('/').to_string()
        }
    }

    fn responses_url(&self) -> String {
        format!("{}/responses", self.base())
    }

    fn models_url(&self) -> String {
        format!("{}/models", self.base())
    }

    /// 构造 Responses API 请求体。
    ///
    /// `messages` 按 role 转成 `input` 项；tool 结果作为 `function_call_output`
    /// 项（Responses API 的工具回灌形态）。
    fn build_body(&self, req: &ChatRequest) -> Value {
        let input = build_input_items(&req.messages);
        let mut body = json!({
            "model": req.model,
            "input": input,
            "stream": true,
        });
        if let Some(tools) = &req.tools
            && !tools.is_empty()
        {
            // Responses API 的工具 schema 是扁平结构（无 function 包装层）
            let tools_json: Vec<Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    })
                })
                .collect();
            body["tools"] = json!(tools_json);
        }
        body
    }
}

/// 把 [`ChatMessage`] 序列转为 Responses API 的 `input` 项数组。
pub fn build_input_items(messages: &[ChatMessage]) -> Vec<Value> {
    use xgent_core::chat::{ContentBlock, Role};
    let mut out: Vec<Value> = Vec::new();
    for m in messages {
        match m.role {
            Role::System => {
                let text = plain_text(&m.content);
                if !text.is_empty() {
                    out.push(json!({
                        "role": "system",
                        "content": [{"type": "input_text", "text": text}],
                    }));
                }
            }
            Role::User => {
                let text = plain_text(&m.content);
                out.push(json!({
                    "role": "user",
                    "content": [{"type": "input_text", "text": text}],
                }));
            }
            Role::Assistant => {
                let text = plain_text(&m.content);
                if !text.is_empty() {
                    out.push(json!({
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": text}],
                    }));
                }
                // 工具调用：Responses API 用 call_id 关联
                for b in &m.content {
                    if let ContentBlock::ToolCall { id, name, args } = b {
                        out.push(json!({
                            "type": "function_call",
                            "call_id": id,
                            "name": name,
                            "arguments": args.to_string(),
                        }));
                    }
                }
            }
            Role::Tool => {
                for b in &m.content {
                    if let ContentBlock::ToolResult {
                        tool_call_id,
                        content,
                        ..
                    } = b
                    {
                        out.push(json!({
                            "type": "function_call_output",
                            "call_id": tool_call_id,
                            "output": content,
                        }));
                    }
                }
            }
        }
    }
    out
}

fn plain_text(content: &[xgent_core::chat::ContentBlock]) -> String {
    use xgent_core::chat::ContentBlock;
    content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[async_trait]
impl LlmProvider for ResponseApiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let resp = self
            .client
            .get(self.models_url())
            .bearer_auth(&self.api_key)
            .timeout(self.request_timeout)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(ProviderError::Api {
                retry_after_secs: crate::provider::parse_retry_after(resp.headers()),
                status: resp.status().as_u16(),
                body: resp.text().await.unwrap_or_default(),
            });
        }
        let v: Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Stream(e.to_string()))?;
        Ok(v["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        let id = m["id"].as_str()?.to_string();
                        Some(ModelInfo {
                            name: id.clone(),
                            id,
                            context_window: m["context_window"].as_u64().map(|n| n as u32),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn chat(&self, req: ChatRequest) -> Result<(StreamId, ChatStream), ProviderError> {
        let body = self.build_body(&req);
        let resp = self
            .client
            .post(self.responses_url())
            .bearer_auth(&self.api_key)
            .json(&body)
            .timeout(self.request_timeout)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(ProviderError::Api {
                retry_after_secs: crate::provider::parse_retry_after(resp.headers()),
                status: resp.status().as_u16(),
                body: resp.text().await.unwrap_or_default(),
            });
        }
        let model = req.model.clone();
        let stream = parse_response_stream(resp);
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            run_responses_stream(stream, model, tx).await;
        });
        Ok((StreamId(0), rx))
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        let resp = self
            .client
            .get(self.models_url())
            .bearer_auth(&self.api_key)
            .timeout(self.request_timeout)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ProviderError::Api {
                retry_after_secs: crate::provider::parse_retry_after(resp.headers()),
                status: resp.status().as_u16(),
                body: resp.text().await.unwrap_or_default(),
            })
        }
    }
}

/// Responses API 事件 → 统一 [`ChatEvent`]。
///
/// 发射规则与 chat-completions 适配器一致：
/// - 文本：首个非空 delta 发 `TextStart`，后续发 `TextDelta`，完成时 `TextEnd`
/// - 工具：`function_call` 项完成时发 `ToolCallStart` → `ToolCallDelta` → `ToolCallEnd`
/// - 结束：统一在流末尾发 `Done`（usage 可能晚于 `response.completed`）
async fn run_responses_stream<S>(stream: S, model: String, tx: mpsc::Sender<ChatEvent>)
where
    S: Stream<Item = Result<Value, ProviderError>> + Send + 'static,
{
    let mut s = Box::pin(stream);
    let mut text_started = false;
    let mut usage: Option<TokenUsage> = None;
    let mut stop_reason = StopReason::Stop;
    let mut tool_index: u32 = 0;

    // 接收端已丢弃时不得拉取上游（含首事件）——与 openai_compat / anthropic
    // 的守卫位置对齐，否则 abort 后仍会多读一帧（R1-2 / A4）。
    if tx.is_closed() {
        return;
    }
    let _ = tx.send(ChatEvent::Start { model }).await;

    loop {
        // 接收端已丢弃（agent abort / daemon 取消流）时停止拉取上游 body——
        // 否则被放弃的流继续生成并计费，正是 R1-2 要消除的失效模式（R1-4）。
        if tx.is_closed() {
            return;
        }
        let next = timeout(Duration::from_secs(IDLE_TIMEOUT_SECS), s.next()).await;
        let item = match next {
            Ok(Some(i)) => i,
            Ok(None) => break,
            Err(_) => {
                let _ = tx
                    .send(ChatEvent::Error {
                        kind: xgent_core::chat::ErrorKind::Network,
                        message: "responses stream idle timeout".into(),

                        retry_after_secs: None,
                    })
                    .await;
                return;
            }
        };
        let v = match item {
            Ok(v) => v,
            Err(e) => {
                let _ = tx
                    .send(ChatEvent::Error {
                        kind: xgent_core::chat::ErrorKind::StreamParse,
                        message: e.to_string(),

                        retry_after_secs: None,
                    })
                    .await;
                return;
            }
        };
        let ty = v["type"].as_str().unwrap_or_default();

        // 流中错误帧
        if ty == "error" || ty == "response.failed" {
            let msg = v["response"]["error"]["message"]
                .as_str()
                .or_else(|| v["message"].as_str())
                .unwrap_or("responses stream error")
                .to_string();
            let _ = tx
                .send(ChatEvent::Error {
                    kind: xgent_core::chat::ErrorKind::ProviderError,
                    message: msg,

                    retry_after_secs: None,
                })
                .await;
            return;
        }

        match ty {
            "response.output_text.delta" => {
                let d = v["delta"].as_str().unwrap_or_default();
                if d.is_empty() {
                    continue;
                }
                if !text_started {
                    text_started = true;
                    let _ = tx.send(ChatEvent::TextStart).await;
                }
                let _ = tx.send(ChatEvent::TextDelta { text: d.into() }).await;
            }
            "response.output_item.done" => {
                let item = &v["item"];
                if item["type"].as_str() == Some("function_call") {
                    let id = item["call_id"]
                        .as_str()
                        .or_else(|| item["id"].as_str())
                        .unwrap_or_default()
                        .to_string();
                    let name = item["name"].as_str().unwrap_or_default().to_string();
                    let args = item["arguments"].as_str().unwrap_or("{}").to_string();
                    let idx = tool_index;
                    tool_index += 1;
                    let _ = tx
                        .send(ChatEvent::ToolCallStart {
                            index: idx,
                            id,
                            name,
                        })
                        .await;
                    if args != "{}" {
                        let _ = tx
                            .send(ChatEvent::ToolCallDelta {
                                index: idx,
                                partial_json: args.clone(),
                            })
                            .await;
                    }
                    let parsed = serde_json::from_str(&args).unwrap_or_else(|e| {
                        // 保留错误标记而非静默变 {}（协议兼容策略）
                        json!({ "__parse_error": e.to_string(), "__raw": args.clone() })
                    });
                    let _ = tx
                        .send(ChatEvent::ToolCallEnd {
                            index: idx,
                            args: parsed,
                        })
                        .await;
                }
            }
            "response.completed" | "response.incomplete" => {
                if ty == "response.incomplete" {
                    stop_reason = StopReason::Length;
                }
                if let Some(u) = extract_usage(&v["response"]["usage"]) {
                    usage = Some(u);
                }
            }
            _ => {}
        }
    }

    if text_started {
        let _ = tx.send(ChatEvent::TextEnd).await;
    }
    let _ = tx
        .send(ChatEvent::Done {
            reason: stop_reason,
            usage: usage.unwrap_or_default(),
        })
        .await;
}

/// Responses API 的 usage 字段名与 chat-completions 不同（`input_tokens` /
/// `output_tokens`）。
fn extract_usage(v: &Value) -> Option<TokenUsage> {
    let input = v["input_tokens"].as_u64()?;
    let output = v["output_tokens"].as_u64().unwrap_or(0);
    Some(TokenUsage {
        prompt: input.min(u32::MAX as u64) as u32,
        completion: output.min(u32::MAX as u64) as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use xgent_core::chat::{ContentBlock, Role};

    fn msg(role: Role, content: Vec<ContentBlock>) -> ChatMessage {
        ChatMessage { role, content }
    }

    #[test]
    fn system_and_user_become_input_items() {
        let msgs = vec![
            msg(
                Role::System,
                vec![ContentBlock::Text { text: "sys".into() }],
            ),
            msg(Role::User, vec![ContentBlock::Text { text: "hi".into() }]),
        ];
        let items = build_input_items(&msgs);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["role"], "system");
        assert_eq!(items[1]["content"][0]["type"], "input_text");
        assert_eq!(items[1]["content"][0]["text"], "hi");
    }

    #[test]
    fn assistant_text_and_tool_calls_both_enter_input() {
        let msgs = vec![msg(
            Role::Assistant,
            vec![
                ContentBlock::Text { text: "ok".into() },
                ContentBlock::ToolCall {
                    id: "c1".into(),
                    name: "read_file".into(),
                    args: json!({"path": "a"}),
                },
            ],
        )];
        let items = build_input_items(&msgs);
        assert_eq!(items.len(), 2);
        assert_eq!(items[1]["type"], "function_call");
        assert_eq!(items[1]["call_id"], "c1");
        assert_eq!(items[1]["name"], "read_file");
        assert!(items[1]["arguments"].as_str().unwrap().contains("path"));
    }

    #[test]
    fn tool_result_becomes_function_call_output() {
        let msgs = vec![msg(
            Role::Tool,
            vec![ContentBlock::ToolResult {
                tool_call_id: "c1".into(),
                content: "done".into(),
                is_error: false,
            }],
        )];
        let items = build_input_items(&msgs);
        assert_eq!(items[0]["type"], "function_call_output");
        assert_eq!(items[0]["call_id"], "c1");
        assert_eq!(items[0]["output"], "done");
    }

    #[test]
    fn usage_field_names_are_mapped() {
        let u = extract_usage(&json!({"input_tokens": 12, "output_tokens": 7})).unwrap();
        assert_eq!(u.prompt, 12);
        assert_eq!(u.completion, 7);
    }

    #[test]
    fn missing_usage_returns_none() {
        assert!(extract_usage(&json!({})).is_none());
    }

    #[tokio::test]
    async fn text_events_map_to_chat_events() {
        let frames = vec![
            Ok(json!({"type": "response.output_text.delta", "delta": "He"})),
            Ok(json!({"type": "response.output_text.delta", "delta": "llo"})),
            Ok(json!({"type": "response.completed",
                      "response": {"usage": {"input_tokens": 3, "output_tokens": 2}}})),
        ];
        let (tx, mut rx) = mpsc::channel(64);
        let s = futures::stream::iter(frames);
        tokio::spawn(async move { run_responses_stream(s, "m".into(), tx).await });
        let mut got = Vec::new();
        while let Some(e) = rx.recv().await {
            got.push(e);
        }
        assert!(matches!(got[0], ChatEvent::Start { .. }));
        assert!(matches!(got[1], ChatEvent::TextStart));
        assert!(matches!(got[2], ChatEvent::TextDelta { .. }));
        assert!(matches!(got[3], ChatEvent::TextDelta { .. }));
        assert!(matches!(got[4], ChatEvent::TextEnd));
        match &got[5] {
            ChatEvent::Done { usage, .. } => assert_eq!(usage.prompt, 3),
            other => panic!("期望 Done，得到 {other:?}"),
        }
    }

    #[tokio::test]
    async fn function_call_item_maps_to_tool_events() {
        let frames = vec![Ok(json!({
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "call_id": "call_9",
                "name": "read_file",
                "arguments": "{\"path\":\"x\"}"
            }
        }))];
        let (tx, mut rx) = mpsc::channel(64);
        let s = futures::stream::iter(frames);
        tokio::spawn(async move { run_responses_stream(s, "m".into(), tx).await });
        let mut start = None;
        let mut end = None;
        let mut done = false;
        while let Some(e) = rx.recv().await {
            match e {
                ChatEvent::ToolCallStart { index, id, name } => {
                    start = Some((index, id, name));
                }
                ChatEvent::ToolCallEnd { index, args } => {
                    end = Some((index, args));
                }
                ChatEvent::Done { .. } => done = true,
                _ => {}
            }
        }
        assert_eq!(
            start,
            Some((0, "call_9".to_string(), "read_file".to_string()))
        );
        let (idx, args) = end.expect("应有 ToolCallEnd");
        assert_eq!(idx, 0);
        assert_eq!(args["path"], "x");
        assert!(done, "流末尾应发 Done");
    }

    #[tokio::test]
    async fn error_frame_maps_to_error_event() {
        let frames = vec![
            Ok(json!({"type": "error", "message": "boom"})),
            Ok(json!({"type": "response.output_text.delta", "delta": "不该到达"})),
        ];
        let (tx, mut rx) = mpsc::channel(64);
        let s = futures::stream::iter(frames);
        tokio::spawn(async move { run_responses_stream(s, "m".into(), tx).await });
        let mut saw_error = false;
        while let Some(e) = rx.recv().await {
            if let ChatEvent::Error { message, .. } = e {
                assert_eq!(message, "boom");
                saw_error = true;
            }
        }
        assert!(saw_error, "应发出 Error 事件");
    }

    #[tokio::test]
    async fn incomplete_response_is_length() {
        let frames = vec![Ok(
            json!({"type": "response.incomplete", "response": {"usage": {"input_tokens": 1, "output_tokens": 1}}}),
        )];
        let (tx, mut rx) = mpsc::channel(64);
        let s = futures::stream::iter(frames);
        tokio::spawn(async move { run_responses_stream(s, "m".into(), tx).await });
        while let Some(e) = rx.recv().await {
            if let ChatEvent::Done { reason, .. } = e {
                assert_eq!(reason, StopReason::Length);
            }
        }
    }
}
