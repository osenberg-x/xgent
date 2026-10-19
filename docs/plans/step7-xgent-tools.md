# Step 7: xgent_tools

## 模块职责

Agent 可调用的工具体系：

1. **工具抽象 trait `Tool`**：统一工具接口（id、schema、tier/approval_for、并发声明、异步可中断执行），对齐 ADR-0007。
2. **内置工具**：ReadFile、WriteFile、SearchFiles、RunCommand（MVP）；Git 系列留 P1。
3. **安全策略分级**：`Approved`（自动执行）/ `NeedsConfirmation`（需确认，MVP 默认）/ `Denied`，由 `resolve_policy` 综合 `tier` + `approval_for` + 配置推导。
4. **执行器**：根据工具调用与安全策略执行；传 `CancellationToken` 支持中断；高危工具经确认流程；`Shared` 工具可并行、`Exclusive` 串行。
5. **确认流程**：`NeedsConfirmation` 工具产生 `ConfirmRequest`，经 `ConfirmCallback` trait 回调（agent 桥接 ECS 事件触发 UI 弹窗），用户决策后执行或拒绝。

## 前置依赖

- xgent_core（ChatEvent::ToolCall、ToolSchema、错误类型）
- xgent_settings_core（ProjectConfig 的 tool_policy）

## 目标文件结构

```
crates/xgent_tools/
├── Cargo.toml
└── src/
    ├── lib.rs              # 模块导出 + 工具注册
    ├── tool.rs             # Tool trait + ToolCtx + ToolResult + SecurityPolicy
    ├── security.rs         # 安全策略判定
    ├── executor.rs         # 执行器：调度工具、处理确认
    ├── builtins/
    │   ├── mod.rs
    │   ├── read_file.rs
    │   ├── write_file.rs
    │   ├── search_files.rs
    │   └── run_command.rs
    └── confirm.rs          # ConfirmRequest/ConfirmResponse 类型
```

## Cargo.toml

```toml
[package]
name = "xgent_tools"
version = "0.1.0"
edition = "2024"

[dependencies]
xgent_core = { path = "../xgent_core" }
xgent_settings_core = { path = "../xgent_settings_core" }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
async-trait = { workspace = true }
thiserror = { workspace = true }
tokio-util = { workspace = true }   # CancellationToken（ADR-0007）
```

说明：MVP 不依赖 Bevy——工具是纯异步逻辑。Bevy 集成（事件、Resource）放 xgent_agent，它把 Tool 调用桥接到 ECS。这样工具可在 daemon 侧未来复用（上移胖后台时）。

## 关键类型与接口

### 1. tool.rs — 抽象 trait

```rust
use async_trait::async_trait;
use serde_json::Value;
use xgent_core::chat::ToolSchema;

/// 工具执行上下文：项目根、工具策略配置等
pub struct ToolCtx {
    pub project_root: std::path::PathBuf,
    pub tool_policy: xgent_settings_core::project::ToolPolicyConfig,
}

/// 工具执行结果
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub output: String,        // 给 LLM 的文本结果
    pub success: bool,
    pub side_effect: Option<SideEffect>,  // 副作用通知（如写文件）
}

#[derive(Debug, Clone)]
pub enum SideEffect {
    FileWritten(std::path::PathBuf),
    CommandRun(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SecurityPolicy {
    Approved,          // 只读，自动执行
    NeedsConfirmation, // 写入/执行，需确认
    Denied,            // 被阻止
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn id(&self) -> &str;
    fn schema(&self) -> ToolSchema;

    /// 工具分层（静态默认）。
    fn tier(&self) -> ToolTier;

    /// 按输入动态决议分层（默认返回 `tier()`）。
    /// 工具可 override 以对特定危险输入返回更高 tier（如 run_command
    /// 检测 `rm -rf` 始终返回 Exec）。`resolve_policy` 用此结果推导策略。
    fn approval_for(&self, _input: &Value) -> ToolTier { self.tier() }

    /// 并发声明（默认 `Shared`）。
    fn concurrency(&self) -> Concurrency { Concurrency::Shared }

    /// 对输入生成人类可读摘要，用于确认弹窗展示。
    fn summarize(&self, input: &Value) -> String;

    /// 异步执行工具。
    /// - `signal`：中断信号，cancel 后工具应尽快返回 `ToolError::Aborted`。
    /// - `on_update`：流式更新回调（MVP 传 `None`）。
    /// - 逻辑失败返回 `Ok(ToolResult { is_error: true, .. })`；
    ///   中断/超时等异常返回 `Err(ToolError::...)`。
    async fn execute(
        &self,
        input: Value,
        ctx: &ToolCtx,
        signal: tokio_util::sync::CancellationToken,
        on_update: Option<&ToolUpdateCallback>,
    ) -> Result<ToolResult, ToolError>;
}
```

伴随的新类型（见 ADR-0007 clean cutover）：

```rust
/// 工具并发声明。Shared=只读可并行，Exclusive=写/执行须串行。
pub enum Concurrency { Shared, Exclusive }

/// 工具分层（供 resolve_policy 推导）。MVP 阶段 Read/Write/Exec 全映射
/// NeedsConfirmation；P1 编辑器引入 UiOnly，默认 Approved（不走确认）。
pub enum ToolTier { Read, Write, Exec, UiOnly }

/// 工具执行错误。Aborted 透传给 agent loop 走 abort 路径；
/// Failed/Timeout 视为异常失败（MVP 下也回灌 LLM）。
/// 逻辑失败（如文件不存在）不抛 ToolError，返回 Ok(ToolResult{is_error:true})。
pub enum ToolError { Failed(String), Aborted, Timeout(u64) }

/// 流式更新回调（长时工具如 run_command 的 stdout 增量）。MVP 传 None。
pub type ToolUpdateCallback = Box<dyn Fn(ToolResult) + Send + Sync>;

/// 工具执行结果。`is_error` 取代旧 `success`（语义反转，对齐 omp）。
pub struct ToolResult {
    pub output: String,            // 给 LLM 的文本结果
    pub is_error: bool,           // 逻辑失败为 true（不抛 ToolError）
    pub side_effect: Option<SideEffect>,
}
```

> 注：旧版 `fn policy() -> SecurityPolicy`、`ToolResult.success`、`execute` 不带 signal/on_update 的签名已按 ADR-0007 clean cutover 删除，不留兼容别名。

### 2. security.rs — 安全策略判定

`resolve_policy` 综合配置覆盖、工具静态 tier 与动态 `approval_for(input)`，得出最终执行策略。

```rust
use crate::tool::{SecurityPolicy, Tool, ToolTier};
use serde_json::Value;
use xgent_settings_core::project::ToolPolicyConfig;

/// 综合判定工具的最终安全策略。
///
/// 决议顺序：
/// 1. `policy.denied` 命中 → [`SecurityPolicy::Denied`]
/// 2. `policy.approved` 命中 → [`SecurityPolicy::Approved`]
/// 3. `tool.approval_for(input)` 动态 tier（MVP 阶段 Read/Write/Exec
///    全映射 [`SecurityPolicy::NeedsConfirmation`]）
/// 4. 兜底 [`SecurityPolicy::NeedsConfirmation`]
pub fn resolve_policy(
    tool_id: &str,
    tier: ToolTier,
    input: &Value,
    tool: &dyn Tool,
    policy: &ToolPolicyConfig,
) -> SecurityPolicy {
    // 1. 配置显式 denied 优先
    if policy.denied.iter().any(|t| t == tool_id) {
        return SecurityPolicy::Denied;
    }
    // 2. 配置显式 approved 次之
    if policy.approved.iter().any(|t| t == tool_id) {
        return SecurityPolicy::Approved;
    }
    // 3. 动态 approval_for（可能比静态 tier 更严格，如 run_command 危险命令）
    let _effective_tier = tool.approval_for(input);
    // 4. MVP 默认：Read/Write/Exec 全映射 NeedsConfirmation
    let _ = tier;
    SecurityPolicy::NeedsConfirmation
}
```

**决议路径说明（4 步顺序）**：

1. **配置 denied 命中 → `Denied`**：`policy.denied` 列表包含 `tool_id` 即拒绝，最高优先级。
2. **配置 approved 命中 → `Approved`**：`policy.approved` 列表包含 `tool_id` 即自动执行（需未被 denied）。
3. **动态 `approval_for(input)`**：调用工具自身的 `approval_for`，允许工具按输入动态收紧 tier（如 `run_command` 对 `rm -rf /` 提升 tier）。MVP 阶段此值仅记录，不改变结果。
4. **兜底 `NeedsConfirmation`**：未命中任何配置时，Read/Write/Exec 全部映射为 `NeedsConfirmation`，需用户确认后执行。

**说明**：MVP 默认所有 tier 均为 `NeedsConfirmation`（包括只读的 ReadFile/SearchFiles）。用户可在项目或全局配置中把常用只读工具提升为 `Approved` 以减少打扰，危险工具可降为 `Denied`。`tier` 参数保留为显式参数，便于未来在 yolo 模式下按 tier 自动批准 Read 工具。这与架构安全模型 11.1 一致。

### 3. confirm.rs — 确认请求/响应

```rust
use serde::{Deserialize, Serialize};

/// 请求用户确认（经 ECS 事件触发 UI 弹窗）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmRequest {
    pub tool_id: String,
    pub input: serde_json::Value,
    pub summary: String,     // 人类可读摘要，如"写入文件 /path/to/x.rs"
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ConfirmDecision {
    Allow,       // 本次允许
    AllowAll,    // 此类工具本次会话全允许（便利特性，可选）
    Deny,        // 拒绝
}

> 确认回调 `ConfirmCallback` trait（`confirm(&self, ConfirmRequest) -> oneshot::Receiver<ConfirmDecision>`）定义在 `executor.rs`，由 agent 桥接层实现：ECS 发事件触发 UI 弹窗，UI 决策后经 oneshot 回传。executor 用 5 分钟超时包裹 `confirm.confirm()` 防止无限挂起。
```

### 4. executor.rs — 执行器

```rust
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use crate::tool::{Tool, ToolCtx, ToolResult, ToolError, SecurityPolicy};
use crate::security::resolve_policy;
use crate::confirm::{ConfirmRequest, ConfirmDecision};
use crate::executor::ConfirmCallback;  // trait 定义在本模块，见下方
// 实际代码中 ConfirmCallback 定义在 executor.rs 内（pub trait），由 agent 桥接实现

pub struct ToolExecutor {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// 会话级 AllowAll 命中集合（用户选 AllowAll 后该 tool_id 不再确认）
    allowed_all: Mutex<HashSet<String>>,
}

impl ToolExecutor {
    pub fn new() -> Self { /* 注册内置工具 */ }

    /// 执行工具调用。
    ///
    /// 流程：
    /// 1. 解析最终策略（配置 denied → approved → tool.approval_for → MVP 默认）；
    /// 2. `Denied` → `Ok(ToolResult{is_error:true})`（逻辑失败回灌 LLM）；
    /// 3. `Approved` 或会话级 AllowAll 命中 → 直接执行；
    /// 4. `NeedsConfirmation` → 经 `confirm` 获取决策，Allow/AllowAll 后执行；
    ///    `Deny` → 同 Denied 逻辑。确认有 5 分钟超时。
    ///
    /// `ToolError::Aborted` 透传给调用方（agent loop 走 abort 路径）。
    /// 工具返回 `Ok(ToolResult{is_error:true})` 时 executor 仍返回 `Ok`
    /// （非异常失败，错误文本回灌 LLM）。
    pub async fn execute(
        &self,
        tool_id: &str,
        input: serde_json::Value,
        ctx: &ToolCtx,
        signal: CancellationToken,
        confirm: &dyn ConfirmCallback,
    ) -> Result<ToolResult, ToolError> {
        let tool = match self.tools.get(tool_id) {
            Some(t) => t,
            None => return Ok(ToolResult {
                output: format!("未知工具: {tool_id}"), is_error: true, side_effect: None,
            }),
        };
        // resolve_policy 传 tool.tier()（动态 approval_for 在其内部调用）
        let policy = resolve_policy(tool_id, tool.tier(), &input, tool.as_ref(), &ctx.tool_policy);
        match policy {
            SecurityPolicy::Denied => Ok(ToolResult {
                output: "工具被策略拒绝".into(), is_error: true, side_effect: None,
            }),
            SecurityPolicy::Approved => tool.execute(input, ctx, signal, None).await,
            SecurityPolicy::NeedsConfirmation => {
                if self.allowed_all.lock().await.contains(tool_id) {
                    return tool.execute(input, ctx, signal, None).await;
                }
                let req = ConfirmRequest {
                    tool_id: tool_id.to_string(),
                    input: input.clone(),
                    summary: tool.summarize(&input),
                };
                let rx = tokio::time::timeout(
                    std::time::Duration::from_secs(300), confirm.confirm(req),
                ).await;
                match rx {
                    Ok(rx) => match rx.await {
                        Ok(ConfirmDecision::Allow) => tool.execute(input, ctx, signal, None).await,
                        Ok(ConfirmDecision::AllowAll) => {
                            self.allowed_all.lock().await.insert(tool_id.to_string());
                            tool.execute(input, ctx, signal, None).await
                        }
                        Ok(ConfirmDecision::Deny) => Ok(ToolResult {
                            output: "用户拒绝".into(), is_error: true, side_effect: None,
                        }),
                        Err(_) => Ok(ToolResult {
                            output: "确认被取消".into(), is_error: true, side_effect: None,
                        }),
                    },
                    Err(_) => Ok(ToolResult {
                        output: "确认请求超时".into(), is_error: true, side_effect: None,
                    }),
                }
            }
        }
    }
}
```

### 5. builtins/ — 内置工具

```rust
// 各内置工具按 ADR-0007 实现 tier()/concurrency()/approval_for()，
// 安全策略由 resolve_policy 综合得出，工具自身不再持有 SecurityPolicy。
// MVP 默认 Read/Write/Exec 全映射 NeedsConfirmation（含只读工具）。

// read_file.rs —— tier=Read, concurrency=Shared
pub struct ReadFile;
#[async_trait]
impl Tool for ReadFile {
    fn id(&self) -> &str { "read_file" }
    fn schema(&self) -> ToolSchema { /* { path: string } */ }
    fn tier(&self) -> ToolTier { ToolTier::Read }
    fn concurrency(&self) -> Concurrency { Concurrency::Shared }
    fn summarize(&self, input: &Value) -> String { /* "读取 {path}" */ }
    async fn execute(
        &self, input: Value, ctx: &ToolCtx,
        signal: CancellationToken, _on_update: Option<&ToolUpdateCallback>,
    ) -> Result<ToolResult, ToolError> {
        let path = input["path"].as_str().unwrap();
        let full = ctx.project_root.join(path);
        // select! 监听 signal：cancel 时返回 ToolError::Aborted
        tokio::select! {
            _ = signal.cancelled() => return Err(ToolError::Aborted),
            r = tokio::fs::read_to_string(&full) => match r {
                Ok(content) => Ok(ToolResult { output: content, is_error: false, side_effect: None }),
                Err(e) => Ok(ToolResult { output: e.to_string(), is_error: true, side_effect: None }),
            }
        }
    }
}

// write_file.rs —— tier=Write, concurrency=Exclusive
//   execute 写文件，返回 side_effect: SideEffect::FileWritten(path)

// search_files.rs —— tier=Read, concurrency=Shared
//   execute 传 signal + on_update（进度推送匹配行增量）

// run_command.rs —— tier=Exec, concurrency=Exclusive
//   approval_for 检测 rm -rf / sudo / mkfs 等危险模式始终返回 Exec；
//   signal 经 tokio::select! 传子进程，cancel 时 kill child 返回 ToolError::Aborted；
//   on_update 推送 stdout 增量。
```

> 内置工具的 `approval_for` 默认实现返回 `tier()`；仅 `run_command` override 以对危险命令收紧 tier。

### 6. lib.rs — 注册

```rust
pub fn default_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(ReadFile),
        Arc::new(WriteFile),
        Arc::new(SearchFiles),
        Arc::new(RunCommand),
    ]
}
```

## 实现要点

1. **不依赖 Bevy**：工具纯异步，Bevy 桥接放 xgent_agent。这样未来上移 daemon 时工具可直接复用。
2. **路径安全**：所有文件工具的路径基于 `project_root` join，校验不越界（防止 `..` 逃逸项目目录）。MVP 做基础校验，后续可加强。
3. **ripgrep 依赖**：SearchFiles 调用系统 `rg`（若存在），否则降级内置简单搜索。不打包 rg，依赖系统安装（或在应用启动时检测并提示）。
4. **RunCommand 沙箱**：MVP 无沙箱，仅靠用户确认。文档警示用户只运行可信命令。未来考虑工作目录限制、超时。
5. **确认流程异步**：`confirm_fn` 返回 oneshot Receiver，UI 侧弹窗后发决策。executor 在等待时不阻塞 tokio runtime。
6. **side_effect**：工具返回副作用信息，agent/UI 侧据此通知 daemon 广播给同项目其他客户端（多客户端文件状态同步）。
7. **AllowAll 便利**：ConfirmDecision::AllowAll 让用户对同类工具本次会话不再确认，提升体验。executor 维护 session 级允许集合。
8. **ToolSchema**：每个工具提供 JSON schema，供 provider 的 tools 参数使用（OpenAI function calling 格式）。

## 验证方法

1. **编译检查**：
   ```bash
   cargo check -p xgent_tools
   ```
2. **工具执行测试**：在临时项目目录测 ReadFile（读存在/不存在文件）、WriteFile（写后读回）、SearchFiles（造几个文件搜匹配）、RunCommand（`echo hello`）。
3. **安全策略测试**：默认所有工具 NeedsConfirmation；配置 approved 提升后自动执行；配置 denied 后拒绝；NeedsConfirmation 工具在 Allow/Deny 决策下分别执行/拒绝。
4. **路径越界测试**：`read_file` 传 `../../etc/passwd`，断言被拒或裁剪到项目内。

## 完成后下一步

xgent_tools 完成后 → 实现 **xgent_context**（项目上下文检索，MVP 方案 A 无索引·按需读取），它依赖 core 类型。
