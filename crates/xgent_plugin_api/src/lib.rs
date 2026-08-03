//! xgent_plugin_api — 插件作者面向的 API。
//!
//! 提供 [`Extension`] trait、WIT 绑定与 [`register_plugin!`] 宏。
//! 插件 crate 以 `crate-type = ["cdylib"]` 编译为 WASM Component，
//! 入口导出函数 `init-extension` 由宿主加载后调用初始化插件实例。
//!
//! 详见 `doc/design/plugin-system-design.md` §5.2 / §13 Step P1。
//! 对标 `zed_extension_api`（Zed `crates/extension_api/src/extension_api.rs`）。

#![allow(clippy::too_many_arguments, clippy::missing_safety_doc)]

// 生成 guest 侧绑定。`skip: ["init-extension"]` 让本 crate 自行定义该导出
// （register_plugin! 宏展开为 #[export_name = "init-extension"]），对齐 Zed
// extension_api.rs:197。
//
// 注意：generate! 必须在 crate 根（不在 mod wit 内），否则 export! 宏
// 无法在 crate 根解析（wit-bindgen 0.22 约束）。
wit_bindgen::generate!({
    path: "wit",
    skip: ["init-extension"],
});

// API 版本段：宿主加载时扫 custom section `xgent:api-version` 校验兼容性
// （见 xgent_plugin::wasm_host::validate_api_version）。MVP 固定 0.1.0。
// 6 字节 big-endian：major(2) + minor(2) + patch(2) = 0.1.0。
// 对齐 Zed `zed:api-version`（Zed 用 build.rs 动态生成，MVP 简化为静态）。
// 仅 wasm target 生成 custom section（native target 的 Mach-O 不接受带冒号 section 名）。
#[cfg(target_arch = "wasm32")]
#[unsafe(link_section = "xgent:api-version")]
#[used]
static XGENT_API_VERSION: [u8; 6] = [0, 0, 0, 1, 0, 0];
pub use exports::xgent::plugin::command::CommandDef as WitCommandDef;
pub use exports::xgent::plugin::context_provider::{
    ContextChunk as WitContextChunk, ContextQuery as WitContextQuery,
    ContextResult as WitContextResult, ProviderDef as WitProviderDef,
};
pub use exports::xgent::plugin::tool::{ToolDef as WitToolDef, ToolTier as WitToolTier};
pub use xgent::plugin::host;
pub use xgent::plugin::host::{
    CommandError as WitCommandError, CommandOutput as WitCommandOutput,
    CommandReq as WitCommandReq, LogLevel as WitLogLevel,
};

use std::sync::OnceLock;

/// 插件扩展点定义集合：工具 / 命令 / ContextProvider。
///
/// 插件在 [`Extension::register_*`] 中返回这些定义，宿主据此注册到
/// `ToolExecutor` / `CommandRegistry` / `ContextHub`。
/// 对标 Zed `Extension` trait（其余方法全默认 impl）。
pub trait Extension: Send + Sync {
    /// 构造插件实例。必需——宿主加载后调 `init-extension` 经此构造实例。
    fn new() -> Self
    where
        Self: Sized;

    /// 注册工具定义（默认空）。
    fn register_tools(&mut self) -> Vec<WitToolDef> {
        Vec::new()
    }

    /// 注册命令面板命令定义（默认空）。
    fn register_commands(&mut self) -> Vec<WitCommandDef> {
        Vec::new()
    }

    /// 注册 ContextProvider 定义（默认空）。
    fn register_context_providers(&mut self) -> Vec<WitProviderDef> {
        Vec::new()
    }

    /// 执行工具（默认返回 failed，插件 override 提供实现）。
    ///
    /// `tool_id` 是插件内短 id；`input` 是 JSON 序列化的输入参数。
    /// 返回 JSON 序列化的 `ToolResult` 字符串，或 `WitToolError`。
    fn execute(&mut self, tool_id: &str, input: &str) -> Result<String, WitToolError> {
        let _ = (tool_id, input);
        Err(WitToolError::Failed("tool execute not implemented".into()))
    }

    /// 执行命令（默认返回 Err，插件 override）。
    fn run_command(&mut self, command_id: &str) -> Result<String, String> {
        let _ = command_id;
        Err("command run not implemented".into())
    }

    /// 检索上下文（默认返回 Err，插件 override）。
    fn retrieve(
        &mut self,
        provider_id: &str,
        query: &WitContextQuery,
    ) -> Result<WitContextResult, String> {
        let _ = (provider_id, query);
        Err("context retrieve not implemented".into())
    }

    /// 通知文件变更（默认空实现，插件可 override 增量更新）。
    fn on_file_changed(&mut self, provider_id: &str, path: Option<&str>) {
        let _ = (provider_id, path);
    }
}
static EXTENSION: OnceLock<std::sync::Mutex<Box<dyn Extension>>> = OnceLock::new();

/// 注册插件构造器：把构造闭包存入全局，`init-extension` 导出函数调用它构造实例。
///
/// 对齐 Zed extension_api.rs:192-206。
pub fn register_extension(f: impl FnOnce() -> Box<dyn Extension> + 'static) {
    let _ = EXTENSION.set(std::sync::Mutex::new(f()));
}

/// 取全局插件实例的可变引用闭包（WIT export 实现内调）。
fn with_extension<R>(f: impl FnOnce(&mut dyn Extension) -> R) -> R {
    let ext = EXTENSION
        .get()
        .expect("init-extension 未调用即触发 WIT export");
    let mut guard = ext.lock().expect("插件实例锁中毒");
    f(guard.as_mut())
}

/// 导出插件入口。在插件 crate 根调用 `register_plugin!(MyExt);` 即可。
///
/// 展开为 `#[export_name = "init-extension"] pub extern "C" fn __init_extension()`，
/// 构造实例并存入全局存储。对齐 Zed `register_extension!`（extension_api.rs:166）。
#[macro_export]
macro_rules! register_plugin {
    ($t:ty) => {
        /// 插件入口导出函数（由 register_plugin! 宏生成）。
        ///
        /// 宿主加载 WASM 后调用此函数构造插件实例。
        #[unsafe(export_name = "init-extension")]
        pub extern "C" fn __init_extension() {
            $crate::register_extension(|| Box::new(<$t>::new()));
        }
    };
}

// ===== WIT export 实现：转发到全局 Extension 实例 =====
//
// wit_bindgen 为 world `plugin` 的三个 export interface 生成 Guest trait：
//   exports::xgent::plugin::tool::Guest
//   exports::xgent::plugin::command::Guest
//   exports::xgent::plugin::context_provider::Guest
// `export!(Component)` 把它们接到 `Component` 上。我们在此桥接到
// Extension trait 的对应方法。

use exports::xgent::plugin::context_provider::Guest as ContextProviderGuest;
pub use exports::xgent::plugin::tool::ToolError as WitToolError;
use exports::xgent::plugin::{command::Guest as CommandGuest, tool::Guest as ToolGuest};

struct Component;

impl ToolGuest for Component {
    fn register() -> Vec<WitToolDef> {
        with_extension(|e| e.register_tools())
    }

    fn execute(tool_id: String, input: String) -> Result<String, WitToolError> {
        with_extension(|e| e.execute(&tool_id, &input))
    }
}

impl CommandGuest for Component {
    fn register() -> Vec<WitCommandDef> {
        with_extension(|e| e.register_commands())
    }

    fn run(command_id: String) -> Result<String, String> {
        with_extension(|e| e.run_command(&command_id))
    }
}

impl ContextProviderGuest for Component {
    fn register() -> Vec<WitProviderDef> {
        with_extension(|e| e.register_context_providers())
    }

    fn retrieve(provider_id: String, query: WitContextQuery) -> Result<WitContextResult, String> {
        with_extension(|e| e.retrieve(&provider_id, &query))
    }

    fn on_file_changed(provider_id: String, path: Option<String>) {
        with_extension(|e| e.on_file_changed(&provider_id, path.as_deref()));
    }
}

export!(Component);
