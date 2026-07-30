//! HostState — WASM Store 数据 + host import 实现。
//!
//! 照设计文档 §4.2 / §7.1。从 `wasm_host.rs` 拆出以控制单文件行数。
//! impl `WasiView`（table + ctx）供 WASI host import 使用，impl
//! `xgent::plugin::host::Host`（bindgen 生成）提供宿主能力给插件。
//!
//! 持有 manifest（权限校验）+ config（host.get_config 读取源）+ cancel_token。

use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;
use wasmtime_wasi::{WasiCtx, WasiView};

use crate::manifest::PluginManifest;

/// 插件 Store 数据（命名为 `HostState` 避免与 bindgen 生成的 `PluginState` 冲突）。
///
/// impl `WasiView`（table + ctx）供 WASI host import 使用。
/// 持有 manifest（权限校验）+ config（host.get_config 读取源）+ cancel_token。
pub struct HostState {
    pub(crate) ctx: WasiCtx,
    pub(crate) table: wasmtime::component::ResourceTable,
    pub(crate) manifest: std::sync::Arc<PluginManifest>,
    pub(crate) config: toml::Value,
    pub(crate) cancel_token: CancellationToken,
    pub(crate) project_root: PathBuf,
    #[allow(dead_code)]
    pub(crate) work_dir: PathBuf,
    /// 工具执行期间的 push-update 回调（JSON 字符串，由 PluginTool 侧反序列化）。
    /// execute 前 set，execute 后 clear。None 表示无流式更新订阅。
    pub(crate) push_update: Option<std::sync::Arc<dyn Fn(String) + Send + Sync>>,
}

impl WasiView for HostState {
    fn table(&mut self) -> &mut wasmtime::component::ResourceTable {
        &mut self.table
    }
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.ctx
    }
}

// ===== Host trait 实现（host import 侧）=====
//
// wasmtime bindgen 为 `interface host` 生成 `xgent::plugin::host::Host` trait。
// 我们在 `HostState` 上 impl 它，转发到 manifest/config/cancel。
// 注意：Host trait 方法是 async（因 `async: true`），返回 `Result<T>`（trappable）。

#[allow(clippy::too_many_arguments)]
impl crate::wasm_host::xgent::plugin::host::Host for HostState {
    async fn read_file(&mut self, path: String) -> wasmtime::Result<Result<String, String>> {
        // 规范化校验（防 .. 穿越与 symlink 逃逸，见 check_fs_perm）
        let abs = match self.resolve_and_check(&path, &self.manifest.permissions.fs_read, "fs-read") {
            Ok(p) => p,
            Err(e) => return Ok(Err(e)),
        };
        match tokio::fs::read_to_string(&abs).await {
            Ok(s) => Ok(Ok(s)),
            Err(e) => Ok(Err(format!("读取文件失败: {e}"))),
        }
    }

    async fn write_file(
        &mut self,
        path: String,
        content: String,
    ) -> wasmtime::Result<Result<(), String>> {
        let abs = match self.resolve_and_check(&path, &self.manifest.permissions.fs_write, "fs-write") {
            Ok(p) => p,
            Err(e) => return Ok(Err(e)),
        };
        match tokio::fs::write(&abs, content).await {
            Ok(()) => Ok(Ok(())),
            Err(e) => Ok(Err(format!("写入文件失败: {e}"))),
        }
    }

    async fn log(
        &mut self,
        level: crate::wasm_host::xgent::plugin::host::LogLevel,
        message: String,
    ) -> wasmtime::Result<()> {
        use crate::wasm_host::xgent::plugin::host::LogLevel;
        match level {
            LogLevel::Debug => tracing::debug!(plugin = %self.manifest.id, "{message}"),
            LogLevel::Info => tracing::info!(plugin = %self.manifest.id, "{message}"),
            LogLevel::Warn => tracing::warn!(plugin = %self.manifest.id, "{message}"),
            LogLevel::Error => tracing::error!(plugin = %self.manifest.id, "{message}"),
        }
        Ok(())
    }

    async fn get_config(&mut self, key: String) -> wasmtime::Result<Option<String>> {
        // key 形如 "<plugin_id>.<field>"，从 config 子表取 <field>。
        let field = match key.split_once('.') {
            Some((pid, f)) if pid == self.manifest.id => f,
            _ => return Ok(None),
        };
        let val = self.config.as_table().and_then(|t| t.get(field));
        match val {
            Some(v) => Ok(Some(v.to_string())),
            None => Ok(None),
        }
    }

    async fn run_command(
        &mut self,
        cmd: crate::wasm_host::xgent::plugin::host::CommandReq,
    ) -> wasmtime::Result<
        Result<
            crate::wasm_host::xgent::plugin::host::CommandOutput,
            crate::wasm_host::xgent::plugin::host::CommandError,
        >,
    > {
        use crate::wasm_host::xgent::plugin::host::{CommandError, CommandOutput};
        if !self.manifest.permissions.command.iter().any(|c| c == &cmd.program) {
            return Ok(Err(CommandError::PermissionDenied));
        }
        let cwd = match cmd.cwd.as_deref() {
            Some(p) => match self.resolve_and_check(p, &self.manifest.permissions.fs_read, "cwd(fs-read)") {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(plugin = %self.manifest.id, error = %e, "run_command cwd 权限校验失败");
                    return Ok(Err(CommandError::PermissionDenied));
                }
            },
            None => self.project_root.clone(),
        };
        let mut command = tokio::process::Command::new(&cmd.program);
        command.args(&cmd.args).current_dir(&cwd);
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(e) => return Ok(Err(CommandError::SpawnFailed(e.to_string()))),
        };
        // cancel 关键点：select on child.wait() vs cancel_token.cancelled()
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let wait_fut = async {
            let status = child.wait().await?;
            let out = read_pipe_to_string(stdout).await;
            let err = read_pipe_to_string(stderr).await;
            std::io::Result::Ok(CommandOutput {
                stdout: out,
                stderr: err,
                exit_code: status.code().unwrap_or(-1),
            })
        };
        tokio::select! {
            biased;
            _ = self.cancel_token.cancelled() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                Ok(Err(CommandError::Cancelled))
            }
            res = wait_fut => match res {
                Ok(o) => Ok(Ok(o)),
                Err(e) => Ok(Err(CommandError::Io(e.to_string()))),
            },
        }
    }

    async fn push_update(&mut self, tool_id: String, update: String) -> wasmtime::Result<()> {
        if let Some(cb) = &self.push_update {
            let _ = tool_id;
            cb(update);
        }
        Ok(())
    }
}

async fn read_pipe_to_string(pipe: Option<impl tokio::io::AsyncRead + Unpin>) -> String {
    use tokio::io::AsyncReadExt;
    match pipe {
        Some(mut s) => {
            let mut buf = String::new();
            s.read_to_string(&mut buf).await.ok();
            buf
        }
        None => String::new(),
    }
}

impl HostState {
    /// 解析插件请求路径为绝对路径并做权限校验。
    ///
    /// 三重防护（防 .. 穿越 / symlink 逃逸 / 越界绝对路径）：
    /// 1. 拒绝含 `..` 组件的输入（Path::components 含 ParentDir 即拒）。
    /// 2. 绝对路径必须在 project_root 内，否则拒。
    /// 3. canonicalize 解析后的真实路径（跟随 symlink）再 starts_with 校验——
    ///    读/cwd 路径须存在；写文件可能不存在，canonicalize 父目录。
    fn resolve_and_check(
        &self,
        input: &str,
        patterns: &[String],
        perm_name: &str,
    ) -> Result<PathBuf, String> {
        // 1. 拒绝含 .. 的输入（防 ../ 穿越沙箱边界）
        if Path::new(input).components().any(|c| c == std::path::Component::ParentDir) {
            return Err(format!("{perm_name}: 路径含 .. 被拒绝（沙箱边界）"));
        }
        // 2. 绝对路径必须在 project_root 内
        let p = Path::new(input);
        if p.is_absolute() && !p.starts_with(&self.project_root) {
            return Err(format!("{perm_name}: 绝对路径不在项目根内"));
        }
        let joined = if p.is_absolute() { p.to_path_buf() } else { self.project_root.join(input) };
        // 3. canonicalize（跟随 symlink，解析 ..）后再 starts_with 校验
        //    路径不存在时 canonicalize 父目录（write_file 目标可能未创建）
        let canonical = match std::fs::canonicalize(&joined) {
            Ok(c) => c,
            Err(_) => {
                // canonicalize 父目录，拼接文件名
                let parent = joined.parent().unwrap_or(Path::new(""));
                match std::fs::canonicalize(parent) {
                    Ok(c) => c.join(joined.file_name().unwrap_or_default()),
                    Err(_) => joined.clone(),
                }
            }
        };
        if !canonical.starts_with(&self.project_root) {
            return Err(format!("{perm_name}: 规范化路径不在项目根内: {}", canonical.display()));
        }
        if patterns.is_empty() {
            return Err(format!("插件未声明 {perm_name} 权限"));
        }
        let rel = canonical
            .strip_prefix(&self.project_root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        for pat in patterns {
            if pat == "**" || glob_match(pat, &rel) {
                return Ok(canonical);
            }
        }
        Err(format!("{perm_name}: 路径不匹配权限: {rel}"))
    }
}

/// glob 匹配：支持 `*`（单段内任意字符，不含 `/`）与 `**`（跨段任意）。
///
/// 按路径段（`/` 分隔）递归匹配：
/// - `**` 匹配零或多段（含跨 `/`）；
/// - `*` 匹配单段内任意字符序列；
/// - 其他字符精确匹配。
fn glob_match(pat: &str, s: &str) -> bool {
    let pat_parts: Vec<&str> = pat.split('/').collect();
    let s_parts: Vec<&str> = s.split('/').collect();
    glob_match_parts(&pat_parts, &s_parts)
}

fn glob_match_parts(pat: &[&str], s: &[&str]) -> bool {
    match (pat.split_first(), s.split_first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some((p, ps)), None) => ps.is_empty() && *p == "**",
        (Some((p, ps)), Some((c, cs))) => {
            if *p == "**" {
                // ** 匹配零或多段：尝试所有可能起点
                if ps.is_empty() {
                    return true;
                }
                (0..=cs.len()).any(|i| glob_match_parts(ps, &cs[i..]))
            } else if glob_seg(p, c) {
                glob_match_parts(ps, cs)
            } else {
                false
            }
        }
    }
}

/// 单段匹配：`*` 匹配任意字符序列（不含 `/`），其他字符精确。
fn glob_seg(pat: &str, s: &str) -> bool {
    let pb: Vec<char> = pat.chars().collect();
    let sb: Vec<char> = s.chars().collect();
    glob_seg_inner(&pb, &sb)
}

fn glob_seg_inner(pat: &[char], s: &[char]) -> bool {
    match (pat.split_first(), s.split_first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some(('*', ps)), _) => {
            if ps.is_empty() {
                return true;
            }
            (0..=s.len()).any(|i| glob_seg_inner(ps, &s[i..]))
        }
        (Some((p, ps)), Some((c, cs))) if *p == *c => glob_seg_inner(ps, cs),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_exact() {
        assert!(glob_match("src/lib.rs", "src/lib.rs"));
        assert!(!glob_match("src/lib.rs", "src/main.rs"));
    }

    #[test]
    fn glob_star_single_seg() {
        assert!(glob_match("*.rs", "lib.rs"));
        assert!(glob_match("src/*.rs", "src/lib.rs"));
        assert!(!glob_match("*.rs", "dir/lib.rs"));
    }

    #[test]
    fn glob_multi_star() {
        // 多通配符：原实现 fallthrough 精确匹配会失败，重写后应正确
        assert!(glob_match("a*b*c", "axbxc"));
        assert!(glob_match("src/*/*.rs", "src/foo/lib.rs"));
        assert!(!glob_match("src/*/*.rs", "src/lib.rs"));
    }

    #[test]
    fn glob_double_star_cross_seg() {
        assert!(glob_match("**", "any/deep/path"));
        assert!(glob_match("src/**", "src/a/b/c.rs"));
        assert!(glob_match("**/mod.rs", "a/b/mod.rs"));
        assert!(glob_match("src/**/test.rs", "src/a/b/test.rs"));
    }

    #[test]
    fn glob_double_star_both_sides() {
        // 标准 glob：**/foo/** 匹配任意深度含 foo 的路径
        assert!(glob_match("**/foo/**", "a/foo/b"));
        assert!(glob_match("**/lib*", "src/lib.rs"));
        assert!(glob_match("src/**", "src/a/b/lib.rs"));
    }

    #[test]
    fn glob_empty() {
        assert!(glob_match("", ""));
        assert!(!glob_match("", "x"));
        assert!(!glob_match("x", ""));
    }
}
