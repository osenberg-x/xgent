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
use xgent_plugin::{WasmCallError, WasmPlugin};
use xgent_tools::tool::{
    Concurrency, Tool, ToolCtx, ToolError, ToolResult, ToolTier, ToolUpdateCallback,
};

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
            ToolTier::Read | ToolTier::UiOnly => Concurrency::Shared,
            ToolTier::Write | ToolTier::Exec | ToolTier::Dangerous => Concurrency::Exclusive,
        }
    }

    /// 生成工具输入的人类可读摘要（§5.3）：经 WIT `tool.summarize` 调插件。
    ///
    /// `Tool::summarize` 已改为 async（见 `xgent_tools::tool::Tool`），故此处
    /// 可以 await WASM 调用——修复前因签名是同步而无法接 WIT，确认弹窗只能
    /// 展示 `tool_id(input)` 的本地兜底文本。
    async fn summarize(&self, input: &Value) -> String {
        let input_json = serde_json::to_string(input).unwrap_or_default();
        match self
            .plugin
            .call_tool_summarize(&self.short_id, &input_json)
            .await
        {
            Ok(s) if !s.trim().is_empty() => s,
            Ok(_) | Err(_) => format!("{}({})", self.short_id, input),
        }
    }

    /// `preview_diff` 经 WIT `tool.preview-diff` 调插件，拿到真实 diff。
    ///
    /// 修复前恒返回 `None`，确认弹窗对插件工具退化为纯文本 summary。
    async fn preview_diff(&self, input: &Value, _ctx: &ToolCtx) -> Option<(String, String)> {
        let input_json = serde_json::to_string(input).ok()?;
        let raw = self
            .plugin
            .call_tool_preview_diff(&self.short_id, &input_json)
            .await
            .ok()?;
        let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let old = v["old"].as_str()?.to_string();
        let new = v["new"].as_str()?.to_string();
        Some((old, new))
    }
    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolCtx,
        signal: CancellationToken,
        on_update: Option<Arc<ToolUpdateCallback>>,
    ) -> Result<ToolResult, ToolError> {
        let input_json = serde_json::to_string(&input).unwrap_or_default();
        // on_update 桥接：`ToolUpdateCallback` 已改为 `Arc`，与 WASM host 需要的
        // `Arc<dyn Fn(String) + Send + Sync>` 同形，故此处真正接通插件的
        // `push-update`（修复前恒传 None，流式中间结果被丢弃）。
        let update_cb: Option<std::sync::Arc<dyn Fn(String) + Send + Sync>> =
            on_update.map(|cb| -> std::sync::Arc<dyn Fn(String) + Send + Sync> {
                std::sync::Arc::new(move |s: String| {
                    cb(ToolResult {
                        output: s,
                        is_error: false,
                        denied: false,
                        side_effect: None,
                    })
                })
            });
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
