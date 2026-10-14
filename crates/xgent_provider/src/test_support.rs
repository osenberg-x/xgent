//! 测试支撑：本机 HTTP/SSE 端点，用于在不访问真实服务的前提下验证
//! provider 的超时链路。
//!
//! 存在的理由：`first_event_timeout_secs`（R3-17）只有让 `chat()` 真的去等一个
//! 不来的首事件，才能证明配置值进入了等待本身；只断言构造后的字段值会漏掉
//! 「取了配置但没用」这一类断裂。

use std::net::SocketAddr;

/// 接受连接、回 SSE 响应头、此后不再产生任何事件的端点。
///
/// 返回监听地址：provider 指向它即可稳定复现「首事件永不到达」。
/// 响应声明 `transfer-encoding: chunked` 且不发送任何 chunk，因此
/// 客户端的 body 流保持挂起，而不是被判成空体立即结束。
pub(crate) async fn bind_stalling_sse_endpoint() -> SocketAddr {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("测试端点绑定失败");
    let addr = listener.local_addr().expect("local_addr");

    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                // 读掉请求头（内容不影响本测试），再回响应头并保持连接打开
                let mut buf = [0u8; 8192];
                let _ = sock.read(&mut buf).await;
                let head = b"HTTP/1.1 200 OK\r\n\
                             content-type: text/event-stream\r\n\
                             cache-control: no-cache\r\n\
                             transfer-encoding: chunked\r\n\
                             \r\n";
                if sock.write_all(head).await.is_err() {
                    return;
                }
                let _ = sock.flush().await;
                tokio::time::sleep(std::time::Duration::from_secs(120)).await;
            });
        }
    });

    addr
}

/// 端点的源地址（不含路径）。
///
/// OpenAI 兼容适配器需要 `{origin}/v1`，Anthropic 适配器直接需要 `{origin}`
/// （它自己拼 `/v1/messages`）。
pub(crate) fn origin(addr: SocketAddr) -> String {
    format!("http://{addr}")
}
