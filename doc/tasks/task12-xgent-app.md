# Task 12: xgent_app

> 对应实现指导：`doc/plans/step12-xgent-app.md`
> 前置：全部 UI 侧 crate 已完成（xgent_core/xgent_settings/xgent_settings_core/xgent_agent/xgent_ui/xui/xui_i18n）

## 任务清单

### 阶段一：脚手架

- [ ] T-12.1 创建 crate 目录与 Cargo.toml
  - 依赖：无
  - 验收：`crates/xgent_app/Cargo.toml` 存在；依赖为 bevy(ui/bevy_gizmos/serialize/png/free_camera)、xgent_core、xgent_settings、xgent_settings_core、xgent_agent、xgent_ui、xui、xui_i18n、serde、serde_json、tokio、async-trait、thiserror、tracing、tracing-subscriber、clap；`[[bin]]` name = "xgent_app"；`cargo check -p xgent_app` 通过。

- [ ] T-12.2 注册到 workspace（bin crate）
  - 依赖：T-12.1
  - 验收：`cargo metadata` 识别；`[[bin]]` 配置正确。

### 阶段二：daemon 拉起与连接

- [ ] T-12.3 实现 `daemon.rs` 的探测与拉起
  - 依赖：T-12.1
  - 验收：`connect_or_spawn_daemon()`：用 `xgent_settings_core::paths::daemon_socket_path()` 探测 socket，未运行则 `std::process::Command::new("xgent_daemon")` 拉起、等待就绪重试连接；编译通过。

- [ ] T-12.4 验证 daemon 拉起
  - 依赖：T-12.3
  - 验收：不预启 daemon，启动 app，断言自动拉起 daemon 并连接成功。

### 阶段三：IPC 客户端

- [ ] T-12.5 实现 `ipc_client.rs` 的 IpcClient
  - 依赖：T-12.3
  - 验收：`IpcClient` 持有 writer（`Arc<Mutex<UnixStream>>`）、pending（`Arc<Mutex<HashMap<id, oneshot::Sender>>>`）、notif_tx；`call(method, params)` 分配 id 发 Request 等 Response；`notif_stream()` 返回通知 Receiver；编译通过。

- [ ] T-12.6 实现 read_loop 读取循环
  - 依赖：T-12.5
  - 验收：tokio task 读 socket 按 JSON-RPC 分发：有 id → Response 唤醒 oneshot；无 id → Notification 送 notif_tx；编译通过。

### 阶段四：ProviderClient 与通知桥接

- [ ] T-12.7 实现 `provider_client.rs`
  - 依赖：T-12.5, T-9.6
  - 验收：`ProviderClient` 经 IPC 调 daemon provider.chat，订阅 stream 通知，把 ChatEvent 喂 mpsc 给 agent bridge；编译通过。

- [ ] T-12.8 实现 `fs_event_bridge.rs`
  - 依赖：T-12.5
  - 验收：`fs_notif_bridge_system` 非阻塞从 ipc.notif_rx try_recv：fs.changed → FileChangedEvent；config.changed → 更新 GlobalConfigRes；peer.fileChanged → FileChangedEvent；编译通过。

### 阶段五：入口与启动

- [ ] T-12.9 实现 `main.rs` 入口
  - 依赖：T-12.5
  - 验收：tracing 初始化、clap 解析（--project/--provider）、组装 DefaultPlugins + XuiPlugin + XgentSettingsPlugin + XgentAgentPlugin + XgentUiPlugin + XgentAppPlugin；编译通过。

- [ ] T-12.10 实现 `startup.rs` 的启动序列
  - 依赖：T-12.3, T-12.7, T-12.8
  - 验收：`startup_sequence`：探测/拉起 daemon 建 IPC → 封装 ProviderClient 注入 AgentBridge → 注入 xui::Strings（Localizer 作 StringSource）→ 打开项目（load ProjectConfig、fs.watch 订阅、加载会话）→ 注册 fs 通知桥接到 Update；编译通过。

- [ ] T-12.11 实现项目打开
  - 依赖：T-12.10
  - 验收：`open_project(root, ipc)`：load ProjectConfig from .xgent/config.toml、call fs.watch 订阅、load 会话历史、init Conversation、构造 ContextProvider（按 strategy）；编译通过。

### 阶段六：生命周期与错误处理

- [ ] T-12.12 实现退出清理
  - 依赖：T-12.5
  - 验收：UI 进程退出时 drop IPC 连接，daemon 检测末个客户端断开后延迟退出；编译通过。

- [ ] T-12.13 实现启动错误友好提示
  - 依赖：T-12.3, T-12.11
  - 验收：daemon 连接失败、项目路径不存在等启动错误友好提示（UI 弹窗或 stderr），不静默崩溃；编译通过。

### 阶段七：端到端测试

- [ ] T-12.14 启动与连接测试
  - 依赖：T-12.9, T-12.10
  - 验收：先手动启 daemon 再启 app，断言连接成功、provider 列表加载。

- [ ] T-12.15 端到端对话测试（需 key/Ollama）
  - 依赖：T-12.7, T-12.10
  - 验收：app 发消息，流式回复出现；无 key 标 `#[ignore]`。

- [ ] T-12.16 文件同步测试
  - 依赖：T-12.8, T-12.10
  - 验收：开两个 app 连同项目，一个写文件，另一个收到 peer.fileChanged 并刷新。

- [ ] T-12.17 项目打开测试
  - 依赖：T-12.11, T-12.10
  - 验收：`--project <path>` 启动，ProjectConfig 加载、fs.watch 订阅、文件树显示。

### 阶段八：MVP 验收

- [ ] T-12.18 MVP 验收清单
  - 依赖：T-12.17
  - 验收：对照 `doc/plans/step12-xgent-app.md` 的 MVP 验收清单逐项确认：F-01~F-09、NF-01~NF-04。

## 完成标志

- `cargo build -p xgent_app` 产可执行二进制
- `cargo test -p xgent_app` 全绿（真实 provider/对话测试可 ignore）
- `cargo run -p xgent_app -- --project <path>` 可端到端运行（自动拉起 daemon、对话、工具、文件同步）
- MVP 验收清单（F-01~F-09 + NF-01~NF-04）全通过
