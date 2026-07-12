//! daemon → UI 的通知名常量。
//!
//! 集中管理避免字符串硬编码不一致。这些是 daemon 通过 JSON-RPC 通知
//! 单向推送给 UI 的事件名。

/// provider 流式对话增量文本（[`crate::chat::ChatEvent::Delta`]）
pub const PROVIDER_DELTA: &str = "provider.delta";
/// provider 工具调用（[`crate::chat::ChatEvent::ToolCall`]）
pub const PROVIDER_TOOL_CALL: &str = "provider.toolCall";
/// provider 流结束（[`crate::chat::ChatEvent::Done`]）
pub const PROVIDER_DONE: &str = "provider.done";
/// provider 出错（[`crate::chat::ChatEvent::Error`]）
pub const PROVIDER_ERROR: &str = "provider.error";
/// 文件变更（[`crate::fs::FileChanged`]）
pub const FS_CHANGED: &str = "fs.changed";
/// 配置变更（[`crate::config::ConfigChanged`]）
pub const CONFIG_CHANGED: &str = "config.changed";
/// 其他客户端修改了文件（多客户端同步）
pub const PEER_FILE_CHANGED: &str = "peer.fileChanged";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_are_nonempty_unique() {
        let all = [
            PROVIDER_DELTA,
            PROVIDER_TOOL_CALL,
            PROVIDER_DONE,
            PROVIDER_ERROR,
            FS_CHANGED,
            CONFIG_CHANGED,
            PEER_FILE_CHANGED,
        ];
        assert!(all.iter().all(|s| !s.is_empty()));
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(all[i], all[j], "duplicate notification name");
            }
        }
    }
}
