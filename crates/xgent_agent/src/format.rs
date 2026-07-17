//! chat 请求构造：把消息历史与上下文结果组装为 [`ChatRequest`]。

use xgent_context::provider::ContextResult;
use xgent_core::chat::{AgentMessage, ChatRequest, Role, ToolSchema, convert_to_llm};

/// agent system 提示词模板。
///
/// 在编译期由 `include_str!` 内联 `prompts/system.md`；运行时再拼接项目上下文
/// （目录树摘要 + 相关上下文片段）注入为 system message。
const SYSTEM_PROMPT: &str = include_str!("prompts/system.md");

/// 构造对话请求。
///
/// 注入顺序：system（角色 + 上下文）→ 历史消息（经 convert_to_llm 转换）。
/// Conversation 持有 AgentMessage[]，调用 LLM 前过滤 UI-only 类型（见 ADR-0005）。
///
/// 系统提示词来自 `prompts/system.md`（include_str! 编译期内联），项目上下文
/// （目录树与相关片段）在运行时通过 format! 拼接到该提示词之后。
pub fn build_request(
    messages: &[AgentMessage],
    context: &ContextResult,
    provider: &str,
    model: &str,
    tools: Option<Vec<ToolSchema>>,
) -> ChatRequest {
    let mut all = convert_to_llm(messages);

    // system message：角色模板 + 项目上下文（插到最前）
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
    all.insert(0, xgent_core::chat::ChatMessage::text(Role::System, system));

    ChatRequest {
        provider: provider.to_string(),
        model: model.to_string(),
        messages: all,
        tools,
    }
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
