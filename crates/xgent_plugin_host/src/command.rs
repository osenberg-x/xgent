//! PluginCommand — 插件命令适配器 + 执行。
//!
//! 照设计文档 §5.4。`run` 调 `WasmPlugin::call_command_run`，
//! 返回 `Result<String, String>`，经 `PluginHost::emit_command_result`
//! 发 `CommandResultMessage`。

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use xgent_plugin::{WasmCallError, WasmPlugin};

/// 插件命令适配器。
#[derive(Clone)]
pub struct PluginCommand {
    pub full_id: String,
    /// 插件内短 id
    pub short_id: String,
    /// 已本地化标签
    pub label: String,
    /// 插件 WASM 实例
    pub plugin: Arc<WasmPlugin>,
}

impl PluginCommand {
    /// 执行命令，返回 `(success, message)`。
    ///
    /// `success=true` 时 `message` 为成功消息；`success=false` 时为错误文本。
    pub async fn run(&self) -> (bool, String) {
        let result = self
            .plugin
            .call_command_run(&self.short_id, CancellationToken::new())
            .await;
        match result {
            Ok(msg) => (true, msg),
            Err(WasmCallError::Aborted) => (false, "命令被中断".into()),
            Err(WasmCallError::Failed(e)) => (false, e),
        }
    }
}
