//! daemon → UI 的通知名常量。
//!
//! 集中管理避免字符串硬编码不一致。这些是 daemon 通过 JSON-RPC 通知
//! 单向推送给 UI 的事件名。

/// provider 流式事件（透传整个 [`crate::chat::ChatEvent`] JSON）
///
/// daemon 不解析 ChatEvent 内部结构，只透传 JSON（见 ADR-0006）。
/// params: `{ stream_id, event: <ChatEvent JSON> }`
pub const PROVIDER_EVENT: &str = "provider.event";
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
            PROVIDER_EVENT,
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
