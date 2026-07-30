//! PluginTool — 插件工具适配器：把 WIT tool 调用桥接为 `Tool` trait。
//!
//! 照设计文档 §5.3 全文。完整 id 拼接 `plugin.<plugin_id>.<short_id>`，
//! `new()` 覆盖 `schema.name` 兑现 id↔schema.name 一致性硬约束。
//! `execute` 调 `WasmPlugin::call_tool_execute`，cancel 穿透。
//! `side_effect` 填 `None`（偏差修正 8：插件工具副作用经 host.run-command 走，不回传）。

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use xgent_core::chat::ToolSchema;
use xgent_tools::tool::{
    Concurrency, Tool, ToolCtx, ToolError, ToolResult, ToolTier, ToolUpdateCallback,
};
use xgent_plugin::{WasmCallError, WasmPlugin};

/// 插件工具适配器。
///
/// **id ↔ schema.name 一致性硬约束**（阻断级）：`Tool::id()` 与 `ToolSchema.name`
/// 必须返回相同的完整 id。`new()` 显式覆盖 `schema.name`，不信任插件作者在
/// schema JSON 里写的 name 字段。
pub struct PluginTool {
    /// 完整 id：plugin.<plugin_id>.<short_id>
    full_id: Arc<str>,
    /// 插件内短 id，传给 WIT execute
    short_id: Arc<str>,
    /// 工具描述
    description: String,
    /// JSON Schema（input_schema）
    input_schema: serde_json::Value,
    /// 工具分层
    tier: ToolTier,
    /// 插件 WASM 实例
    plugin: Arc<WasmPlugin>,
}

impl PluginTool {
    /// 构造时拼接完整 id 并解析 schema。
    ///
    /// 照设计文档 §5.3：反序列化后覆盖 `name` 为完整 id（硬约束）。
    pub fn new(
        plugin_id: &str,
        tool_def: &xgent_plugin::WitToolDef,
        plugin: Arc<WasmPlugin>,
    ) -> Result<Self, String> {
        let full_id: Arc<str> = format!("plugin.{}.{}", plugin_id, tool_def.id).into();
        // tool_def.schema 是 JSON Schema 字符串。ToolSchema 的 input_schema 是
        // 该 JSON Schema（描述工具输入参数）；name/description 由我们填充。
        let input_schema: serde_json::Value = match serde_json::from_str(&tool_def.schema) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(plugin = %plugin_id, tool = %tool_def.id, error = %e, "插件工具 schema 解析失败，降级为空 object");
                serde_json::json!({"type":"object"})
            }
        };
        let tier = match tool_def.tier {
            xgent_plugin::WitToolTier::Read => ToolTier::Read,
            xgent_plugin::WitToolTier::Write => ToolTier::Write,
            xgent_plugin::WitToolTier::Exec => ToolTier::Exec,
        };
        Ok(Self {
            full_id,
            short_id: tool_def.id.clone().into(),
            description: tool_def.description.clone(),
            input_schema,
            tier,
            plugin,
        })
    }

    /// 构造 `ToolSchema`（name 为完整 id，兑现硬约束）。
    fn build_schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.full_id.to_string(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
        }
    }
}

#[async_trait]
impl Tool for PluginTool {
    fn id(&self) -> &str {
        &self.full_id
    }

    fn schema(&self) -> ToolSchema {
        self.build_schema()
    }

    fn tier(&self) -> ToolTier {
        self.tier
    }

    /// 按 tier 推导并发：Read→Shared，Write/Exec→Exclusive。
    /// 必须显式实现——trait 默认恒 Shared，与 §5.3 承诺不符。
    fn concurrency(&self) -> Concurrency {
        match self.tier {
            ToolTier::Read => Concurrency::Shared,
            ToolTier::Write | ToolTier::Exec => Concurrency::Exclusive,
            ToolTier::UiOnly => Concurrency::Shared, // 不会命中（tier 无 UiOnly）
        }
    }

    /// 生成工具输入的人类可读摘要（§5.3）。
    ///
    /// 设计 §5.3 要求经 WIT `tool.summarize` 调用插件，但 WIT 方法是 async，
    /// 而 `Tool::summarize` 是同步 trait 方法（被 `ToolExecutor::execute` 在
    /// 确认流程中同步调用）。MVP 保留本地默认摘要；WIT `tool.summarize` 接口
    /// 已声明（供未来 summarize 改 async 后接通）。
    fn summarize(&self, input: &Value) -> String {
        format!("{}({})", self.short_id, input)
    }

    /// approval_for / preview_diff：MVP 裁决（设计 §5.3 289 行）暂不接 WIT，
    /// 回退 trait 默认（approval_for = tier()，preview_diff = None）。
    /// 确认弹窗对插件工具退化为纯文本 summary（与内建只读工具一致）。

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolCtx,
        signal: CancellationToken,
        on_update: Option<&ToolUpdateCallback>,
    ) -> Result<ToolResult, ToolError> {
        let input_json = serde_json::to_string(&input).unwrap_or_default();
        // on_update 桥接受限：ToolUpdateCallback 是 Box<dyn Fn>（非 'static），
        // call_tool_execute 要求 Arc<dyn Fn + 'static>，且 ToolExecutor MVP
        // 总传 None（executor.rs:126/155）。push-update 基础设施（WIT/HostState/
        // call_tool_execute 参数）已就绪，待 ToolUpdateCallback 改 Arc 后接通。
        let _ = on_update;
        let update_cb: Option<std::sync::Arc<dyn Fn(String) + Send + Sync>> = None;
        match self
            .plugin
            .call_tool_execute(&self.short_id, &input_json, signal, update_cb)
            .await
        {
            Ok(s) => {
                // 反序列化为 ToolResult；失败则构造默认（偏差修正 8：side_effect=None）
                let result: ToolResult = match serde_json::from_str(&s) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(tool = %self.short_id, error = %e, "插件工具 execute 返回非法 ToolResult JSON，降级为原始字符串");
                        ToolResult {
                            output: s.clone(),
                            is_error: false,
                            denied: false,
                            side_effect: None,
                        }
                    }
                };
                Ok(result)
            }
            Err(e) => match e {
                WasmCallError::Aborted => Err(ToolError::Aborted),
                WasmCallError::Failed(msg) => Err(ToolError::Failed(msg)),
            },
        }
    }
}

