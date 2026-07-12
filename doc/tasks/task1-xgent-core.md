# Task 1: xgent_core

> 对应实现指导：`doc/plans/step1-xgent-core.md`
> 前置：无（最底层 crate）

## 任务清单

### 阶段一：脚手架

- [ ] T-1.1 创建 crate 目录与 Cargo.toml
  - 依赖：无
  - 验收：`crates/xgent_core/Cargo.toml` 存在；依赖仅 serde、serde_json、thiserror（workspace 引用）；`cargo check -p xgent_core` 通过（空 lib.rs）。

- [ ] T-1.2 注册到 workspace
  - 依赖：T-1.1
  - 验收：根 `Cargo.toml` 的 `members` 含 `crates/*` 或显式含 `xgent_core`；`cargo metadata` 能识别该 crate。

### 阶段二：错误与 ID 类型

- [ ] T-1.3 实现 `error.rs`
  - 依赖：T-1.1
  - 验收：定义 `XgentError`（Ipc/Provider/Config/Tool/Io/Serde 变体，`#[from]` for io::Error 与 serde_json::Error）与 `XgentResult<T>` 别名；编译通过。

- [ ] T-1.4 实现 `ids.rs`
  - 依赖：T-1.1
  - 验收：定义 `ClientId`/`SessionId`/`StreamId`（u64 newtype，derive Debug/Clone/Copy/Partial/Eq/Hash/Serialize/Deserialize）与 `Display`；编译通过。

### 阶段三：对话与流式类型

- [ ] T-1.5 实现 `chat.rs`
  - 依赖：T-1.4
  - 验收：定义 `Role`、`ChatMessage`、`ChatRequest`、`ChatEvent`（tag 枚举 Delta/ToolCall/Done/Error）、`TokenUsage`、`ToolSchema`；`ChatEvent` 用 `#[serde(tag = "type")]`；编译通过。

- [ ] T-1.6 验证 `ChatEvent` 序列化格式
  - 依赖：T-1.5
  - 验收：`ChatEvent::Delta{text:"hi".into()}` 序列化为含 `"type":"delta"` 的 JSON；写往返测试断言 deserialize 还原。

### 阶段四：JSON-RPC 协议契约

- [ ] T-1.7 实现 `proto.rs`
  - 依赖：T-1.3
  - 验收：定义 `Request`/`Response`/`Notification`/`RpcError`（jsonrpc="2.0" 字段、id、method、params、result/error）；编译通过。

- [ ] T-1.8 实现 `methods.rs`
  - 依赖：T-1.7
  - 验收：定义 `methods::*` 常量（PROVIDER_CHAT/PROVIDER_LIST_MODELS/CONFIG_READ/CONFIG_WRITE/FS_WATCH）与 `notifications::*` 常量（PROVIDER_DELTA/PROVIDER_TOOL_CALL/PROVIDER_DONE/PROVIDER_ERROR/FS_CHANGED/CONFIG_CHANGED/PEER_FILE_CHANGED）；编译通过。

### 阶段五：文件与配置事件类型

- [ ] T-1.9 实现 `fs.rs`
  - 依赖：T-1.3
  - 验收：定义 `FileChanged`（project_root/path/kind）、`FileChangeKind`（Created/Modified/Removed/Renamed）、`WatchRequest`；编译通过。

- [ ] T-1.10 实现 `config.rs`
  - 依赖：T-1.3
  - 验收：定义 `ConfigScope`（Global/Project）、`ConfigReadRequest`、`ConfigChanged`；编译通过。

### 阶段六：lib 导出与测试

- [ ] T-1.11 实现 `lib.rs` 模块导出
  - 依赖：T-1.3~T-1.10
  - 验收：`lib.rs` 导出所有子模块的公开类型；`cargo doc -p xgent_core` 无警告。

- [ ] T-1.12 编写 serde 往返测试
  - 依赖：T-1.5, T-1.7, T-1.9, T-1.10
  - 验收：对 `ChatEvent`、`Request`、`Response`、`Notification`、`FileChanged`、`ConfigChanged` 各写 serialize→deserialize 往返测试，断言相等；`cargo test -p xgent_core` 通过。

- [ ] T-1.13 编写 JSON-RPC 契约测试
  - 依赖：T-1.7, T-1.8
  - 验收：构造 `Request`/`Response`/`Notification`，序列化后断言 JSON 结构符合 JSON-RPC 2.0（含 "jsonrpc":"2.0"、id/method 或 result/error）；测试通过。

## 完成标志

- `cargo check -p xgent_core` 通过
- `cargo test -p xgent_core` 全绿
- `cargo tree -p xgent_core` 仅含 serde/serde_json/thiserror，无 bevy/tokio
- 所有跨进程类型有 serde 往返测试覆盖
