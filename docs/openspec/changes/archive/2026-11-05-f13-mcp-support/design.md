---
comet_change: f13-mcp-support
role: technical-design
canonical_spec: openspec
archived-with: 2026-11-05-f13-mcp-support
status: final
---

# Design

## Context

- `xgent_tools::mcp` 目前只有 `McpTransport` trait 与 `McpError`，无实现。
- 工具执行链路已具备 `Tool` trait、`ToolExecutor` 动态 `register`、按工具 id 的 `[tool_policy]` 与确认流程；MCP 工具可直接复用。
- 插件 WIT 的 `host.run-command` 是一次性命令，无法承载 stdio 双向长连接，因此 MCP 不能按 `docs/design/plugin-system-design.md` 的迁移表做成 WASM 插件。
- 配置路径由 `xgent_settings_core::paths` 统一：全局 `~/.xgent/agent/`，项目 `<root>/.xgent/`。
- 协议基线为 MCP `2025-06-18`（initialize/initialized 有状态握手）；`2026-07-28` 无状态规范作为后续项。

## Goals / Non-Goals

**Goals:**

- 本地 stdio MCP Server 的配置、连接、工具发现、注册与调用形成可验证闭环。
- 工具沿用现有确认、策略、并发与错误链路；默认安全。
- 项目级配置默认不执行，避免打开仓库即运行其中命令。

**Non-Goals:**

- HTTP/SSE、resources、prompts、sampling、OAuth、2026-07-28 无状态协议。
- 设置面板 MCP 分区、3D 空间站、自动重连、`notifications/tools/list_changed` 热刷新。
- 多客户端/daemon 共享 MCP 连接；每个 UI 进程各自连接。

## Decisions

### D1：宿主侧实现，扩展 `xgent_tools::mcp`

MCP 连接器放在 `xgent_tools`，复用现有 `Tool`/`ToolExecutor`，不新建 crate，也不做成 WASM 插件。理由是 stdio 需要持久子进程与双向路由，插件 WIT 不提供该能力；放 `xgent_tools` 可让 `McpTool` 与内置工具走同一注册、策略与并发入口。

### D2：自定义最小 stdio 客户端，不引入 rmcp

首期只需 tools/list 与 tools/call，使用 `tokio::process` + 换行分隔 JSON-RPC 即可覆盖；避免引入 `rmcp` 的版本/MSRV 约束。`McpTransport` trait 保留，新增 `StdioTransport` 实现；协议层 `McpClient` 负责 initialize/list/call。后续若扩展 HTTP 或 2026 规范，可在 trait 后替换/新增实现。

### D3：stdio 协议

- 子进程 stdin/stdout，每行一个 JSON-RPC 对象；stderr 逐行读取、单行截断后记日志。
- 启动后发送 `initialize`：`protocolVersion = "2025-06-18"`，`clientInfo = { name: "xgent", version }`，`capabilities = {}`；成功响应后发送 `notifications/initialized`。
- 请求 id 用自增 `u64`；pending map 将响应路由到 `oneshot`。超时默认：初始化/列表 30s、工具调用 120s。
- 服务端发来的 `ping` 请求回空结果；未知请求回 `-32601`；通知只记日志。首期不处理 list_changed，重启应用刷新。
- 初始化失败、进程提前退出、EOF 均标记连接断开，并让全部 pending 请求返回传输错误。

### D4：配置与项目信任

- 全局文件 `~/.xgent/agent/mcp.toml`，项目文件 `<project_root>/.xgent/mcp.toml`，结构均为 `[[servers]]`。
- 全局文件支持 `allow_project_servers = false`（默认）。关闭时**完全不读取**项目文件；开启后按 server id 合并，项目同名项整体替换全局项。
- server 字段：`id`、`command`、`args`、`env`、`cwd`、`enabled`（默认 true）、`auto_start`（默认 true）。`cwd` 相对项目根解析。
- 所有字符串字段支持 `${VAR}` 展开；变量缺失时跳过该 server 并 warn，不用空值启动。
- 配置解析失败只影响 MCP，不阻止应用启动；`enabled=false` 或 `auto_start=false` 不自动连接。

### D5：工具映射与安全

- 完整 id 为 `mcp__<server>__<tool>`（双下划线而非点号：OpenAI/Anthropic 的 function name 只接受 `[A-Za-z0-9_-]`，点号会被 provider 拒绝）；server id 限制为 `[A-Za-z0-9][A-Za-z0-9_-]{0,23}`。
- 工具名按 `[A-Za-z0-9_-]` 清洗，超长时截断并附加稳定短哈希，保证完整 id ≤64；同时保存原始工具名用于 `tools/call`。
- 工具必须有 `name` 与对象型 `inputSchema`，否则跳过；单 server 最多 256 个工具，schema 上限 64 KiB，描述截断到 2 KiB。
- 完整 id 与现有工具冲突时跳过并 warn；注册失败不影响内置工具。
- `McpTool::tier()` 固定 `Exec`，`concurrency()` 固定 `Exclusive`；不读取 Server 的只读标注。默认确认由现有 `resolve_policy` 产生；用户可在 `[tool_policy]` 按完整 id 提升为 Approved。
- 工具结果：按顺序拼接 `content` 中的文本块；无文本时返回结果 JSON；`isError=true` 映射为 `ToolResult { is_error: true }`；输出截断到 64 KiB。
- 协议/传输/超时/断连返回 `ToolError::Failed`/`Timeout`；取消时发送 `notifications/cancelled` 并返回 `ToolError::Aborted`。

### D6：生命周期与并发

- `McpManager::start(configs, executor)` 为每个启用 server 启动一个 tokio task：连接 → initialize → tools/list → 注册工具。启动不阻塞 UI。
- 单 server 失败只标记该连接，不影响其他 server 与应用；已注册工具在断连后仍保留，调用返回可读错误（避免工具列表随连接抖动）。
- 子进程用 `kill_on_drop(true)`；manager 提供 `shutdown()`，`xgent_app` 在退出时调用并中止 reader/writer task。
- 工具执行本身由 `ToolExecutor` 串行调度；同一连接上的请求通过 id 并发安全路由。

### D7：接线

- `xgent_app` 启动时读取全局/项目 MCP 配置，在 `AgentBridge` 的 tokio runtime 中调用 `McpManager::start`，把 manager 作为 Bevy Resource 持有；新增退出系统触发 `shutdown`。
- `AgentBridge` 的工具 schema 改为每轮从 `ToolExecutor` 读取，后台注册的 MCP 工具在下一轮请求即对 LLM 可见（不再使用启动时快照）。
- 日志记录 server 连接状态、工具数量与失败原因；不新增设置面板。
- 文档同步 `AGENTS.md`、`docs/dev-tutorial.md`、`docs/design/architecture.md`。

### D8：测试策略

- `xgent_settings_core`：解析、默认值、合并、项目开关、环境变量缺失、损坏文件。
- `xgent_tools`：用 `tokio::io::duplex` 假端点测 initialize、分页、调用、错误、超时、取消与 EOF；用测试专用假 Server 二进制跑真实 stdio 进程，覆盖启动、注册、失败隔离与退出清理。
- 工具层：schema/命名清洗、Exec 与默认确认、策略提升、只读标注不生效、结果拼接与截断。
- 只读：连接/发现/调用前后项目文件字节不变。

## Risks / Trade-offs

- [子进程树终止] → `npx` 等会派生 node 子进程，`kill_on_drop` 只保证直接子进程；首期记录该限制，后续用平台进程组/Job Object 修复。
- [Windows 无扩展名命令] → `CreateProcessW` 不按 PATHEXT 解析 `npx`；`StdioTransport` 在 Windows 按 PATH/PATHEXT 查找 `.cmd`/`.bat`/`.exe`。
- [Provider function name] → 工具 id 使用 `mcp__...__...`，避免点号被 OpenAI/Anthropic 拒绝；`plugin.*` 的既有命名不在本次范围。
- [stdout 大行] → 单条消息上限 8 MiB，超限断开，避免第三方 Server 撑爆内存。
- [退出顺序] → `shutdown_on_exit` 放入 Bevy `OnAppExitSystems`，确保读到关窗产生的 `AppExit`；`McpManager` 用 `shutting_down` 标志避免连接中途关闭竞态。
- [stderr 敏感信息] → stderr 默认 `debug` 且单行截断，避免 WARN 落盘泄露凭据。
- [确认弹窗信息不足] → MCP 写操作在确认弹窗只显示截断摘要，参数完整展示留后续 UI 改进。
- [配置即代码执行] → 项目配置默认忽略，需全局显式开启；全局配置仍视为用户可信输入。
- [旧协议兼容] → 只支持 2025-06-18 握手，2026-07-28 Server 暂不能连接；后续加协议协商。
- [工具列表刷新] → 不处理 list_changed，Server 动态增删工具需重启应用。
- [Schema/token 膨胀] → 工具数、schema 与描述均有上限；超出部分跳过并记录。
- [断连工具残留] → 保留注册项会让调用返回错误而非从列表消失；可接受，避免工具列表抖动。
- [无自动重连] → Server 崩溃后需重启应用恢复；首期明确。

## Migration Plan

- 无数据迁移。未创建 `mcp.toml` 时行为与现在完全一致。
- 回滚：删除 `mcp.toml` 或设 `enabled=false` 即停用；代码回滚不涉及状态文件。

## Open Questions

- 2026-07-28 无状态协议与 HTTP 传输的优先级，待后续迭代决定。
- 设置面板 MCP 分区与 3D 空间站映射，待 UI 侧需求明确。
