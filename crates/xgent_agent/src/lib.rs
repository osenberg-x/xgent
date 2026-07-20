//! xgent_agent — Agent 核心引擎，组合 provider/tools/context 并接入 Bevy ECS。
//!
//! 通过 tokio channel 桥接异步逻辑到 Bevy 系统；对话状态作为 Resource；
//! 与 UI 仅通过 Events 通信（禁止直接方法调用）。

pub mod agent_loop;
pub mod bridge;
pub mod compaction;
pub mod conversation;
pub mod events;
pub mod format;
pub mod provider_state;
pub mod session_store;
pub mod tokenizer;

#[cfg(test)]
mod bridge_tests;

pub use agent_loop::agent_poll_system;
pub use bridge::{AgentBridge, AgentBridgeConfig, AgentCommand, AgentEvent, ProviderClient};
pub use compaction::{
    CompactionError, CompactionProvider, CompactionResult, CompactionSettings, LlmCompactor,
    apply_compaction, compaction_context_tokens, effective_reserve_tokens, find_cut_point,
    resolve_threshold_tokens, should_compact,
};
pub use conversation::{Conversation, ConversationStatus};
pub use events::*;
pub use format::build_request;
pub use provider_state::{ContextState, ProviderInfo};
pub use session_store::{SessionStore, session_file_path};

use bevy::prelude::*;

/// XGent Agent 插件。
///
/// 注册事件、对话状态、provider/context 状态与轮询系统。
/// `AgentBridge` 由 [`xgent_app`]（或测试）注入，本插件不创建它。
pub struct XgentAgentPlugin;

impl Plugin for XgentAgentPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<UserInputMessage>()
            .add_message::<AbortMessage>()
            .add_message::<DeltaMessage>()
            .add_message::<ToolCallMessage>()
            .add_message::<ToolResultMessage>()
            .add_message::<ConfirmRequestMessage>()
            .add_message::<ConfirmDecisionMessage>()
            .add_message::<DoneMessage>()
            .add_message::<ErrorMessage>()
            .add_message::<RetryMessage>()
            .add_message::<SteeringMessage>()
            .add_message::<FollowUpMessage>()
            .add_message::<CompactedMessage>()
            .add_message::<NewSessionMessage>()
            .add_message::<SessionClearedMessage>()
            .init_resource::<Conversation>()
            .init_resource::<ProviderInfo>()
            .init_resource::<ContextState>()
            .add_systems(Update, agent_poll_system);
    }
}
