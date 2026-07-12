# Step 6: xgent_daemon

## 模块职责

全局唯一的后台守护进程（瘦后台，MVP 职责范围）：

1. **IPC 服务端**：监听本地 socket（Unix socket / named pipe），处理 UI 进程的 JSON-RPC 请求与通知。
2. **Provider 连接池**：持有各 provider 的 `LlmProvider` 实例，复用连接；流式响应经 IPC notification 推送回 UI。
3. **全局配置协调**：持有全局配置权威副本，处理 config.read/write，变更时广播给所有客户端。
4. **文件监听**：监听项目目录变更，推送给订阅该项目的客户端。
5. **多客户端文件状态同步**：某客户端写文件后，广播 FileChanged 给同项目其他客户端。
6. **生命周期**：随用随启——首个 UI 客户端连接时若 daemon 未运行则拉起，末个客户端退出后延迟退出。

## 前置依赖

- xgent_core（协议、类型、错误）
- xgent_provider（LlmProvider trait 与适配器、build_provider）
- xgent_settings（GlobalConfig、ProviderConfig、ConfigStore）

## 目标文件结构

```
crates/xgent_daemon/
├── Cargo.toml
└── src/
    ├── main.rs                 # 入口：解析参数、启动 daemon
    ├── server.rs               # IPC 服务端：监听 socket、分发请求
    ├── session.rs              # 单个 UI 客户端连接的会话状态
    ├── registry.rs             # 客户端注册表（ClientId 分配、项目订阅）
    ├── provider_pool.rs        # Provider 连接池
    ├── config_store.rs         # 全局配置权威副本 + 读写协调
    ├── fs_watcher.rs           # 文件监听（notify）+ 订阅管理
    └── lifecycle.rs            # 生命周期：退出计时、优雅关闭
```

## Cargo.toml

```toml
[package]
name = "xgent_daemon"
version = "0.1.0"
edition = "2024"

[dependencies]
xgent_core = { path = "../xgent_core" }
xgent_provider = { path = "../xgent_provider" }
xgent_settings_core = { path = "../xgent_settings_core" }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
async-trait = { workspace = true }
thiserror = { workspace = true }
notify = "6"
tracing = "0.1"
tracing-subscriber = "0.3"
```

说明：不依赖 Bevy——daemon 是纯 tokio 服务，保持轻量。这是多开低占用的关键（daemon 不背 Bevy 渲染开销）。

## 关键类型与接口

### 1. main.rs — 入口

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::init();
    let addr = daemon_socket_path();   // 平台特定 socket 路径
    let daemon = Daemon::new(addr);
    daemon.run().await
}
```

### 2. server.rs — IPC 服务端

```rust
use tokio::net::UnixListener;  // Windows 用 named pipe，封装统一 trait
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct Daemon {
    addr: String,
    registry: Arc<RwLock<ClientRegistry>>,
    pool: Arc<ProviderPool>,
    config: Arc<RwLock<ConfigStore>>,
    watcher: Arc<FsWatcher>,
}

impl Daemon {
    pub async fn run(&self) -> Result<()> {
        let listener = bind_socket(&self.addr).await?;
        loop {
            let (stream, _) = listener.accept().await?;
            let session = Session::new(stream, self.shared.clone());
            tokio::spawn(session.handle());
        }
    }
}
```

### 3. session.rs — 客户端会话

```rust
pub struct Session { /* socket 读写、共享状态引用 */ }

impl Session {
    pub async fn handle(self) {
        // 1. 注册客户端，分配 ClientId
        // 2. 循环读 JSON-RPC 消息（按行/长度前缀）
        //    - Request -> 分发到对应 handler，回 Response
        //    - 通知 -> 处理（如 config.write 完成后广播）
        // 3. 连接断开 -> 注销客户端，触发退出计时检查
    }

    async fn handle_request(&self, req: Request) -> Response {
        match req.method.as_str() {
            methods::PROVIDER_CHAT => self.provider_chat(req).await,
            methods::PROVIDER_LIST_MODELS => self.provider_list_models(req).await,
            methods::CONFIG_READ => self.config_read(req).await,
            methods::CONFIG_WRITE => self.config_write(req).await,
            methods::FS_WATCH => self.fs_watch(req).await,
            _ => Response::method_not_found(req.id),
        }
    }
}
```

### 4. registry.rs — 客户端注册表

```rust
pub struct ClientRegistry {
    pub clients: HashMap<ClientId, ClientEntry>,
    next_id: u64,
}

pub struct ClientEntry {
    pub sender: mpsc::Sender<Notification>,  // 推送通知给该客户端
    pub subscribed_projects: HashSet<PathBuf>,
}

impl ClientRegistry {
    pub fn register(&mut self, sender) -> ClientId { /* ... */ }
    pub fn unregister(&mut self, id: ClientId) { /* ... */ }
    /// 广播给订阅某项目的所有客户端（排除来源）
    pub fn broadcast_to_project(&self, project: &Path, notif: Notification, exclude: Option<ClientId>) { /* ... */ }
    pub fn broadcast_all(&self, notif: Notification) { /* ... */ }
}
```

### 5. provider_pool.rs — Provider 连接池

```rust
pub struct ProviderPool {
    providers: RwLock<HashMap<String, Arc<dyn LlmProvider>>>,
    config: Arc<RwLock<ConfigStore>>,
}

impl ProviderPool {
    /// 获取或创建 provider（按 config 中的 ProviderConfig 构造）
    pub async fn get(&self, id: &str) -> Result<Arc<dyn LlmProvider>> { /* ... */ }

    /// 流式对话：调用 provider.chat()，把 ChatEvent 转成 IPC notification 推送回客户端
    pub async fn chat(&self, req: ChatRequest, client: ClientId, stream_id: StreamId, sender: mpsc::Sender<Notification>) {
        let provider = self.get(&req.provider).await?;
        let (_, mut stream) = provider.chat(req).await?;
        while let Some(ev) = stream.recv().await {
            let method = match &ev {
                ChatEvent::Delta { .. } => notifications::PROVIDER_DELTA,
                ChatEvent::ToolCall { .. } => notifications::PROVIDER_TOOL_CALL,
                ChatEvent::Done { .. } => notifications::PROVIDER_DONE,
                ChatEvent::Error { .. } => notifications::PROVIDER_ERROR,
            };
            let _ = sender.send(Notification { /* params = ev */ }).await;
        }
    }
}
```

### 6. config_store.rs — 全局配置权威副本

```rust
pub struct ConfigStore {
    config: GlobalConfig,
}

impl ConfigStore {
    pub async fn load() -> Self { /* 从 GlobalConfigStore::load() */ }
    pub fn read(&self, key: &str) -> serde_json::Value { /* ... */ }
    /// 写入：更新内存 + 持久化 TOML + 触发广播
    pub async fn write(&mut self, key: &str, value: Value) -> Result<ConfigChanged> { /* ... */ }
}
```

### 7. fs_watcher.rs — 文件监听

```rust
use notify::{Watcher, RecursiveMode, Event};

pub struct FsWatcher {
    watcher: notify::RecommendedWatcher,
    subscriptions: RwLock<HashMap<PathBuf, HashSet<ClientId>>>,
}

impl FsWatcher {
    pub fn new(sender: mpsc::Sender<(PathBuf, FileChanged)>) -> Self { /* ... */ }
    pub fn watch(&self, project: &Path, client: ClientId) { /* 添加监听 + 记订阅 */ }
    pub fn unwatch(&self, client: ClientId) { /* 客户端断开时清理 */ }
    /// 收到文件变更事件 -> 转成 FileChanged -> 经 channel 送广播
}
```

### 8. lifecycle.rs — 生命周期

```rust
/// 客户端计数归零后延迟退出（避免快速重启抖动）
pub struct Lifecycle {
    client_count: AtomicU64,
    shutdown_timer: Mutex<Option<JoinHandle<()>>>,
}

impl Lifecycle {
    pub fn on_client_connect(&self) { /* count++, 取消退出计时 */ }
    pub fn on_client_disconnect(&self) {
        // count--，若归零则启动延迟退出计时（如 30s）
    }
}
```

## 实现要点

1. **tokio 多任务并发**：每个客户端连接一个 task；provider chat 一个 task；文件监听一个 task。共享状态用 `Arc<RwLock<>>`，锁粒度小（registry、config、pool 各自独立锁）。
2. **socket 抽象**：Unix socket（macOS/Linux）与 named pipe（Windows）封装统一 trait，跨平台。socket 路径放 `dirs::cache_dir()/xgent/daemon.sock`（缓存目录适合临时文件）。
3. **流式 IPC**：provider 的 `ChatStream`（mpsc Receiver）由 daemon task 消费，每个 ChatEvent 转 JSON-RPC notification 推回客户端的 sender channel，客户端再转成 Bevy Event。
4. **配置写协调**：config.write 是唯一写入点，写完后广播 config.changed 给所有客户端，各客户端刷新本地副本 Resource。
5. **文件监听去重**：同项目多客户端订阅时，notify 只监听一次，事件广播给所有订阅者。
6. **多客户端文件状态同步**：工具写文件（step5，在 UI 侧执行）后，UI 主动发通知给 daemon，daemon 广播 peer.fileChanged 给同项目其他客户端（fs_watcher 也能捕获本地改动，但显式通知更即时且语义清晰）。
7. **不依赖 Bevy**：daemon 纯 tokio，保持轻量，多开时只一个 daemon 共享。
8. **生命周期**：首客户端连接触发 daemon 启动（若未运行，UI 进程负责拉起）；末客户端断开后延迟退出。daemon 拉起逻辑在 xgent_app（step12）实现，daemon 自身只管运行与退出计时。
9. **错误隔离**：单客户端连接异常不影响其他客户端；provider 调用错误经 IPC 错误响应返回。

## 验证方法

1. **编译检查**：
   ```bash
   cargo check -p xgent_daemon
   ```
2. **启动与连接测试**：手动启动 daemon，用一个最小 JSON-RPC 客户端（可用 Python/curl 或写个测试 bin）连接，发送 config.read，断言响应。
3. **provider 流式测试**（需 API key/Ollama）：config 里配 provider，发 provider.chat，断言收到 delta/done 通知序列。
4. **文件监听测试**：fs.watch 一个临时目录，touch 文件，断言收到 fs.changed 通知。
5. **多客户端同步测试**：开两个连接订阅同项目，一个发文件变更通知，断言另一个收到 peer.fileChanged。
6. **生命周期测试**：所有客户端断开后，断言 daemon 在延迟后退出。

## 完成后下一步

xgent_daemon 完成后 → 实现 **xgent_tools**（工具枚举 + 安全策略 + 执行器），它依赖 core 类型，MVP 在 UI 侧执行。
