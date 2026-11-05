# Brainstorm Summary

- Change: f13-mcp-support
- Date: 2026-11-05

## Confirmed Technical Approach

宿主侧 MCP Client：`xgent_settings_core` 负责全局/项目 `mcp.toml` 解析与合并（项目配置默认被 `allow_project_servers=false` 忽略），`xgent_tools::mcp` 实现 `StdioTransport`、`McpClient`（MCP 2025-06-18 initialize/initialized、tools/list 分页、tools/call）与 `McpTool`，`xgent_app` 启动时后台连接并在退出时关闭。MCP 工具注册为 `mcp.<server>.<tool>`，复用现有 `ToolExecutor`、确认与 `[tool_policy]`。

## Key Tradeoffs and Risks

- 不做 WASM 插件：插件 WIT 的 `run-command` 无法承载 stdio 长连接。
- 不引入 `rmcp`：首期仅需 tools，自定义最小客户端减少依赖/MSRV 风险。
- 项目配置默认忽略，避免打开不可信仓库即执行命令；需全局显式开启。
- 工具固定 `Exec` + `Exclusive`，不信任 Server 只读标注；完整 id 清洗并限长。
- 仅支持 2025-06-18；2026-07-28 无状态规范、HTTP/SSE、resources/prompts、UI/3D 留后续。
- 已知限制：子进程树终止、无自动重连、不处理 list_changed。

## Test Strategy

- `xgent_settings_core`：解析、默认值、项目开关、同名覆盖、变量缺失、损坏文件。
- `xgent_tools`：duplex 假端点覆盖握手/分页/调用/错误/超时/取消/EOF；测试用假 stdio Server 覆盖真实进程启动、注册、失败隔离与退出清理。
- 安全与只读：Exec/默认确认、策略提升、只读标注不生效、项目文件字节不变。

## Spec Patch

None；`allow_project_servers` 行为已在 Open 阶段写入 delta spec。
