use serde::{Deserialize, Serialize};
use std::fmt;

/// UI 客户端标识（daemon 分配）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientId(pub u64);

/// 会话标识
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u64);

/// provider 流式对话的流 ID，用于关联 chunk 通知与请求
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamId(pub u64);

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "client:{}", self.0)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "session:{}", self.0)
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "stream:{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_roundtrip() {
        let c = ClientId(7);
        let s = SessionId(42);
        let st = StreamId(99);
        for (name, json) in [
            ("client", serde_json::to_string(&c).unwrap()),
            ("session", serde_json::to_string(&s).unwrap()),
            ("stream", serde_json::to_string(&st).unwrap()),
        ] {
            match name {
                "client" => {
                    let c2: ClientId = serde_json::from_str(&json).unwrap();
                    assert_eq!(c2, c);
                }
                "session" => {
                    let s2: SessionId = serde_json::from_str(&json).unwrap();
                    assert_eq!(s2, s);
                }
                "stream" => {
                    let st2: StreamId = serde_json::from_str(&json).unwrap();
                    assert_eq!(st2, st);
                }
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn id_serializes_as_number() {
        // newtype(u64) 序列化为裸数字
        assert_eq!(serde_json::to_string(&ClientId(3)).unwrap(), "3");
        let c: ClientId = serde_json::from_str("3").unwrap();
        assert_eq!(c, ClientId(3));
    }

    #[test]
    fn display_formats() {
        assert_eq!(ClientId(1).to_string(), "client:1");
        assert_eq!(SessionId(2).to_string(), "session:2");
        assert_eq!(StreamId(3).to_string(), "stream:3");
    }
}
