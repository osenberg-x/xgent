//! 平台配置路径工具。
//!
//! 统一约定 UI 与 daemon 共用的配置目录、文件路径与 socket 路径。
//!
//! 借鉴 pi 的目录布局（见 `pi/packages/coding-agent/src/config.ts`）：
//! - **全局用户目录** `~/.xgent/agent/`：跨项目共享的配置、会话、认证等。
//!   可经环境变量 `XGENT_AGENT_DIR` 覆盖（开发隔离 / 多实例）。
//! - **项目级目录** `<project_root>/.xgent/`：项目特定配置与资源。
//!
//! 全局目录用 `dirs::home_dir()` 而非 `dirs::config_dir()`，跨平台一致、
//! 用户易找（macOS 不埋在 `~/Library/Application Support` 下）。

use std::path::{Path, PathBuf};

/// 覆盖全局 agent 目录的环境变量名。
///
/// 借鉴 pi 的 `PI_CODING_AGENT_DIR`：设置后所有全局路径（config/sessions/socket）
/// 都改用该目录，便于开发隔离与多实例测试。
pub const ENV_AGENT_DIR: &str = "XGENT_AGENT_DIR";

/// 把 `~` 前缀展开为 home 目录。
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(path)
}

/// 纯函数实现：根据 env 覆盖与 home 目录计算 agent_dir。
///
/// 供测试无副作用调用（不读实际环境变量）。
fn agent_dir_impl(env_val: Option<&str>, home: Option<&Path>) -> PathBuf {
    if let Some(dir) = env_val.filter(|s| !s.is_empty()) {
        return expand_tilde(dir);
    }
    home.unwrap_or_else(|| Path::new("."))
        .join(".xgent")
        .join("agent")
}

/// 全局 agent 目录：`~/.xgent/agent/`，可经 `XGENT_AGENT_DIR` 覆盖。
///
/// 该目录承载跨项目共享的资源：
/// - `config.toml`：全局配置（provider 列表、默认模型、偏好）
/// - `sessions/`：会话历史 JSONL（ADR-0008）
/// - `sessions.db`：会话 SQLite（D-04 预留，未启用）
/// - `auth.json` / `models.json`：预留
///
/// 目录可能不存在；调用方写入前应确保创建。
pub fn agent_dir() -> PathBuf {
    let env_val = std::env::var(ENV_AGENT_DIR).ok();
    let home = dirs::home_dir();
    agent_dir_impl(env_val.as_deref(), home.as_deref())
}

/// 全局配置文件路径（`<agent_dir>/config.toml`）。
pub fn global_config_file() -> PathBuf {
    agent_dir().join("config.toml")
}

/// 会话历史目录（`<agent_dir>/sessions/`）。
///
/// 每个会话一个 JSONL 文件（ADR-0008）。跨项目共享，按 session_id 命名。
pub fn sessions_dir() -> PathBuf {
    agent_dir().join("sessions")
}
/// 单个会话 JSONL 文件路径（`<sessions_dir>/<session_id>.jsonl`）。
pub fn session_file_path(session_id: &str) -> PathBuf {
    sessions_dir().join(format!("{session_id}.jsonl"))
}

/// 会话历史 SQLite 数据库路径（`<agent_dir>/sessions.db`）。
///
/// D-04 预留：MVP 用 JSONL（`session_file_path`），SQLite 未启用。
pub fn sessions_db_path() -> PathBuf {
    agent_dir().join("sessions.db")
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
/// 默认用平台缓存目录（OS 约定 socket 放缓存目录）；
/// 若设置 `XGENT_AGENT_DIR`，则改用其下的 `daemon.sock`，便于开发隔离。
///
/// Windows 用命名管道，路径以 `\\.\pipe\` 前缀表示（MVP 约定）。
pub fn daemon_socket_path() -> PathBuf {
    #[cfg(not(windows))]
    {
        if let Ok(dir) = std::env::var(ENV_AGENT_DIR)
            && !dir.is_empty()
        {
            return expand_tilde(&dir).join("daemon.sock");
        }
        dirs::cache_dir()
            .unwrap_or_else(|| dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")))
            .join("xgent")
            .join("daemon.sock")
    }
    #[cfg(windows)]
    {
        // Windows 命名管道不读环境变量（管道名需固定，UI/daemon 约定一致）
        let _ = ENV_AGENT_DIR;
        PathBuf::from(r"\\.\pipe\xgent-daemon")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use parking_lot::Mutex;

    /// 串行化 env 集成测试（set_var/remove_var 在 edition 2024 为 unsafe 且进程级）。
    static ENV_LOCK: Mutex<()> = parking_lot::const_mutex(());

    #[test]
    fn agent_dir_default_under_home() {
        let home = Path::new("/home/user");
        let dir = agent_dir_impl(None, Some(home));
        assert_eq!(dir, PathBuf::from("/home/user/.xgent/agent"));
    }

    #[test]
    fn agent_dir_env_override() {
        let home = Path::new("/home/user");
        let dir = agent_dir_impl(Some("/custom/dir"), Some(home));
        assert_eq!(dir, PathBuf::from("/custom/dir"));
    }

    #[test]
    fn agent_dir_env_empty_falls_back_to_home() {
        let home = Path::new("/home/user");
        let dir = agent_dir_impl(Some(""), Some(home));
        assert_eq!(dir, PathBuf::from("/home/user/.xgent/agent"));
    }

    #[test]
    fn agent_dir_env_tilde_expansion() {
        let home = dirs::home_dir().expect("home");
        let dir = agent_dir_impl(Some("~/xgent-custom"), Some(&home));
        assert_eq!(dir, home.join("xgent-custom"));
    }

    #[test]
    fn agent_dir_env_integration() {
        // 集成测试：验证真实 env 读取（edition 2024 set_var 为 unsafe）
        let _guard = ENV_LOCK.lock();
        let home = dirs::home_dir().expect("home");
        // SAFETY: 持锁串行化，测试结束清理 env
        unsafe { std::env::set_var(ENV_AGENT_DIR, "/tmp/xgent-integration") };
        assert_eq!(agent_dir(), PathBuf::from("/tmp/xgent-integration"));
        // SAFETY: 持锁串行化
        unsafe { std::env::remove_var(ENV_AGENT_DIR) };
        assert_eq!(agent_dir(), home.join(".xgent").join("agent"));
    }

    #[test]
    fn global_config_file_is_config_toml() {
        let f = global_config_file();
        assert!(f.ends_with("config.toml"));
        assert!(f.starts_with(agent_dir()));
    }

    #[test]
    fn sessions_dir_under_agent_dir() {
        let p = sessions_dir();
        assert!(p.ends_with("sessions"));
        assert!(p.starts_with(agent_dir()));
    }

    #[test]
    fn session_file_path_layout() {
        let p = session_file_path("abc123");
        assert!(p.ends_with("abc123.jsonl"));
        assert!(p.starts_with(sessions_dir()));
    }

    #[test]
    fn sessions_db_under_agent_dir() {
        let p = sessions_db_path();
        assert!(p.ends_with("sessions.db"));
        assert!(p.starts_with(agent_dir()));
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

    #[test]
    fn daemon_socket_env_override() {
        // 纯逻辑：daemon_socket_path 在 XGENT_AGENT_DIR 设置时用其下 daemon.sock
        // 通过 env 集成测（串行化）
        let _guard = ENV_LOCK.lock();
        // SAFETY: 持锁串行化，测试结束清理
        unsafe { std::env::set_var(ENV_AGENT_DIR, "/tmp/xgent-test-sock") };
        let p = daemon_socket_path();
        assert_eq!(p, PathBuf::from("/tmp/xgent-test-sock/daemon.sock"));
        // SAFETY: 持锁串行化
        unsafe { std::env::remove_var(ENV_AGENT_DIR) };
    }
}
