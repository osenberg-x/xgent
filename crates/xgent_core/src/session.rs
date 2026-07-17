//! 会话持久化类型（JSONL entry），见 ADR-0008。
//!
//! 会话历史用 append-only JSONL 存储：每行一个 [`SessionEntry`]。
//! MVP 只持久化不恢复；[`SessionStore`](../../xgent_agent/session_store/) 负责 IO。

use serde::{Deserialize, Serialize};

use crate::chat::AgentMessage;

/// JSONL 单行 entry（`#[serde(tag = "type")]` 内部标签）。
///
/// MVP 只有 3 种变体；Compaction 等留 P1。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SessionEntry {
    /// 会话起始标记，一行一份，包含元信息。
    Header(SessionHeader),
    /// 一条对话消息（assistant 完成时 append）。
    Message(SessionMessage),
    /// 模型切换记录。
    ModelChange(ModelChangeEntry),
}

/// 会话 Header：会话开始时写入一次。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionHeader {
    /// 会话唯一 id
    pub id: String,
    /// 存储格式版本
    pub version: u32,
    /// 会话工作目录
    pub cwd: String,
    /// 创建时间戳（ms epoch）
    pub timestamp: u64,
    /// 可选标题
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// 单条对话消息 entry。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMessage {
    /// 消息 id
    pub id: String,
    /// 父消息 id（树形结构预留，MVP 全 None 即线性）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// 时间戳（ms epoch）
    pub timestamp: u64,
    /// agent 层消息（来自 [`crate::chat::AgentMessage`]）
    pub message: AgentMessage,
}

/// 模型切换记录 entry。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelChangeEntry {
    /// 本条记录 id
    pub id: String,
    /// 父 entry id
    pub parent_id: String,
    /// 时间戳（ms epoch）
    pub timestamp: u64,
    /// 切换后的模型名
    pub model: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{AgentMessage, AssistantMessage, ContentBlock};

    #[test]
    fn entry_tagged_serialization_roundtrip() {
        let header = SessionEntry::Header(SessionHeader {
            id: "s1".into(),
            version: 1,
            cwd: "/tmp/proj".into(),
            timestamp: 1700000000000,
            title: Some("hello".into()),
        });
        let msg = SessionEntry::Message(SessionMessage {
            id: "m1".into(),
            parent_id: None,
            timestamp: 1700000001000,
            message: AgentMessage::Assistant(AssistantMessage {
                content: vec![ContentBlock::Text { text: "hi".into() }],
                model: Some("claude-3".into()),
                usage: None,
                timestamp: 0,
            }),
        });
        let mc = SessionEntry::ModelChange(ModelChangeEntry {
            id: "c1".into(),
            parent_id: "m1".into(),
            timestamp: 1700000002000,
            model: "gpt-4o".into(),
        });

        for entry in [header, msg, mc] {
            let line = serde_json::to_string(&entry).unwrap();
            let back: SessionEntry = serde_json::from_str(&line).unwrap();
            assert_eq!(back, entry, "roundtrip failed for line: {line}");
            // 确认带 type 标签
            assert!(line.contains("\"type\":\""));
        }
    }

    #[test]
    fn header_camelcase_type_tag() {
        let header = SessionEntry::Header(SessionHeader {
            id: "s1".into(),
            version: 1,
            cwd: "/p".into(),
            timestamp: 0,
            title: None,
        });
        let line = serde_json::to_string(&header).unwrap();
        // type 标签为 header（camelCase）
        assert!(line.contains("\"type\":\"header\""));
        // title:None 不序列化（skip_serializing_if）
        assert!(!line.contains("title"));
    }

    #[test]
    fn message_entry_has_type_message() {
        let msg = SessionEntry::Message(SessionMessage {
            id: "m1".into(),
            parent_id: None,
            timestamp: 0,
            message: AgentMessage::Assistant(AssistantMessage {
                content: vec![ContentBlock::Text { text: "x".into() }],
                model: None,
                usage: None,
                timestamp: 0,
            }),
        });
        let line = serde_json::to_string(&msg).unwrap();
        assert!(line.contains("\"type\":\"message\""));
        // parent_id:None 不序列化
        assert!(!line.contains("parent_id"));
    }

    #[test]
    fn model_change_entry_has_type_modelchange() {
        let mc = SessionEntry::ModelChange(ModelChangeEntry {
            id: "c1".into(),
            parent_id: "m1".into(),
            timestamp: 0,
            model: "gpt-4o".into(),
        });
        let line = serde_json::to_string(&mc).unwrap();
        assert!(line.contains("\"type\":\"modelChange\""));
    }
}
