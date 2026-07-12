//! JSON-RPC 方法名常量（UI → daemon 的请求方法）。
//!
//! daemon → UI 的通知名见 [`crate::notifications`]。

/// 发起 provider 流式对话，返回 `StreamId`，后续通过通知推送事件。
pub const PROVIDER_CHAT: &str = "provider.chat";
/// 列出 provider 可用模型。
pub const PROVIDER_LIST_MODELS: &str = "provider.listModels";
/// 读取配置项。
pub const CONFIG_READ: &str = "config.read";
/// 写入配置项。
pub const CONFIG_WRITE: &str = "config.write";
/// 订阅项目文件变更。
pub const FS_WATCH: &str = "fs.watch";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn methods_are_nonempty_unique() {
        let all = [
            PROVIDER_CHAT,
            PROVIDER_LIST_MODELS,
            CONFIG_READ,
            CONFIG_WRITE,
            FS_WATCH,
        ];
        assert!(all.iter().all(|s| !s.is_empty()));
        // 唯一性
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(all[i], all[j], "duplicate method name");
            }
        }
    }
}
