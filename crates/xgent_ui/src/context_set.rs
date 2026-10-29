//! ContextSet 建模：会话上下文的权威数据源。
//!
//! M6-T6：独立 Resource，可增删、可持久化、显示 token 占比。
//! 替换 `context_scope.rs` 的只读镜像。

use bevy::prelude::*;
use std::collections::HashSet;

/// 上下文集合：持有当前会话的上下文项。
#[derive(Resource, Default, Debug, Clone)]
pub struct ContextSet {
    /// 上下文项列表。
    items: Vec<ContextItem>,
    /// 总 token 数（用于显示占比）。
    total_tokens: u64,
    /// 用户显式移除的项（tab 同步不再自动加回）。
    removed: HashSet<String>,
}

impl ContextSet {
    /// 添加上下文项。
    pub fn add(&mut self, item: ContextItem) {
        self.removed.remove(&item.id);
        self.total_tokens += item.token_count;
        self.items.push(item);
    }

    /// 移除上下文项（按 id）。
    pub fn remove(&mut self, id: &str) -> bool {
        if let Some(pos) = self.items.iter().position(|i| i.id == id) {
            let item = self.items.remove(pos);
            self.removed.insert(item.id);
            self.total_tokens = self.total_tokens.saturating_sub(item.token_count);
            true
        } else {
            false
        }
    }

    /// 获取所有上下文项（只读）。
    pub fn items(&self) -> &[ContextItem] {
        &self.items
    }

    /// 获取总 token 数。
    pub fn total_tokens(&self) -> u64 {
        self.total_tokens
    }

    /// 清空上下文。
    pub fn clear(&mut self) {
        self.items.clear();
        self.removed.clear();
        self.total_tokens = 0;
    }

    /// 该项是否被用户显式移除（tab 同步跳过）。
    pub fn is_removed(&self, id: &str) -> bool {
        self.removed.contains(id)
    }

    /// 标记为已移除（会话恢复时回放 sidecar 用）。
    pub fn mark_removed(&mut self, id: &str) {
        self.removed.insert(id.to_string());
    }

    /// 已移除项 id 列表。
    pub fn removed_ids(&self) -> Vec<String> {
        self.removed.iter().cloned().collect()
    }

    /// 检查是否包含某 id。
    pub fn contains(&self, id: &str) -> bool {
        self.items.iter().any(|i| i.id == id)
    }
}

/// 上下文项：一个上下文来源。
#[derive(Debug, Clone, PartialEq)]
pub struct ContextItem {
    /// 唯一标识。
    pub id: String,
    /// 类型（文件、符号、selection 等）。
    pub kind: ContextKind,
    /// 显示名称。
    pub label: String,
    /// token 数。
    pub token_count: u64,
    /// 内容（用于发送给 LLM）。
    pub content: String,
}

/// 上下文类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextKind {
    /// 文件。
    File,
    /// 符号（函数、类等）。
    Symbol,
    /// 选区。
    Selection,
    /// 其他。
    Other,
}

impl ContextKind {
    /// 持久化字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            ContextKind::File => "file",
            ContextKind::Symbol => "symbol",
            ContextKind::Selection => "selection",
            ContextKind::Other => "other",
        }
    }

    /// 从持久化字符串还原。
    pub fn from_persisted(value: &str) -> Self {
        match value {
            "symbol" => ContextKind::Symbol,
            "selection" => ContextKind::Selection,
            "file" => ContextKind::File,
            _ => ContextKind::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_set_add_remove() {
        let mut set = ContextSet::default();
        set.add(ContextItem {
            id: "file1".into(),
            kind: ContextKind::File,
            label: "test.rs".into(),
            token_count: 100,
            content: "fn main() {}".into(),
        });
        assert_eq!(set.items().len(), 1);
        assert_eq!(set.total_tokens(), 100);
        assert!(set.contains("file1"));

        assert!(set.remove("file1"));
        assert_eq!(set.items().len(), 0);
        assert_eq!(set.total_tokens(), 0);
        assert!(!set.contains("file1"));
    }

    #[test]
    fn removed_item_is_not_readded_by_tab_sync() {
        let mut set = ContextSet::default();
        let item = ContextItem {
            id: "src/main.rs".into(),
            kind: ContextKind::File,
            label: "main.rs".into(),
            token_count: 10,
            content: "fn main() {}".into(),
        };
        set.add(item.clone());
        assert!(set.remove("src/main.rs"));
        assert!(set.is_removed("src/main.rs"));
        assert!(!set.contains("src/main.rs"));
        // 显式加入才会清除 removed 标记
        set.add(item);
        assert!(!set.is_removed("src/main.rs"));
        assert!(set.contains("src/main.rs"));
    }

    #[test]
    fn context_set_clear() {
        let mut set = ContextSet::default();
        set.add(ContextItem {
            id: "file1".into(),
            kind: ContextKind::File,
            label: "test.rs".into(),
            token_count: 100,
            content: "fn main() {}".into(),
        });
        set.add(ContextItem {
            id: "file2".into(),
            kind: ContextKind::File,
            label: "other.rs".into(),
            token_count: 50,
            content: "fn other() {}".into(),
        });
        assert_eq!(set.total_tokens(), 150);

        set.clear();
        assert_eq!(set.items().len(), 0);
        assert_eq!(set.total_tokens(), 0);
    }
}
