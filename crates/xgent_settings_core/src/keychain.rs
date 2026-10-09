//! API Key 的 OS 凭据存储读写（keychain）。
//!
//! 优先从操作系统凭据存储读取，失败时回退配置文件里的 TOML 值。
//!
//! 平台支持：
//! - macOS：`security` CLI（`security add-generic-password` / `find-generic-password`）
//! - Linux：`secret-tool`（libsecret，GNOME Keyring/KWallet）
//! - Windows / 其他：无凭据存储集成，一律走 TOML
//!
//! 两条硬约束：
//! 1. **永不删除已有 TOML 值**——凭据存储不可用时必须仍有回退，否则用户
//!    会因一次失败的迁移永久丢失 API Key。
//! 2. **读失败即回退**，不视为错误——凭据存储是可选增强，不是硬依赖。

use std::path::Path;

/// 凭据条目使用的服务名（macOS keychain 的 "service" / secret-tool 的 service）。
pub const KEYCHAIN_SERVICE: &str = "xgent";

/// 凭据条目使用的账号名（对应 provider id）。
pub fn keychain_account(provider_id: &str) -> String {
    format!("xgent-provider-{provider_id}")
}

/// 从 OS 凭据存储读取 API Key。
///
/// 返回 `None` 表示"取不到"（无凭据存储集成 / 条目不存在 / 读取失败），
/// 调用方应回退到配置文件值。
pub fn read_api_key(provider_id: &str) -> Option<String> {
    let account = keychain_account(provider_id);
    match std::env::consts::OS {
        "macos" => read_macos(&account),
        "linux" => read_linux(&account),
        // Windows 等平台无集成：走 TOML
        _ => None,
    }
}

/// 把 API Key 写入 OS 凭据存储。
///
/// 返回 `Ok(false)` 表示该平台不支持（调用方不应删除 TOML 值）。
pub fn write_api_key(provider_id: &str, api_key: &str) -> std::io::Result<bool> {
    let account = keychain_account(provider_id);
    match std::env::consts::OS {
        "macos" => write_macos(&account, api_key),
        "linux" => write_linux(&account, api_key),
        _ => Ok(false),
    }
}

/// 删除凭据存储中的条目（仅显式调用时；读取路径永不删除）。
pub fn delete_api_key(provider_id: &str) -> std::io::Result<bool> {
    let account = keychain_account(provider_id);
    match std::env::consts::OS {
        "macos" => {
            let out = std::process::Command::new("security")
                .args([
                    "delete-generic-password",
                    "-s",
                    KEYCHAIN_SERVICE,
                    "-a",
                    &account,
                ])
                .output()?;
            Ok(out.status.success())
        }
        "linux" => {
            let out = std::process::Command::new("secret-tool")
                .args(["clear", "service", KEYCHAIN_SERVICE, "username", &account])
                .output()?;
            Ok(out.status.success())
        }
        _ => Ok(false),
    }
}

fn read_macos(account: &str) -> Option<String> {
    let out = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            account,
            // 只取密码字段，不含属性行
            "-w",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8(out.stdout).ok()?;
    let v = v.trim().to_string();
    if v.is_empty() { None } else { Some(v) }
}

fn write_macos(account: &str, api_key: &str) -> std::io::Result<bool> {
    // 先删后加：security 不支持原地更新；删除失败（条目不存在）不算错误
    let _ = std::process::Command::new("security")
        .args([
            "delete-generic-password",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            account,
        ])
        .output();
    let out = std::process::Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            account,
            "-w",
            api_key,
        ])
        .output()?;
    if out.status.success() {
        Ok(true)
    } else {
        // 凭据存储写入失败不致命：调用方保留 TOML 值
        Ok(false)
    }
}

fn read_linux(account: &str) -> Option<String> {
    let out = std::process::Command::new("secret-tool")
        .args(["lookup", "service", KEYCHAIN_SERVICE, "username", account])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8(out.stdout).ok()?;
    let v = v.trim().to_string();
    if v.is_empty() { None } else { Some(v) }
}

fn write_linux(account: &str, api_key: &str) -> std::io::Result<bool> {
    // secret-tool store 需从 stdin 读值
    use std::io::Write;
    let mut child = std::process::Command::new("secret-tool")
        .args([
            "store",
            "--label=XGent API Key",
            "service",
            KEYCHAIN_SERVICE,
            "username",
            account,
        ])
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut si) = child.stdin.take() {
        si.write_all(api_key.as_bytes())?;
    }
    let status = child.wait()?;
    Ok(status.success())
}

/// 解析 API Key：凭据存储优先，回退配置文件值。
///
/// `toml_value` 永不被修改或清除——凭据存储不可用时它是唯一来源。
pub fn resolve_api_key(provider_id: &str, toml_value: &str) -> String {
    match read_api_key(provider_id) {
        Some(k) if !k.trim().is_empty() => k,
        _ => toml_value.to_string(),
    }
}

/// 尝试把 API Key 存入凭据存储（成功则返回 true）。
///
/// 供配置写入路径调用：写入后 keychain 持有值，TOML 值保留作为回退。
pub fn persist_api_key(provider_id: &str, api_key: &str) -> bool {
    if api_key.trim().is_empty() {
        return false;
    }
    matches!(write_api_key(provider_id, api_key), Ok(true))
}

/// 凭据存储是否受当前平台支持。
pub fn is_supported() -> bool {
    matches!(std::env::consts::OS, "macos" | "linux") && helper_available()
}

/// 平台对应的 helper 命令是否可用（仅探测存在性，不读凭据）。
fn helper_available() -> bool {
    let cmd = match std::env::consts::OS {
        "macos" => "security",
        "linux" => "secret-tool",
        _ => return false,
    };
    // 用 `which` 探测避免执行 helper 本身
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 配置目录下的凭据文件名（供未来非 CLI 凭据存储后端使用）。
pub fn credential_file_name(provider_id: &str) -> String {
    format!("{provider_id}.key")
}

/// 凭据文件的完整路径（位于用户配置目录）。
pub fn credential_path(config_dir: &Path, provider_id: &str) -> std::path::PathBuf {
    config_dir.join(credential_file_name(provider_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_differs_per_provider() {
        assert_ne!(
            keychain_account("openai"),
            keychain_account("anthropic"),
            "不同 provider 必须是不同凭据条目"
        );
        assert!(keychain_account("openai").contains("openai"));
    }

    #[test]
    fn resolve_falls_back_to_toml() {
        // TOML 优先（无 keychain 条目时）
        assert_eq!(
            resolve_api_key("nonexistent-test-provider", "sk-toml"),
            "sk-toml"
        );
    }

    #[test]
    fn persist_rejects_empty_value() {
        assert!(!persist_api_key("some-provider", ""));
        assert!(!persist_api_key("some-provider", "   "));
    }

    #[test]
    fn read_unknown_entry_returns_none() {
        // 不应 panic，且不应误返回一个真实值
        assert_eq!(read_api_key("xgent-test-nonexistent-provider-xyz"), None);
    }

    #[test]
    fn delete_unknown_entry_is_ok() {
        // 幂等：删除不存在的条目返回 Ok
        assert!(delete_api_key("xgent-test-nonexistent-provider-xyz").is_ok());
    }

    #[test]
    fn credential_path_under_config_dir() {
        let p = credential_path(Path::new("/tmp/xgent"), "openai");
        assert_eq!(p, Path::new("/tmp/xgent/openai.key"));
    }
}
