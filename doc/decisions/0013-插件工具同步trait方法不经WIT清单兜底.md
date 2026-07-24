# 0013-插件工具同步 trait 方法不经 WIT 清单兜底

## 状态

accepted（grilling Round 1/2 裁决已确认）
## 背景

`Tool` trait（`crates/xgent_tools/src/tool.rs`，ADR-0007 锁定签名）有两类方法：

- **async**：`preview_diff` / `execute` —— 在 `ToolExecutor::execute`（async，`agent_loop_task` tokio task 内）调，见 `executor.rs:108/131/135`。
- **同步**：`id` / `schema` / `tier` / `concurrency` / `approval_for(&Value) -> ToolTier` / `summarize(&Value) -> String`。

插件系统设计文档（`doc/design/plugin-system-design.md` §5.2/§5.3）的 `PluginTool` 适配器声称：`approval_for` 与 `summarize` 经 WIT `tool.approval-for` / `tool.summarize` "同步调用，在当前 tokio task 内直调"。

## 矛盾

WIT 经 `wit-bindgen` 生成、在 wasmtime **async store**（`config.async_support(true)`，设计文档 §4.2）下调用的绑定是 **async 函数**。wasmtime v40 起 WIT async 是**全有或全无**——所有 WIT 函数要么全 async、要么全 sync，不能混用（wasmtime v39→v40 migration，bytecodealliance/wasmtime#12226）。`execute` 必须 async（长任务可中断），则 `approval_for`/`summarize` 若进 WIT 也被迫 async——async store 下不存在"同步版 WIT 函数"。

`approval_for` / `summarize` 是 `Tool` trait 的**同步**方法（非 async，ADR-0007 锁定）。同步方法体内无法 `.await` 一个 async WIT 调用——即使在 `ToolExecutor::execute`（async，`agent_loop_task` tokio task 内）被调，方法签名本身是同步，不能 await。四条出路：

1. **`approval_for`/`summarize` 改 async**：破坏 ADR-0007 clean cutover，`resolve_policy`（`security.rs:23`，调 `approval_for`）与 `ToolExecutor::execute`（`executor.rs:88`，调 `resolve_policy` + `executor.rs:115` 调 `summarize`）全连锁改 async，4 内置工具 + `EditorTool` + `executor` + `security` + `bridge` 全改。
2. **双 Engine（同步 + async）**：为 `approval_for`/`summarize` 单开 `async_support(false)` 的第二个 Engine/Store。与 §4.2 单 Engine 冲突，且 v40 的全有或全无规则使同 WIT 接口不能跨 Engine 混用 sync/async 绑定。
3. **不经 WIT，清单兜底**：WIT 删 `summarize`/`approval-for`，`approval_for` 回退 `self.tier()`（ADR-0007 默认实现即此，`tool.rs:152-154`），`summarize` 回退清单 `[tools].definitions[].description`。插件不实现这两个方法。
4. **独立同步 channel 阻塞**：`mpsc` 发到另一线程同步执行——过度工程，YAGNI。

## 决策

**选 (3)：WIT `tool` 接口删除 `summarize` 与 `approval-for`，`PluginTool::approval_for` 回退 `self.tier()`，`PluginTool::summarize` 回退清单 `description`。** 设计文档 §5.2 的 WIT `interface tool` 块删掉 `summarize` 与 `approval-for` 两行；§5.3 的 `PluginTool` impl 删掉这两个方法的 WIT 调用，改为清单兜底。

## 后果

- **能力降级**：插件工具丢失"按输入动态收紧 tier"能力——内置 `RunCommand.approval_for` 检测 `rm -rf`/`sudo`/`mkfs` 返回 `Exec`（`run_command.rs:276/281/286`，ADR-0007 §7）的能力，插件版做不到。MVP 插件为 `git_diff`/`git_log`（只读，无危险输入需动态收紧），此降级可接受。
- **`summarize` 退化为例行 description**：确认弹窗对插件工具显示清单 description（静态，不按输入定制）。内置工具仍按输入定制（`run_command.rs:268` 测试）。
- **WIT 接口简化**：`tool` 接口仅 `register` + `execute` + `preview-diff`，契约更小，版本管理负担轻。
- **线程模型（Round 2 Q2.2 裁决）**：wasmtime async future 是 `!Send`，不能在多线程 tokio task 直接 spawn。WASM 调用跑在专用 `LocalSet` task，经 `mpsc::UnboundedSender<PluginCall>` channel（对齐 §4.2 已有的序列化设计）与 `agent_loop_task` 通信——同一插件的调用串行、不同插件并行，`LocalSet` task 消费 channel 自然满足。`agent_loop_task` 不直接 await WASM future，经 channel 解耦 `!Send` 约束。
- **ContextProvider 运行时替换（Round 1 Q1.4 裁决）**：`AgentBridgeConfig.context`（`bridge.rs:252`，原 `Arc<dyn ContextProvider>` 构造时固定）改 `Arc<RwLock<Box<dyn ContextProvider>>>`，插件加载/卸载时 swap。对齐工具/命令的运行时增删能力——context 此前是唯一不支持运行时替换的扩展点。


## 备选方案

### 方案 2：双 Engine

否决理由：单 Engine 是 §4.2 既定设计（资源效率、Store 共享），双 Engine 破坏此约束且增复杂度；同步 store 与 async store 的 WIT 绑定需分别生成，插件作者心智负担大。若未来 P1 真需动态 tier，可作方案 2 的子集回退（仅为 approval_for 开同步 Engine），MVP 不预支。

### 方案 1：approval_for/summarize 改 async

否决理由：clean cutover 反面——ADR-0007 明确"一次性完成，不保留旧 trait"，把同步方法改 async 是反向破坏，且 `resolve_policy` 在 ECS 同步侧的调用点（未来 yolo 模式下可能 ECS 内调）无法 await。
