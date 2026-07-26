//! PluginHost — 插件存储：管理安装/卸载/索引/重载。
//!
//! 照设计文档 §7.3 + §8（生命周期）+ §13 Step P4。对标 Zed `ExtensionStore`。
//!
//! 核心方法：`reload()` / `install_extension` / `uninstall_extension` /
//! `install_dev_extension` / `extensions_updated` / `load_builtin_plugins`。
//!
//! **启动时序硬性**（§13 Step P4）：`xgent_app` 在各业务 Plugin init 后显式调
//! `load_builtin_plugins()`，此时 proxy 已就绪，register_* 生效。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::index::{PluginIndex, PluginIndexEntry};
use crate::manifest::PluginManifest;
use crate::proxy::PluginHostProxy;
use crate::wasm_host::{WasmCallError, WasmHost, WasmPlugin};

/// 插件宿主事件（发到 ECS，由 PluginPollSystem 消费转 Message）。
#[derive(Debug, Clone)]
pub enum PluginEvent {
    /// 命令执行完成（command.run 返回）。
    CommandResult { command_id: String, success: bool, message: String },
    /// 插件卸载完成（通知 ECS 清理 ToolExecutor/CommandRegistry/ContextHub）。
    Unregister { plugin_id: String },
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
    /// 索引（内存缓存，启动时加载）。
    index: Mutex<PluginIndex>,
    /// 事件 channel：发到 ECS（PluginPollSystem drain）。
    event_tx: mpsc::UnboundedSender<PluginEvent>,
    /// 内建插件资源目录（随二进制发布的预装 wasm）。
    assets_dir: Option<PathBuf>,
    /// 插件级配置子表（plugin_id → toml::Value，来自 GlobalConfig.plugin_settings）。
    /// `host.get_config("<id>.<field>")` 从此取值（§10.4）。
    plugin_settings: Mutex<std::collections::BTreeMap<String, toml::Value>>,
}

impl PluginHost {
    /// 构造 PluginHost。`event_rx` 由调用方持有，在 ECS 轮询系统 drain。
    pub fn new(
        proxy: Arc<PluginHostProxy>,
        wasm_host: Arc<WasmHost>,
        plugins_dir: PathBuf,
        assets_dir: Option<PathBuf>,
        plugin_settings: std::collections::BTreeMap<String, toml::Value>,
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<PluginEvent>) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let installed_dir = plugins_dir.join("installed");
        let index_path = plugins_dir.join("index.json");
        // 加载索引
        let index = std::fs::read(&index_path)
            .ok()
            .and_then(|b| PluginIndex::from_json(&b).ok())
            .unwrap_or_default();
        let host = Arc::new(Self {
            proxy,
            wasm_host,
            installed_dir,
            index_path,
            loaded: Mutex::new(Vec::new()),
            pending_drop: Mutex::new(Vec::new()),
            index: Mutex::new(index),
            event_tx,
            assets_dir,
            plugin_settings: Mutex::new(plugin_settings),
        });
        (host, event_rx)
    }

    /// 更新插件配置（配置文件变更时调，§10.4）。
    ///
    /// 注意：仅更新内存缓存，已加载插件的 `PluginState.config` 不会热更新——
    /// 需重新加载插件（uninstall + reload）才能生效。MVP 接受此限制。
    pub fn update_plugin_settings(
        &self,
        plugin_settings: std::collections::BTreeMap<String, toml::Value>,
    ) {
        *self.plugin_settings.lock() = plugin_settings;
    }

    /// 启动文件监听（生产模式，§8.5）。
    ///
    /// notify 监听 `installed_dir`，`.wasm` 文件 `close-write` 事件经 200ms debounce
    /// 后调 `reload_all`。dev 模式不监听（显式 `reload` 命令触发）。
    ///
    /// 由 `xgent_app` 在构造 PluginHost 后调，传入 tokio runtime handle。
    pub fn start_file_watcher(self: &Arc<Self>, handle: tokio::runtime::Handle) {
        let installed_dir = self.installed_dir.clone();
        let host = self.clone();
        // notify watcher 是 blocking，放独立线程；事件经 channel 发到 tokio task
        let (tx, mut rx) = mpsc::unbounded_channel::<()>();
        std::thread::spawn(move || {
            use notify::{EventKind, RecursiveMode, Watcher};
            use notify::event::{AccessKind, AccessMode};
            let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(ev) = res {
                    // 仅 .wasm 文件的写后关闭事件（避免部分写入时加载损坏 WASM）
                    let is_wasm_close = matches!(
                        ev.kind,
                        EventKind::Access(AccessKind::Close(AccessMode::Write))
                    ) && ev.paths.iter().any(|p| p.extension().is_some_and(|e| e == "wasm"));
                    if is_wasm_close {
                        let _ = tx.send(());
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
            // 保持 watcher 活着：线程阻塞在此
            std::thread::park();
        });
        // tokio task：debounce 200ms 后 reload
        handle.spawn(async move {
            let mut last = std::time::Instant::now();
            let mut pending = false;
            loop {
                match tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await {
                    Ok(Some(())) => {
                        pending = true;
                        last = std::time::Instant::now();
                    }
                    Ok(None) => break, // sender drop
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
        let mut entries = Vec::new();
        if self.installed_dir.exists() {
            for entry in std::fs::read_dir(&self.installed_dir).into_iter().flatten() {
                if let Ok(e) = entry {
                    let dir = e.path();
                    if dir.is_dir() {
                        entries.push(dir);
                    }
                }
            }
        }
        // 收集新清单
        let mut new_index = PluginIndex::default();
        for dir in &entries {
            let manifest_path = dir.join("plugin.toml");
            let toml_str = match std::fs::read_to_string(&manifest_path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let manifest = match PluginManifest::from_toml(&toml_str) {
                Ok(m) => Arc::new(m),
                Err(e) => {
                    tracing::warn!(dir = %dir.display(), error = %e, "清单解析失败，跳过");
                    continue;
                }
            };
            new_index.plugins.insert(
                manifest.id.as_str().into(),
                PluginIndexEntry {
                    manifest: (*manifest).clone(),
                    dev: false,
                    enabled: true,
                },
            );
        }
        // diff：找出新增/移除
        let old_ids: std::collections::HashSet<String> = {
            self.loaded
                .lock()
                .iter()
                .map(|(m, _)| m.id.clone())
                .collect()
        };
        let new_ids: std::collections::HashSet<String> = new_index
            .plugins
            .keys()
            .map(|k| k.to_string())
            .collect();
        // 卸载移除的
        for id in old_ids.difference(&new_ids) {
            self.unregister(id).await;
        }
        // 加载新增的
        for (id_key, entry) in &new_index.plugins {
            if old_ids.contains(id_key.as_str()) {
                continue;
            }
            if let Err(e) = self.load_plugin(Arc::new(entry.manifest.clone())).await {
                tracing::warn!(plugin = %id_key, error = %e, "加载插件失败");
            }
        }
        // 写索引
        if let Some(parent) = self.index_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(bytes) = new_index.to_json() {
            let _ = std::fs::write(&self.index_path, bytes);
        }
        Ok(())
    }

    /// 加载单个插件：读 wasm + instantiate + register 工具/命令/provider。
    async fn load_plugin(
        &self,
        manifest: Arc<PluginManifest>,
    ) -> Result<(), PluginHostError> {
        let plugin_dir = self.installed_dir.join(&manifest.id);
        let wasm_path = plugin_dir.join("extension.wasm");
        let wasm_bytes = std::fs::read(&wasm_path)
            .map_err(|e| PluginHostError::Load(format!("读 wasm 失败: {e}")))?;
        // 配置子表：从 plugin_settings 取该插件的 [plugin.<id>] 段（§10.4），
        // 缺失则空 table。
        let config = self
            .plugin_settings
            .lock()
            .get(&manifest.id)
            .cloned()
            .unwrap_or_else(|| toml::Value::Table(Default::default()));
        let work_dir = plugin_dir.join("work");
        let _ = std::fs::create_dir_all(&work_dir);
        let wasm_plugin = self
            .wasm_host
            .load(&wasm_bytes, manifest.clone(), config, work_dir)
            .await
            .map_err(|e| PluginHostError::Load(e.to_string()))?;

        // register 工具
        let tool_defs = wasm_plugin.call_tool_register().await.unwrap_or_default();
        if !tool_defs.is_empty() {
            if let Err(e) = self
                .proxy
                .tool()
                .and_then(|p| p.register_tools(manifest.clone(), wasm_plugin.clone(), tool_defs))
            {
                tracing::warn!(plugin = %manifest.id, error = %PluginHostError::from(e), "注册工具失败");
            }
        }
        // register 命令
        let cmd_defs = wasm_plugin.call_command_register().await.unwrap_or_default();
        if !cmd_defs.is_empty() {
            if let Err(e) = self
                .proxy
                .command()
                .and_then(|p| p.register_commands(manifest.clone(), wasm_plugin.clone(), cmd_defs))
            {
                tracing::warn!(plugin = %manifest.id, error = %PluginHostError::from(e), "注册命令失败");
            }
        }
        // register context providers
        let provider_defs = wasm_plugin.call_context_provider_register().await.unwrap_or_default();
        if !provider_defs.is_empty() {
            if let Err(e) = self.proxy.context().and_then(|p| {
                p.register_providers(
                    manifest.clone(),
                    wasm_plugin.clone(),
                    provider_defs,
                    self.wasm_host.project_root().to_path_buf(),
                )
            }) {
                tracing::warn!(plugin = %manifest.id, error = %PluginHostError::from(e), "注册 ContextProvider 失败");
            }
        }

        self.loaded.lock().push((manifest, wasm_plugin));
        Ok(())
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
                // 60s 超时强制 drop（此时若有残留调用会 panic，属异常路径）
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
    /// 60s 超时强制 drop：超时后即使 in_flight>0 也移除（残留调用会 panic，
    /// 属异常路径——理论上 host.run-command cancel 已保证 in-flight 在 1s 内归零）。
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

// 静默 BTreeMap 未用警告（索引类型预留）
#[allow(dead_code)]
fn _silence(_: BTreeMap<String, String>) {}
