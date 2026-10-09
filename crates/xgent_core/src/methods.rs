//! JSON-RPC 方法名常量（UI → daemon 的请求方法）。
//!
//! daemon → UI 的通知名见 [`crate::notifications`]。

/// 发起 provider 流式对话，返回 `StreamId`，后续通过通知推送事件。
pub const PROVIDER_CHAT: &str = "provider.chat";
/// 取消指定 provider 流（用户 abort）。
///
/// daemon 侧据此终止该流的推送 task 并 drop provider 接收端，上游 HTTP
/// 请求随之取消——否则被放弃的流会继续消费到自然结束并继续计费。
/// 取消已结束的流视为成功（幂等）。
pub const PROVIDER_CANCEL: &str = "provider.cancel";
/// 列出 provider 可用模型。
pub const PROVIDER_LIST_MODELS: &str = "provider.listModels";
/// 读取配置项。
pub const CONFIG_READ: &str = "config.read";
/// 写入配置项。
pub const CONFIG_WRITE: &str = "config.write";
/// 订阅项目文件变更。
pub const FS_WATCH: &str = "fs.watch";
/// 通知 daemon 文件已被本客户端修改（保存），daemon 广播 `peer.fileChanged`
/// 给同项目其他客户端（排除来源）。与 `fs.watch` 的被动监听互补：
/// `fs.watch` 由 notify 检测外部变更；`fs.notify` 由 UI 主动报告用户保存。
pub const FS_NOTIFY: &str = "fs.notify";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_names_are_unique() {
        let all = [
            PROVIDER_CHAT,
            PROVIDER_CANCEL,
            PROVIDER_LIST_MODELS,
            CONFIG_READ,
            CONFIG_WRITE,
            FS_WATCH,
            FS_NOTIFY,
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
