//! 插件索引（`index.json` 缓存）。
//!
//! 照设计文档 §7.4。索引文件缓存已安装插件清单，启动时同步加载，按需异步重建。
//!
//! 注：`Arc<PluginManifest>` 不实现 Serialize/Deserialize，故索引用 owned
//! `PluginManifest`（运行时 `Arc::new` 包装）。key 用 `String`（serde 友好）。

use serde::{Deserialize, Serialize};

use crate::manifest::PluginManifest;

/// 插件索引：`plugin_id → 条目`。
#[derive(Default, Serialize, Deserialize)]
pub struct PluginIndex {
    pub plugins: std::collections::BTreeMap<String, PluginIndexEntry>,
}

/// 索引条目。
#[derive(Serialize, Deserialize, Clone)]
pub struct PluginIndexEntry {
    pub manifest: PluginManifest,
    pub dev: bool,
    pub enabled: bool,
}

impl PluginIndex {
    /// 从 JSON 字节反序列化。
    pub fn from_json(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        if bytes.is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_slice(bytes)
    }

    /// 序列化为 JSON 字节。
    pub fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec_pretty(self)
    }
}
