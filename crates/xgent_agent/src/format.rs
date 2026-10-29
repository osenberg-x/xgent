//! chat 请求构造：把消息历史与上下文结果组装为 [`ChatRequest`]。

use xgent_context::provider::ContextResult;
use xgent_core::chat::{AgentMessage, ChatMessage, ChatRequest, Role, ToolSchema, convert_to_llm};

/// agent system 提示词模板。
///
/// 在编译期由 `include_str!` 内联 `prompts/system.md`；运行时再拼接项目上下文
/// （目录树摘要 + 相关上下文片段）注入为 system message。
const SYSTEM_PROMPT: &str = include_str!("prompts/system.md");

/// 构造 system 消息文本：角色模板 + 项目上下文（目录树 + 相关片段）。
///
/// 抽出来供 bridge 异步侧在 `context.retrieve()` 完成后调用，
/// 覆盖 ECS 侧构造的占位 system 消息（ECS 系统同步，无法 await retrieve）。
pub fn build_system_text(context: &ContextResult) -> String {
    let mut system = String::from(SYSTEM_PROMPT);
    if let Some(tree) = &context.tree_summary {
        system.push_str("\n\n## 项目结构\n");
        system.push_str(tree);
    }
    if !context.chunks.is_empty() {
        system.push_str("\n\n## 相关上下文\n");
        for chunk in &context.chunks {
            system.push_str(&format!(
                "### {}（{}）\n```\n{}\n```\n",
                chunk.path.display(),
                chunk.relevance,
                chunk.content
            ));
        }
    }
    system
}

/// 构造对话请求。
///
/// 注入顺序：system（角色 + 上下文）→ 历史消息（经 convert_to_llm 转换）。
/// Conversation 持有 AgentMessage[]，调用 LLM 前过滤 UI-only 类型（见 ADR-0005）。
///
/// 系统提示词来自 `prompts/system.md`（include_str! 编译期内联），项目上下文
/// （目录树与相关片段）在运行时通过 format! 拼接到该提示词之后。
///
/// **注意**：ECS 系统同步，无法 await `context.retrieve()`，故 ECS 侧构造 req 时
/// 传 `&ContextResult::default()`（无上下文）；bridge 异步侧接到 `StartLoop` 后
/// 调 [`refresh_system_message`] 用真实检索结果覆盖首条 system 消息。
pub fn build_request(
    messages: &[AgentMessage],
    context: &ContextResult,
    provider: &str,
    model: &str,
    tools: Option<Vec<ToolSchema>>,
) -> ChatRequest {
    let mut all = convert_to_llm(messages);
    let system = build_system_text(context);
    all.insert(0, ChatMessage::text(Role::System, system));

    ChatRequest {
        provider: provider.to_string(),
        model: model.to_string(),
        messages: all,
        tools,
    }
}

/// 把用户显式保留的上下文项并入检索结果。
///
/// 两条规则：
/// 1. **用户显式项优先**——同 path 的检索片段（检索侧已截断）让位给完整项；
/// 2. 合并后仍受 `max_tokens` 约束——显式项是整份文件，不受限会让请求超出
///    上下文窗口被 provider 直接拒掉。超出剩余预算的部分按 1 token ≈ 4 字符裁剪。
pub fn merge_pinned_context(
    result: &mut ContextResult,
    pinned: &[xgent_context::provider::ContextChunk],
    max_tokens: u32,
) {
    for chunk in pinned {
        let existing = result
            .chunks
            .iter()
            .position(|item| item.path == chunk.path);
        let used: u32 = result.chunks.iter().map(|item| item.token_estimate).sum();
        // 被替换掉的检索片段腾出的预算可以复用。
        let freed = existing
            .map(|idx| result.chunks[idx].token_estimate)
            .unwrap_or(0);
        let room = max_tokens.saturating_sub(used.saturating_sub(freed));
        let kept = truncate_chunk_to_tokens(chunk, room);
        match existing {
            Some(idx) => result.chunks[idx] = kept,
            None => result.chunks.push(kept),
        }
    }
    result.total_tokens = result.chunks.iter().map(|chunk| chunk.token_estimate).sum();
}

/// 把上下文片段裁剪到给定 token 预算（与检索侧同一 1 token ≈ 4 字符口径）。
fn truncate_chunk_to_tokens(
    chunk: &xgent_context::provider::ContextChunk,
    max_tokens: u32,
) -> xgent_context::provider::ContextChunk {
    let mut kept = chunk.clone();
    kept.token_estimate = chunk.token_estimate.min(max_tokens);
    if max_tokens == 0 {
        kept.content = String::new();
        return kept;
    }
    let limit = (max_tokens as usize).saturating_mul(4);
    if chunk.content.chars().count() > limit {
        let head: String = chunk.content.chars().take(limit).collect();
        kept.content = format!("{head}\n…（按上下文预算截断）");
    }
    kept
}

/// 用最新上下文检索结果刷新 req 的首条 system 消息。
///
/// bridge 异步侧在 `StartLoop` 接到 req 后、调 `run_agent_loop` 前调用：
/// 1. 用用户最近一条消息构造 `ContextQuery`；
/// 2. `context.retrieve(query)` 得到 `ContextResult`；
/// 3. 用 [`build_system_text`] 生成新 system 文本，覆盖 `req.messages[0]`。
///
/// 若 `req.messages[0]` 不是 System 角色（不应发生），则直接 insert 到最前。
pub fn refresh_system_message(req: &mut ChatRequest, context: &ContextResult) {
    let system_text = build_system_text(context);
    let new_system = ChatMessage::text(Role::System, system_text);
    if req
        .messages
        .first()
        .map(|m| m.role == Role::System)
        .unwrap_or(false)
    {
        req.messages[0] = new_system;
    } else {
        req.messages.insert(0, new_system);
    }
}

/// 从消息历史中提取最后一条 user 文本，供上下文检索构造 query。
///
/// 返回 `None` 表示无 user 消息（不应发生的调用路径）。
pub fn last_user_text(messages: &[ChatMessage]) -> Option<String> {
    use xgent_core::chat::ContentBlock;
    messages
        .iter()
        .rev()
        .find(|m| m.role == Role::User)
        .and_then(|m| {
            let text: String = m
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            if text.is_empty() { None } else { Some(text) }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use xgent_context::provider::ContextChunk;
    use xgent_core::chat::{AgentMessage, ContentBlock, UserMessage};

    /// 从 content blocks 提取所有 Text 块拼接为字符串（测试辅助）。
    fn text_of(content: &[ContentBlock]) -> String {
        content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// 构造纯文本 UserMessage（测试辅助）。
    fn user_msg(text: &str) -> AgentMessage {
        AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text { text: text.into() }],
            timestamp: 0,
        })
    }

    #[test]
    fn build_request_includes_system_and_history() {
        let msgs = vec![user_msg("hi")];
        let ctx = ContextResult::default();
        let req = build_request(&msgs, &ctx, "openai", "gpt-4", None);
        assert_eq!(req.provider, "openai");
        assert_eq!(req.model, "gpt-4");
        assert_eq!(req.messages.len(), 2); // system + user
        assert_eq!(req.messages[0].role, Role::System);
        let sys_text = text_of(&req.messages[0].content);
        // 系统提示词来自模板（include_str! 加载 system.md），含 XGent 角色
        assert!(sys_text.contains("XGent"));
        assert!(sys_text.contains("工具使用规则"));
        assert!(sys_text.contains("工作流程"));
        assert!(sys_text.contains("交付契约"));
        assert_eq!(text_of(&req.messages[1].content), "hi");
        assert!(req.tools.is_none());
    }

    #[test]
    fn build_request_injects_context() {
        let ctx = ContextResult {
            chunks: vec![ContextChunk {
                path: PathBuf::from("src/main.rs"),
                content: "fn main(){}".into(),
                relevance: "用户问题相关".into(),
                token_estimate: 3,
            }],
            tree_summary: Some("src/\n  main.rs\n".into()),
            total_tokens: 10,
        };
        let req = build_request(&[], &ctx, "p", "m", None);
        let sys_text = text_of(&req.messages[0].content);
        assert!(sys_text.contains("项目结构"));
        assert!(sys_text.contains("src/main.rs"));
        assert!(sys_text.contains("相关上下文"));
        assert!(sys_text.contains("fn main(){}"));
    }

    #[test]
    fn build_request_filters_notification() {
        let msgs = vec![
            user_msg("hi"),
            AgentMessage::Notification(xgent_core::chat::NotificationMessage {
                text: "UI-only".into(),
                timestamp: 0,
            }),
        ];
        let ctx = ContextResult::default();
        let req = build_request(&msgs, &ctx, "p", "m", None);
        // Notification 被过滤，只剩 system + user
        assert_eq!(req.messages.len(), 2);
    }

    fn chunk(path: &str, content: &str, tokens: u32) -> xgent_context::provider::ContextChunk {
        xgent_context::provider::ContextChunk {
            path: std::path::PathBuf::from(path),
            content: content.into(),
            relevance: "retrieval".into(),
            token_estimate: tokens,
        }
    }

    #[test]
    fn pinned_context_replaces_retrieved_chunk_by_path() {
        let mut result = ContextResult {
            chunks: vec![chunk("a.rs", "retrieved", 10)],
            tree_summary: None,
            total_tokens: 10,
        };
        let pinned = vec![
            xgent_context::provider::ContextChunk {
                relevance: "user".into(),
                ..chunk("a.rs", "pinned", 20)
            },
            xgent_context::provider::ContextChunk {
                relevance: "user".into(),
                ..chunk("b.rs", "pinned-b", 30)
            },
        ];
        merge_pinned_context(&mut result, &pinned, 1_000);
        assert_eq!(
            result.chunks.len(),
            2,
            "同路径不得出现两份：{:?}",
            result.chunks
        );
        assert_eq!(
            result.chunks[0].content, "pinned",
            "用户显式项优先于检索片段（检索侧可能已截断）"
        );
        assert!(
            result
                .chunks
                .iter()
                .any(|c| c.path == std::path::Path::new("b.rs")),
            "显式项要真的进入请求"
        );
        assert_eq!(result.total_tokens, 50);
    }

    #[test]
    fn pinned_context_is_clamped_to_remaining_budget() {
        // 检索已用满预算时，显式项不得把整份文件塞进请求（provider 会直接拒）。
        let mut result = ContextResult {
            chunks: vec![chunk("a.rs", &"x".repeat(400), 100)],
            tree_summary: None,
            total_tokens: 100,
        };
        let pinned = vec![chunk("big.rs", &"y".repeat(4000), 1000)];
        merge_pinned_context(&mut result, &pinned, 100);
        assert_eq!(result.chunks[1].token_estimate, 0, "无剩余预算时不占用额度");
        assert!(result.chunks[1].content.is_empty());
        assert_eq!(result.total_tokens, 100, "合并后总量不得超过 max_tokens");
    }

    #[test]
    fn pinned_context_truncates_to_leftover_budget() {
        let mut result = ContextResult {
            chunks: vec![chunk("a.rs", &"x".repeat(40), 10)],
            tree_summary: None,
            total_tokens: 10,
        };
        let pinned = vec![chunk("b.rs", &"y".repeat(400), 100)];
        merge_pinned_context(&mut result, &pinned, 30);
        let kept = &result.chunks[1];
        assert_eq!(kept.token_estimate, 20, "只保留剩余预算");
        assert!(
            kept.content.starts_with(&"y".repeat(80)),
            "预算内的内容原样保留：{}",
            kept.content
        );
        assert!(
            kept.content.contains("按上下文预算截断"),
            "截断必须留痕，不能让模型以为拿到了完整文件：{}",
            kept.content
        );
        assert_eq!(result.total_tokens, 30);
    }
}
