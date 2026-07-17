//! 会话 JSONL append-only 持久化（见 ADR-0008）。
//!
//! [`SessionStore`] 负责单会话 JSONL 文件的追加与读取。
//! - `append` 同步追加一行（`writeln` 即持久化，flush 落盘）；
//! - `load_all` 读取全部 entry（MVP 定义但不调用，恢复留 P1）。
//!
//! MVP：每个会话对应一个文件，文件路径由调用方（bridge）按
//! `<project_root>/.xgent/sessions/<session_id>.jsonl` 约定构造。

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

/// 计算项目会话 JSONL 文件路径：`<project_root>/.xgent/sessions/<session_id>.jsonl`。
///
/// 与 [`xgent_settings_core::paths::project_config_dir`] 对齐（同在 `.xgent/` 下）。
pub fn session_file_path(project_root: &Path, session_id: &str) -> PathBuf {
    project_root
        .join(".xgent")
        .join("sessions")
        .join(format!("{session_id}.jsonl"))
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
        let p = session_file_path(Path::new("/proj"), "abc");
        assert_eq!(p, Path::new("/proj/.xgent/sessions/abc.jsonl"));
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
