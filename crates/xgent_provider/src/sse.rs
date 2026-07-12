//! SSE 解析辅助。
//!
//! 用 [`eventsource_stream::Eventsource`] 把 HTTP 响应字节流转成 SSE 事件流，
//! 再解析每个事件的 data 为 JSON，处理 OpenAI 风格的 `[DONE]` 终止标记。

use eventsource_stream::{Event, Eventsource};
use futures::Stream;
use futures::StreamExt;
use serde_json::Value;

use crate::provider::ProviderError;

/// OpenAI SSE 流的终止标记。
pub const DONE_MARKER: &str = "[DONE]";

/// 把 SSE 事件流解析为 JSON Value 流。
///
/// - `data` 为空（心跳）的 event 跳过；
/// - `data` 为 `[DONE]` 时结束流；
/// - 其余 `data` 解析为 JSON，失败转为 [`ProviderError::Stream`]。
///
/// 泛型 `E` 兼容 `eventsource_stream::EventStreamError` 等多种错误类型。
pub fn parse_sse_events<S, E>(events: S) -> impl Stream<Item = Result<Value, ProviderError>>
where
    S: Stream<Item = Result<Event, E>> + Send + 'static,
    E: std::fmt::Display + 'static,
{
    events
        .map(|ev| match ev {
            Ok(e) => Ok(e),
            Err(e) => Err(ProviderError::Stream(format!("sse: {e}"))),
        })
        .take_while(|item| {
            // 遇到 [DONE] 停止
            futures::future::ready(!matches!(item, Ok(e) if e.data.trim() == DONE_MARKER))
        })
        .filter_map(|item| async move {
            match item {
                Ok(e) => {
                    let data = e.data.trim();
                    if data.is_empty() {
                        // 心跳，跳过
                        None
                    } else if data == DONE_MARKER {
                        // 已被 take_while 截断，理论不达
                        None
                    } else {
                        match serde_json::from_str::<Value>(data) {
                            Ok(v) => Some(Ok(v)),
                            Err(e) => Some(Err(ProviderError::Stream(format!(
                                "json: {e}; data={data}"
                            )))),
                        }
                    }
                }
                Err(e) => Some(Err(e)),
            }
        })
}

/// 把 reqwest 响应的字节流转成 JSON Value 流。
///
/// 先经 `Eventsource` 转为 SSE 事件，再经 [`parse_sse_events`] 解析。
pub fn parse_response_stream(
    resp: reqwest::Response,
) -> impl Stream<Item = Result<Value, ProviderError>> + Send + 'static {
    parse_sse_events(resp.bytes_stream().eventsource())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use serde_json::json;

    fn ev(data: &str) -> Result<Event, std::io::Error> {
        Ok(Event {
            event: String::new(),
            data: data.to_string(),
            id: String::new(),
            retry: None,
        })
    }

    #[tokio::test]
    async fn parse_basic_delta_stream() {
        let events = stream::iter(vec![
            ev(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#),
            ev(r#"{"choices":[{"delta":{"content":" world"}}]}"#),
            ev(DONE_MARKER),
        ]);
        let mut s = Box::pin(parse_sse_events(events));
        let mut results = Vec::new();
        while let Some(item) = s.next().await {
            results.push(item.unwrap());
        }
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["choices"][0]["delta"]["content"], json!("Hello"));
        assert_eq!(
            results[1]["choices"][0]["delta"]["content"],
            json!(" world")
        );
    }

    #[tokio::test]
    async fn parse_skips_heartbeats() {
        let events = stream::iter(vec![
            ev(""), // 心跳
            ev(r#"{"choices":[{"delta":{"content":"hi"}}]}"#),
            ev(""), // 心跳
            ev(DONE_MARKER),
        ]);
        let mut s = Box::pin(parse_sse_events(events));
        let mut results = Vec::new();
        while let Some(item) = s.next().await {
            results.push(item.unwrap());
        }
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["choices"][0]["delta"]["content"], json!("hi"));
    }

    #[tokio::test]
    async fn parse_stops_at_done_marker() {
        let events = stream::iter(vec![
            ev(r#"{"choices":[{"delta":{"content":"a"}}]}"#),
            ev(DONE_MARKER),
            // done 之后不应再被消费
            ev(r#"{"choices":[{"delta":{"content":"b"}}]}"#),
        ]);
        let mut s = Box::pin(parse_sse_events(events));
        let mut results = Vec::new();
        while let Some(item) = s.next().await {
            results.push(item.unwrap());
        }
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["choices"][0]["delta"]["content"], json!("a"));
    }

    #[tokio::test]
    async fn parse_invalid_json_returns_error() {
        let events = stream::iter(vec![ev("not-json"), ev(DONE_MARKER)]);
        let mut s = Box::pin(parse_sse_events(events));
        let item = s.next().await.unwrap();
        assert!(item.is_err());
        match item.unwrap_err() {
            ProviderError::Stream(_) => {}
            other => panic!("expected Stream error, got {other:?}"),
        }
    }
}
