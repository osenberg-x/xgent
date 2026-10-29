//! Transcript 有序树模型：会话内容的权威数据源。
//!
//! M6-T1 建立模型；M6.5-T1 补齐流式增量、工具状态、终稿标记与恢复。
//! 渲染系统按块序列顺序 spawn/update/despawn 实体，不手动挪子节点。
//! 块只追加、不重排；`block_index` 取追加时的序列下标。

use bevy::prelude::*;

/// 会话 Transcript：持有有序的块序列。
#[derive(Resource, Default, Debug, Clone)]
pub struct Transcript {
    /// 块序列，按 `(turn_index, block_index)` 排序。
    pub blocks: Vec<BlockRef>,
    /// 下一个 turn 索引（用户消息递增）。
    next_turn_index: u64,
    /// 代次：clear/restore 时递增，渲染层据此丢弃旧实体。
    generation: u64,
}

impl Transcript {
    /// 追加用户消息。
    pub fn push_user_message(&mut self, content: String) -> (u64, usize) {
        let turn_index = self.next_turn_index;
        self.next_turn_index += 1;
        let block_index = self.blocks.len();
        self.blocks.push(BlockRef::UserMessage {
            turn_index,
            block_index,
            content,
        });
        (turn_index, block_index)
    }

    /// 开始助手回复分段（创建空的 AssistantMessage 块）。
    ///
    /// 同一轮出现新的助手分段（例如工具调用后的续写）时，之前的分段视为已终稿。
    pub fn begin_assistant_message(&mut self, turn_index: u64) -> usize {
        for block in &mut self.blocks {
            if let BlockRef::AssistantMessage {
                turn_index: t,
                finalized,
                ..
            } = block
                && *t == turn_index
            {
                *finalized = true;
            }
        }
        let block_index = self.blocks.len();
        self.blocks.push(BlockRef::AssistantMessage {
            turn_index,
            block_index,
            blocks: Vec::new(),
            finalized: false,
        });
        block_index
    }

    /// 追加内容块到指定助手消息。
    pub fn append_to_assistant(&mut self, block_index: usize, content: ContentBlock) {
        if let Some(BlockRef::AssistantMessage { blocks, .. }) = self.blocks.get_mut(block_index) {
            blocks.push(content);
        }
    }

    /// 把流式增量追加到该轮最后一个助手消息的文本块。
    ///
    /// 该轮还没有助手消息时自动创建；最后一个内容块不是文本时新建文本块。
    pub fn append_text_delta(&mut self, turn_index: u64, text: &str) {
        let last_assistant = self.blocks.iter().rposition(
            |b| matches!(b, BlockRef::AssistantMessage { turn_index: t, .. } if *t == turn_index),
        );
        let last_tool = self.blocks.iter().rposition(|b| {
            b.turn_index() == turn_index
                && matches!(b, BlockRef::ToolCall { .. } | BlockRef::ToolResult { .. })
        });
        let idx = match last_assistant {
            Some(idx) if last_tool.is_none_or(|tool| idx > tool) => idx,
            _ => self.begin_assistant_message(turn_index),
        };
        if let Some(BlockRef::AssistantMessage { blocks, .. }) = self.blocks.get_mut(idx) {
            match blocks.last_mut() {
                Some(ContentBlock::Text(current)) => current.push_str(text),
                _ => blocks.push(ContentBlock::Text(text.to_string())),
            }
        }
    }

    /// 标记该轮最后一个助手消息为终稿（Markdown 渲染依据）。
    pub fn finalize_assistant(&mut self, turn_index: u64) {
        if let Some(BlockRef::AssistantMessage { finalized, .. }) = self
            .blocks
            .iter_mut()
            .rev()
            .find(|b| matches!(b, BlockRef::AssistantMessage { turn_index: t, .. } if *t == turn_index))
        {
            *finalized = true;
        }
    }

    /// 追加工具调用。
    pub fn push_tool_call(
        &mut self,
        turn_index: u64,
        tool_call_id: String,
        tool_name: String,
        arguments: String,
    ) -> (u64, usize) {
        let block_index = self.blocks.len();
        self.blocks.push(BlockRef::ToolCall {
            turn_index,
            block_index,
            tool_call_id,
            tool_name,
            arguments,
            status: ToolCallStatus::Pending,
            progress: None,
        });
        (turn_index, block_index)
    }

    /// 更新工具调用状态；未知 id 不改动。
    pub fn set_tool_status(&mut self, tool_call_id: &str, status: ToolCallStatus) -> bool {
        for block in &mut self.blocks {
            if let BlockRef::ToolCall {
                tool_call_id: id,
                status: current,
                ..
            } = block
                && id == tool_call_id
            {
                *current = status;
                return true;
            }
        }
        false
    }

    /// 更新工具执行中的进度文本；未知 id 不改动。
    pub fn set_tool_progress(&mut self, tool_call_id: &str, text: &str) -> bool {
        for block in &mut self.blocks {
            if let BlockRef::ToolCall {
                tool_call_id: id,
                progress,
                status,
                ..
            } = block
                && id == tool_call_id
            {
                *progress = Some(text.to_string());
                *status = ToolCallStatus::Running;
                return true;
            }
        }
        false
    }

    /// 该工具调用是否已有对应的 `ToolCall` 块。
    pub fn has_tool_call(&self, tool_call_id: &str) -> bool {
        self.blocks.iter().any(|block| {
            matches!(block, BlockRef::ToolCall { tool_call_id: id, .. } if id == tool_call_id)
        })
    }

    /// 追加或更新工具结果，并把对应工具调用标记为完成/失败/拒绝。
    ///
    /// 没有对应 `ToolCall` 时返回 `None` 且不改动序列（调用方按 Spec 记 warn）。
    pub fn set_tool_result(
        &mut self,
        turn_index: u64,
        tool_call_id: String,
        content: String,
        is_error: bool,
        denied: bool,
    ) -> Option<usize> {
        if !self.has_tool_call(&tool_call_id) {
            return None;
        }
        let status = if denied || is_error {
            ToolCallStatus::Failed
        } else {
            ToolCallStatus::Completed
        };
        if let Some(idx) = self.blocks.iter().position(
            |b| matches!(b, BlockRef::ToolResult { tool_call_id: id, .. } if id == &tool_call_id),
        ) {
            if let BlockRef::ToolResult {
                content: current,
                is_error: current_err,
                denied: current_denied,
                ..
            } = &mut self.blocks[idx]
            {
                *current = content;
                *current_err = is_error;
                *current_denied = denied;
            }
            self.set_tool_status(&tool_call_id, status);
            return Some(idx);
        }
        let block_index = self.blocks.len();
        self.blocks.push(BlockRef::ToolResult {
            turn_index,
            block_index,
            tool_call_id: tool_call_id.clone(),
            content,
            is_error,
            denied,
        });
        self.set_tool_status(&tool_call_id, status);
        Some(block_index)
    }

    /// 追加压缩通知。
    pub fn push_compression_notice(
        &mut self,
        turn_index: u64,
        tokens_before: u32,
        tokens_after: u32,
    ) -> (u64, usize) {
        let block_index = self.blocks.len();
        self.blocks.push(BlockRef::CompressionNotice {
            turn_index,
            block_index,
            tokens_before,
            tokens_after,
        });
        (turn_index, block_index)
    }

    /// 追加通用通知（undo 结果、错误、重试状态等）。
    pub fn push_notice(&mut self, turn_index: u64, text: String, kind: NoticeKind) -> (u64, usize) {
        let block_index = self.blocks.len();
        self.blocks.push(BlockRef::Notice {
            turn_index,
            block_index,
            text,
            kind,
        });
        (turn_index, block_index)
    }

    /// 当前轮次（最近一条用户消息的轮次）。
    pub fn current_turn(&self) -> Option<u64> {
        if self.next_turn_index == 0 {
            None
        } else {
            Some(self.next_turn_index - 1)
        }
    }

    /// 获取块序列（只读）。
    pub fn blocks(&self) -> &[BlockRef] {
        &self.blocks
    }

    /// 当前代次。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 整体替换（会话恢复）。
    pub fn restore(&mut self, blocks: Vec<BlockRef>) {
        let max_turn = blocks
            .iter()
            .map(|b| b.turn_index())
            .max()
            .map(|t| t + 1)
            .unwrap_or(0);
        self.blocks = blocks;
        self.next_turn_index = max_turn;
        self.generation = self.generation.wrapping_add(1);
    }

    /// 清空 transcript。
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.next_turn_index = 0;
        self.generation = self.generation.wrapping_add(1);
    }
}

/// 块引用：Transcript 中的一个元素。
#[derive(Debug, Clone, PartialEq)]
pub enum BlockRef {
    /// 用户消息。
    UserMessage {
        turn_index: u64,
        block_index: usize,
        content: String,
    },
    /// 助手消息（包含多个内容块）。
    AssistantMessage {
        turn_index: u64,
        block_index: usize,
        blocks: Vec<ContentBlock>,
        /// 是否已终稿（Markdown 渲染依据）。
        finalized: bool,
    },
    /// 工具调用。
    ToolCall {
        turn_index: u64,
        block_index: usize,
        tool_call_id: String,
        tool_name: String,
        arguments: String,
        status: ToolCallStatus,
        /// 执行中的中间输出。
        progress: Option<String>,
    },
    /// 工具结果。
    ToolResult {
        turn_index: u64,
        block_index: usize,
        tool_call_id: String,
        content: String,
        is_error: bool,
        /// 用户拒绝执行（与「执行失败」区分，重建卡片时标签不能变）。
        denied: bool,
    },
    /// 压缩通知。
    CompressionNotice {
        turn_index: u64,
        block_index: usize,
        tokens_before: u32,
        tokens_after: u32,
    },
    /// 通用通知（undo 结果、错误、重试状态等）。
    Notice {
        turn_index: u64,
        block_index: usize,
        text: String,
        kind: NoticeKind,
    },
}

impl BlockRef {
    /// 所属轮次。
    pub fn turn_index(&self) -> u64 {
        match self {
            BlockRef::UserMessage { turn_index, .. }
            | BlockRef::AssistantMessage { turn_index, .. }
            | BlockRef::ToolCall { turn_index, .. }
            | BlockRef::ToolResult { turn_index, .. }
            | BlockRef::CompressionNotice { turn_index, .. }
            | BlockRef::Notice { turn_index, .. } => *turn_index,
        }
    }

    /// 获取顺序键 `(turn_index, block_index)`。
    pub fn order_key(&self) -> (u64, usize) {
        match self {
            BlockRef::UserMessage {
                turn_index,
                block_index,
                ..
            }
            | BlockRef::AssistantMessage {
                turn_index,
                block_index,
                ..
            }
            | BlockRef::ToolCall {
                turn_index,
                block_index,
                ..
            }
            | BlockRef::ToolResult {
                turn_index,
                block_index,
                ..
            }
            | BlockRef::CompressionNotice {
                turn_index,
                block_index,
                ..
            }
            | BlockRef::Notice {
                turn_index,
                block_index,
                ..
            } => (*turn_index, *block_index),
        }
    }
}

/// 内容块：助手消息内的元素。
#[derive(Debug, Clone, PartialEq)]
pub enum ContentBlock {
    /// 文本段落（可能是 Markdown）。
    Text(String),
    /// 代码块。
    Code { language: String, code: String },
}

/// 通知类型（决定渲染色调）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoticeKind {
    /// 普通信息。
    #[default]
    Info,
    /// 错误。
    Error,
    /// 重试状态。
    Retry,
}

/// 工具调用状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolCallStatus {
    /// 待执行。
    #[default]
    Pending,
    /// 执行中。
    Running,
    /// 已完成。
    Completed,
    /// 失败。
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_order_is_monotonic() {
        let mut t = Transcript::default();
        let (t0, _) = t.push_user_message("hello".into());
        let b0 = t.begin_assistant_message(t0);
        t.append_to_assistant(b0, ContentBlock::Text("response".into()));
        let (t1, _) = t.push_tool_call(t0, "tc1".into(), "read_file".into(), "{}".into());
        t.set_tool_result(t0, "tc1".into(), "content".into(), false, false);

        let keys: Vec<_> = t.blocks().iter().map(|b| b.order_key()).collect();
        for i in 1..keys.len() {
            assert!(keys[i] > keys[i - 1], "order must be strictly increasing");
        }
        assert_eq!(t0, t1);
    }

    #[test]
    fn transcript_clear_resets_state() {
        let mut t = Transcript::default();
        t.push_user_message("hello".into());
        t.clear();
        assert_eq!(t.blocks().len(), 0);
        assert_eq!(t.next_turn_index, 0);
    }

    #[test]
    fn append_delta_creates_and_extends_text() {
        let mut t = Transcript::default();
        let (turn, _) = t.push_user_message("hi".into());
        t.append_text_delta(turn, "Hel");
        t.append_text_delta(turn, "lo");
        match &t.blocks()[1] {
            BlockRef::AssistantMessage { blocks, .. } => {
                assert_eq!(blocks, &vec![ContentBlock::Text("Hello".into())]);
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn tool_status_and_result_pair_by_call_id() {
        let mut t = Transcript::default();
        let (turn, _) = t.push_user_message("hi".into());
        t.push_tool_call(turn, "tc1".into(), "read_file".into(), "{}".into());
        assert!(t.set_tool_progress("tc1", "reading"));
        assert!(t.set_tool_status("tc1", ToolCallStatus::Running));
        t.set_tool_result(turn, "tc1".into(), "ok".into(), false, false);
        t.set_tool_result(turn, "tc1".into(), "ok2".into(), true, false);
        let results: Vec<_> = t
            .blocks()
            .iter()
            .filter_map(|b| match b {
                BlockRef::ToolResult {
                    content, is_error, ..
                } => Some((content.clone(), *is_error)),
                _ => None,
            })
            .collect();
        assert_eq!(results, vec![("ok2".to_string(), true)]);
        let status = t
            .blocks()
            .iter()
            .find_map(|b| match b {
                BlockRef::ToolCall { status, .. } => Some(*status),
                _ => None,
            })
            .unwrap();
        assert_eq!(status, ToolCallStatus::Failed);
    }

    #[test]
    fn orphan_tool_result_is_rejected() {
        let mut t = Transcript::default();
        let (turn, _) = t.push_user_message("hi".into());
        // 没有对应 ToolCall 的结果：不建块、不改顺序（调用方按 Spec 记 warn）。
        assert_eq!(
            t.set_tool_result(turn, "ghost".into(), "out".into(), false, false),
            None
        );
        assert_eq!(t.blocks().len(), 1);
    }

    #[test]
    fn denied_result_keeps_denied_flag_and_fails_status() {
        let mut t = Transcript::default();
        let (turn, _) = t.push_user_message("hi".into());
        t.push_tool_call(turn, "tc1".into(), "write_file".into(), "{}".into());
        assert!(
            t.set_tool_result(turn, "tc1".into(), "用户拒绝".into(), false, true)
                .is_some()
        );
        let status = t
            .blocks()
            .iter()
            .find_map(|b| match b {
                BlockRef::ToolCall { status, .. } => Some(*status),
                _ => None,
            })
            .unwrap();
        assert_eq!(status, ToolCallStatus::Failed);
        let denied = t.blocks().iter().find_map(|b| match b {
            BlockRef::ToolResult {
                denied, is_error, ..
            } => Some((*denied, *is_error)),
            _ => None,
        });
        assert_eq!(denied, Some((true, false)), "拒绝与失败必须可区分");
    }

    #[test]
    fn restore_rebuilds_turn_counter() {
        let mut t = Transcript::default();
        let (turn, _) = t.push_user_message("old".into());
        t.begin_assistant_message(turn);
        t.restore(vec![BlockRef::UserMessage {
            turn_index: 7,
            block_index: 0,
            content: "restored".into(),
        }]);
        assert_eq!(t.current_turn(), Some(7));
        let (next, _) = t.push_user_message("new".into());
        assert_eq!(next, 8);
    }
}
