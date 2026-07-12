//! 文件系统变更事件与项目订阅类型。
//!
//! daemon 监听项目目录变更，通过 JSON-RPC 通知推送给订阅的 UI 客户端。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 文件变更事件。
///
/// daemon 检测到项目内文件变更后，封装此结构通过
/// [`crate::methods::notifications::FS_CHANGED`] 通知 UI。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChanged {
    /// 项目根目录（绝对路径）
    pub project_root: PathBuf,
    /// 变更文件路径（相对或绝对，由 daemon 约定；MVP 用相对 project_root）
    pub path: PathBuf,
    /// 变更类型
    pub kind: FileChangeKind,
}

/// 文件变更类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileChangeKind {
    /// 新建
    Created,
    /// 修改
    Modified,
    /// 删除
    Removed,
    /// 重命名
    Renamed,
}

/// 订阅项目路径的请求参数（UI → daemon `fs.watch`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRequest {
    /// 要监听的项目根目录
    pub project_root: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_changed_roundtrip() {
        let fc = FileChanged {
            project_root: PathBuf::from("/proj"),
            path: PathBuf::from("src/main.rs"),
            kind: FileChangeKind::Modified,
        };
        let j = serde_json::to_string(&fc).unwrap();
        let fc2: FileChanged = serde_json::from_str(&j).unwrap();
        assert_eq!(fc2.project_root, PathBuf::from("/proj"));
        assert_eq!(fc2.path, PathBuf::from("src/main.rs"));
        assert_eq!(fc2.kind, FileChangeKind::Modified);
    }

    #[test]
    fn file_change_kind_serializes_lowercase() {
        let j = serde_json::to_string(&FileChangeKind::Created).unwrap();
        assert_eq!(j, r#""created""#);
        let k: FileChangeKind = serde_json::from_str(r#""renamed""#).unwrap();
        assert_eq!(k, FileChangeKind::Renamed);
    }

    #[test]
    fn watch_request_roundtrip() {
        let w = WatchRequest {
            project_root: PathBuf::from("/abs/proj"),
        };
        let j = serde_json::to_string(&w).unwrap();
        let w2: WatchRequest = serde_json::from_str(&j).unwrap();
        assert_eq!(w2.project_root, PathBuf::from("/abs/proj"));
    }
}
