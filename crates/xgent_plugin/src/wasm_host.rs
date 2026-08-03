//! WasmHost — wasmtime 引擎 + Store 管理 + WASI ctx + host import 实现。
//!
//! 照设计文档 §4.2 / §4.4 / §7.1 + 偏差修正 3（cancel 用 Zed 模式，非 cancel_handle）。
//! 对标 Zed `extension_host/src/wasm_host.rs`。
//!
//! 核心结构：
//! - `wasm_engine()` — 全局单例 Engine（component_model + async）。
//! - `WasmHost` — 持 Engine + Linker（LazyLock 单例，含 host import + WASI）。
//! - `WasmPlugin` — 每插件实例：专有 tokio Task 串行处理调用（独占 Store::&mut），
//!   对齐 Zed wasm_host.rs:380-386。
//! - `HostState` — Store 数据（见 `host_state.rs`）。
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
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder};

// 宿主侧绑定：生成 `Plugin` world struct + `xgent::plugin::host::Host` trait。
wasmtime::component::bindgen!({
    async: true,
    trappable_imports: true,
    path: "../xgent_plugin_api/wit",
});

use crate::host_state::HostState;
use crate::manifest::PluginManifest;
use crate::proxy::PluginHostProxy;

/// WASM 调用错误。
///
/// 照设计文档 §5.3：`Aborted` 对应 `ToolError::Aborted`，`Failed` 对应 `ToolError::Failed`。
#[derive(Debug, Error)]
pub enum WasmCallError {
    #[error("WASM 调用被中断")]
    Aborted,
    #[error("WASM 调用失败: {0}")]
    Failed(String),
}


/// 校验插件 API 版本（计划 Step 3.2 + 偏差修正）。
///
/// 扫 WASM component 的 custom section `xgent:api-version`，反解 6 字节
/// （major(2) + minor(2) + patch(2)，big-endian），MVP 只接受 `0.1.0`。
/// 缺失 section 或版本不兼容返回 `Err`，拒绝加载。
fn validate_api_version(wasm_bytes: &[u8]) -> Result<(), WasmCallError> {
    use wasmparser::Parser;
    let mut found: Option<[u8; 6]> = None;
    let parser = Parser::new(0);
    for payload in parser.parse_all(wasm_bytes) {
        let payload = match payload {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "WASM payload 解析失败，跳过该段");
                continue;
            }
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
    let bytes = found
        .ok_or_else(|| WasmCallError::Failed("插件缺少 xgent:api-version custom section".into()))?;
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
///
/// 用 `LazyLock<Result<Engine, WasmCallError>>` 持存构造结果，避免库代码
/// `expect`（AGENTS §5.7）。构造失败极罕见（系统资源不足），首次失败后
/// 被缓存，后续调用复用同一错误。
///
/// 注：稳定 Rust（1.97）`OnceLock::get_or_try_init` 仍不稳定（issue #109737），
/// 故用 `LazyLock` 而非 `OnceLock + get_or_try_init`。
fn wasm_engine() -> Result<&'static Engine, WasmCallError> {
    static ENGINE: std::sync::LazyLock<Result<Engine, WasmCallError>> =
        std::sync::LazyLock::new(|| {
            let mut config = wasmtime::Config::new();
            config.wasm_component_model(true);
            config.async_support(true);
            Engine::new(&config)
                .map_err(|e| WasmCallError::Failed(format!("wasmtime Engine 构造失败: {e}")))
        });
    ENGINE.as_ref().map_err(|e| WasmCallError::Failed(e.to_string()))
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
    /// 构造 WasmHost。Engine/Linker 构造失败经 `Result` 传播（AGENTS §5.7）。
    pub fn new(
        proxy: Arc<PluginHostProxy>,
        project_root: PathBuf,
    ) -> Result<Arc<Self>, WasmCallError> {
        let engine = wasm_engine()?.clone();
        let mut linker: Linker<HostState> = Linker::new(&engine);
        Plugin::add_to_linker(&mut linker, |state: &mut HostState| state)
            .map_err(|e| WasmCallError::Failed(format!("add_to_linker 失败: {e}")))?;
        wasmtime_wasi::add_to_linker_async(&mut linker)
            .map_err(|e| WasmCallError::Failed(format!("add_to_linker_async 失败: {e}")))?;
        Ok(Arc::new(Self {
            engine,
            linker: Arc::new(linker),
            proxy,
            project_root,
        }))
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
            push_update: None,
        };
        let mut store = Store::new(&self.engine, state);

        let bindings = Plugin::instantiate_async(&mut store, &component, &self.linker)
            .await
            .map_err(|e| WasmCallError::Failed(format!("instantiate 失败: {e}")))?;

        // 调 init-extension：构造插件 Extension 实例（register_plugin! 宏导出此函数）。
        // 必须在 register/execute 前调，否则 with_extension panic（OnceLock 未 set）。
        bindings
            .call_init_extension(&mut store)
            .await
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
    dyn Send
        + for<'a> FnOnce(
            &'a mut Plugin,
            &'a mut Store<HostState>,
        ) -> futures::future::BoxFuture<'a, ()>,
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
        Self { tx, in_flight }
    }

    async fn dispatch<F, R>(&self, cancel_token: CancellationToken, build: F) -> Result<R, WasmCallError>
    where
        F: FnOnce(CancellationToken, oneshot::Sender<Result<R, WasmCallError>>) -> PluginCall
            + Send
            + 'static,
        R: Send + 'static,
    {
        let _guard = InFlightGuard::new(self.in_flight.clone());
        let (resp_tx, resp_rx) = oneshot::channel::<Result<R, WasmCallError>>();
        // clone cancel_token 给 build（select 侧保留原 token 的借用）
        let call = build(cancel_token.child_token(), resp_tx);
        if self.tx.send(call).is_err() {
            return Err(WasmCallError::Failed("插件 Task 已退出".into()));
        }
        // guard 在 select! 命中 resp_rx 后由 Drop 减计数；cancel 分支
        // 同样经 guard drop 减计数（future 结束）。
        tokio::select! {
            biased;
            _ = cancel_token.cancelled() => Err(WasmCallError::Aborted),
            res = resp_rx => match res {
                Ok(r) => r,
                Err(_) => Err(WasmCallError::Failed("插件调用 oneshot 失败".into())),
            },
        }
    }

    /// 调用插件 `tool.execute`（cancel 穿透）。
    pub async fn call_tool_execute(
        &self,
        short_id: &str,
        input_json: &str,
        cancel_token: CancellationToken,
        on_update: Option<std::sync::Arc<dyn Fn(String) + Send + Sync>>,
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
                let on_update = on_update.clone();
                Box::pin(async move {
                    store.data_mut().cancel_token = cancel;
                    // set push-update 回调（§5.3.1），execute 后 clear
                    store.data_mut().push_update = on_update;
                    let result = ext
                        .xgent_plugin_tool()
                        .call_execute(&mut *store, &short_id, &input)
                        .await;
                    store.data_mut().push_update = None;
                    let mapped = map_tool_result(result);
                    let _ = resp_tx.send(mapped);
                })
            })
        })
        .await
    }

    /// 调用插件 `tool.summarize`（async WIT 调用，无 cancel 需求）。
    pub async fn call_tool_summarize(
        &self,
        short_id: &str,
        input_json: &str,
    ) -> Result<String, WasmCallError> {
        let short_id = short_id.to_string();
        let input = input_json.to_string();
        self.dispatch(CancellationToken::new(), move |_cancel, resp_tx| {
            let short_id = short_id.clone();
            let input = input.clone();
            Box::new(move |ext: &mut Plugin, store: &mut Store<HostState>| {
                let short_id = short_id.clone();
                let input = input.clone();
                Box::pin(async move {
                    let result = ext
                        .xgent_plugin_tool()
                        .call_summarize(store, &short_id, &input)
                        .await;
                    let mapped = map_trap(result);
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
                    let mapped = map_result_string(result);
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
                    let mapped = map_result_string(result);
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
                    let mapped = map_trap(result);
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
                    let mapped = map_trap(result);
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
                    let mapped = map_trap(result);
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
                    let mapped = map_trap(result);
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

/// `tool.execute` 返回的 `wasmtime::Result<Result<String, ToolError>>` 映射为 `WasmCallError`。
fn map_tool_result(
    result: wasmtime::Result<Result<String, exports::xgent::plugin::tool::ToolError>>,
) -> Result<String, WasmCallError> {
    match result {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => match e {
            exports::xgent::plugin::tool::ToolError::Aborted => Err(WasmCallError::Aborted),
            exports::xgent::plugin::tool::ToolError::Failed(msg) => Err(WasmCallError::Failed(msg)),
        },
        Err(trap) => Err(WasmCallError::Failed(format!("wasm trap: {trap}"))),
    }
}

/// `Result<Result<T, String>, wasmtime::Error>` → `Result<T, WasmCallError>`。
///
/// command.run / context-provider.retrieve 的 WIT 返回 `result<T, string>`，
/// 外层是 wasmtime trap。统一映射避免各处手写。
fn map_result_string<T>(
    result: wasmtime::Result<Result<T, String>>,
) -> Result<T, WasmCallError> {
    match result {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(WasmCallError::Failed(e)),
        Err(trap) => Err(WasmCallError::Failed(format!("wasm trap: {trap}"))),
    }
}

/// `wasmtime::Result<T>` 的 trap 映射为 `WasmCallError`（单层，非双层 execute）。
fn map_trap<T>(result: wasmtime::Result<T>) -> Result<T, WasmCallError> {
    result.map_err(|e| WasmCallError::Failed(format!("wasm trap: {e}")))
}

/// in-flight 调用计数 RAII guard：构造时 +1，Drop 时 -1。
///
/// 避免错误路径手写 fetch_sub 遗漏（AGENTS §5.7 DRY）。
struct InFlightGuard(Arc<std::sync::atomic::AtomicUsize>);

impl InFlightGuard {
    fn new(counter: Arc<std::sync::atomic::AtomicUsize>) -> Self {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self(counter)
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}
