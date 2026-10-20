//! 错误类型。

use std::path::PathBuf;

use thiserror::Error;

/// `xgent_spatial` 的错误。
#[derive(Debug, Error)]
pub enum SpatialError {
    /// 读取文件失败。
    #[error("读取 {} 失败：{source}", path.display())]
    Io {
        /// 出错的路径。
        path: PathBuf,
        /// 底层 IO 错误。
        #[source]
        source: std::io::Error,
    },

    /// TOML 语法或结构错误。
    #[error("解析 {} 失败：{source}", path.as_ref().map_or_else(|| "<内存>".to_string(), |p| p.display().to_string()))]
    Toml {
        /// 来源路径；纯字符串解析时为 `None`。
        path: Option<PathBuf>,
        /// 底层 TOML 错误。
        #[source]
        source: Box<toml::de::Error>,
    },

    /// manifest 缺少 `[package].name`。
    #[error("{} 缺少 [package].name", path.as_ref().map_or_else(|| "<内存>".to_string(), |p| p.display().to_string()))]
    MissingPackage {
        /// 来源路径；纯字符串解析时为 `None`。
        path: Option<PathBuf>,
    },

    /// 工作区内 package 重名。
    #[error("工作区存在重复 package 名：{name}")]
    DuplicatePackage {
        /// 重复的名字。
        name: String,
    },

    /// 工作区结构无效（例如根目录不存在）。
    #[error("工作区结构无效：{reason}")]
    InvalidWorkspace {
        /// 具体原因。
        reason: String,
    },
}
