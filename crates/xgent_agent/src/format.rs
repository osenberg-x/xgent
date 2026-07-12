//! chat 请求构造：把消息历史与上下文结果组装为 [`ChatRequest`]。

use xgent_context::provider::ContextResult;
use xgent_core::chat::{ChatMessage, ChatRequest, Role, ToolSchema};

/// agent system 提示词。
const SYSTEM_PROMPT: &str = "你是 XGent，一个面向个人开发者的 AI 代码助手。\
你通过工具读写文件、运行命令来协助编码。请优先使用工具完成实际操作，\
给出简洁准确的说明。仅在你确定安全时建议执行命令。";

/// 构造对话请求。
///
/// 注入顺序：system（角色 + 上下文）→ 历史消息。
pub fn build_request(
    messages: &[ChatMessage],
    context: &ContextResult,
    provider: &str,
    model: &str,
    tools: Option<Vec<ToolSchema>>,
) -> ChatRequest {
    let mut all = Vec::new();

    // system message：角色 + 上下文
    let mut system = String::from(SYSTEM_PROMPT);
    if let Some(tree) = &context.tree_summary {
        system.push_str("\n\n## 项目结构\n```\n");
        system.push_str(tree);
        system.push_str("\n```");
    }
    if !context.chunks.is_empty() {
        system.push_str("\n\n## 相关上下文\n");
        for chunk in &context.chunks {
            system.push_str(&format!(
                "\n### {}\n（{}）\n```\n{}\n```",
                chunk.path.display(),
                chunk.relevance,
                chunk.content
            ));
        }
    }
    all.push(ChatMessage {
        role: Role::System,
        content: system,
    });

    // 历史消息
    all.extend_from_slice(messages);

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

    #[test]
    fn build_request_includes_system_and_history() {
        let msgs = vec![ChatMessage {
            role: Role::User,
            content: "hi".into(),
        }];
        let ctx = ContextResult::default();
        let req = build_request(&msgs, &ctx, "openai", "gpt-4", None);
        assert_eq!(req.provider, "openai");
        assert_eq!(req.model, "gpt-4");
        assert_eq!(req.messages.len(), 2); // system + user
        assert_eq!(req.messages[0].role, Role::System);
        assert!(req.messages[0].content.contains("XGent"));
        assert_eq!(req.messages[1].content, "hi");
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
        let sys = &req.messages[0].content;
        assert!(sys.contains("项目结构"));
        assert!(sys.contains("src/main.rs"));
        assert!(sys.contains("相关上下文"));
        assert!(sys.contains("fn main(){}"));
    }
}
