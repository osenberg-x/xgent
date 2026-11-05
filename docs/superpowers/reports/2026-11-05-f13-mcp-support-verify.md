# F-13 MCP 支持验证报告

- Change: `f13-mcp-support`
- Date: 2026-11-05
- 验证模式：full
- 结论：**通过（0 CRITICAL / 0 IMPORTANT）**，可归档；保留若干已知限制。

## 摘要

| 维度 | 结果 |
|:---|:---|
| Completeness | 10/10 tasks；7 个 Requirement（21 个 Scenario）均有实现 |
| Correctness | 7/7 Requirement 覆盖；核心场景 21/21 有实现与测试证据 |
| Coherence | Design Doc 决策与实现一致；delta spec 已按修复同步 |

## 实际检查

- Build：`cargo check --workspace -j 2` 通过。
- Verify：`CARGO_PROFILE_TEST_DEBUG=0 cargo test -p xgent_settings_core -p xgent_tools -p xgent_agent -p xgent_app -j 2 -- --test-threads=1` 通过：
  - settings 57、tools 162 + mcp_manager 7、agent 79、app 14 + ipc_proto 5。
  - 串行执行用于隔离 `xgent_agent` 既有固定帧并发测试的偶发时序 flake；相关断言已改为墙钟轮询。
- 独立复审两轮：第一轮 3 个 IMPORTANT（退出清理顺序、Windows `npx`、工具名点号）已修复；第二轮修复 diff 复核 0 CRITICAL/0 IMPORTANT。
- `comet classic openspec -- validate f13-mcp-support --strict`：通过。

## 覆盖要点

- 配置：全局/项目 `mcp.toml`、`allow_project_servers=false` 默认忽略项目、同名覆盖、`${VAR}` 缺失跳过、损坏文件降级、非法 server id 跳过。
- 连接：stdio initialize/initialized、初始化失败隔离、`npx` Windows PATHEXT 解析。
- 发现：tools/list 分页（最多 32 页/256 工具）、非法工具跳过、页数/数量上限。
- 注册：`mcp__<server>__<tool>`、名称清洗/截断哈希/冲突去重、schema/描述上限、内置工具不受影响。
- 调用：文本拼接、isError、JSON-RPC 错误、超时、断连、字符串 id、取消通知、输出截断。
- 安全：`Exec` + 默认 `NeedsConfirmation`、`[tool_policy]` 提升有效、Server 只读标注不生效。
- 生命周期：AppExit 使用 `OnAppExitSystems`、后台连接、单点失败隔离、shutdown 竞态清理、精确移除工具、退出杀子进程。
- 集成：`AgentBridge` 每轮从 `ToolExecutor` 读取 schema，后台注册工具下一轮可见。

## 修复记录

- 退出清理顺序：`shutdown_on_exit` 加入 `bevy::app::OnAppExitSystems`。
- Windows：按 `PATH`/`PATHEXT` 解析无扩展名命令（`npx`）。
- 工具 id：改为 provider 安全的 `mcp__<server>__<tool>`，并同步 spec/design/docs/tests。
- 协议健壮性：字符串 JSON-RPC id、pending 清理、8 MiB 行上限、写 stdin 超时、stderr 行上限、tools/list 页数上限、协议版本校验。
- 并发：`McpManager` 增加 `shutting_down`；shutdown 与连接完成竞态下精确移除已注册工具并关闭连接。
- 安全/日志：stderr 默认 `debug`；非 Unicode 环境变量过滤；工具 schema 稳定排序。
- 测试：stderr 洪泛、重复工具、真实调用不写盘、损坏配置、Windows PATHEXT、字符串 id、超长行、页数上限、版本拒绝。

## 已知限制（非阻塞）

- HTTP/SSE、resources/prompts、sampling、设置面板、3D 空间站、自动重连、list_changed 热刷新、配置热重载不在本期。
- 子进程树终止依赖直接子进程；`npx` 派生的 node 子进程需后续 Job Object/进程组处理。
- 确认弹窗对 MCP 写操作只显示截断摘要，完整参数展示留后续 UI 改进。
- 子进程继承父进程环境变量；敏感值应放 `env` 或后续白名单。
- `plugin.*` 既有工具 id 仍使用点号；不在 F-13 范围。
- 2025-06-18 以外的协议版本（含更老版本）当前拒绝连接，后续做版本协商。
- 全量测试默认并行时 `xgent_agent` 个别固定帧用例仍可能时序抖动；Verify 以串行命令留证。

## 复审结论

- 第一轮：0 CRITICAL，3 IMPORTANT，均已修复。
- 第二轮：0 CRITICAL/0 IMPORTANT；修复项正确，剩余为 WARNING/SUGGESTION（记录于上）。
