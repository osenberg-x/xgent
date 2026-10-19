# 0011-用户终端-PTY-选 portable-pty 决策

## 背景

F-??（终端）引入嵌入式用户终端（`TermView`，`SideView` 三种互斥子视图之一）。用户在此手动敲 shell 命令、查看彩色输出。终端需要真正的伪终端（PTY）而非管道（pipe）：PTY 让子进程 `isatty()` 返回 true，CLI（cargo / git / ripgrep）才会输出彩色、支持交互式 stdin、响应窗口 resize（`SIGWINCH` / Win ConPTY）；pipe 方案下这些全部退化。

项目既有 `xgent_tools::RunCommand` 工具用 `tokio::process::Command`（pipe，`read_to_end` 一次性取全量输出），是"跑完拿结果"的批处理模型，与用户终端的"交互式 PTY"模型底层机制完全不同——见 CONTEXT.md「终端 vs RunCommand 工具（双路径）」：两者进程互不相干、UI 互不复用，故终端不复用 RunCommand 的 pipe 实现，需独立选 PTY 库。

## 决策

**用户终端 PTY 库选 [`portable-pty`](https://crates.io/crates/portable-pty)（wezterm 团队维护）。**

具体契约：

1. 跨平台（Windows ConPTY / Unix pty），MIT 协议，wezterm 生产环境背书，活跃维护。
2. 同步 API：PTY 读写为阻塞调用，用 `tokio::task::spawn_blocking` 包到独立线程，经 mpsc channel 桥接到 ECS（复用项目既有异步桥接模式，见 AGENTS.md §5.3、`xgent_agent` 的 tokio task + channel 回 ECS）。
3. 默认 shell：Windows `powershell.exe`；Unix 从 `$SHELL` 取、fallback `sh`。MVP 不在 settings 暴露 `terminal.shell` 配置项（留后续）。
4. 初始工作目录 = `ProjectRoot` resource（项目根），与 `RunCommand` 工具、编辑器、agent workspace 一致。
5. PTY 进程归属用户终端 tab，不走工具系统、不经 `resolve_policy`、不经 NeedsConfirmation——属"用户主动行为"（对齐「用户保存」vs `WriteFile` 工具的既有二分）。

## 备选方案

### 方案 B：用 `tokio::process::Command`（pipe）凑合

否决：pipe 下子进程 `isatty()` false，`cargo build` 失去彩色、无交互式 stdin、无窗口 resize 响应。原型图 `tv-body` 的彩色日志（`tv-out`/`tv-err`/`tv-ok`）将退化，`cargo` 这类按 isatty 切换行为的 CLI 体验降级严重。原型图明确要求彩色输出，pipe 方案不满足。

### 方案 C：自建 PTY 层（Unix pty + Windows ConPTY 各写一套）

否决：重复造轮子。Windows ConPTY API 复杂（`CreatePseudoConsole` / pipe 传参 / `ResizePseudoConsole` / 进程组生命周期），Unix pty 又是另一套（`openpty` / `forkpty` / `SIGWINCH`）。自建两套换取的收益（去一个依赖）远不抵维护成本与 bug 风险。`portable-pty` 已封装这两套差异且经 wezterm 长期验证。

### 方案 D：仅 Windows，用 `conpty` crate 直接调 ConPTY

否决：放弃跨平台。AGENTS.md §1 明确"面向个人开发者桌面端"，项目虽 Windows 优先但目标是跨平台工具（架构文档多平台 IPC 已落地：Unix socket / Windows named pipe）。单平台 PTY 库会让后续 macOS/Linux 移植重写终端层。

## 结论与后果

- **依赖增量**：`crates/xgent_ui`（或终端所在 crate）`Cargo.toml` 加 `portable-pty`。`portable-pty` 是同步库，不进 tokio async 上下文，需 `spawn_blocking` 隔离。
- **异步桥接**：PTY 读循环跑在 `spawn_blocking` task，输出经 mpsc channel 送回 ECS 系统；ECS 系统（用户输入、中断信号、resize）经另一 channel 下发到 task。对齐 `xgent_agent` 的桥接模式，不引入新并发原语。
- **PTY 生命周期**：跟随终端 tab——tab 关闭则 PTY kill + 释放；daemon 退出不影响（PTY 在 UI 侧，见架构文档进程模型，MVP 工具执行/交互在 UI 侧）。
- **MVP 不做**：`terminal.shell` / `terminal.cwd` / `terminal.args` 等 settings 配置项；终端 tab 持久化与跨会话恢复；多窗口共享终端。
- **领域语言**：见 CONTEXT.md「终端（F-??，P?）」小节——PTY 是终端的实现细节，不进 CONTEXT.md（CONTEXT 只记语言不记实现）；终端的领域定义（用户人机交互终端、双路径）已在 CONTEXT.md 钉死。
