//! xgent_app IPC client 与 provider_client 单元测试。
//!
//! 不启动真实 daemon，只测协议解析逻辑。

#![cfg(test)]

use xgent_core::proto::{Notification, Request, Response, RpcError};

#[test]
fn request_serializes_with_id() {
    let req = Request::new(7, "provider.chat", serde_json::json!({"model": "gpt-4"}));
    let line = serde_json::to_string(&req).unwrap();
    assert!(line.contains(r#""id":7"#));
    assert!(line.contains(r#""method":"provider.chat""#));
}

#[test]
fn response_parses_with_id() {
    let line = r#"{"jsonrpc":"2.0","id":7,"result":{"stream_id":42}}"#;
    let resp: Response = serde_json::from_str(line).unwrap();
    assert_eq!(resp.id, 7);
    assert_eq!(resp.result.unwrap()["stream_id"], 42);
}

#[test]
fn notification_parses_no_id() {
    let line = r#"{"jsonrpc":"2.0","method":"provider.event","params":{"stream_id":1,"event":{"type":"textDelta","text":"hi"}}}"#;
    let n: Notification = serde_json::from_str(line).unwrap();
    assert_eq!(n.method, "provider.event");
    assert_eq!(n.params["event"]["text"], "hi");
    // 通知无 id 字段
    assert!(!line.contains(r#""id""#));
}

#[test]
fn error_response_parses() {
    let resp = Response::err(3, RpcError::new(-32601, "no method", None));
    let line = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&line).unwrap();
    assert!(parsed.result.is_none());
    assert_eq!(parsed.error.unwrap().code, -32601);
}

/// 验证 provider_client 的通知→ChatEvent 路由逻辑（纯函数式，不连 IPC）。
///
/// daemon 透传整个 ChatEvent JSON（见 ADR-0006），UI 侧反序列化。
#[test]
fn route_provider_event_notification_to_chat_event() {
    use xgent_core::chat::ChatEvent;
    let notif = Notification::new(
        xgent_core::notifications::PROVIDER_EVENT,
        serde_json::json!({
            "stream_id": 5,
            "event": ChatEvent::TextDelta { text: "hello".into() }
        }),
    );
    // 模拟 provider_client 的路由判定
    match notif.method.as_str() {
        xgent_core::notifications::PROVIDER_EVENT => {
            let ev: ChatEvent = serde_json::from_value(notif.params["event"].clone()).unwrap();
            assert!(matches!(ev, ChatEvent::TextDelta { text } if text == "hello"));
        }
        _ => unreachable!(),
    }
}
