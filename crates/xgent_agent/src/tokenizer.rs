//! 对话 token 估算（compaction 触发依据）。
//!
//! 借鉴 omp `estimateTokens`：compaction 只需粗略估算（threshold 带 15% reserve
//! 缓冲，误差容忍）。不引入 tiktoken 重依赖，改用启发式：
//!
//! - 文本：`max(chars, bytes/4)`——英文约 4 char/token，中文 UTF-8 3 字节/字
//!   约等效 1.5 char/token，取两者最大值偏保守（宁可早压缩）。
//! - ToolCall args：按 JSON 序列化字节 / 4 估算。
//! - ToolResult content：同文本。
//! - Image：固定 1200 token（对齐 omp `IMAGE_TOKEN_ESTIMATE`）。
//!
//! 单条消息额外 +4 token overhead（role/分隔符，对齐 tiktoken 习惯）。

use xgent_core::chat::{AgentMessage, ContentBlock};

/// 图片内容固定 token 估算（对齐 omp）。
const IMAGE_TOKEN_ESTIMATE: u32 = 1200;

/// 单条消息 overhead（role 标记与分隔符）。
const MESSAGE_OVERHEAD: u32 = 4;

/// 估算单条消息的 token 数。
pub fn estimate_message_tokens(msg: &AgentMessage) -> u32 {
    let body = match msg {
        AgentMessage::User(m) => sum_blocks(&m.content),
        AgentMessage::Assistant(m) => sum_blocks(&m.content),
        AgentMessage::ToolResult(m) => estimate_text(&m.content),
        AgentMessage::Notification(m) => estimate_text(&m.text),
    };
    body.saturating_add(MESSAGE_OVERHEAD)
}

/// 估算消息序列总 token。
pub fn estimate_messages_tokens(msgs: &[AgentMessage]) -> u32 {
    msgs.iter().map(estimate_message_tokens).sum()
}

/// 累加 content blocks 的 token。
fn sum_blocks(blocks: &[ContentBlock]) -> u32 {
    blocks.iter().map(estimate_block).sum()
}

/// 估算单个 content block 的 token。
fn estimate_block(block: &ContentBlock) -> u32 {
    match block {
        ContentBlock::Text { text } => estimate_text(text),
        ContentBlock::ToolCall { args, .. } => {
            // args JSON 序列化后按字节估算
            let bytes = serde_json::to_string(args)
                .map(|s| s.len())
                .unwrap_or(0);
            (bytes as u32) / 4
        }
        ContentBlock::ToolResult { content, .. } => estimate_text(content),
        ContentBlock::Image { .. } => IMAGE_TOKEN_ESTIMATE,
    }
}

/// 文本 token 估算：`max(chars, bytes/4)`，对中英文都偏保守。
fn estimate_text(text: &str) -> u32 {
    let chars = text.chars().count() as u32;
    let bytes = text.len() as u32 / 4;
    chars.max(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xgent_core::chat::{AssistantMessage, ContentBlock, UserMessage};

    fn user_text(text: &str) -> AgentMessage {
        AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text { text: text.into() }],
            timestamp: 0,
        })
    }

    #[test]
    fn english_text_approx_4_chars_per_token() {
        // "hello world" = 11 chars → max(11, 11/4=2) = 11 + 4 overhead = 15
        let m = user_text("hello world");
        assert_eq!(estimate_message_tokens(&m), 15);
    }

    #[test]
    fn chinese_text_uses_char_count() {
        // 12 个中文字 = 12 chars, 36 bytes → max(12, 9) = 12 + 4 = 16
        let m = user_text("你好世界你好世界你好世界");
        assert_eq!(estimate_message_tokens(&m), 16);
    }

    #[test]
    fn empty_text_only_overhead() {
        let m = user_text("");
        assert_eq!(estimate_message_tokens(&m), MESSAGE_OVERHEAD);
    }

    #[test]
    fn image_fixed_estimate() {
        let m = AgentMessage::Assistant(AssistantMessage {
            content: vec![ContentBlock::Image {
                data: "base64".into(),
                mime_type: "image/png".into(),
            }],
            model: None,
            usage: None,
            timestamp: 0,
        });
        // image 1200 + overhead 4
        assert_eq!(estimate_message_tokens(&m), 1204);
    }

    #[test]
    fn sum_over_messages() {
        let msgs = vec![user_text("hi"), user_text("there")];
        // (max(2,0)+4) + (max(5,1)+4) = 6 + 9 = 15
        assert_eq!(estimate_messages_tokens(&msgs), 15);
    }

    #[test]
    fn toolcall_args_estimated_by_json_bytes() {
        let m = AgentMessage::Assistant(AssistantMessage {
            content: vec![ContentBlock::ToolCall {
                id: "x".into(),
                name: "read".into(),
                args: serde_json::json!({"path": "/a/b/c.rs"}),
            }],
            model: None,
            usage: None,
            timestamp: 0,
        });
        // args JSON ~21 bytes → 5 tokens; name/id 不计（保守，只算 args）
        let tokens = estimate_message_tokens(&m);
        assert!(tokens >= 5 + MESSAGE_OVERHEAD);
    }
}
