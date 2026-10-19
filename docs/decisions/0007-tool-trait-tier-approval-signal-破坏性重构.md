# 0007-Tool trait tier+approval+signal 破坏性重构

## 背景

当前 `xgent_tools/src/tool.rs` 的 Tool trait：`fn id()` / `fn schema()` / `fn policy() -> SecurityPolicy` / `fn summarize()` / `async fn execute(&self, input, ctx) -> ToolResult`。

差距（优化文档 O3 §4.1）：无 abort signal（工具不可中断）、无流式更新（长时工具无进度）、无并发声明（全串行）、无 ToolError（错误混在 ToolResult.success 里）、静态 SecurityPolicy（无法按参数决议）。

优化文档 O3 给出新 trait 设计（`tier()` + `approval_for()` + `concurrency()` + `execute(signal, on_update) -> Result<ToolResult, ToolError>`），§4.4 说 SecurityPolicy 保留作运行时决议结果，§12.2 说"一次性完成，不保留旧 trait"。

## 决策

**clean cutover：删除 `fn policy() -> SecurityPolicy`，新增 `fn tier() -> ToolTier` + `fn approval_for(&Value) -> ToolTier` + `fn concurrency() -> Concurrency`，execute 签名改为 `async fn execute(&self, input, ctx, signal, on_update) -> Result<ToolResult, ToolError>`。`SecurityPolicy` 类型保留，作为运行时决议结果（由 `resolve_policy` 从 ToolTier + 配置推导），不再是 trait 方法返回值。**

具体契约：

1. 新增类型：`Concurrency { Shared, Exclusive }` / `ToolTier { Read, Write, Exec }` / `ToolError { Failed, Aborted, Timeout }`。
2. `ToolResult` 调整：`output: String` / `is_error: bool`（取代 `success`，语义对齐 omp 的 isError） / `side_effect: Option<SideEffect>`。
3. `ToolUpdateCallback = Box<dyn Fn(ToolResult) + Send + Sync>`。
4. `Tool::execute` 签名：`async fn execute(&self, input: Value, ctx: &ToolCtx, signal: tokio_util::sync::CancellationToken, on_update: Option<&ToolUpdateCallback>) -> Result<ToolResult, ToolError>`。
5. `resolve_policy(tool_id, tier, input, tool, policy) -> SecurityPolicy`：按"配置 denied → 配置 approved → tool.approval_for(input) 动态 tier → MVP 默认全 NeedsConfirmation"顺序决议。
6. 内置工具 tier/concurrency：ReadFile(Read, Shared) / WriteFile(Write, Exclusive) / SearchFiles(Read, Shared) / RunCommand(Exec, Exclusive)。
7. RunCommand 的 `approval_for` 检测危险模式（`rm -rf` / `sudo` / `mkfs`）始终返回 Exec（即使配置 yolo 也需确认——MVP 暂无 yolo，此 override 逻辑预留）。

## 备选方案

### 方案 B：保留 `fn policy()` 作默认实现，新方法并行

`fn policy() -> SecurityPolicy` 保留（默认 NeedsConfirmation），新增 `fn tier()` + `fn approval_for()` 并行存在。

否决：双轨制使"安全策略来源"模糊——`policy()` 与 `tier()`+`approval_for()` 可能给出冲突决议；调用方不知该信谁。clean cutover 消除歧义。

### 方案 C：MVP 不引入 ToolError，错误仍走 ToolResult.is_error

`execute` 返回 `ToolResult`（非 Result），错误用 `is_error: true` 表达，abort 用特殊 output 字符串。

否决：abort 与"工具逻辑失败"语义不同——abort 时 agent loop 应停止后续工具并退出循环，工具逻辑失败时 agent loop 应把错误文本回灌 LLM 让模型自纠。混在 is_error 里无法区分，agent loop 行为歧义。ToolError::Aborted 让 agent loop 走 abort 路径，ToolError::Failed/Timeout 走错误回灌路径。

## 结论与后果

- **破坏性影响面**：4 个内置工具（`xgent_tools/src/builtins/*.rs`）+ `executor.rs` + `security.rs` + `xgent_agent/src/bridge.rs`（传 CancellationToken）。文档 §12.2 估的"4+1+1"漏了 security.rs，实际 4 工具 + executor + security + bridge。
- **依赖新增**：`xgent_tools/Cargo.toml` 加 `tokio-util = { workspace = true }`（CancellationToken）。
- **abort 传播链**：`AgentCommand::Abort` → `CancellationToken::cancel()` → `stream_llm_response` 的 `tokio::select!` 检测 → `Tool::execute` 的 signal 参数 → 工具内部检查或传子进程。
- **MVP 并发调度简化**：`ToolExecutor` 按 `concurrency()` 调度，Exclusive 工具串行（等前序完成），Shared 工具可并行（tokio::join_all）。MVP 不做 omp 的 lastExclusive 队列复杂度。
- **MVP 默认全 NeedsConfirmation**：`resolve_policy` 的 tier→SecurityPolicy 映射 MVP 阶段 Read/Write/Exec 全映射 NeedsConfirmation。P1 引入 ApprovalMode（always-ask/write/yolo）后，Read 在 yolo 下自动批准。
- **ToolError 的 panic 兜底**：非 ToolError 的 panic/未捕获异常由 agent loop catch 块兜底为 `ToolResult { is_error: true, output: "<panic message>", .. }`，对齐 omp §3.6。
- **此 ADR 取代优化文档 O3/O7 的原方案描述**：O7 已并入 O3，不单独实施。
