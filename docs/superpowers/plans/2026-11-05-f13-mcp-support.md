---
change: f13-mcp-support
design-doc: docs/openspec/changes/f13-mcp-support/design.md
base-ref: 22bd0ffb2a2673c1aa3b0077526faef3b72b86b6
---

<!-- comet-task-authority: docs/openspec/changes/f13-mcp-support/tasks.md -->

# F-13 MCP 支持实现计划

## 实施约束

- 分支 `comet/f13-mcp-support`，目标 `main`；不新增第三方依赖（tokio/serde_json/toml 已有）。
- TDD：每个任务先提交/记录 RED（预期失败原因），再实现到 GREEN；RED 必须是缺失行为导致，不是环境错误。
- 协议基线 MCP `2025-06-18`；仅 stdio 与 tools。
- 项目级 `mcp.toml` 默认被 `allow_project_servers=false` 忽略。
- MCP 工具固定 `Exec` + `Exclusive`，默认 `NeedsConfirmation`；不读取 Server 只读标注。

## 1. 配置层（tasks 1.1–1.2）

<!-- comet-task-ref:c066666e-89fb-483d-80ed-9fbd21261f4a -->

**结果**：`xgent_settings_core` 能解析全局/项目 MCP 配置，按开关合并，展开环境变量，并给出可诊断的跳过原因。

- 模块：`crates/xgent_settings_core/src/mcp.rs`，并在 `lib.rs` 重导出；`paths.rs` 增加 `mcp_config_file()`（全局）与 `project_mcp_config_file(root)`。
- 类型：`McpConfig { allow_project_servers, servers: BTreeMap<String, McpServerConfig> }`；`McpServerConfig { id, command, args, env, cwd, enabled, auto_start }`，均带 serde 默认值。
- 行为：全局文件不存在 → 空配置；`allow_project_servers=false` → 不读项目文件；开启后按 id 合并（项目整项替换）；`${VAR}` 缺失 → 跳过该项并记录 warning；损坏 TOML → 空/部分配置且不 panic。
- 测试（RED→GREEN）：`mcp::tests::*` 覆盖默认开关、项目忽略、同名覆盖、变量存在/缺失、损坏文件、示例文档可解析。
- 验证：`cargo test -p xgent_settings_core mcp`。

<!-- comet-task-ref:a594a80c-5803-448f-9ee6-0fe1d88c0517 -->

**结果**：开发指南包含可直接使用的配置示例与信任开关说明。

- 在 `docs/dev-tutorial.md` 新增 MCP 小节：全局/项目路径、`[[servers]]` 字段、`allow_project_servers` 默认关闭、`${VAR}`、安全默认与首期限制。
- 示例字符串同时进入 1.1 的解析测试，保证文档不腐烂。
- 验证：`cargo test -p xgent_settings_core mcp::tests::doc_example_parses`。

## 2. stdio 传输、协议与工具适配（tasks 2.1–2.3）

<!-- comet-task-ref:253e40e4-d00a-4a2a-b545-f4c169f3231f -->

**结果**：`StdioTransport` 能管理一个 MCP 子进程的生命周期与 JSON-RPC 请求路由。

- 将 `crates/xgent_tools/src/mcp.rs` 改为 `mcp/` 模块：`transport.rs`、`client.rs`、`tool.rs`、`manager.rs`，`mod.rs` 保留导出与文档。
- `StdioTransport`：`tokio::process::Command` + `kill_on_drop(true)`；stdin/stdout 换行 JSON-RPC；自增 u64 id + pending `oneshot`；请求超时；stderr 独立 task（单行截断）；EOF/进程退出让所有 pending 失败；`close` 中止 task 并 kill 子进程。
- 为可测试性提供基于 `tokio::io::duplex` 的内部连接构造；真实 spawn 只在 manager/集成测试使用。
- 测试（RED→GREEN）：请求/响应配对、乱序响应、超时、EOF 清理、stderr 行不阻塞主循环。
- 验证：`cargo test -p xgent_tools mcp::transport`。

<!-- comet-task-ref:6516c4ed-2886-45e0-9ede-2a68be70e1a6 -->

**结果**：`McpClient` 完成 MCP 2025-06-18 的初始化与工具发现/调用。

- `initialize` 发送 `protocolVersion: "2025-06-18"`、`clientInfo`、`capabilities: {}`；成功后发 `notifications/initialized`。
- `tools/list` 跟随 `nextCursor`，上限 256；工具缺 `name`/对象型 `inputSchema` 跳过。
- `tools/call` 发送 `{name, arguments}`；服务端 `ping` 回空结果，未知请求回 `-32601`，通知记日志。
- 测试（RED→GREEN）：握手顺序、分页汇总、非法工具跳过、JSON-RPC 错误、ping、取消（见 2.3 的调用路径）。
- 验证：`cargo test -p xgent_tools mcp::client`。

<!-- comet-task-ref:78a1eb79-33bd-4ad3-8c2b-598fc1aec602 -->

**结果**：`McpTool` 把 MCP 工具完整接入现有 `Tool`/确认链路。

- 完整 id：`mcp__<server>__<tool>`；server id 校验，工具名清洗为 `[A-Za-z0-9_-]`，超长截断加稳定哈希，完整 id ≤64；保留原始工具名用于调用。
- schema 描述截断 2 KiB、schema 64 KiB；`tier=Exec`、`concurrency=Exclusive`；`summarize` 含 server/tool 与输入摘要。
- 结果：文本块按序拼接、无文本时 JSON、输出截断 64 KiB、`isError` → `ToolResult { is_error: true }`；协议/超时/断连 → `ToolError`；取消发 `notifications/cancelled` 并返回 `Aborted`。
- 测试（RED→GREEN）：id 清洗/冲突、schema 上限、结果拼接/截断、isError、默认 `NeedsConfirmation`、`[tool_policy]` 提升、只读标注不改变策略。
- 验证：`cargo test -p xgent_tools mcp::tool`。

## 3. 管理器与 app 接线（tasks 3.1–3.3）

<!-- comet-task-ref:f97d6aeb-b6e2-4e38-ac3d-986819733cd7 -->

**结果**：`McpManager` 能并发连接多个 Server，注册工具并隔离失败。

- `McpManager::start(configs, executor)`：每 server 一个 tokio task；连接→初始化→发现→注册；单 server 失败只标记自身。
- 注册前检查完整 id 是否已存在，冲突跳过；断连后保留注册项，调用返回可读错误。
- `shutdown()` 中止 task 并关闭子进程；`Drop` 兜底 kill。
- 新增测试用假 MCP Server 二进制（`crates/xgent_tools/src/bin/xgent_mcp_fixture.rs`），实现 initialize/list/call 与可控失败模式。
- 测试（RED→GREEN）：真实 stdio 启动/注册、单 server 失败隔离、shutdown 后进程退出、项目文件字节不变。
- 验证：`cargo test -p xgent_tools mcp::manager`。

<!-- comet-task-ref:29a1a4bc-4a5d-4aab-a594-86fe893bdba1 -->

**结果**：应用启动时按配置连接 MCP，退出时清理。

- `crates/xgent_app/src/mcp_boot.rs`：加载 config、在 `AgentBridge` runtime 上启动 manager、返回可持有 Resource。
- `main.rs` 插入 `McpRuntime`；新增 `AppExit` 系统调用 `shutdown`。
- `AgentBridge` 的 tool schema 改为每轮从 executor 读取，确保后台注册的 MCP 工具下一轮可见。
- 测试（RED→GREEN）：无配置时 no-op、配置加载失败不阻止启动、manager 关闭路径；真实进程覆盖留在 3.1。
- 验证：`cargo test -p xgent_app mcp_boot` 与 `cargo check -p xgent_app`。

<!-- comet-task-ref:b359ae7d-7f55-455f-b322-b894ff6c5e82 -->

**结果**：架构与开发指南反映 MCP 实际接线与限制。

- `docs/design/architecture.md`：工具抽象下补充 MCP Client、配置路径、生命周期与后续项。
- `docs/dev-tutorial.md`：MCP 小节增加 manager/app 接线、日志、测试命令与已知限制。
- 验证：文档中的路径与测试名由 `rg` 确认存在。

## 4. 集成与回归（tasks 4.1–4.2）

<!-- comet-task-ref:9d782aa0-2579-4c03-b8d0-93be946a7322 -->

**结果**：相关 crate 与 workspace 编译、测试通过。

- 命令：`CARGO_PROFILE_TEST_DEBUG=0 cargo test -p xgent_settings_core -p xgent_tools -p xgent_app -j 2`；`cargo check --workspace -j 2`。
- 记录实际输出；若触及 UI/agent 共享接口，追加对应 crate 测试。

<!-- comet-task-ref:7457ca30-7b06-4085-94e8-14c571838226 -->

**结果**：安全默认与只读行为有回归保护。

- 测试：MCP 工具默认确认、`[tool_policy]` Approved 生效、Server 只读标注不改变策略；连接/发现/调用前后项目文件哈希不变。
- 验证：`cargo test -p xgent_tools mcp`。
