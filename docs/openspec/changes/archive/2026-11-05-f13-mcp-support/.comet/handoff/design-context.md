# Comet Design Handoff

- Change: f13-mcp-support
- Phase: design
- Mode: compact
- Context hash: 224c08917ec73e4b7c56d33cd074a90de90066e6720d7f574df05345055ff6ec

Generated-by: comet-handoff.sh
Task hash policy: task-content-v1. Read tasks.md for live completion; excerpts are design-time context.

OpenSpec remains the canonical capability spec. This handoff is a deterministic, source-traceable context pack, not an agent-authored summary.

## docs/openspec/changes/f13-mcp-support/proposal.md

- Source: docs/openspec/changes/f13-mcp-support/proposal.md
- Lines: 1-32
- SHA256: 96adf9e844d00ff42aebf488ed0bf16bf07489f7db03888bce0d55079d375ffb

```md
# Proposal

## Why

XGent 的 Agent 目前只能使用内置工具与插件工具，无法接入 MCP 生态中已有的文件系统、GitHub、浏览器等 Server。F-13 已从 MVP-1 顺延到 P1；插件 WIT 只提供一次性 `run-command`，无法承载 MCP stdio 的长连接双向 JSON-RPC，因此需要宿主侧 MCP Client。

## What Changes

- 新增宿主侧 MCP Client：以 stdio 启动本地 MCP Server，完成 `initialize` 握手、`tools/list`（含分页）与 `tools/call`。
- MCP 工具自动注册进现有 `ToolExecutor`，完整 id 为 `mcp__<server>__<tool>`；LLM 看到其名称、描述与输入 schema。
- 新增 MCP 配置：全局 `~/.xgent/agent/mcp.toml` 与项目 `.xgent/mcp.toml`，项目按 server id 覆盖/追加全局项；全局 `allow_project_servers` 默认 `false`，关闭时完全忽略项目配置，避免打开不可信仓库即执行其中命令。支持 `${ENV_VAR}` 展开、`enabled` 与 `auto_start`。
- 安全：MCP 工具统一按 `Exec` 层处理，默认 `NeedsConfirmation`；用户可在 `[tool_policy]` 按完整工具 id 提升。不信任 Server 自报的只读标注。
- 生命周期：应用启动后异步连接 `auto_start` Server；单个 Server 失败不阻塞应用或其他 Server；退出时关闭子进程；断连后的工具调用返回可读错误。
- 不在本期：HTTP/SSE 传输、resources、prompts、sampling、OAuth、设置面板 MCP 分区、3D 空间站、自动重连、多客户端共享连接。

## Capabilities

### New Capabilities

- `mcp-client`: MCP Server 配置合并、stdio 连接与工具发现、工具注册/调用、安全默认与失败隔离。

### Modified Capabilities

（无。现有工具执行与安全策略的契约不变，MCP 工具沿用同一 `Tool` trait。）

## Impact

- `crates/xgent_settings_core`：新增 MCP 配置类型与 `mcp.toml` 读写/合并。
- `crates/xgent_tools`：实现 `McpTransport` 的 stdio 版本、MCP 客户端协议层、`McpTool` 适配器；无新增第三方依赖（复用 tokio/serde_json）。
- `crates/xgent_app`：启动时加载配置、连接 Server、注册工具，退出时清理；日志记录连接与工具数量。
- 测试：配置解析/合并、协议握手与分页、工具注册与调用、失败隔离、确认策略、只读不写盘；使用测试用假 MCP Server。
- 文档：`docs/dev-tutorial.md`、`docs/design/architecture.md`、`AGENTS.md` 同步。

```

## docs/openspec/changes/f13-mcp-support/design.md

- Source: docs/openspec/changes/f13-mcp-support/design.md
- Lines: 1-112
- SHA256: 6ee767cc62e7504d731f544b322bedde9385c5cbb3eb7c151c85ace401badee0

[TRUNCATED]

```md
---
comet_change: f13-mcp-support
role: technical-design
canonical_spec: openspec
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


```

Full source: docs/openspec/changes/f13-mcp-support/design.md

## docs/openspec/changes/f13-mcp-support/tasks.md

- Source: docs/openspec/changes/f13-mcp-support/tasks.md
- Lines: 1-28
- SHA256: e8195bd4804ad9188da580534b4656effa79aeb598ca82c6b825cbbdc2cea495

```md
# Tasks

## 1. MCP 配置层

- [x] 1.1 在 `xgent_settings_core` 增加 MCP 配置类型、全局/项目 `mcp.toml` 路径、解析与合并、`allow_project_servers`（默认 false）与 `${VAR}` 展开；验证 `cargo test -p xgent_settings_core` 覆盖默认忽略项目、同名覆盖、变量缺失跳过、损坏文件不阻止启动。 <!-- comet-task:c066666e-89fb-483d-80ed-9fbd21261f4a -->
- [x] 1.2 在 `docs/dev-tutorial.md` 增加 MCP 配置字段与项目信任开关说明；验证文档示例可被 1.1 的解析测试原样读取。 <!-- comet-task:a594a80c-5803-448f-9ee6-0fe1d88c0517 -->

## 2. stdio 传输与协议

- [x] 2.1 在 `xgent_tools::mcp` 实现 `StdioTransport`：spawn 子进程、换行 JSON-RPC、pending 请求路由、请求超时、stderr 截断日志、EOF 清理与 `close`；验证 duplex/假端点测试覆盖响应、超时、EOF 与 stderr 不阻塞。 <!-- comet-task:253e40e4-d00a-4a2a-b545-f4c169f3231f -->
- [x] 2.2 实现 `McpClient`：`initialize`/`initialized`、`tools/list` 分页、`tools/call`、JSON-RPC 错误、`ping` 与未知服务端请求；验证假端点测试覆盖握手、分页、错误与取消。 <!-- comet-task:6516c4ed-2886-45e0-9ede-2a68be70e1a6 -->
- [x] 2.3 实现 `McpTool`：完整 id 清洗与长度上限、schema/描述上限、`Exec` + `Exclusive`、`summarize`、文本结果拼接/截断、`isError`、取消通知；验证单元测试与默认 `NeedsConfirmation`、策略提升测试。 <!-- comet-task:78a1eb79-33bd-4ad3-8c2b-598fc1aec602 -->

## 3. 管理器与接线

- [x] 3.1 实现 `McpManager`：并发连接、工具注册、单 server 失败隔离、断连保留、`shutdown`；新增测试用假 MCP Server 二进制；验证真实 stdio 集成测试覆盖启动/注册/单 server 失败/退出清理。 <!-- comet-task:f97d6aeb-b6e2-4e38-ac3d-986819733cd7 -->
- [x] 3.2 在 `xgent_app` 启动时加载配置、启动 manager、持有 Bevy Resource、退出时 shutdown；验证 app 级测试或启动日志/工具注册断言。 <!-- comet-task:29a1a4bc-4a5d-4aab-a594-86fe893bdba1 -->
- [x] 3.3 同步 `docs/design/architecture.md` 与 `docs/dev-tutorial.md` 的 MCP 接线、限制与测试命令；验证文档路径/测试名与实现一致。 <!-- comet-task:b359ae7d-7f55-455f-b322-b894ff6c5e82 -->

## 4. 集成与回归

- [x] 4.1 运行相关 crate 回归与 workspace check；验证 `cargo test -p xgent_settings_core -p xgent_tools -p xgent_app` 与 `cargo check --workspace` 通过。 <!-- comet-task:9d782aa0-2579-4c03-b8d0-93be946a7322 -->
- [x] 4.2 安全与只读回归：MCP 工具默认确认、`[tool_policy]` 提升生效、Server 只读标注不改变策略；连接/发现/调用前后项目文件字节不变；验证对应测试。 <!-- comet-task:7457ca30-7b06-4085-94e8-14c571838226 -->

## Workflow follow-up

- 归档前按项目评审要求完成最终验证。
- 归档后按用户选择的交付方式合并/推送；未获授权前不推送。

```

## docs/openspec/changes/f13-mcp-support/.openspec.yaml

- Source: docs/openspec/changes/f13-mcp-support/.openspec.yaml
- Lines: 1-2
- SHA256: 3e16994612e657a7bbaefe38b70ef46f3e8e9f382f092dcaceb60ebabe7a5da0

```md
schema: spec-driven
created: 2026-11-05

```

## docs/openspec/changes/f13-mcp-support/specs/mcp-client/spec.md

- Source: docs/openspec/changes/f13-mcp-support/specs/mcp-client/spec.md
- Lines: 1-119
- SHA256: 5736ccbcd727f8b5b8555ff24a9b4d6ef31648bccc4c87d03d32f10a099f102c

[TRUNCATED]

```md
# Spec Delta

## Purpose

让 XGent 作为 MCP Client 连接本地 stdio Server，把 Server 暴露的工具自动注册给 Agent，并沿用现有确认、执行与错误处理链路。

## ADDED Requirements

### Requirement: 配置来源与合并

系统 SHALL 从全局 `~/.xgent/agent/mcp.toml` 与项目 `.xgent/mcp.toml` 读取 `[[servers]]`；全局 `allow_project_servers` 默认 `false`，关闭时忽略项目文件。开启后项目同名 server 覆盖全局项，其余按 id 追加；`enabled = false` 的 server 不连接；字符串中的 `${ENV_VAR}` 在连接前展开，变量缺失时跳过该 server 并记录警告。

#### Scenario: 默认忽略项目配置
- **WHEN** 全局 `allow_project_servers` 未设置或为 `false`，项目存在 `.xgent/mcp.toml`
- **THEN** 只连接全局 server，项目文件中的 server 与覆盖均不生效

#### Scenario: 全局与项目合并
- **WHEN** `allow_project_servers = true`，全局与项目都定义 server，其中同名项参数不同
- **THEN** 同名项以项目配置为准，其他全局项保留，最终每个 id 只连接一次

#### Scenario: 环境变量展开
- **WHEN** `env` 或参数中包含 `${TOKEN}` 且环境变量存在
- **THEN** 启动子进程前替换为实际值；变量缺失时该 server 被跳过并记录警告

#### Scenario: 停用与损坏配置
- **WHEN** server 设置 `enabled = false`，或 `mcp.toml` 无法解析
- **THEN** 对应 server 不启动；解析失败不阻止应用启动与其他 server 连接

### Requirement: stdio 启动与初始化

系统 SHALL 为启用的 auto_start server 启动子进程并通过 stdin/stdout 换行分隔 JSON-RPC 通信；启动后发送 `initialize`（含协议版本与 clientInfo），成功后发送 `notifications/initialized`；初始化超时或进程提前退出时关闭该连接并记录错误。

#### Scenario: 成功握手
- **WHEN** server 正常响应 initialize
- **THEN** 客户端发送 initialized 通知，并进入工具发现阶段

#### Scenario: 初始化失败隔离
- **WHEN** 某个 server 启动失败、initialize 超时或进程提前退出
- **THEN** 该连接标记失败，其他 server 与应用启动继续

### Requirement: 工具发现与分页

系统 SHALL 在初始化后调用 `tools/list` 并跟随 `nextCursor`，最多 32 页或 256 个工具（先到者为准）；每个工具必须有 `name` 与对象型 `inputSchema`；缺失或非法项跳过并记录警告；发现结果按稳定顺序返回。

#### Scenario: 分页汇总
- **WHEN** server 将工具分多页返回且总页数不超过 32 页
- **THEN** 客户端汇总所有页，不重复、不丢失

#### Scenario: 非法工具跳过
- **WHEN** 某工具缺少 name 或 inputSchema
- **THEN** 跳过该项，其他合法工具仍被注册

#### Scenario: 数量上限
- **WHEN** server 返回的工具数超过 256 个，或分页超过 32 页
- **THEN** 只保留上限内的稳定结果，并记录警告

### Requirement: 工具注册

系统 SHALL 将发现的工具注册为 `mcp__<server>__<tool>`，向 LLM 暴露工具名称、描述与输入 schema；工具名清洗为 `[A-Za-z0-9_-]`，超长时截断并附加稳定哈希；server id 非法、清洗后为空或完整 id 冲突时跳过并记录警告；注册失败 MUST NOT 影响内置工具。

#### Scenario: 注册后可见
- **WHEN** server 暴露合法工具
- **THEN** 执行器注册对应完整 id，后续请求的工具列表包含其 schema

#### Scenario: 冲突与非法名称
- **WHEN** 两个工具清洗后映射到相同完整 id，或清洗后名称为空
- **THEN** 冲突项被跳过并记录警告，内置工具保持可用

### Requirement: 工具调用

系统 SHALL 把 Agent 的工具调用转为 `tools/call`，参数为 JSON 对象；返回内容中的文本块拼接为工具结果；MCP `isError`、JSON-RPC 错误、超时与断连 MUST 转为可读错误而不 panic；调用被中断时向 server 发送取消通知并返回中断。

#### Scenario: 文本结果
- **WHEN** server 返回一个或多个文本内容块
- **THEN** 客户端按顺序拼接文本作为工具结果

#### Scenario: 逻辑错误
- **WHEN** server 返回 `isError: true` 或 JSON-RPC 错误
- **THEN** 工具结果标记为错误并包含可读原因，Agent 循环继续


```

Full source: docs/openspec/changes/f13-mcp-support/specs/mcp-client/spec.md
