# Step 12: xgent_app

## 模块职责

UI 进程入口，组装所有 UI 侧插件并处理进程级职责：

1. **插件组装**：DefaultPlugins + XgentSettingsPlugin + XgentAgentPlugin + XgentUiPlugin。
2. **daemon 拉起与连接**：启动时探测本地 daemon socket，未运行则 fork 拉起，建立 IPC 连接。
3. **IPC 客户端**：封装 JSON-RPC 客户端，调用 daemon 的 provider/chat、config.read/write、fs.watch；把 daemon 通知转成 Bevy Event 喂入 ECS。
4. **项目打开**：解析命令行参数或 UI 选择的项目路径，init ProjectConfig、订阅 fs.watch、加载会话。
5. **ProviderClient 桥接**：把 IPC 客户端封装为 agent bridge 用的 provider 调用接口（trait 实现）。
6. **生命周期**：UI 进程退出时断开 daemon 连接（daemon 末个客户端退出后自退出）。

## 前置依赖

- 全部 UI 侧 crate：xgent_core、xgent_settings、xgent_settings_core、xgent_agent、xgent_ui、xui、xui_i18n
- daemon 的 socket 契约（与 xgent_daemon 对接，但不依赖其 crate——靠协议）

## 目标文件结构

```
crates/xgent_app/
├── Cargo.toml
└── src/
    ├── main.rs            # 入口：解析参数、组装 App、run
    ├── daemon.rs          # daemon 探测、拉起、连接
    ├── ipc_client.rs      # JSON-RPC 客户端封装
    ├── provider_client.rs # ProviderClient（IPC 实现 LlmProvider 调用）
    ├── fs_event_bridge.rs # daemon fs.changed 通知 → Bevy Event
    └── startup.rs         # 启动系统：打开项目、初始化资源
```

## Cargo.toml

```toml
[package]
name = "xgent_app"
version = "0.1.0"
edition = "2024"

[dependencies]
bevy = { workspace = true, features = [
    "ui",
    "bevy_gizmos",
    "serialize",
    "png",
    "free_camera",   # 未来 3D 预留，MVP 可不启用
] }
xgent_core = { path = "../xgent_core" }
xgent_settings = { path = "../xgent_settings" }
xgent_settings_core = { path = "../xgent_settings_core" }
xgent_agent = { path = "../xgent_agent" }
xgent_ui = { path = "../xgent_ui" }
xui = { path = "../xui" }
xui_i18n = { path = "../xui_i18n" }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
async-trait = { workspace = true }
thiserror = { workspace = true }
tracing = "0.1"
tracing-subscriber = "0.3"
clap = { version = "4", features = ["derive"] }
```

说明：xgent_app 是唯一启用较多 Bevy feature 的 crate（窗口、渲染）。free_camera 等可待 3D 阶段启用。MVP feature 集最小化。

## 关键类型与接口

### 1. main.rs — 入口

```rust
fn main() {
    tracing_subscriber::init();
    let args = Args::parse();   // clap: --project <path>, --provider <id> ...

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "XGent".into(),
            ..default()
        }),
        ..default()
    }))
    .add_plugins((
        XuiPlugin,             // xui 通用组件
        XgentSettingsPlugin,
        XgentAgentPlugin,
        XgentUiPlugin,
        XgentAppPlugin { args },   // 本 crate 的启动逻辑
    ))
    .run();
}
```

### 2. daemon.rs — daemon 探测与拉起

```rust
pub struct XgentAppPlugin { pub args: Args }

impl Plugin for XgentAppPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, startup_sequence);
    }
}

fn startup_sequence(
    mut commands: Commands,
    args: Res<Args>,
    mut bridge: ResMut<AgentBridge>,
) {
    // 1. 探测/拉起 daemon，建立 IPC 连接
    let ipc = connect_or_spawn_daemon();   // 同步阻塞或异步 task
    // 2. 封装 ProviderClient（IPC 实现）
    // 3. 注入到 AgentBridge（让 agent loop 用 IPC 调 provider）
    // 3.5 注入 xui::Strings（把 xgent_settings::Localizer 包装为 xui_i18n::StringSource trait impl）
    // 4. 打开项目：load ProjectConfig, fs.watch 订阅, 加载会话
    // 5. 把 IPC 通知桥接系统注册到 Update
}

fn connect_or_spawn_daemon() -> IpcClient {
    let path = daemon_socket_path();
    if let Ok(c) = connect_socket(&path) { return c; }
    // 未运行 → 启动 xgent_daemon 进程（std::process::Command）
    // 等待 socket 就绪 → 重试连接
    spawn_daemon_process();
    wait_for_socket(&path);
    connect_socket(&path).expect("daemon failed to start")
}
```

### 3. ipc_client.rs — JSON-RPC 客户端

```rust
use tokio::net::UnixStream;  // Windows: named pipe
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};

pub struct IpcClient {
    writer: Arc<Mutex<UnixStream>>,   // 写请求
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Response>>>>,
    notif_tx: mpsc::Sender<Notification>,  // 通知推送给桥接
}

impl IpcClient {
    pub async fn call(&self, method: &str, params: Value) -> Result<Response> {
        // 分配 id，发 Request，注册 oneshot，等待响应
    }
    pub fn notif_stream(&self) -> mpsc::Receiver<Notification> { /* ... */ }
}

/// 读取循环：tokio task 读 socket，按 JSON-RPC 分发
async fn read_loop(stream: UnixStream, pending: ..., notif_tx: ...) {
    // 按行/长度前缀读 JSON
    //   有 id -> Response，唤醒 pending oneshot
    //   无 id -> Notification，送 notif_tx
}
```

### 4. provider_client.rs — IPC 调 provider

```rust
use async_trait::async_trait;
use xgent_provider::provider::{LlmProvider, ChatStream, ModelInfo, ProviderError};
use xgent_core::{chat::{ChatRequest, ChatEvent}, ids::StreamId};

/// 经 IPC 调 daemon 的 provider 池
pub struct ProviderClient { ipc: Arc<IpcClient> }

impl ProviderClient {
    pub async fn chat(&self, req: ChatRequest) -> Result<(StreamId, ChatStream), ProviderError> {
        // call provider.chat -> 拿 stream_id
        // 订阅该 stream 的通知（delta/toolCall/done/error）
        // 起 task 把通知转成 ChatEvent 喂 mpsc channel
        // 返回 Receiver
    }
}
```

注：ProviderClient 不直接 impl LlmProvider（语义略不同——它调远端池），但接口对称。agent bridge 用它调 chat。

### 5. fs_event_bridge.rs — 通知转 ECS Event

```rust
use bevy::prelude::*;

#[derive(Event)]
pub struct FileChangedEvent(pub FileChanged);   // 从 xgent_core

pub fn fs_notif_bridge_system(
    ipc: Res<IpcClientResource>,
    mut events: EventWriter<FileChangedEvent>,
) {
    // 非阻塞从 ipc.notif_rx try_recv
    //   fs.changed -> FileChangedEvent
    //   config.changed -> 更新 GlobalConfig Resource（或发 ConfigChangedEvent）
    //   peer.fileChanged -> FileChangedEvent（同处理）
}
```

### 6. startup.rs — 项目打开

```rust
pub fn open_project(project_root: &Path, ipc: &IpcClient) {
    // 1. load ProjectConfig from .xgent/config.toml
    // 2. call fs.watch 订阅 project_root
    // 3. load 会话历史（从 sessions.db，MVP 可先空）
    // 4. init Conversation Resource
    // 5. 构造 ContextProvider（按 strategy）
}
```

## 实现要点

1. **daemon 拉起**：UI 进程负责探测与 fork 拉起 xgent_daemon 进程。用 `std::process::Command::new("xgent_daemon")`（需 daemon 二进制在同 PATH 或相对路径）。生产部署需处理安装路径。
2. **socket 路径**：与 daemon 约定一致（用 `xgent_settings_core::paths::daemon_socket_path()`）。UI 与 daemon 都用同一函数，避免路径不一致。
3. **IPC 客户端线程模型**：读 socket 是 tokio task（长驻），写用 `Arc<Mutex>`。请求用 oneshot 等待响应，通知用 mpsc 推送。
4. **ProviderClient 不直连 provider**：所有 provider 调用经 daemon，UI 不持有 HTTP 连接、API key。API key 留 daemon 侧（安全）。
5. **通知桥接**：daemon 的通知（provider delta、fs.changed、peer.fileChanged、config.changed）经 IPC task 读出，喂 Bevy Event。provider delta 直接给 agent bridge（不经 Event，走 channel）；fs/config/peer 给对应 Event。
6. **项目打开入口**：命令行 `--project <path>` 或 UI 文件选择器。MVP 先支持命令行，UI 选择后续加。
7. **生命周期**：UI 进程退出时 drop IPC 连接，daemon 检测到末个客户端断开后延迟退出。
8. **DefaultPlugins 配置**：窗口标题 XGent，关闭 vsync 资源消耗？保留默认。MVP 不开 3d。
9. **错误处理**：daemon 连接失败、项目路径不存在等启动错误要友好提示（UI 弹窗或 stderr），不静默崩溃。
10. **跨平台**：socket 用 Unix/Windows 统一抽象（与 daemon 一致）。

## 验证方法

1. **编译检查**：
   ```bash
   cargo check -p xgent_app
   cargo build -p xgent_app   # 产二进制
   ```
2. **启动 daemon 测试**：先手动启动 daemon，再启动 app，断言连接成功、provider 列表加载。
3. **自动拉起测试**：不预启 daemon，启动 app，断言 app 自动拉起 daemon 并连接。
4. **端到端对话测试**（需 API key/Ollama）：在 app 里发消息，断言流式回复出现。
5. **文件同步测试**：开两个 app 连同项目，一个写文件，断言另一个收到 peer.fileChanged 并刷新。
6. **项目打开测试**：`--project <path>` 启动，断言 ProjectConfig 加载、fs.watch 订阅、文件树显示。

## MVP 完成验证

xgent_app 完成后，MVP 可端到端运行：

```bash
# 启动（自动拉起 daemon）
cargo run -p xgent_app -- --project /path/to/project --provider openai
# 或先启 daemon
cargo run -p xgent_daemon &
cargo run -p xgent_app -- --project /path/to/project
```

MVP 验收清单（对照需求 F-01~F-09 + NF-01~04）：
- [ ] F-01 多轮对话（中断/重试）
- [ ] F-02 流式输出
- [ ] F-03 工具调用（读/写/搜/运行）
- [ ] F-04 操作确认（写/运行需确认）
- [ ] F-05 项目上下文（方案 A 检索）
- [ ] F-06 会话管理（新建/切换/历史）
- [ ] F-07 provider 切换（OpenAI compatible/Ollama/自定义）
- [ ] F-08 命令面板
- [ ] F-09 快捷键（参考 VSCode）
- [ ] NF-01 跨平台（macOS/Linux/Windows）
- [ ] NF-02 多开共享 daemon、项目配置隔离
- [ ] NF-03 UI 零延迟响应
- [ ] NF-04 模块独立可测、headless 可运行

## 后续

MVP 完成后进入 P1 迭代（见需求 7.2）：Git、内置编辑器、成本统计、MCP、虚拟宠物。其中内置编辑器上线触发 OQ-08 检索升级（C→D→E）。
