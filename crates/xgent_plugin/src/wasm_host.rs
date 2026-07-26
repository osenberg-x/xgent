//! WasmHost — wasmtime 引擎 + Store 管理 + WASI ctx + host import 实现。
//!
//! 照设计文档 §4.2 / §4.4 / §7.1 + 偏差修正 3（cancel 用 Zed 模式，非 cancel_handle）。
//! 对标 Zed `extension_host/src/wasm_host.rs`。
//!
//! 核心结构：
//! - `wasm_engine()` — 全局单例 Engine（component_model + async）。
//! - `WasmHost` — 持 Engine + Linker（OnceLock 单例，含 host import + WASI）。
//! - `WasmPlugin` — 每插件实例：专有 tokio Task 串行处理调用（独占 Store::&mut），
//!   对齐 Zed wasm_host.rs:380-386。
//! - `HostState` — Store 数据：WASI ctx + ResourceTable + manifest + config + cancel。
//!
//! cancel 机制（偏差修正 3）：`call_tool_execute` 用 `tokio::select!` on
//! `signal.cancelled()` vs oneshot Receiver；cancel 时丢弃 oneshot（上层 future
//! 返回 `Err(Aborted)`），不主动中断 WASM——当前调用要么自然完成（结果丢弃），
//! 要么卡在 host import 时由 `host.run_command` 内部的 `tokio::select!` 杀子进程后返回。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, WasiView};

// 宿主侧绑定：生成 `Plugin` world struct + `xgent::plugin::host::Host` trait。
wasmtime::component::bindgen!({
    async: true,
    trappable_imports: true,
    path: "../xgent_plugin_api/wit",
});

use crate::manifest::PluginManifest;
use crate::proxy::PluginHostProxy;

/// WASM 调用错误。
///
/// 照设计文档 §5.3：`Aborted` 对应 `ToolError::Aborted`，`Failed` 对应 `ToolError::Failed`。
#[derive(Debug, Error)]
pub enum WasmCallError {
    #[error("插件调用被中断")]
    Aborted,
    #[error("插件调用失败: {0}")]
    Failed(String),
}

/// 插件 Store 数据（命名为 `HostState` 避免与 bindgen 生成的 `PluginState` 冲突）。
///
/// impl `WasiView`（table + ctx）供 WASI host import 使用。
/// 持有 manifest（权限校验）+ config（host.get_config 读取源）+ cancel_token。
pub struct HostState {
    pub ctx: WasiCtx,
    pub table: wasmtime::component::ResourceTable,
    pub manifest: Arc<PluginManifest>,
    /// 插件级配置子表（来自 GlobalConfig.plugin_settings[<id>]）。
    pub config: toml::Value,
    /// 当前调用的 cancel token（每次 call_tool_execute 注入新 token）。
    pub cancel_token: CancellationToken,
    /// 项目根（host.read_file/write_file 路径校验 + run-command 默认 cwd）。
    pub project_root: PathBuf,
    /// 插件工作目录（WASI preopen 沙箱）。
    pub work_dir: PathBuf,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.ctx
    }
    fn table(&mut self) -> &mut wasmtime::component::ResourceTable {
        &mut self.table
    }
}

// ===== Host trait 实现（host import 侧）=====
//
// wasmtime bindgen 为 `interface host` 生成 `xgent::plugin::host::Host` trait。
// 我们在 `HostState` 上 impl 它，转发到 manifest/config/cancel。
// 注意：Host trait 方法是 async（因 `async: true`），返回 `Result<T>`（trappable）。

impl xgent::plugin::host::Host for HostState {
    async fn read_file(&mut self, path: String) -> wasmtime::Result<Result<String, String>> {
        let abs = self.resolve_project_path(&path);
        match self.check_fs_read(&abs) {
            Err(e) => return Ok(Err(e)),
            Ok(()) => {}
        }
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
        let abs = self.resolve_project_path(&path);
        match self.check_fs_write(&abs) {
            Err(e) => return Ok(Err(e)),
            Ok(()) => {}
        }
        match tokio::fs::write(&abs, content).await {
            Ok(()) => Ok(Ok(())),
            Err(e) => Ok(Err(format!("写入文件失败: {e}"))),
        }
    }

    async fn log(
        &mut self,
        level: xgent::plugin::host::LogLevel,
        message: String,
    ) -> wasmtime::Result<()> {
        match level {
            xgent::plugin::host::LogLevel::Debug => {
                tracing::debug!(plugin = %self.manifest.id, "{message}")
            }
            xgent::plugin::host::LogLevel::Info => {
                tracing::info!(plugin = %self.manifest.id, "{message}")
            }
            xgent::plugin::host::LogLevel::Warn => {
                tracing::warn!(plugin = %self.manifest.id, "{message}")
            }
            xgent::plugin::host::LogLevel::Error => {
                tracing::error!(plugin = %self.manifest.id, "{message}")
            }
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
        cmd: xgent::plugin::host::CommandReq,
    ) -> wasmtime::Result<Result<xgent::plugin::host::CommandOutput, xgent::plugin::host::CommandError>>
    {
        if !self.manifest.permissions.command.iter().any(|c| c == &cmd.program) {
            return Ok(Err(xgent::plugin::host::CommandError::PermissionDenied));
        }
        let cwd = cmd
            .cwd
            .as_deref()
            .map(|p| self.resolve_project_path(p))
            .unwrap_or_else(|| self.project_root.clone());
        let mut command = tokio::process::Command::new(&cmd.program);
        command.args(&cmd.args).current_dir(&cwd);
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(e) => {
                return Ok(Err(xgent::plugin::host::CommandError::SpawnFailed(
                    e.to_string(),
                )))
            }
        };
        // cancel 关键点：select on child.wait() vs cancel_token.cancelled()
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let wait_fut = async {
            let status = child.wait().await?;
            let out = match stdout {
                Some(mut s) => {
                    use tokio::io::AsyncReadExt;
                    let mut buf = String::new();
                    s.read_to_string(&mut buf).await.ok();
                    buf
                }
                None => String::new(),
            };
            let err = match stderr {
                Some(mut s) => {
                    use tokio::io::AsyncReadExt;
                    let mut buf = String::new();
                    s.read_to_string(&mut buf).await.ok();
                    buf
                }
                None => String::new(),
            };
            std::io::Result::Ok(xgent::plugin::host::CommandOutput {
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
                Ok(Err(xgent::plugin::host::CommandError::Cancelled))
            }
            res = wait_fut => match res {
                Ok(o) => Ok(Ok(o)),
                Err(e) => Ok(Err(xgent::plugin::host::CommandError::Io(e.to_string()))),
            },
        }
    }

    async fn http_get(&mut self, _url: String) -> wasmtime::Result<Result<String, String>> {
        Ok(Err("http-get not implemented".into()))
    }
}

impl HostState {
    fn resolve_project_path(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            if p.starts_with(&self.project_root) {
                p.to_path_buf()
            } else {
                self.project_root.join(path)
            }
        } else {
            self.project_root.join(path)
        }
    }

    fn check_fs_read(&self, abs: &Path) -> Result<(), String> {
        if !abs.starts_with(&self.project_root) {
            return Err(format!("路径不在项目根内: {}", abs.display()));
        }
        if self.manifest.permissions.fs_read.is_empty() {
            return Err("插件未声明 fs-read 权限".into());
        }
        let rel = abs
            .strip_prefix(&self.project_root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        for pat in &self.manifest.permissions.fs_read {
            if pat == "**" || glob_match(pat, &rel) {
                return Ok(());
            }
        }
        Err(format!("路径不匹配 fs-read 权限: {rel}"))
    }

    fn check_fs_write(&self, abs: &Path) -> Result<(), String> {
        if !abs.starts_with(&self.project_root) {
            return Err(format!("路径不在项目根内: {}", abs.display()));
        }
        if self.manifest.permissions.fs_write.is_empty() {
            return Err("插件未声明 fs-write 权限".into());
        }
        let rel = abs
            .strip_prefix(&self.project_root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        for pat in &self.manifest.permissions.fs_write {
            if pat == "**" || glob_match(pat, &rel) {
                return Ok(());
            }
        }
        Err(format!("路径不匹配 fs-write 权限: {rel}"))
    }
}

/// 简易 glob 匹配：支持 `*`（单层任意）与 `**`（多层任意）。
fn glob_match(pat: &str, s: &str) -> bool {
    if pat == "**" {
        return true;
    }
    if pat.contains("**") {
        let parts: Vec<&str> = pat.split("**").collect();
        if parts.len() == 2 {
            return s.starts_with(parts[0]) && s.ends_with(parts[1]);
        }
    }
    if pat.contains('*') {
        let parts: Vec<&str> = pat.split('*').collect();
        if parts.len() == 2 {
            return s.starts_with(parts[0]) && s.ends_with(parts[1]);
        }
    }
    pat == s
}
/// 校验插件 API 版本（计划 Step 3.2 + 偏差修正）。
///
/// 扫 WASM component 的 custom section `xgent:api-version`，反解 6 字节
/// （3 字节 major + 3 字节 minor + patch，big-endian），MVP 只接受 `0.1.0`。
/// 缺失 section 或版本不兼容返回 `Err`，拒绝加载。
fn validate_api_version(wasm_bytes: &[u8]) -> Result<(), WasmCallError> {
    use wasmparser::Parser;
    let mut found: Option<[u8; 6]> = None;
    let parser = Parser::new(0);
    for payload in parser.parse_all(wasm_bytes) {
        let payload = match payload {
            Ok(p) => p,
            Err(_) => continue,
        };
        if let wasmparser::Payload::CustomSection(cs) = payload {
            if cs.name() == "xgent:api-version" {
                if cs.data().len() == 6 {
                    let mut buf = [0u8; 6];
                    buf.copy_from_slice(cs.data());
                    found = Some(buf);
                    break;
                }
            }
        }
    }
    let bytes = found.ok_or_else(|| {
        WasmCallError::Failed("插件缺少 xgent:api-version custom section".into())
    })?;
    // 6 字节：major(2) + minor(2) + patch(2) big-endian（对齐 Zed zed:api-version）
    let major = u16::from_be_bytes([bytes[0], bytes[1]]);
    let minor = u16::from_be_bytes([bytes[2], bytes[3]]);
    let patch = u16::from_be_bytes([bytes[4], bytes[5]]);
    // MVP 只接受 0.1.0（D-P2：多版本目录留待后续）
    if (major, minor, patch) == (0, 1, 0) {
        Ok(())
    } else {
        Err(WasmCallError::Failed(format!(
            "插件 API 版本不兼容：{major}.{minor}.{patch}，宿主要求 0.1.0"
        )))
    }
}

/// WASM 引擎单例（全局唯一，所有插件共享）。照设计文档 §4.2。
fn wasm_engine() -> &'static Engine {
    use std::sync::OnceLock;
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        Engine::new(&config).expect("wasmtime Engine 构造失败")
    })
}

/// 构造 WASI ctx：preopen 插件 work_dir 为 `.`，inherit stdio，env。
fn build_wasi_ctx(work_dir: &Path) -> WasiCtx {
    let mut builder = WasiCtxBuilder::new();
    builder.inherit_stdio();
    if work_dir.exists() {
        let _ = builder.preopened_dir(work_dir, ".", DirPerms::all(), FilePerms::all());
    }
    builder.env("PWD", work_dir.to_string_lossy().as_ref());
    builder.env("RUST_BACKTRACE", "full");
    builder.build()
}

/// WASM 宿主：持 Engine + Linker（host import + WASI 已注册）。
pub struct WasmHost {
    engine: Engine,
    linker: Arc<Linker<HostState>>,
    proxy: Arc<PluginHostProxy>,
    project_root: PathBuf,
}

impl WasmHost {
    pub fn new(proxy: Arc<PluginHostProxy>, project_root: PathBuf) -> Arc<Self> {
        let engine = wasm_engine().clone();
        let mut linker: Linker<HostState> = Linker::new(&engine);
        Plugin::add_to_linker(&mut linker, |state: &mut HostState| state)
            .expect("add_to_linker 失败");
        wasmtime_wasi::add_to_linker_async(&mut linker).expect("add_to_linker_async 失败");
        Arc::new(Self {
            engine,
            linker: Arc::new(linker),
            proxy,
            project_root,
        })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn proxy(&self) -> &Arc<PluginHostProxy> {
        &self.proxy
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// 加载插件 WASM 组件，返回 `WasmPlugin`。
    pub async fn load(
        self: &Arc<Self>,
        wasm_bytes: &[u8],
        manifest: Arc<PluginManifest>,
        config: toml::Value,
        work_dir: PathBuf,
    ) -> Result<Arc<WasmPlugin>, WasmCallError> {
        // 校验 API 版本（必须在 Component::from_binary 之前，避免加载不兼容插件）
        validate_api_version(wasm_bytes)?;
        let component = Component::from_binary(&self.engine, wasm_bytes)
            .map_err(|e| WasmCallError::Failed(format!("Component 加载失败: {e}")))?;
        let wasi_ctx = build_wasi_ctx(&work_dir);
        let state = HostState {
            ctx: wasi_ctx,
            table: wasmtime::component::ResourceTable::new(),
            manifest: manifest.clone(),
            config,
            cancel_token: CancellationToken::new(),
            project_root: self.project_root.clone(),
            work_dir: work_dir.clone(),
        };
        let mut store = Store::new(&self.engine, state);

        let bindings = Plugin::instantiate_async(&mut store, &component, &self.linker)
            .await
            .map_err(|e| WasmCallError::Failed(format!("instantiate 失败: {e}")))?;

        // 调 init-extension：构造插件 Extension 实例（register_plugin! 宏导出此函数）。
        // 必须在 register/execute 前调，否则 with_extension panic（OnceLock 未 set）。
        bindings.call_init_extension(&mut store).await
            .map_err(|e| WasmCallError::Failed(format!("init-extension 调用失败: {e}")))?;
        let plugin = Arc::new(WasmPlugin::new(bindings, store));
        Ok(plugin)
    }
}

/// 一次插件调用（专有 Task 串行执行）。
///
/// 闭包签名为 `for<'a> FnOnce(&'a mut Plugin, &'a mut Store<HostState>) -> BoxFuture<'a, ()>`，
/// future 生命周期绑定到调用时的借用（对齐 Zed `ExtensionCall`，wasm_host.rs:310）。
type PluginCall = Box<
    dyn Send + for<'a> FnOnce(&'a mut Plugin, &'a mut Store<HostState>) -> futures::future::BoxFuture<'a, ()>,
>;

/// 加载后的插件实例：专有 tokio Task 串行处理同一插件的调用（独占 Store::&mut）。
pub struct WasmPlugin {
    tx: mpsc::UnboundedSender<PluginCall>,
    in_flight: Arc<std::sync::atomic::AtomicUsize>,
}

impl WasmPlugin {
    fn new(bindings: Plugin, mut store: Store<HostState>) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<PluginCall>();
        let in_flight = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        tokio::spawn(async move {
            let mut bindings = bindings;
            while let Some(call) = rx.recv().await {
                call(&mut bindings, &mut store).await;
            }
            drop(bindings);
            drop(store);
        });
        Self {
            tx,
            in_flight,
        }
    }

    async fn dispatch<F, R>(&self, cancel_token: CancellationToken, build: F) -> Result<R, WasmCallError>
    where
        F: FnOnce(CancellationToken, oneshot::Sender<Result<R, WasmCallError>>) -> PluginCall + Send + 'static,
        R: Send + 'static,
    {
        self.in_flight
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let (resp_tx, resp_rx) = oneshot::channel::<Result<R, WasmCallError>>();
        // clone cancel_token 给 build（select 侧保留原 token 的借用）
        let call = build(cancel_token.child_token(), resp_tx);
        if self.tx.send(call).is_err() {
            self.in_flight
                .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            return Err(WasmCallError::Failed("插件 Task 已退出".into()));
        }
        let in_flight = self.in_flight.clone();
        tokio::select! {
            biased;
            _ = cancel_token.cancelled() => Err(WasmCallError::Aborted),
            res = resp_rx => {
                in_flight.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                match res {
                    Ok(r) => r,
                    Err(_) => Err(WasmCallError::Failed("插件调用 oneshot 失败".into())),
                }
            }
        }
    }

    /// 调用插件 `tool.execute`（cancel 穿透）。
    pub async fn call_tool_execute(
        &self,
        short_id: &str,
        input_json: &str,
        cancel_token: CancellationToken,
    ) -> Result<String, WasmCallError> {
        let short_id = short_id.to_string();
        let input = input_json.to_string();
        self.dispatch(cancel_token, move |cancel, resp_tx| {
            let short_id = short_id.clone();
            let input = input.clone();
            Box::new(move |ext: &mut Plugin, store: &mut Store<HostState>| {
                let short_id = short_id.clone();
                let input = input.clone();
                let cancel = cancel.clone();
                Box::pin(async move {
                    store.data_mut().cancel_token = cancel;
                    let result = ext
                        .xgent_plugin_tool()
                        .call_execute(store, &short_id, &input)
                        .await;
                    let mapped = match result {
                        Ok(Ok(s)) => Ok(s),
                        Ok(Err(e)) => match e {
                            exports::xgent::plugin::tool::ToolError::Aborted => {
                                Err(WasmCallError::Aborted)
                            }
                            exports::xgent::plugin::tool::ToolError::Failed(msg) => {
                                Err(WasmCallError::Failed(msg))
                            }
                        },
                        Err(trap) => Err(WasmCallError::Failed(format!("wasm trap: {trap}"))),
                    };
                    let _ = resp_tx.send(mapped);
                })
            })
        })
        .await
    }

    /// 调用插件 `command.run`。
    pub async fn call_command_run(
        &self,
        short_id: &str,
        cancel_token: CancellationToken,
    ) -> Result<String, WasmCallError> {
        let short_id = short_id.to_string();
        self.dispatch(cancel_token, move |cancel, resp_tx| {
            let short_id = short_id.clone();
            Box::new(move |ext: &mut Plugin, store: &mut Store<HostState>| {
                let short_id = short_id.clone();
                let cancel = cancel.clone();
                Box::pin(async move {
                    store.data_mut().cancel_token = cancel;
                    let result = ext.xgent_plugin_command().call_run(store, &short_id).await;
                    let mapped = match result {
                        Ok(Ok(s)) => Ok(s),
                        Ok(Err(e)) => Err(WasmCallError::Failed(e)),
                        Err(trap) => Err(WasmCallError::Failed(format!("wasm trap: {trap}"))),
                    };
                    let _ = resp_tx.send(mapped);
                })
            })
        })
        .await
    }

    /// 调用插件 `context-provider.retrieve`。
    pub async fn call_retrieve(
        &self,
        short_id: &str,
        query: exports::xgent::plugin::context_provider::ContextQuery,
        cancel_token: CancellationToken,
    ) -> Result<exports::xgent::plugin::context_provider::ContextResult, WasmCallError> {
        let short_id = short_id.to_string();
        self.dispatch(cancel_token, move |cancel, resp_tx| {
            let short_id = short_id.clone();
            let query = query.clone();
            Box::new(move |ext: &mut Plugin, store: &mut Store<HostState>| {
                let short_id = short_id.clone();
                let query = query.clone();
                let cancel = cancel.clone();
                Box::pin(async move {
                    store.data_mut().cancel_token = cancel;
                    let result = ext
                        .xgent_plugin_context_provider()
                        .call_retrieve(store, &short_id, &query)
                        .await;
                    let mapped = match result {
                        Ok(Ok(r)) => Ok(r),
                        Ok(Err(e)) => Err(WasmCallError::Failed(e)),
                        Err(trap) => Err(WasmCallError::Failed(format!("wasm trap: {trap}"))),
                    };
                    let _ = resp_tx.send(mapped);
                })
            })
        })
        .await
    }

    /// 调用插件 `context-provider.on-file-changed`（fire-and-forget，无返回值）。
    ///
    /// `path` 为相对项目根路径的 `Option<String>`（None 表示路径不在项目根内）。
    /// 失败仅记 warn（不阻断，见 §5.5）。
    pub async fn call_on_file_changed(
        &self,
        short_id: &str,
        path: Option<String>,
    ) -> Result<(), WasmCallError> {
        let short_id = short_id.to_string();
        self.dispatch(CancellationToken::new(), move |_cancel, resp_tx| {
            let short_id = short_id.clone();
            Box::new(move |ext: &mut Plugin, store: &mut Store<HostState>| {
                let short_id = short_id.clone();
                Box::pin(async move {
                    let result = ext
                        .xgent_plugin_context_provider()
                        .call_on_file_changed(store, &short_id, path.as_deref())
                        .await;
                    let mapped = result.map_err(|e| WasmCallError::Failed(format!("wasm trap: {e}")));
                    let _ = resp_tx.send(mapped);
                })
            })
        })
        .await
    }
    /// 调用插件 `tool.register`（加载时调，返回工具定义列表）。
    pub async fn call_tool_register(
        &self,
    ) -> Result<Vec<exports::xgent::plugin::tool::ToolDef>, WasmCallError> {
        self.dispatch(CancellationToken::new(), |_cancel, resp_tx| {
            Box::new(move |ext: &mut Plugin, store: &mut Store<HostState>| {
                Box::pin(async move {
                    let result = ext.xgent_plugin_tool().call_register(store).await;
                    let mapped = result.map_err(|e| WasmCallError::Failed(format!("wasm trap: {e}")));
                    let _ = resp_tx.send(mapped);
                })
            })
        })
        .await
    }

    /// 调用插件 `command.register`。
    pub async fn call_command_register(
        &self,
    ) -> Result<Vec<exports::xgent::plugin::command::CommandDef>, WasmCallError> {
        self.dispatch(CancellationToken::new(), |_cancel, resp_tx| {
            Box::new(move |ext: &mut Plugin, store: &mut Store<HostState>| {
                Box::pin(async move {
                    let result = ext.xgent_plugin_command().call_register(store).await;
                    let mapped = result.map_err(|e| WasmCallError::Failed(format!("wasm trap: {e}")));
                    let _ = resp_tx.send(mapped);
                })
            })
        })
        .await
    }

    /// 调用插件 `context-provider.register`。
    pub async fn call_context_provider_register(
        &self,
    ) -> Result<Vec<exports::xgent::plugin::context_provider::ProviderDef>, WasmCallError> {
        self.dispatch(CancellationToken::new(), |_cancel, resp_tx| {
            Box::new(move |ext: &mut Plugin, store: &mut Store<HostState>| {
                Box::pin(async move {
                    let result = ext
                        .xgent_plugin_context_provider()
                        .call_register(store)
                        .await;
                    let mapped = result.map_err(|e| WasmCallError::Failed(format!("wasm trap: {e}")));
                    let _ = resp_tx.send(mapped);
                })
            })
        })
        .await
    }

    /// 当前 in-flight 调用数（升级 in-flight 处理用，§8.4）。
    pub fn in_flight(&self) -> usize {
        self.in_flight.load(std::sync::atomic::Ordering::SeqCst)
    }
}
