//! PluginHost — 插件存储：管理安装/卸载/索引/重载。
//!
//! 照设计文档 §7.3 + §8（生命周期）+ §13 Step P4。对标 Zed `ExtensionStore`。
//!
//! 核心方法：`reload()` / `install_extension` / `uninstall_extension` /
//! `install_dev_extension` / `extensions_updated` / `load_builtin_plugins`。
//!
//! **启动时序硬性**（§13 Step P4）：`xgent_app` 在各业务 Plugin init 后显式调
//! `load_builtin_plugins()`，此时 proxy 已就绪，register_* 生效。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::index::{PluginIndex, PluginIndexEntry};
use crate::manifest::PluginManifest;
use crate::proxy::PluginHostProxy;
use crate::wasm_host::{WasmCallError, WasmHost, WasmPlugin};
use crate::{WitCommandDef, WitContextProviderDef, WitToolDef};

/// 插件宿主事件（发到 ECS，由 PluginPollSystem 消费转 Message）。
#[derive(Debug, Clone)]
pub enum PluginEvent {
    /// 命令执行完成（command.run 返回）。
    CommandResult { command_id: String, success: bool, message: String },
    /// 插件卸载完成（通知 ECS 清理 ToolExecutor/CommandRegistry/ContextHub）。
    Unregister { plugin_id: String },
    /// 插件加载完成（通知 ECS 刷新插件管理面板）。
    /// 照设计文档 §4.3 生命周期事件经 mpsc→PluginPollSystem→Message。
    Loaded { plugin_id: String },
}

/// PluginHost：加载/卸载/索引/重载。
pub struct PluginHost {
    proxy: Arc<PluginHostProxy>,
    wasm_host: Arc<WasmHost>,
    installed_dir: PathBuf,
    index_path: PathBuf,
    /// 已加载的插件：(manifest, wasm_plugin)。
    loaded: Mutex<Vec<(Arc<PluginManifest>, Arc<WasmPlugin>)>>,
    /// 升级期间待 drop 的旧实例 + 入队时间（§8.4，60s 超时强制 drop）。
    pending_drop: Mutex<Vec<(Arc<WasmPlugin>, std::time::Instant)>>,
    /// 事件 channel：发到 ECS（PluginPollSystem drain）。
    event_tx: mpsc::UnboundedSender<PluginEvent>,
    /// 内建插件资源目录（随二进制发布的预装 wasm）。
    assets_dir: Option<PathBuf>,
    /// 插件级配置子表（plugin_id → toml::Value，来自 GlobalConfig.plugin_settings）。
    /// `host.get_config("<id>.<field>")` 从此取值（§10.4）。
    plugin_settings: Mutex<std::collections::BTreeMap<String, toml::Value>>,
    /// 已安装插件 id → 是否启用（来自 GlobalConfig.plugin.enabled，§10.2）。
    /// reload_all 据此过滤：disabled 的已安装插件不加载。
    enabled: Mutex<std::collections::BTreeMap<String, bool>>,
    watcher_stop: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,

}

impl PluginHost {
    /// 构造 PluginHost。`event_rx` 由调用方持有，在 ECS 轮询系统 drain。
    pub fn new(
        proxy: Arc<PluginHostProxy>,
        wasm_host: Arc<WasmHost>,
        plugins_dir: PathBuf,
        assets_dir: Option<PathBuf>,
        plugin_settings: std::collections::BTreeMap<String, toml::Value>,
        enabled: std::collections::BTreeMap<String, bool>,
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<PluginEvent>) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let installed_dir = plugins_dir.join("installed");
        let index_path = plugins_dir.join("index.json");
        let host = Arc::new(Self {
            proxy,
            wasm_host,
            installed_dir,
            index_path,
            loaded: Mutex::new(Vec::new()),
            pending_drop: Mutex::new(Vec::new()),
            event_tx,
            assets_dir,
            plugin_settings: Mutex::new(plugin_settings),
            enabled: Mutex::new(enabled),
            watcher_stop: Mutex::new(None),
        });
        (host, event_rx)
    }

    /// 更新插件配置（配置文件变更时调，§10.4/§10.2）。
    ///
    /// 注意：仅更新内存缓存，已加载插件的 `HostState.config` 不会热更新——
    /// 需重新加载插件（uninstall + reload）才能生效。MVP 接受此限制。
    /// enabled 变更需调用方随后调 `reload_all` 生效（enable/disable 已封装该方法）。
    pub fn update_plugin_settings(
        &self,
        plugin_settings: std::collections::BTreeMap<String, toml::Value>,
        enabled: std::collections::BTreeMap<String, bool>,
    ) {
        *self.plugin_settings.lock() = plugin_settings;
        *self.enabled.lock() = enabled;
    }

    /// 插件是否启用（§8.3）。enabled 表中缺失视为启用（默认 true）。
    pub fn is_enabled(&self, plugin_id: &str) -> bool {
        self.enabled.lock().get(plugin_id).copied().unwrap_or(true)
    }

    /// 启用插件（§8.3）：设 enabled=true + reload 加载。
    pub async fn enable_extension(&self, plugin_id: &str) -> Result<(), PluginHostError> {
        self.enabled.lock().insert(plugin_id.to_string(), true);
        self.reload_all().await
    }

    /// 禁用插件（§8.3）：设 enabled=false + unregister（不卸载 WASM）。
    pub async fn disable_extension(&self, plugin_id: &str) -> Result<(), PluginHostError> {
        self.enabled.lock().insert(plugin_id.to_string(), false);
        self.unregister(plugin_id).await;
        Ok(())
    }

    /// 从本地源码目录安装 dev 插件（§8.5 dev 模式）。
    ///
    /// 用 symlink 指向源码目录（设计 §8.5 要求），源码改动自动反映到
    /// `installed/<id>/`，dev 热重载靠 `start_file_watcher` 监听 symlink
    /// 目标的 `.wasm` 变更。symlink 失败（如 Windows 非开发者模式）时
    /// 回退为复制目录 + warn。源码目录需含 `plugin.toml` 与 `extension.wasm`。
    pub async fn install_dev_extension(
        &self,
        src: &Path,
        plugin_id: &str,
    ) -> Result<(), PluginHostError> {
        let dst = self.installed_dir.join(plugin_id);
        if dst.exists() {
            std::fs::remove_dir_all(&dst)
                .map_err(|e| PluginHostError::Uninstall(format!("清理旧 dev 目录失败: {e}")))?;
        }
        // 优先 symlink（设计 §8.5），失败回退复制（跨平台兼容）
        if let Err(e) = symlink_dir(src, &dst) {
            tracing::warn!(plugin = %plugin_id, error = %e, "symlink 创建失败，回退复制目录");
            copy_dir_recursive(src, &dst).map_err(|e| {
                PluginHostError::Load(format!("复制 dev 插件目录失败: {e}"))
            })?;
        }
        self.reload_all().await
    }

    /// 启动文件监听（生产模式，§8.5）。
    ///
    /// notify 监听 `installed_dir`，`.wasm` 文件 `close-write` 事件经 200ms debounce
    /// 后调 `reload_all`。dev 模式不监听（显式 `reload` 命令触发）。
    ///
    /// 由 `xgent_app` 在构造 PluginHost 后调，传入 tokio runtime handle。
    pub fn start_file_watcher(self: &Arc<Self>, handle: tokio::runtime::Handle) {
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        *self.watcher_stop.lock() = Some(stop_tx);
        let installed_dir = self.installed_dir.clone();
        let host = self.clone();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel::<()>();
        Self::spawn_watcher_thread(installed_dir, ev_tx, stop_rx);
        Self::spawn_debounce_task(handle, host, ev_rx);
    }

    /// 停止文件监听（测试/关闭时调， watcher 线程退出，§8.5 生命周期）。
    pub fn stop_file_watcher(&self) {
        if let Some(tx) = self.watcher_stop.lock().take() {
            let _ = tx.send(());
        }
    }

    /// watcher 线程：notify 监听 installed_dir，.wasm close-write 事件发 ev_tx。
    /// stop_rx 收到或 sender drop 时退出（watcher Drop 自动清理 notify 句柄）。
    fn spawn_watcher_thread(
        installed_dir: PathBuf,
        ev_tx: mpsc::UnboundedSender<()>,
        stop_rx: tokio::sync::oneshot::Receiver<()>,
    ) {
        std::thread::spawn(move || {
            use notify::{EventKind, RecursiveMode, Watcher};
            use notify::event::{AccessKind, AccessMode};
            // notify watcher 是 blocking，放独立线程；事件经 channel 发到 tokio task
            let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(ev) = res {
                    // 仅 .wasm 文件的写后关闭事件（避免部分写入时加载损坏 WASM）
                    let is_wasm_close = matches!(
                        ev.kind,
                        EventKind::Access(AccessKind::Close(AccessMode::Write))
                    ) && ev.paths.iter().any(|p| p.extension().is_some_and(|e| e == "wasm"));
                    if is_wasm_close {
                        let _ = ev_tx.send(());
                    }
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!(error = %e, "插件目录文件监听启动失败");
                    return;
                }
            };
            let _ = watcher.watch(&installed_dir.as_path(), RecursiveMode::Recursive);
            // 阻塞至 stop 信号；watcher 在闭包末尾 Drop 清理 notify 句柄。
            let _ = stop_rx.blocking_recv();
        });
    }

    /// debounce task：200ms 无新事件后 reload_all。
    fn spawn_debounce_task(
        handle: tokio::runtime::Handle,
        host: Arc<PluginHost>,
        mut ev_rx: mpsc::UnboundedReceiver<()>,
    ) {
        handle.spawn(async move {
            let mut last = std::time::Instant::now();
            let mut pending = false;
            loop {
                match tokio::time::timeout(std::time::Duration::from_millis(200), ev_rx.recv()).await {
                    Ok(Some(())) => {
                        pending = true;
                        last = std::time::Instant::now();
                    }
                    Ok(None) => break, // sender drop（watcher 线程退出）
                    Err(_) => {
                        // timeout：若有 pending 且距 last >= 200ms，触发 reload
                        if pending && last.elapsed() >= std::time::Duration::from_millis(200) {
                            pending = false;
                            if let Err(e) = host.reload_all().await {
                                tracing::warn!(error = %e, "插件文件变更 reload 失败");
                            }
                        }
                    }
                }
            }
        });
    }
    pub fn proxy(&self) -> &Arc<PluginHostProxy> {
        &self.proxy
    }

    pub fn wasm_host(&self) -> &Arc<WasmHost> {
        &self.wasm_host
    }

    /// 加载内建插件（首次启动预装）。
    ///
    /// 照设计文档 §13 Step P6：检测 `installed/` 为空时从 `assets/plugins/` 预装。
    pub async fn load_builtin_plugins(&self) -> Result<(), PluginHostError> {
        let assets = match &self.assets_dir {
            Some(d) if d.exists() => d.clone(),
            _ => return Ok(()),
        };
        // 扫描 assets/plugins/<id>/
        for entry in std::fs::read_dir(&assets).into_iter().flatten() {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let id = match dir.file_name().and_then(|n| n.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            // 若已安装则跳过
            let installed = self.installed_dir.join(&id);
            if installed.exists() {
                continue;
            }
            // 复制 assets/plugins/<id>/ → installed/<id>/
            if let Err(e) = copy_dir_recursive(&dir, &installed) {
                tracing::warn!(plugin = %id, error = %e, "预装内建插件失败");
                continue;
            }
            tracing::info!(plugin = %id, "预装内建插件");
        }
        // 加载所有已安装插件
        self.reload_all().await
    }

    /// 重载所有已安装插件（扫描 installed/，diff 加载/卸载）。
    pub async fn reload_all(&self) -> Result<(), PluginHostError> {
        let manifests = self.scan_installed_manifests();
        let new_ids: std::collections::HashSet<String> =
            manifests.iter().map(|m| m.id.clone()).collect();
        let old_ids: std::collections::HashSet<String> = {
            self.loaded.lock().iter().map(|(m, _)| m.id.clone()).collect()
        };
        // 卸载移除的
        for id in old_ids.difference(&new_ids) {
            self.unregister(id).await;
        }
        // 加载新增的（跳过 disabled，§10.2 enabled 过滤）
        for manifest in &manifests {
            if old_ids.contains(&manifest.id) {
                continue;
            }
            if !self.is_enabled(&manifest.id) {
                tracing::debug!(plugin = %manifest.id, "插件已禁用，跳过加载");
                continue;
            }
            if let Err(e) = self.load_plugin(manifest.clone()).await {
                tracing::warn!(plugin = %manifest.id, error = %e, "加载插件失败");
            }
        }
        self.persist_index(&manifests);
        Ok(())
    }

    /// 扫描 installed/ 目录，解析所有 plugin.toml 为清单（失败跳过+warn）。
    fn scan_installed_manifests(&self) -> Vec<Arc<PluginManifest>> {
        let mut out = Vec::new();
        if !self.installed_dir.exists() {
            return out;
        }
        for entry in std::fs::read_dir(&self.installed_dir).into_iter().flatten().flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let toml_str = match std::fs::read_to_string(dir.join("plugin.toml")) {
                Ok(s) => s,
                Err(_) => continue,
            };
            match PluginManifest::from_toml(&toml_str) {
                Ok(m) => out.push(Arc::new(m)),
                Err(e) => tracing::warn!(dir = %dir.display(), error = %e, "清单解析失败，跳过"),
            }
        }
        out
    }

    /// 持久化插件索引到 index.json（失败 warn 不阻断）。
    fn persist_index(&self, manifests: &[Arc<PluginManifest>]) {
        if let Some(parent) = self.index_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!(error = %e, "创建 index.json 父目录失败");
            }
        }
        let index = PluginIndex {
            plugins: manifests
                .iter()
                .map(|m| {
                    (
                        m.id.as_str().into(),
                        PluginIndexEntry { manifest: (**m).clone(), dev: false, enabled: true },
                    )
                })
                .collect(),
        };
        if let Ok(bytes) = index.to_json() {
            if let Err(e) = std::fs::write(&self.index_path, bytes) {
                tracing::warn!(error = %e, "写 index.json 失败");
            }
        }
    }

    /// 加载单个插件：读 wasm + instantiate + register 扩展点。
    async fn load_plugin(
        &self,
        manifest: Arc<PluginManifest>,
    ) -> Result<(), PluginHostError> {
        let wasm_plugin = self.instantiate_plugin(&manifest).await?;
        self.register_extensions(&manifest, &wasm_plugin).await;
        // 发加载完成事件（ECS 侧刷新插件管理面板，§4.3）
        let _ = self.event_tx.send(PluginEvent::Loaded {
            plugin_id: manifest.id.clone(),
        });
        self.loaded.lock().push((manifest, wasm_plugin));
        Ok(())
    }

    /// 读 wasm + 配置 + instantiate（拆出控制 load_plugin 行数）。
    async fn instantiate_plugin(
        &self,
        manifest: &Arc<PluginManifest>,
    ) -> Result<Arc<WasmPlugin>, PluginHostError> {
        let plugin_dir = self.installed_dir.join(&manifest.id);
        let wasm_bytes = std::fs::read(plugin_dir.join("extension.wasm"))
            .map_err(|e| PluginHostError::Load(format!("读 wasm 失败: {e}")))?;
        let config = self
            .plugin_settings
            .lock()
            .get(&manifest.id)
            .cloned()
            .unwrap_or_else(|| toml::Value::Table(Default::default()));
        let work_dir = plugin_dir.join("work");
        if let Err(e) = std::fs::create_dir_all(&work_dir) {
            tracing::warn!(plugin = %manifest.id, error = %e, "创建 work_dir 失败");
        }
        self.wasm_host
            .load(&wasm_bytes, manifest.clone(), config, work_dir)
            .await
            .map_err(|e| PluginHostError::Load(e.to_string()))
    }

    /// 注册工具/命令/ContextProvider 三类扩展点（失败 warn 不阻断）。
    async fn register_extensions(&self, manifest: &Arc<PluginManifest>, plugin: &Arc<WasmPlugin>) {
        let pid = &manifest.id;
        match plugin.call_tool_register().await {
            Ok(defs) if !defs.is_empty() => self.register_tool_proxy(manifest, plugin, defs),
            Ok(_) => {}
            Err(e) => tracing::warn!(plugin = %pid, error = %e, "register tools 失败"),
        }
        match plugin.call_command_register().await {
            Ok(defs) if !defs.is_empty() => self.register_command_proxy(manifest, plugin, defs),
            Ok(_) => {}
            Err(e) => tracing::warn!(plugin = %pid, error = %e, "register commands 失败"),
        }
        match plugin.call_context_provider_register().await {
            Ok(defs) if !defs.is_empty() => self.register_provider_proxy(manifest, plugin, defs),
            Ok(_) => {}
            Err(e) => tracing::warn!(plugin = %pid, error = %e, "register providers 失败"),
        }
    }

    fn register_tool_proxy(&self, manifest: &Arc<PluginManifest>, plugin: &Arc<WasmPlugin>, defs: Vec<WitToolDef>) {
        let pid = &manifest.id;
        if let Err(e) = self
            .proxy
            .tool()
            .and_then(|p| p.register_tools(manifest.clone(), plugin.clone(), defs))
        {
            tracing::warn!(plugin = %pid, error = %PluginHostError::from(e), "注册工具失败");
        }
    }

    fn register_command_proxy(&self, manifest: &Arc<PluginManifest>, plugin: &Arc<WasmPlugin>, defs: Vec<WitCommandDef>) {
        let pid = &manifest.id;
        if let Err(e) = self
            .proxy
            .command()
            .and_then(|p| p.register_commands(manifest.clone(), plugin.clone(), defs))
        {
            tracing::warn!(plugin = %pid, error = %PluginHostError::from(e), "注册命令失败");
        }
    }

    fn register_provider_proxy(&self, manifest: &Arc<PluginManifest>, plugin: &Arc<WasmPlugin>, defs: Vec<WitContextProviderDef>) {
        let pid = &manifest.id;
        let project_root = self.wasm_host.project_root().to_path_buf();
        if let Err(e) = self
            .proxy
            .context()
            .and_then(|p| p.register_providers(manifest.clone(), plugin.clone(), defs, project_root))
        {
            tracing::warn!(plugin = %pid, error = %PluginHostError::from(e), "注册 ContextProvider 失败");
        }
    }

    /// 卸载插件：发 Unregister 事件给 ECS（清理 ToolExecutor 等），保留 WasmPlugin
    /// 直到 in-flight=0（§8.4）。
    pub async fn unregister(&self, plugin_id: &str) {
        let removed = {
            let mut loaded = self.loaded.lock();
            let idx = loaded.iter().position(|(m, _)| m.id == plugin_id);
            match idx {
                Some(i) => {
                    let (manifest, plugin) = loaded.remove(i);
                    // 发 Unregister 事件（ECS 侧清理 ToolExecutor/CommandRegistry/ContextHub）
                    let _ = self.event_tx.send(PluginEvent::Unregister {
                        plugin_id: manifest.id.clone(),
                    });
                    Some(plugin)
                }
                None => None,
            }
        };
        if let Some(plugin) = removed {
            if plugin.in_flight() > 0 {
                // 入 pending_drop，等 in-flight=0 后 drop；
                // 60s 超时强制移除——drop WasmPlugin 只 drop tx channel，
                // 残留 in-flight 调用 oneshot 返回 Failed（不 panic，见 drain_pending_drop）
                self.pending_drop
                    .lock()
                    .push((plugin, std::time::Instant::now()));
            }
            // in_flight=0 时直接 drop（Plugin 已无引用，Store drop）
        }
    }

    /// 卸载插件（用户命令入口）：删目录 + reload。
    pub async fn uninstall_extension(&self, plugin_id: &str) -> Result<(), PluginHostError> {
        let dir = self.installed_dir.join(plugin_id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .map_err(|e| PluginHostError::Uninstall(format!("删除目录失败: {e}")))?;
        }
        self.unregister(plugin_id).await;
        Ok(())
    }

    /// 取已加载插件的 WasmPlugin（按 id）。
    pub fn get_plugin(&self, plugin_id: &str) -> Option<Arc<WasmPlugin>> {
        self.loaded
            .lock()
            .iter()
            .find(|(m, _)| m.id == plugin_id)
            .map(|(_, p)| p.clone())
    }

    /// 取已加载插件清单（按 id）。
    pub fn get_manifest(&self, plugin_id: &str) -> Option<Arc<PluginManifest>> {
        self.loaded
            .lock()
            .iter()
            .find(|(m, _)| m.id == plugin_id)
            .map(|(m, _)| m.clone())
    }

    /// 列出已加载插件 id。
    pub fn list_loaded(&self) -> Vec<String> {
        self.loaded
            .lock()
            .iter()
            .map(|(m, _)| m.id.clone())
            .collect()
    }

    /// 检查 pending_drop 中 in-flight=0 的实例并 drop（升级 in-flight 处理，§8.4）。
    ///
    /// 60s 超时强制 drop：超时后即使 in_flight>0 也移除——drop WasmPlugin 只 drop
    /// tx channel，专有 task 的 `rx.recv()` 返回 None 自然退出，残留 in-flight 调用
    /// 的 oneshot 返回 `WasmCallError::Failed`（不 panic）。host.run-command cancel
    /// 已保证 in-flight 在 1s 内归零，60s 超时是兜底。
    pub fn drain_pending_drop(&self) {
        let timeout = std::time::Duration::from_secs(60);
        let now = std::time::Instant::now();
        let mut pd = self.pending_drop.lock();
        pd.retain(|(p, t)| p.in_flight() > 0 && now.duration_since(*t) < timeout);
    }

    /// 发命令执行结果事件（PluginCommand::run 完成后调）。
    pub fn emit_command_result(&self, command_id: String, success: bool, message: String) {
        let _ = self.event_tx.send(PluginEvent::CommandResult {
            command_id,
            success,
            message,
        });
    }
}

/// PluginHost 错误。
#[derive(Debug, thiserror::Error)]
pub enum PluginHostError {
    #[error("加载失败: {0}")]
    Load(String),
    #[error("卸载失败: {0}")]
    Uninstall(String),
    #[error("proxy 错误: {0}")]
    Proxy(#[from] crate::proxy::ProxyError),
    #[error("WASM 调用错误: {0}")]
    WasmCall(#[from] WasmCallError),
}

/// 递归复制目录。
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// 跨平台创建目录 symlink（dev 模式，§8.5）。
#[cfg(unix)]
fn symlink_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(windows)]
fn symlink_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(src, dst)
}

#[cfg(not(any(unix, windows)))]
fn symlink_dir(_src: &Path, _dst: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "symlink 不支持此平台"))
}

