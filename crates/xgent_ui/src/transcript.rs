//! Transcript 有序树模型：会话内容的权威数据源。
//!
//! M6-T1：渲染系统按顺序键 `(turn_index, block_index)` spawn/despawn/update 实体，
//! 不手动挪子节点。流式插入工具卡时顺序稳定不倒退。

use bevy::prelude::*;

/// 会话 Transcript：持有有序的块序列。
#[derive(Resource, Default, Debug, Clone)]
pub struct Transcript {
    /// 块序列，按 `(turn_index, block_index)` 排序。
    pub blocks: Vec<BlockRef>,
    /// 下一个 turn 索引（用户消息递增）。
    next_turn_index: u64,
}

impl Transcript {
    /// 追加用户消息。
    pub fn push_user_message(&mut self, content: String) -> (u64, usize) {
        let turn_index = self.next_turn_index;
        self.next_turn_index += 1;
        let block_index = self.blocks.len();
        self.blocks.push(BlockRef::UserMessage {
            turn_index,
            content,
        });
        (turn_index, block_index)
    }

    /// 开始助手回复（创建空的 AssistantMessage 块）。
    pub fn begin_assistant_message(&mut self, turn_index: u64) -> usize {
        let block_index = self.blocks.len();
        self.blocks.push(BlockRef::AssistantMessage {
            turn_index,
            blocks: Vec::new(),
        });
        block_index
    }

    /// 追加内容块到当前助手消息。
    pub fn append_to_assistant(&mut self, block_index: usize, content: ContentBlock) {
        if let Some(BlockRef::AssistantMessage { blocks, .. }) = self.blocks.get_mut(block_index) {
            blocks.push(content);
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
        });
        (turn_index, block_index)
    }

    /// 追加工具结果。
    pub fn push_tool_result(
        &mut self,
        turn_index: u64,
        tool_call_id: String,
        content: String,
    ) -> (u64, usize) {
        let block_index = self.blocks.len();
        self.blocks.push(BlockRef::ToolResult {
            turn_index,
            block_index,
            tool_call_id,
            content,
        });
        (turn_index, block_index)
    }

    /// 追加压缩通知。
    pub fn push_compression_notice(&mut self, turn_index: u64, message: String) -> (u64, usize) {
        let block_index = self.blocks.len();
        self.blocks.push(BlockRef::CompressionNotice {
            turn_index,
            message,
        });
        (turn_index, block_index)
    }

    /// 获取块序列（只读）。
    pub fn blocks(&self) -> &[BlockRef] {
        &self.blocks
    }

    /// 清空 transcript。
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.next_turn_index = 0;
    }
}

/// 块引用：Transcript 中的一个元素。
#[derive(Debug, Clone, PartialEq)]
pub enum BlockRef {
    /// 用户消息。
    UserMessage {
        turn_index: u64,
        content: String,
    },
    /// 助手消息（包含多个内容块）。
    AssistantMessage {
        turn_index: u64,
        blocks: Vec<ContentBlock>,
    },
    /// 工具调用。
    ToolCall {
        turn_index: u64,
        block_index: usize,
        tool_call_id: String,
        tool_name: String,
        arguments: String,
        status: ToolCallStatus,
    },
    /// 工具结果。
    ToolResult {
        turn_index: u64,
        block_index: usize,
        tool_call_id: String,
        content: String,
    },
    /// 压缩通知。
    CompressionNotice {
        turn_index: u64,
        message: String,
    },
}

impl BlockRef {
    /// 获取顺序键 `(turn_index, block_index)`。
    pub fn order_key(&self) -> (u64, usize) {
        match self {
            BlockRef::UserMessage { turn_index, .. } => (*turn_index, 0),
            BlockRef::AssistantMessage { turn_index, .. } => (*turn_index, 0),
            BlockRef::ToolCall {
                turn_index,
                block_index,
                ..
            } => (*turn_index, *block_index),
            BlockRef::ToolResult {
                turn_index,
                block_index,
                ..
            } => (*turn_index, *block_index),
            BlockRef::CompressionNotice { turn_index, .. } => (*turn_index, 0),
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
        let (t2, _) = t.push_tool_result(t0, "tc1".into(), "content".into());

        let keys: Vec<_> = t.blocks().iter().map(|b| b.order_key()).collect();
        for i in 1..keys.len() {
            assert!(keys[i] >= keys[i - 1], "order must be monotonic");
        }
    }

    #[test]
    fn transcript_clear_resets_state() {
        let mut t = Transcript::default();
        t.push_user_message("hello".into());
        t.clear();
        assert_eq!(t.blocks().len(), 0);
        assert_eq!(t.next_turn_index, 0);
    }
}
