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
}
