//! 会话 JSONL append-only 持久化（见 ADR-0008）。
//!
//! [`SessionStore`] 负责单会话 JSONL 文件的追加与读取。
//! - `append` 同步追加一行（`writeln` 即持久化，flush 落盘）；
//! - `load_all` 读取全部 entry（MVP 定义但不调用，恢复留 P1）。
//!
//! 会话文件存全局用户目录 `<agent_dir>/sessions/<session_id>.jsonl`
//!（见 [`xgent_settings_core::paths::session_file_path`]），跨项目共享，
//! 对齐 pi 的 `~/.pi/agent/sessions/` 布局。

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use xgent_core::session::SessionEntry;

/// 会话 JSONL 存储句柄：持有文件路径，按需打开文件 append / read。
///
/// 不常驻文件句柄以避免崩溃时写半行损坏；每次 `append` 都重新以 append 模式
/// 打开文件并 `writeln` 一行（返回即已落盘）。
#[derive(Debug)]
pub struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    /// 打开（或创建）会话存储。不立即创建文件——首次 `append` 时写入。
    /// 父目录若不存在则创建。
    pub fn open(path: PathBuf) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(Self { path })
    }

    /// 追加一行 JSONL（同步，返回即已持久化）。
    pub fn append(&mut self, entry: &SessionEntry) -> io::Result<()> {
        let line = serde_json::to_string(entry)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{line}")?;
        file.sync_all().ok(); // 尽力落盘；错误不致命（已 writeln 到内核）
        Ok(())
    }

    /// 读取全部 entry（每行反序列化）。空行跳过。
    pub fn load_all(&self) -> io::Result<Vec<SessionEntry>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut out = Vec::new();
        for (i, line) in reader.lines().enumerate() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry: SessionEntry = serde_json::from_str(trimmed).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("第 {} 行反序列化失败: {e}", i + 1),
                )
            })?;
            out.push(entry);
        }
        Ok(out)
    }

    /// 会话文件路径（测试与 bridge 可用）。
    pub fn path(&self) -> &Path {
        &self.path
    }
}
/// 会话摘要（用于历史列表 UI）。
#[derive(Debug, Clone)]
pub struct SessionSummary {
    /// 会话 id（文件名 stem）
    pub id: String,
    /// 创建时间戳（ms epoch），从 Header entry 读取
    pub timestamp: u64,
    /// 可选标题
    pub title: Option<String>,
    /// 消息条数（Message entry 计数）
    pub message_count: usize,
    /// 工作目录
    pub cwd: String,
}

/// 扫描 sessions 目录，返回所有会话的摘要（按时间倒序）。
///
/// 遍历 `<sessions_dir>/*.jsonl`，读取每文件的 Header（首行）与 Message 计数。
/// 损坏文件跳过（不阻塞其他会话的列出）。
pub fn list_sessions() -> Vec<SessionSummary> {
    let dir = xgent_settings_core::paths::sessions_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut summaries = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string());
        let Some(stem) = stem else {
            continue;
        };
        let store = match SessionStore::open(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let Ok(entries) = store.load_all() else {
            continue;
        };
        let mut summary = SessionSummary {
            id: stem,
            timestamp: 0,
            title: None,
            message_count: 0,
            cwd: String::new(),
        };
        for entry in &entries {
            match entry {
                xgent_core::session::SessionEntry::Header(h) => {
                    summary.timestamp = h.timestamp;
                    summary.title = h.title.clone();
                    summary.cwd = h.cwd.clone();
                }
                xgent_core::session::SessionEntry::Message(_) => {
                    summary.message_count += 1;
                }
                _ => {}
            }
        }
        // 跳过无 Header 的无效文件
        if summary.timestamp > 0 {
            summaries.push(summary);
        }
    }
    // 按时间倒序
    summaries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    summaries
}

/// 从 JSONL 文件恢复会话消息历史。
///
/// 读取 `session_id` 对应的 JSONL，重建 `AgentMessage` 列表。
/// Compaction entry 之后的历史为压缩后保留的消息。
/// Error entry 不进消息历史。
pub fn restore_session(session_id: &str) -> Option<Vec<xgent_core::chat::AgentMessage>> {
    let path = session_file_path(session_id);
    let store = SessionStore::open(path).ok()?;
    let entries = store.load_all().ok()?;

    let mut messages = Vec::new();
    for entry in &entries {
        match entry {
            xgent_core::session::SessionEntry::Header(_) => {}
            xgent_core::session::SessionEntry::Message(m) => {
                messages.push(m.message.clone());
            }
            xgent_core::session::SessionEntry::Compaction(c) => {
                // 遇到 compaction：用摘要替换之前的所有消息
                messages.clear();
                messages.push(xgent_core::chat::AgentMessage::User(
                    xgent_core::chat::UserMessage {
                        content: vec![xgent_core::chat::ContentBlock::Text {
                            text: format!("[前序对话摘要]\n{}", c.summary),
                        }],
                        timestamp: c.timestamp,
                    },
                ));
            }
            xgent_core::session::SessionEntry::ModelChange(_) => {}
            xgent_core::session::SessionEntry::Error(_) => {}
        }
    }
    Some(messages)
}

/// 计算会话 JSONL 文件路径：`<agent_dir>/sessions/<session_id>.jsonl`。
///
/// 转发到 [`xgent_settings_core::paths::session_file_path`]（全局用户目录）。
pub fn session_file_path(session_id: &str) -> PathBuf {
    xgent_settings_core::paths::session_file_path(session_id)
}

/// 当前时间戳（ms epoch）。持久化 entry 时间戳用。
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xgent_core::chat::{AgentMessage, AssistantMessage, ContentBlock, UserMessage};
    use xgent_core::session::{ModelChangeEntry, SessionHeader, SessionMessage};

    fn header() -> SessionEntry {
        SessionEntry::Header(SessionHeader {
            id: "s1".into(),
            version: 1,
            cwd: "/tmp/proj".into(),
            timestamp: 1700000000000,
            title: Some("test session".into()),
        })
    }

    fn message() -> SessionEntry {
        SessionEntry::Message(SessionMessage {
            id: "m1".into(),
            parent_id: None,
            timestamp: 1700000001000,
            message: AgentMessage::Assistant(AssistantMessage {
                content: vec![ContentBlock::Text {
                    text: "hello".into(),
                }],
                model: Some("claude-3".into()),
                usage: None,
                timestamp: 0,
            }),
        })
    }

    fn model_change() -> SessionEntry {
        SessionEntry::ModelChange(ModelChangeEntry {
            id: "c1".into(),
            parent_id: "m1".into(),
            timestamp: 1700000002000,
            model: "gpt-4o".into(),
        })
    }

    fn user_message() -> SessionEntry {
        SessionEntry::Message(SessionMessage {
            id: "u1".into(),
            parent_id: None,
            timestamp: 1700000000500,
            message: AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text {
                    text: "ping".into(),
                }],
                timestamp: 0,
            }),
        })
    }

    #[test]
    fn append_then_load_all_roundtrip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("s1.jsonl");
        let mut store = SessionStore::open(path.clone()).expect("open");

        let entries = vec![header(), user_message(), message(), model_change()];
        for e in &entries {
            store.append(e).expect("append");
        }

        let loaded = store.load_all().expect("load_all");
        assert_eq!(loaded, entries);
    }

    #[test]
    fn append_creates_parent_dirs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("nested").join("deep").join("s.jsonl");
        let mut store = SessionStore::open(path.clone()).expect("open");
        store.append(&header()).expect("append");
        assert!(path.exists(), "file should exist after append");
        assert_eq!(store.load_all().unwrap().len(), 1);
    }

    #[test]
    fn load_all_on_missing_file_returns_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("nope.jsonl");
        let store = SessionStore::open(path).expect("open");
        let loaded = store.load_all().expect("load_all");
        assert!(loaded.is_empty());
    }

    #[test]
    fn load_all_skips_empty_lines() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("s.jsonl");
        let mut store = SessionStore::open(path.clone()).expect("open");
        store.append(&header()).expect("append");

        // 追加两个空行模拟编辑器手动写入
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .and_then(|mut f| writeln!(f).and_then(|_| writeln!(f)))
            .expect("write blank lines");

        let loaded = store.load_all().expect("load_all");
        assert_eq!(loaded.len(), 1, "should skip empty lines");
    }

    #[test]
    fn session_file_path_layout() {
        // 路径在全局 agent_dir/sessions/ 下
        let p = session_file_path("abc");
        assert!(p.ends_with("abc.jsonl"));
        assert!(p.starts_with(xgent_settings_core::paths::sessions_dir()));
    }

    #[test]
    fn append_three_different_types_roundtrip() {
        // 任务验收：append 3 条不同类型 entry，load_all 返回相同
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("s2.jsonl");
        let mut store = SessionStore::open(path).expect("open");

        let entries = vec![header(), message(), model_change()];
        for e in &entries {
            store.append(e).expect("append");
        }
        let loaded = store.load_all().expect("load_all");
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded, entries);
    }
}
