# 0012-终端独立 crate-PTY-抽象 trait 决策

## 背景

F-??（终端）需在 UI 侧引入用户交互式 PTY（见 ADR-0011 选 `portable-pty`）。PTY 进程归属经 grilling 决议为 **UI 侧**（对齐 AGENTS.md §5.1 进程模型：交互类在 UI 侧、daemon 瘦后台）。本 ADR 回答下一个问题：PTY 抽象 + 实现放哪个 crate。

候选三选：
- (a) `xgent_core`——跨进程共享类型层。否决：PTY 是 UI 侧实现细节，污染跨进程协议层。
- (b) `xgent_ui::terminal` 模块——trait 与实现都在 UI 业务层。被否决（见下）。
- (c) 新建独立 crate `xgent_terminal`——PTY 抽象 trait + 本地实现，`xgent_ui` 仅依赖 trait/类型，不依赖 `portable-pty` 实现细节。**采纳**。

## 决策

**新建独立 crate `xgent_terminal`（lib，不依赖 Bevy），承载 PTY 抽象 trait + `portable-pty` 本地实现；`xgent_ui` 仅依赖 trait与类型，实现可由 `xgent_app` 注入或 `xgent_ui` 默认带入。**

具体契约：

1. `xgent_terminal` 形态对齐 `xgent_context` / `xgent_tools`：不依赖 Bevy，依赖 `xgent_core` + `xgent_settings_core` + `tokio` + `async-trait` + `thiserror` + `portable-pty`。
2. `TerminalBackend` trait（异步，`async-trait`）：`spawn(shell, cwd, cols, rows) -> Result<TerminalId>` / `read(TerminalId) -> async 流/channel` / `write(TerminalId, bytes)` / `resize(TerminalId, cols, rows)` / `kill(TerminalId)`。抽象 `portable-pty` 的同步 API，内部用 `tokio::task::spawn_blocking` + mpsc channel 桥接。
3. MVP 唯一实现 `LocalPtyBackend`（`portable-pty`）。将来 Web 端 / 多窗口共享场景需上移 daemon 时，新增 `DaemonPtyBackend`（走 JSON-RPC），调用方（`xgent_ui::terminal` ECS 系统）不改——对齐 AGENTS.md §5.1 "可上移职责用 trait 抽象，切换不破坏调用方"。
4. PTY 生命周期跟随终端 tab Entity：tab 销毁 → `kill(TerminalId)` + 释放资源。
5. 多 tab 数 MVP 不硬限，受 UI 渲染约束（与编辑器多标签一致）。每个 tab 一个独立 PTY 会话。

## 备选方案

### 方案 B：放 `xgent_ui::terminal` 模块（trait 与实现都在 UI 层）

否决：`xgent_ui` 依赖 Bevy，若 PTY 实现也放这里，`portable-pty` 会与 Bevy 同 crate。PTY 本质是纯异步 IO（不依赖 Bevy），放 UI 层让 UI 直接碰 `portable-pty` 实现细节，未来换库或上移 daemon 时 `xgent_ui` 要改动。独立 crate 让 `xgent_ui` 只见 trait，符合既有"不依赖 Bevy 的纯逻辑层独立成 crate"模式（`xgent_tools`/`xgent_context` 同构），实现替换零波及 UI。

### 方案 D：trait 放 `xgent_core`、实现在 `xgent_terminal`

否决：`xgent_core` 是跨进程协议类型层（错误、JSON-RPC、chat 事件、文件/配置事件、ID），PTY 操作是 UI 侧实现细节、不跨进程（MVP UI 侧）。把 `TerminalBackend` trait 放 core 会让"UI 侧实现 trait"的细节渗入跨进程协议层，污染领域边界。对比 `EditorState` trait 放 `xgent_core`（因 `xgent_context` 经 trait 查询、跨 crate 共享且有反转依赖需求）——终端 trait 无跨 crate 反转依赖消费者，不需放 core。

## 结论与后果

- **crate 拓扑增量**：AGENTS.md §4 crate 表新增 `xgent_terminal`（lib，依赖 `xgent_core` + `settings_core` + `portable-pty`，不依赖 Bevy）。依赖关系图加 `xgent_terminal ← xgent_ui`。`doc/dev-tutorial.md` crate 拓扑章节同步更新。
- **依赖增量**：`xgent_terminal` 加 `portable-pty`；`xgent_ui` 加 `xgent_terminal`（path 依赖）。`portable-pty` 不进 `xgent_ui` Cargo.toml——UI 层只见 trait。
- **实现注入点**：MVP `xgent_ui` 默认注册 `LocalPtyBackend`（`from_ref` 或启动系统注入）；`xgent_app` 若要覆盖可在插件构建时注入。对齐 `xgent_settings::Localizer` 注入 `StringSource` 的既有模式。
- **领域语言**：`TerminalBackend` 是实现抽象，不进 CONTEXT.md（CONTEXT 只记领域语言不记实现）；终端的领域定义（用户人机交互终端、双路径）已在 CONTEXT.md 钉死。`TermView`（UI 视图）是布局术语，已在 CONTEXT.md「布局」小节。
