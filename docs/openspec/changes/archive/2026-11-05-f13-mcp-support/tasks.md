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
