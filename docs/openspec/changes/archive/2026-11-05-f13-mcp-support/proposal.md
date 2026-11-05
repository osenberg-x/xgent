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
