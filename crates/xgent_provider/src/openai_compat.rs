//! OpenAI compatible 接口适配器。
//!
//! 适用于 OpenAI、DeepSeek、Ollama 兼容模式等遵循 OpenAI `/v1/chat/completions`
//! 协议的 provider。支持流式输出与工具调用（按 index 聚合）。

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use xgent_core::chat::{ChatEvent, ChatMessage, ChatRequest, TokenUsage};
use xgent_core::ids::StreamId;

use crate::provider::{ChatStream, LlmProvider, ModelInfo, ProviderError};
use crate::sse::parse_response_stream;

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

/// 把 [`ChatMessage`] 转为 OpenAI API 的 message JSON。
fn message_to_json(m: &ChatMessage) -> Value {
    json!({
        "role": role_str(m.role),
        "content": m.content,
    })
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

        tokio::spawn(async move {
            let mut s = Box::pin(stream);
            // 工具调用按 index 聚合（OpenAI 分块到达）
            let mut tool_calls: Vec<ToolCallAccum> = Vec::new();
            while let Some(item) = s.next().await {
                match item {
                    Ok(v) => {
                        if let Err(e) = handle_chunk(&v, &tx, &mut tool_calls).await {
                            let _ = tx
                                .send(ChatEvent::Error {
                                    kind: e.to_error_kind(),
                                    message: e.to_string(),
                                })
                                .await;
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = tx
                            .send(ChatEvent::Error {
                                kind: e.to_error_kind(),
                                message: e.to_string(),
                            })
                            .await;
                        return;
                    }
                }
            }
            // 流自然结束（未收到 Done）也补一个 Done
            let _ = tx
                .send(ChatEvent::Done {
                    usage: TokenUsage::default(),
                })
                .await;
        });

        Ok((stream_id, rx))
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        // list_models 成功即健康
        self.list_models().await.map(|_| ())
    }
}

/// 工具调用累积器（按 index 聚合分块）。
#[derive(Default, Clone)]
struct ToolCallAccum {
    id: String,
    name: String,
    args: String,
}

/// 处理单个 SSE chunk，转换为 [`ChatEvent`] 发送。
///
/// 返回 `Err` 表示致命解析错误，应终止流。
async fn handle_chunk(
    v: &Value,
    tx: &mpsc::Sender<ChatEvent>,
    tool_calls: &mut Vec<ToolCallAccum>,
) -> Result<(), ProviderError> {
    // usage 可能在最后一个 chunk（finish_reason + stream_options.include_usage）
    let usage = extract_usage(v);

    let choices: &[Value] = match v["choices"].as_array() {
        Some(c) if !c.is_empty() => c,
        _ => {
            // 无 choices 或空 choices（纯 usage chunk）
            if let Some(u) = usage {
                let _ = tx.send(ChatEvent::Done { usage: u }).await;
            }
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
                if let Some(id) = tc["id"].as_str() {
                    accum.id = id.to_string();
                }
                if let Some(name) = tc["function"]["name"].as_str() {
                    accum.name = name.to_string();
                }
                if let Some(args) = tc["function"]["arguments"].as_str() {
                    accum.args.push_str(args);
                }
            }
        }

        // 文本 delta
        if let Some(content) = choice["delta"]["content"].as_str()
            && !content.is_empty()
        {
            let _ = tx
                .send(ChatEvent::Delta {
                    text: content.to_string(),
                })
                .await;
        }

        // finish_reason
        if let Some(reason) = choice["finish_reason"].as_str() {
            // 工具调用完成时发 ToolCall（聚合的 args 解析为 JSON）
            for accum in tool_calls.drain(..) {
                if !accum.name.is_empty() {
                    let args_val: Value = if accum.args.is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&accum.args).unwrap_or(json!({}))
                    };
                    let _ = tx
                        .send(ChatEvent::ToolCall {
                            id: accum.id,
                            name: accum.name,
                            args: args_val,
                        })
                        .await;
                }
            }

            if reason == "stop" || reason == "length" || reason == "tool_calls" {
                let _ = tx
                    .send(ChatEvent::Done {
                        usage: usage.clone().unwrap_or_default(),
                    })
                    .await;
            }
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
    async fn handle_chunk_text_delta_sends_delta_event() {
        let (tx, mut rx) = mpsc::channel::<ChatEvent>(8);
        let mut tool_calls = Vec::new();
        let v = chunk(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#);
        handle_chunk(&v, &tx, &mut tool_calls).await.unwrap();
        let ev = recv(&mut rx).await;
        assert!(matches!(ev, ChatEvent::Delta { ref text } if text == "Hello"));
    }

    #[tokio::test]
    async fn handle_chunk_finish_stop_sends_done() {
        let (tx, mut rx) = mpsc::channel::<ChatEvent>(8);
        let mut tool_calls = Vec::new();
        let v = chunk(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#);
        handle_chunk(&v, &tx, &mut tool_calls).await.unwrap();
        let ev = recv(&mut rx).await;
        assert!(matches!(ev, ChatEvent::Done { .. }));
    }

    #[tokio::test]
    async fn handle_chunk_with_usage_sends_done_with_usage() {
        let (tx, mut rx) = mpsc::channel::<ChatEvent>(8);
        let mut tool_calls = Vec::new();
        let v = chunk(r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#);
        handle_chunk(&v, &tx, &mut tool_calls).await.unwrap();
        let ev = recv(&mut rx).await;
        match ev {
            ChatEvent::Done { usage } => {
                assert_eq!(usage.prompt, 10);
                assert_eq!(usage.completion, 5);
            }
            _ => panic!("expected Done"),
        }
    }

    #[tokio::test]
    async fn handle_chunk_tool_call_aggregation() {
        let (tx, mut rx) = mpsc::channel::<ChatEvent>(8);
        let mut tool_calls = Vec::new();
        // 第一块：tool_call 开始
        let v1 = chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}"#,
        );
        handle_chunk(&v1, &tx, &mut tool_calls).await.unwrap();
        // 第二块：arguments 继续
        let v2 = chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"/x\"}"}}]}}]}"#,
        );
        handle_chunk(&v2, &tx, &mut tool_calls).await.unwrap();
        // 第三块：finish
        let v3 = chunk(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#);
        handle_chunk(&v3, &tx, &mut tool_calls).await.unwrap();

        // 处理完毕，drop tx 使 rx.recv() 返回 None 退出循环
        drop(tx);

        // 应收到 ToolCall 事件与 Done
        let mut got_tool_call = false;
        let mut got_done = false;
        while let Some(ev) = recv_opt(&mut rx).await {
            match ev {
                ChatEvent::ToolCall { id, name, args } => {
                    assert_eq!(id, "call_1");
                    assert_eq!(name, "read_file");
                    assert_eq!(args, json!({"path": "/x"}));
                    got_tool_call = true;
                }
                ChatEvent::Done { .. } => {
                    got_done = true;
                }
                _ => {}
            }
        }
        assert!(got_tool_call, "应收到 ToolCall 事件");
        assert!(got_done, "应收到 Done 事件");
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
            messages: vec![ChatMessage {
                role: Role::User,
                content: "hi".into(),
            }],
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
}
