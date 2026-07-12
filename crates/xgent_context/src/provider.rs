//! 上下文提供者抽象 trait 与相关类型。
//!
//! 定义统一的检索接口，支持未来升级（方案 A→B→C→D→E）。
//! agent 侧通过 trait 调用，无感于具体策略实现。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 一次上下文检索请求。
#[derive(Debug, Clone)]
pub struct ContextQuery {
    /// 用户当前问题
    pub user_message: String,
    /// 当前打开文件（若有）
    pub current_file: Option<PathBuf>,
    /// 额外线索（文件名、符号名等）
    pub hints: Vec<String>,
    /// 上下文预算（token 数）
    pub max_tokens: u32,
}

/// 检索返回的上下文片段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextChunk {
    /// 文件路径（相对项目根）
    pub path: PathBuf,
    /// 文件内容或片段
    pub content: String,
    /// 相关性说明，给 LLM 理解
    pub relevance: String,
    /// token 估算
    pub token_estimate: u32,
}

/// 检索结果。
#[derive(Debug, Clone, Default)]
pub struct ContextResult {
    /// 上下文片段集合
    pub chunks: Vec<ContextChunk>,
    /// 目录树摘要（方案 A 用）
    pub tree_summary: Option<String>,
    /// 总 token 估算
    pub total_tokens: u32,
}

/// 上下文提供者抽象。
#[async_trait]
pub trait ContextProvider: Send + Sync {
    /// 检索上下文。
    async fn retrieve(&self, query: &ContextQuery) -> ContextResult;

    /// 通知文件变更（供索引类实现增量更新，方案 A 空实现）。
    async fn on_file_changed(&self, _path: &PathBuf) {}
}

/// 粗略估算字符串的 token 数。
///
/// 简单规则：1 token ≈ 4 字符（英文），中文按 1 字符 ≈ 1 token 折算混合。
/// MVP 用字符数 / 4 上取整的粗估，足以做预算控制。
pub fn estimate_tokens(s: &str) -> u32 {
    // 中文字符按 1 token，其余按 4 字符 1 token
    let cjk: usize = s
        .chars()
        .filter(|c| {
            (*c >= '\u{4E00}' && *c <= '\u{9FFF}')
                || (*c >= '\u{3040}' && *c <= '\u{30FF}')
                || (*c >= '\u{AC00}' && *c <= '\u{D7AF}')
        })
        .count();
    let other = s.chars().count() - cjk;
    ((cjk + other / 4) as u32).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_ascii() {
        assert_eq!(estimate_tokens("hello world!"), 3); // 12/4=3
        assert_eq!(estimate_tokens("hi"), 1); // 2/4 → max(1,0)=1
    }

    #[test]
    fn estimate_tokens_cjk() {
        // 5 个中文字符 → 5 token
        assert_eq!(estimate_tokens("你好世界啊"), 5);
        // 混合：3 中文 + 4 英文 = 3 + 1 = 4
        assert_eq!(estimate_tokens("你好世abcd"), 4);
    }
}
