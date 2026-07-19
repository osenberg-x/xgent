//! Undo/redo 栈。
//!
//! MVP 用文本快照栈：每次文本变化压栈，undo 弹出到 redo 栈，redo 弹回 undo。
//! 栈容量有界（默认 100），溢出丢弃最老快照。
//!
//! 简单可靠：实现 O(1) push/undo/redo，内存 O(历史 × 文本大小)。
//! 大文件场景可升级为操作级 diff（设计文档 5.3 节）。

use super::buffer::TextSnapshot;

/// undo/redo 栈。
#[derive(Debug, Clone)]
pub struct UndoStack {
    undo: Vec<TextSnapshot>,
    redo: Vec<TextSnapshot>,
    /// 最大历史长度
    max: usize,
}

impl Default for UndoStack {
    /// 默认容量 100（与 [`Self::new`] 一致）。
    fn default() -> Self {
        Self::new()
    }
}

impl UndoStack {
    /// 默认容量 100。
    pub const DEFAULT_MAX: usize = 100;

    /// 构造默认栈。
    pub fn new() -> Self {
        Self::with_max(Self::DEFAULT_MAX)
    }

    /// 构造指定容量栈。
    pub fn with_max(max: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            max,
        }
    }

    /// 压入快照。若与栈顶相同则跳过（避免重复）。
    pub fn push(&mut self, snap: TextSnapshot) {
        if self.undo.last().is_some_and(|top| *top == snap) {
            return;
        }
        // max == 0 视为"无历史"，不压栈（防御 with_max(0) 误用）
        if self.max == 0 {
            return;
        }
        if self.undo.len() >= self.max {
            self.undo.remove(0);
        }
        self.undo.push(snap);
        // 新编辑分支清空 redo
        self.redo.clear();
    }

    /// undo：弹出 undo 栈顶到 redo，返回弹出快照（用于恢复前一状态）。
    ///
    /// 注意：调用方在收到快照前应先把**当前**文本压入 redo——
    /// 但简化模型下，我们让 undo 把当前状态转移到 redo，再返回前一状态。
    /// 此处实现：undo 栈存历史状态；undo 取栈顶之上一个状态。
    ///
    /// 重写为更直观的模型：
    /// - `undo` 栈顶始终是"当前应恢复到的状态"
    /// - push 初始状态后，每次编辑 push 新状态
    /// - undo：若 undo 栈至少 2 个，把栈顶移到 redo，返回新栈顶
    pub fn undo(&mut self) -> Option<TextSnapshot> {
        if self.undo.len() < 2 {
            return None;
        }
        let top = self.undo.pop().unwrap();
        self.redo.push(top);
        Some(self.undo.last().cloned().unwrap())
    }

    /// redo：从 redo 栈弹回 undo，返回弹出快照（即应恢复到的状态）。
    pub fn redo(&mut self) -> Option<TextSnapshot> {
        let snap = self.redo.pop()?;
        self.undo.push(snap.clone());
        Some(snap)
    }

    /// undo 栈深度（历史状态数，含初始）。
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// redo 栈深度。
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// 是否可 undo。
    pub fn can_undo(&self) -> bool {
        self.undo.len() >= 2
    }

    /// 是否可 redo。
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(s: &str) -> TextSnapshot {
        TextSnapshot { text: s.to_string() }
    }

    #[test]
    fn push_grows_undo_clears_redo() {
        let mut s = UndoStack::new();
        s.push(snap("a"));
        s.push(snap("b"));
        assert_eq!(s.undo_depth(), 2);
        // push 后 redo 清空
        s.push(snap("c"));
        assert_eq!(s.redo_depth(), 0);
    }

    #[test]
    fn push_dedup_identical_top() {
        let mut s = UndoStack::new();
        s.push(snap("a"));
        s.push(snap("a"));
        assert_eq!(s.undo_depth(), 1);
    }

    #[test]
    fn default_has_max_100_not_zero() {
        // 回归：derive(Default) 曾让 max=0，导致 push 越界 panic
        let s = UndoStack::default();
        assert_eq!(s.max, 100);
        assert_eq!(s.undo_depth(), 0);
    }

    #[test]
    fn push_with_max_zero_does_not_panic() {
        // with_max(0) 不应 panic（防御误用）
        let mut s = UndoStack::with_max(0);
        s.push(snap("a"));
        s.push(snap("b"));
        assert_eq!(s.undo_depth(), 0);
        assert!(!s.can_undo());
    }

    #[test]
    fn default_push_many_under_capacity() {
        // 模拟 update_syntax_highlight 每帧 push 的场景，确认不越界
        let mut s = UndoStack::default();
        for i in 0..200 {
            s.push(snap(&format!("v{i}")));
        }
        assert_eq!(s.undo_depth(), 100); // 不超过 max
    }

    #[test]
    fn undo_returns_previous_and_moves_to_redo() {
        let mut s = UndoStack::new();
        s.push(snap("a"));
        s.push(snap("b"));
        s.push(snap("c"));
        let r = s.undo().unwrap();
        assert_eq!(r.text, "b");
        assert_eq!(s.undo_depth(), 2);
        assert_eq!(s.redo_depth(), 1);
    }

    #[test]
    fn undo_with_single_state_returns_none() {
        let mut s = UndoStack::new();
        s.push(snap("a"));
        assert!(s.undo().is_none());
    }

    #[test]
    fn redo_restores_state() {
        let mut s = UndoStack::new();
        s.push(snap("a"));
        s.push(snap("b"));
        s.push(snap("c"));
        let _ = s.undo(); // → "b"
        let r = s.redo().unwrap();
        assert_eq!(r.text, "c");
        assert_eq!(s.redo_depth(), 0);
        assert_eq!(s.undo_depth(), 3);
    }

    #[test]
    fn max_capacity_drops_oldest() {
        let mut s = UndoStack::with_max(3);
        s.push(snap("a"));
        s.push(snap("b"));
        s.push(snap("c"));
        s.push(snap("d"));
        assert_eq!(s.undo_depth(), 3);
        // 最老 "a" 被丢弃，栈顶是 "d"
        assert_eq!(s.undo.last().unwrap().text, "d");
    }

    #[test]
    fn can_undo_redo_flags() {
        let mut s = UndoStack::new();
        assert!(!s.can_undo());
        assert!(!s.can_redo());
        s.push(snap("a"));
        s.push(snap("b"));
        assert!(s.can_undo());
        assert!(!s.can_redo());
        s.undo();
        assert!(s.can_redo());
    }
}
