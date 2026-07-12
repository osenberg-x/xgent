//! 平台规范路径工具。
//!
//! 统一约定 UI 与 daemon 共用的配置目录、文件路径与 socket 路径，
//! 跨平台一致（依赖 `dirs` crate 获取平台规范位置）。

use std::path::{Path, PathBuf};

/// 全局配置根目录（平台规范）。
///
/// - macOS: `~/Library/Application Support/xgent/`
/// - Windows: `%APPDATA%/xgent/`
/// - Linux: `~/.config/xgent/`
///
/// 目录可能不存在；调用方写入前应确保创建。
pub fn global_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("xgent")
}

/// 全局配置文件路径（`<global_config_dir>/config.toml`）。
pub fn global_config_file() -> PathBuf {
    global_config_dir().join("config.toml")
}

/// 会话历史 SQLite 数据库路径。
pub fn sessions_db_path() -> PathBuf {
    global_config_dir().join("sessions.db")
}

/// 项目级配置目录（`<project_root>/.xgent/`）。
pub fn project_config_dir(project_root: &Path) -> PathBuf {
    project_root.join(".xgent")
}

/// 项目级配置文件路径（`<project_root>/.xgent/config.toml`）。
pub fn project_config_file(project_root: &Path) -> PathBuf {
    project_config_dir(project_root).join("config.toml")
}

/// daemon socket 路径（跨进程约定，UI 与 daemon 共用）。
///
/// macOS/Linux 用 Unix domain socket（缓存目录下）；
/// Windows 用命名管道，路径以 `\\.\pipe\` 前缀表示（MVP 约定）。
pub fn daemon_socket_path() -> PathBuf {
    #[cfg(not(windows))]
    {
        dirs::cache_dir()
            .unwrap_or_else(|| dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")))
            .join("xgent")
            .join("daemon.sock")
    }
    #[cfg(windows)]
    {
        PathBuf::from(r"\\.\pipe\xgent-daemon")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_config_dir_ends_with_xgent() {
        let dir = global_config_dir();
        assert!(
            dir.ends_with("xgent"),
            "global_config_dir should end with 'xgent', got: {dir:?}"
        );
    }

    #[test]
    fn global_config_file_is_config_toml() {
        let f = global_config_file();
        assert!(f.ends_with("config.toml"));
        assert!(f.starts_with(global_config_dir()));
    }

    #[test]
    fn sessions_db_under_config_dir() {
        let p = sessions_db_path();
        assert!(p.ends_with("sessions.db"));
        assert!(p.starts_with(global_config_dir()));
    }

    #[test]
    fn project_config_paths() {
        let root = Path::new("/tmp/proj");
        assert_eq!(project_config_dir(root), Path::new("/tmp/proj/.xgent"));
        assert_eq!(
            project_config_file(root),
            Path::new("/tmp/proj/.xgent/config.toml")
        );
    }

    #[test]
    fn daemon_socket_path_nonempty() {
        let p = daemon_socket_path();
        assert!(
            !p.as_os_str().is_empty(),
            "daemon socket path must not be empty"
        );
    }
}
