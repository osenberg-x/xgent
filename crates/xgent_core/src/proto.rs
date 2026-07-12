//! JSON-RPC 2.0 协议契约。
//!
//! 定义 UI 进程与 daemon 之间通信的请求、响应、通知结构。
//! 所有跨进程类型经 serde_json 序列化，遵循 JSON-RPC 2.0 规范。

use serde::{Deserialize, Serialize};

/// JSON-RPC 版本字符串。
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC 请求。
///
/// 带有 id，daemon 必须返回对应 id 的 [`Response`]。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// 固定为 `"2.0"`
    #[serde(default = "default_jsonrpc")]
    pub jsonrpc: String,
    /// 请求 id，由调用方生成，用于匹配响应
    pub id: u64,
    /// 方法名，见 [`crate::methods`]
    pub method: String,
    /// 方法参数
    pub params: serde_json::Value,
}

impl Request {
    /// 构造一个新请求，`jsonrpc` 字段自动填充为 `"2.0"`。
    pub fn new(id: u64, method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 响应。
///
/// 成功时 `result` 为 `Some`，失败时 `error` 为 `Some`，两者互斥。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// 固定为 `"2.0"`
    #[serde(default = "default_jsonrpc")]
    pub jsonrpc: String,
    /// 对应请求的 id
    pub id: u64,
    /// 成功结果
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// 错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    /// 构造成功响应。
    pub fn ok(id: u64, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// 构造错误响应。
    pub fn err(id: u64, error: RpcError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// JSON-RPC 通知（无 id，单向，不需要响应）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// 固定为 `"2.0"`
    #[serde(default = "default_jsonrpc")]
    pub jsonrpc: String,
    /// 通知方法名，见 [`crate::methods`]
    pub method: String,
    /// 通知参数
    pub params: serde_json::Value,
}

impl Notification {
    /// 构造一个新通知，`jsonrpc` 字段自动填充为 `"2.0"`。
    pub fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params,
        }
    }
}

/// serde 默认值函数，返回 `"2.0"`。反序列化缺失 `jsonrpc` 字段时使用。
fn default_jsonrpc() -> String {
    JSONRPC_VERSION.to_string()
}

/// JSON-RPC 错误对象。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    /// 错误码（参考 JSON-RPC 2.0 规范的预定义码，或自定义）
    pub code: i32,
    /// 错误简要描述
    pub message: String,
    /// 可选的附加数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl RpcError {
    /// 构造错误对象。
    pub fn new(code: i32, message: impl Into<String>, data: Option<serde_json::Value>) -> Self {
        Self {
            code,
            message: message.into(),
            data,
        }
    }
}

// 标准错误码（JSON-RPC 2.0 规范定义）
/// 解析错误：daemon 收到无效 JSON。
pub const PARSE_ERROR: i32 = -32700;
/// 无效请求：发送的 JSON 不是合法的 Request 对象。
pub const INVALID_REQUEST: i32 = -32600;
/// 方法不存在。
pub const METHOD_NOT_FOUND: i32 = -32601;
/// 方法参数无效。
pub const INVALID_PARAMS: i32 = -32602;
/// 内部错误。
pub const INTERNAL_ERROR: i32 = -32603;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip() {
        let req = Request::new(42, "provider.chat", serde_json::json!({"model": "gpt-4"}));
        let j = serde_json::to_string(&req).unwrap();
        let req2: Request = serde_json::from_str(&j).unwrap();
        assert_eq!(req2.id, 42);
        assert_eq!(req2.method, "provider.chat");
        assert_eq!(req2.jsonrpc, "2.0");
    }

    #[test]
    fn response_ok_roundtrip() {
        let resp = Response::ok(7, serde_json::json!({"ok": true}));
        let j = serde_json::to_string(&resp).unwrap();
        // 成功响应不应包含 error 字段
        assert!(!j.contains(r#""error"#));
        let resp2: Response = serde_json::from_str(&j).unwrap();
        assert_eq!(resp2.id, 7);
        assert!(resp2.result.is_some());
        assert!(resp2.error.is_none());
    }

    #[test]
    fn response_err_roundtrip() {
        let resp = Response::err(7, RpcError::new(METHOD_NOT_FOUND, "no such method", None));
        let j = serde_json::to_string(&resp).unwrap();
        // 错误响应不应包含 result 字段
        assert!(!j.contains(r#""result"#));
        let resp2: Response = serde_json::from_str(&j).unwrap();
        assert_eq!(resp2.id, 7);
        assert!(resp2.result.is_none());
        let err = resp2.error.unwrap();
        assert_eq!(err.code, METHOD_NOT_FOUND);
        assert_eq!(err.message, "no such method");
    }

    #[test]
    fn notification_roundtrip() {
        let n = Notification::new("fs.changed", serde_json::json!({"path": "/x"}));
        let j = serde_json::to_string(&n).unwrap();
        // 通知无 id 字段
        assert!(!j.contains(r#""id""#));
        let n2: Notification = serde_json::from_str(&j).unwrap();
        assert_eq!(n2.method, "fs.changed");
        assert_eq!(n2.jsonrpc, "2.0");
    }

    #[test]
    fn notification_jsonrpc_field_is_static() {
        // 验证 jsonrpc 字段反序列化后为 "2.0"
        let n = Notification::new("x", serde_json::Value::Null);
        let j = serde_json::to_string(&n).unwrap();
        assert!(j.contains(r#""jsonrpc":"2.0""#));
    }
}
