# Task 6: xgent_daemon

> 对应实现指导：`doc/plans/step6-xgent-daemon.md`
> 前置：step1 xgent_core、step3 xgent_settings_core、step5 xgent_provider 已完成

## 任务清单

### 阶段一：脚手架

- [ ] T-6.1 创建 crate 目录与 Cargo.toml
  - 依赖：无
  - 验收：`crates/xgent_daemon/Cargo.toml` 存在；依赖为 xgent_core、xgent_provider、xgent_settings_core、serde、serde_json、tokio、async-trait、thiserror、notify、tracing、tracing-subscriber；**不依赖 bevy**；`cargo check -p xgent_daemon` 通过（空 main）。

- [ ] T-6.2 注册到 workspace（bin crate）
  - 依赖：T-6.1
  - 验收：`cargo metadata` 识别；`[[bin]]` 配置正确（name = "xgent_daemon"）。

### 阶段二：IPC 服务端骨架

- [ ] T-6.3 实现跨平台 socket 绑定
  - 依赖：T-6.1
  - 验收：实现 `bind_socket(path)` 统一抽象（macOS/Linux 用 `tokio::net::UnixListener`，Windows 用 named pipe 或等价）；编译通过。

- [ ] T-6.4 实现 `server.rs` 的 Daemon 结构与 run 循环
  - 依赖：T-6.3
  - 验收：`Daemon` 持有 registry/pool/config/watcher 共享状态；`run()` 接受连接、每连接 spawn session task；编译通过。

- [ ] T-6.5 实现 `main.rs` 入口
  - 依赖：T-6.4
  - 验收：`#[tokio::main]`，初始化 tracing，取 socket 路径（`xgent_settings_core::paths::daemon_socket_path()`），构造并 run Daemon；编译通过；`cargo run -p xgent_daemon` 能启动并监听。

### 阶段三：客户端会话与注册表

- [ ] T-6.6 实现 `session.rs` 的 Session
  - 依赖：T-6.4
  - 验收：`Session::handle()` 注册客户端分配 ClientId、循环读 JSON-RPC、分发 Request 回 Response、处理通知、断开时注销；编译通过。

- [ ] T-6.7 实现 `session.rs` 的请求分发
  - 依赖：T-6.6
  - 验收：`handle_request` 按 method 分发到 provider_chat/provider_list_models/config_read/config_write/fs_watch，未知方法回 method_not_found；编译通过。

- [ ] T-6.8 实现 `registry.rs` 的 ClientRegistry
  - 依赖：T-6.6
  - 验收：`register/unregister/broadcast_to_project(exclude)/broadcast_all`；用 `Arc<RwLock>`；编译通过。

### 阶段四：Provider 池

- [ ] T-6.9 实现 `provider_pool.rs` 的 ProviderPool
  - 依赖：T-5.10, T-6.8
  - 验收：`get(id)` 从 ConfigStore 读 ProviderConfig 调 `build_provider`，缓存实例；`chat(req, client, stream_id, sender)` 调 provider.chat，把 ChatEvent 转 notification 推送回客户端 sender；编译通过。

### 阶段五：配置协调

- [ ] T-6.10 实现 `config_store.rs` 的 ConfigStore
  - 依赖：T-3.8, T-6.8
  - 验收：`load()` 从 `GlobalConfigStore::load()`；`read(key)`、`write(key, value)`（更新内存 + save TOML + 返回 ConfigChanged 供广播）；`Arc<RwLock>`；编译通过。

### 阶段六：文件监听

- [ ] T-6.11 实现 `fs_watcher.rs` 的 FsWatcher
  - 依赖：T-6.8
  - 验收：用 `notify::RecommendedWatcher` 监听项目目录；`watch(project, client)` 记订阅、`unwatch(client)` 清理；文件变更经 channel 转 `FileChanged`；同项目多客户端只监听一次；编译通过。

### 阶段七：生命周期

- [ ] T-6.12 实现 `lifecycle.rs`
  - 依赖：T-6.8
  - 验收：`on_client_connect`（计数++、取消退出计时）、`on_client_disconnect`（计数--，归零启动延迟退出计时如 30s）；编译通过。

- [ ] T-6.13 集成生命周期到 session
  - 依赖：T-6.6, T-6.12
  - 验收：连接时调 on_client_connect，断开时 on_client_disconnect；编译通过。

### 阶段八：测试

- [ ] T-6.14 启动与连接测试
  - 依赖：T-6.5
  - 验收：启动 daemon，用测试 JSON-RPC 客户端连接，发 config.read 收到响应。

- [ ] T-6.15 provider 流式测试（需 key/Ollama，可选）
  - 依赖：T-6.5, T-6.9
  - 验收：config 配 provider，发 provider.chat，收到 delta/done 通知序列；无 key 标 `#[ignore]`。

- [ ] T-6.16 文件监听测试
  - 依赖：T-6.11
  - 验收：fs.watch 临时目录，touch 文件，收到 fs.changed 通知。

- [ ] T-6.17 多客户端同步测试
  - 依赖：T-6.8, T-6.11
  - 验收：两连接订阅同项目，一个发文件变更通知，另一个收到 peer.fileChanged。

- [ ] T-6.18 生命周期测试
  - 依赖：T-6.12
  - 验收：所有客户端断开后，daemon 在延迟后退出。

- [ ] T-6.19 验证不依赖 Bevy
  - 依赖：T-6.13
  - 验收：`cargo tree -p xgent_daemon` 不含 bevy。

## 完成标志

- `cargo check -p xgent_daemon` 通过
- `cargo test -p xgent_daemon` 全绿（真实 provider 测试可 ignore）
- `cargo tree -p xgent_daemon` 不含 bevy
- daemon 可启动、接受连接、处理 provider/config/fs 请求、广播通知、随用随启退出
