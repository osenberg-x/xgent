//! OpenAI compatible 接口适配器。
//!
//! 适用于 OpenAI、DeepSeek、Ollama 兼容模式等遵循 OpenAI `/v1/chat/completions`
//! 协议的 provider。支持流式输出与工具调用（按 index 聚合）。

use async_trait::async_trait;
use futures::Stream;
use reqwest::Client;
use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use std::collections::HashMap;
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
    /// 非流式请求与建流（time-to-headers）的超时。
    ///
    /// 不设在 Client 整体上——那会杀死长流式响应；流式 body 由
    /// first/idle 超时兜底（见 run_stream）。
    request_timeout: Duration,
    /// 额外请求头（`CustomApiProvider` 用：部分第三方接口需自定义鉴权头）。
    ///
    /// 非法头名/值在构造时丢弃（reqwest::header 只接受合法 ASCII），
    /// 不因一个畸形头让整个 provider 构造失败。
    extra_headers: HeaderMap,
}

impl OpenAiCompatProvider {
    /// 构造适配器。
    ///
    /// `api_base` 不含尾部 `/`，方法内部拼接路径。
    pub fn new(id: String, api_base: String, api_key: String) -> Self {
        Self::with_timeout(id, api_base, api_key, 60)
    }

    /// 指定非流式请求超时秒数构造（daemon 用配置的 `timeout_secs`）。
    pub fn with_timeout(id: String, api_base: String, api_key: String, timeout_secs: u64) -> Self {
        let client = Client::builder()
            // 只限制连接建立（TCP+TLS 握手），防止半开连接在握手阶段永久挂起
            .connect_timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Self {
            id,
            api_base,
            api_key,
            client,
            request_timeout: Duration::from_secs(timeout_secs),
            extra_headers: HeaderMap::new(),
        }
    }

    /// 指定额外请求头构造（`CustomApiProvider` 经此透传用户自定义头）。
    pub fn with_extra_headers(
        id: String,
        api_base: String,
        api_key: String,
        timeout_secs: u64,
        extra_headers: &HashMap<String, String>,
    ) -> Self {
        let mut me = Self::with_timeout(id, api_base, api_key, timeout_secs);
        for (k, v) in extra_headers {
            match (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                (Ok(name), Ok(val)) => {
                    me.extra_headers.insert(name, val);
                }
                _ => {
                    eprintln!("[provider] 非法自定义请求头名或值，已忽略: {k}");
                }
            }
        }
        me
    }

    /// api_base 是否为空（自定义 API 必须显式给出 base URL）。
    pub fn api_base_is_empty(&self) -> bool {
        self.api_base.trim().is_empty()
    }

    /// 给请求构造器套上额外头。
    fn apply_extra_headers(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.extra_headers.is_empty() {
            return rb;
        }
        rb.headers(self.extra_headers.clone())
    }

    /// 用已有 Client 构造（便于测试与连接复用）。
    pub fn with_client(id: String, api_base: String, api_key: String, client: Client) -> Self {
        Self {
            id,
            api_base,
            api_key,
            client,
            request_timeout: Duration::from_secs(60),
            extra_headers: HeaderMap::new(),
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
            .apply_extra_headers(
                self.client
                    .get(self.models_url())
                    .bearer_auth(&self.api_key),
            )
            .timeout(self.request_timeout)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let retry_after_secs = crate::provider::parse_retry_after(resp.headers());
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                retry_after_secs,
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
        // 建流（等待响应头）限时：半开连接下 send() 可能永久挂起，而
        // first/idle 超时要等流建立后才生效。timeout 只覆盖 send() 本身，
        // 响应头返回后 body 流式不受影响。
        let resp = timeout(self.request_timeout, async {
            self.apply_extra_headers(self.client.post(self.chat_url()).bearer_auth(&self.api_key))
                .json(&body)
                .send()
                .await
        })
        .await
        .map_err(|_| {
            ProviderError::Network(format!(
                "建流超时（{}s 无响应）",
                self.request_timeout.as_secs()
            ))
        })??;
        let status = resp.status();
        if !status.is_success() {
            let retry_after_secs = crate::provider::parse_retry_after(resp.headers());
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                retry_after_secs,
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
    let mut st = StreamState::default();

    // 接收端已关闭（agent abort → daemon 取消流 → drop 本 receiver）时，
    // **必须终止对上游 body 的拉取**：继续读会把模型生成出来的 token 白白
    // 消耗掉——这正是 R1-2 要消除的失效模式。break 后 `s` 被 drop，
    // reqwest 的 body 流随之关闭，HTTP 连接断开。
    if tx.is_closed() {
        return;
    }

    // 流开始
    let _ = tx.send(ChatEvent::Start { model }).await;

    // 首事件单独用 first_timeout 等待；后续用 idle_timeout
    let first = match timeout(first_timeout, s.next()).await {
        Ok(Some(item)) => item,
        // 流在首事件前就结束：当作正常空流，走收尾逻辑
        Ok(None) => {
            st.finish(&tx).await;
            return;
        }
        Err(_) => {
            let _ = tx
                .send(ChatEvent::Error {
                    kind: ErrorKind::Network,
                    message: "stream first event timeout".into(),

                    retry_after_secs: None,
                })
                .await;
            return;
        }
    };

    // 处理首事件
    if !handle_item(first, &tx, &mut st).await {
        return;
    }

    // 后续事件用 idle_timeout 逐个等待
    loop {
        // 每轮先看接收端是否已被丢弃（见上方说明）：是则停止拉取上游。
        if tx.is_closed() {
            return;
        }
        match timeout(idle_timeout, s.next()).await {
            Ok(Some(item)) => {
                if !handle_item(item, &tx, &mut st).await {
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

                        retry_after_secs: None,
                    })
                    .await;
                return;
            }
        }
    }

    // 流自然结束，统一收尾（Done 延迟到此处发射，带上最后看到的 usage）
    st.finish(&tx).await;
}

/// 流式解析跨 chunk 状态。
#[derive(Default)]
struct StreamState {
    /// 工具调用按 index 聚合（OpenAI 分块到达）
    tool_calls: Vec<ToolCallAccum>,
    /// 是否已发 TextStart
    text_started: bool,
    /// 最后一次看到的 usage。
    ///
    /// OpenAI 协议中 usage 携带在空 choices 的独立 chunk 里，且在
    /// finish_reason chunk **之后**到达——Done 统一延迟到流末尾发射，
    /// 才能带上真实 usage（否则恒为 0，token 统计与 compaction 触发失真）。
    last_usage: Option<TokenUsage>,
    /// finish_reason 映射的 StopReason
    stop_reason: Option<xgent_core::chat::StopReason>,
}

impl StreamState {
    /// 流末尾统一收尾：补 TextEnd + 发 Done（带最后看到的 usage）。
    async fn finish(&mut self, tx: &mpsc::Sender<ChatEvent>) {
        if self.text_started {
            self.text_started = false;
            let _ = tx.send(ChatEvent::TextEnd).await;
        }
        let _ = tx
            .send(ChatEvent::Done {
                reason: self
                    .stop_reason
                    .unwrap_or(xgent_core::chat::StopReason::Stop),
                usage: self.last_usage.clone().unwrap_or_default(),
            })
            .await;
    }
}

/// 提取流中 `{"error": {...}}` 数据帧的错误消息（OpenAI 兼容服务的
/// mid-stream 错误形态）。无错误返回 None。
fn extract_stream_error(v: &Value) -> Option<String> {
    let err = v.get("error")?;
    if err.is_null() {
        return None;
    }
    let msg = err["message"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| err.to_string());
    Some(msg)
}

/// 处理单个流 item，返回 false 表示应终止流（已发 Error）。
async fn handle_item(
    item: Result<Value, ProviderError>,
    tx: &mpsc::Sender<ChatEvent>,
    st: &mut StreamState,
) -> bool {
    match item {
        Ok(v) => {
            // 流内 error 数据帧：不处理会被静默跳过，半截文本被当完整回复
            if let Some(msg) = extract_stream_error(&v) {
                let _ = tx
                    .send(ChatEvent::Error {
                        kind: xgent_core::chat::ErrorKind::ProviderError,
                        message: msg,

                        retry_after_secs: None,
                    })
                    .await;
                return false;
            }
            if let Err(e) = handle_chunk(&v, tx, st).await {
                let _ = tx
                    .send(ChatEvent::Error {
                        kind: e.to_error_kind(),
                        message: e.to_string(),

                        retry_after_secs: None,
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

                    retry_after_secs: None,
                })
                .await;
            false
        }
    }
}

/// 处理单个 SSE chunk，转换为细粒度 [`ChatEvent`] 发送。
///
/// 事件发射规则（对齐 ADR-0006）：
/// - 文本：首个非空 content 发 `TextStart`，后续 `TextDelta`，finish 时 `TextEnd`
/// - 工具调用：按 index 首次见发 `ToolCallStart`，参数片段发 `ToolCallDelta`，finish 时 `ToolCallEnd`（全量 args）
/// - finish_reason：只记录 StopReason 到状态，**不发 Done**——usage chunk
///   可能在其后到达，Done 统一由 [`StreamState::finish`] 在流末尾发射
///
/// 返回 `Err` 表示致命解析错误，应终止流。
async fn handle_chunk(
    v: &Value,
    tx: &mpsc::Sender<ChatEvent>,
    st: &mut StreamState,
) -> Result<(), ProviderError> {
    // 每帧都提取 usage（含空 choices 的纯 usage chunk）
    if let Some(u) = extract_usage(v) {
        st.last_usage = Some(u);
    }

    let Some(choices) = v["choices"].as_array().filter(|c| !c.is_empty()) else {
        // 无 choices 或空 choices（纯 usage chunk）——usage 已提取
        return Ok(());
    };

    for choice in choices {
        // 工具调用 delta
        if let Some(tc_arr) = choice["delta"]["tool_calls"].as_array() {
            for tc in tc_arr {
                let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                if idx >= st.tool_calls.len() {
                    st.tool_calls.resize(idx + 1, ToolCallAccum::default());
                }
                let accum = &mut st.tool_calls[idx];

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
            if !st.text_started {
                st.text_started = true;
                let _ = tx.send(ChatEvent::TextStart).await;
            }
            let _ = tx
                .send(ChatEvent::TextDelta {
                    text: content.to_string(),
                })
                .await;
        }

        // finish_reason：记录 StopReason，Done 延迟到流末尾
        if let Some(reason) = choice["finish_reason"].as_str() {
            // 文本块结束（若已开始）
            if st.text_started {
                st.text_started = false;
                let _ = tx.send(ChatEvent::TextEnd).await;
            }

            // 工具调用结束：发 ToolCallEnd（聚合 args 解析为 JSON）
            for (idx, accum) in st.tool_calls.drain(..).enumerate() {
                if accum.started {
                    let args_val = parse_tool_args(&accum.args);
                    let _ = tx
                        .send(ChatEvent::ToolCallEnd {
                            index: idx as u32,
                            args: args_val,
                        })
                        .await;
                }
            }

            st.stop_reason = Some(map_stop_reason(reason));
        }
    }

    Ok(())
}

/// 解析工具调用聚合 args；畸形 JSON 保留为带标记的错误对象而非空 `{}`，
/// 让调用方拿到「参数解析失败」的明确信号（空 `{}` 会误导 LLM 以缺参重试）。
fn parse_tool_args(args: &str) -> Value {
    if args.is_empty() {
        return json!({});
    }
    serde_json::from_str(args).unwrap_or_else(|_| json!({ "__parse_error": args }))
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
        let mut st = StreamState::default();
        // 文本 chunk
        let v1 = chunk(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#);
        handle_chunk(&v1, &tx, &mut st).await.unwrap();
        // finish chunk
        let v2 = chunk(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#);
        handle_chunk(&v2, &tx, &mut st).await.unwrap();
        // 流末尾统一收尾
        st.finish(&tx).await;
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
        let mut st = StreamState::default();
        let v = chunk(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#);
        handle_chunk(&v, &tx, &mut st).await.unwrap();
        st.finish(&tx).await;
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
        let mut st = StreamState::default();
        let v = chunk(r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#);
        handle_chunk(&v, &tx, &mut st).await.unwrap();
        st.finish(&tx).await;
        let ev = recv(&mut rx).await;
        assert!(matches!(
            ev,
            ChatEvent::Done {
                reason: xgent_core::chat::StopReason::Length,
                ..
            }
        ));
    }

    /// Done 延迟到流末尾发射：finish_reason 之后的独立 usage chunk（空 choices）
    /// 必须计入 Done.usage（回归：OpenAI include_usage 的 usage 恒为 0）。
    #[tokio::test]
    async fn handle_chunk_usage_chunk_after_finish_is_kept() {
        let (tx, mut rx) = mpsc::channel::<ChatEvent>(8);
        let mut st = StreamState::default();
        let v1 = chunk(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#);
        handle_chunk(&v1, &tx, &mut st).await.unwrap();
        // 尾部 usage chunk：choices 为空
        let v2 = chunk(r#"{"choices":[],"usage":{"prompt_tokens":11,"completion_tokens":7}}"#);
        handle_chunk(&v2, &tx, &mut st).await.unwrap();
        st.finish(&tx).await;
        let ev = recv(&mut rx).await;
        assert!(
            matches!(
                &ev,
                ChatEvent::Done {
                    usage: TokenUsage {
                        prompt: 11,
                        completion: 7
                    },
                    ..
                }
            ),
            "Done 应带尾部 usage chunk 的真实用量，实际: {ev:?}"
        );
    }

    #[tokio::test]
    async fn handle_chunk_tool_call_emits_start_delta_end() {
        let (tx, mut rx) = mpsc::channel::<ChatEvent>(32);
        let mut st = StreamState::default();
        // 第一块：tool_call 开始 + 参数片段
        let v1 = chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}"#,
        );
        handle_chunk(&v1, &tx, &mut st).await.unwrap();
        // 第二块：arguments 继续
        let v2 = chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"/x\"}"}}]}}]}"#,
        );
        handle_chunk(&v2, &tx, &mut st).await.unwrap();
        // 第三块：finish
        let v3 = chunk(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#);
        handle_chunk(&v3, &tx, &mut st).await.unwrap();
        st.finish(&tx).await;
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
            ChatEvent::Error { kind, message, .. } => {
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
            ChatEvent::Error { kind, message, .. } => {
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

#[cfg(test)]
mod receiver_drop_tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::mpsc;

    /// 构造一个"无限"上游：每次被拉取就把 `pulled` 自增。
    ///
    /// 关键是让"是否还在拉取上游"成为**外部可观测量**——早期版本用
    /// `stream::iter` 预生成内存帧，接收端 drop 后 `send` 立即返回 Err，
    /// 不加守卫也会瞬间跑完，测试对修复完全不敏感（判别力为零）。
    fn counting_upstream(
        pulled: Arc<AtomicUsize>,
        frames: usize,
    ) -> impl Stream<Item = Result<Value, ProviderError>> {
        futures::stream::unfold(0usize, move |n| {
            let pulled = pulled.clone();
            async move {
                if n >= frames {
                    return None;
                }
                pulled.fetch_add(1, Ordering::SeqCst);
                let val = json!({"choices": [{"delta": {"content": "x"}, "index": 0}]});
                Some((Ok(val), n + 1))
            }
        })
    }

    /// 接收端**预先**被丢弃：上游一帧都不应被拉取。
    ///
    /// 无守卫时计数会等于总帧数；修复后为 0。
    #[tokio::test]
    async fn dropped_receiver_never_pulls_upstream() {
        let (tx, rx) = mpsc::channel::<ChatEvent>(1);
        drop(rx);

        let pulled = Arc::new(AtomicUsize::new(0));
        let s = counting_upstream(pulled.clone(), 1000);

        run_stream_with_timeout(
            s,
            "m".into(),
            tx,
            Duration::from_secs(5),
            Duration::from_secs(5),
        )
        .await;

        assert_eq!(
            pulled.load(Ordering::SeqCst),
            0,
            "接收端已丢弃时不应拉取上游任何一帧（实际拉了 {} 帧）",
            pulled.load(Ordering::SeqCst)
        );
    }

    /// 接收端在流**中途**被丢弃：拉取计数必须停止增长。
    #[tokio::test]
    async fn receiver_dropped_midstream_stops_pulling() {
        let (tx, rx) = mpsc::channel::<ChatEvent>(8);
        let pulled = Arc::new(AtomicUsize::new(0));
        let s = counting_upstream(pulled.clone(), 1_000_000);

        let task = tokio::spawn(run_stream_with_timeout(
            s,
            "m".into(),
            tx,
            Duration::from_secs(5),
            Duration::from_secs(5),
        ));

        // 让它跑一会儿，接收若干帧
        for _ in 0..5 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        drop(rx);
        let at_drop = pulled.load(Ordering::SeqCst);
        assert!(at_drop > 0, "丢弃前应已拉取若干帧");

        // 给足时间让（无守卫的）实现继续跑完剩余帧
        tokio::time::sleep(Duration::from_millis(50)).await;
        let after_wait = pulled.load(Ordering::SeqCst);

        assert!(
            after_wait <= at_drop + 8,
            "接收端丢弃后拉取计数应停止（丢弃时 {at_drop} → 等待后 {after_wait}）"
        );
        let _ = tokio::time::timeout(Duration::from_secs(2), task).await;
    }
}
