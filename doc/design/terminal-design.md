# XGent 内置终端设计文档

> 状态：落地 v1（crate + UI 层已实现，`cargo check --workspace` 通过）
>
> 范围：F-19 内置终端（P1）。基于 grill 会话决策落地，覆盖 UI 界面、架构边界、数据流、状态机、与编辑器/SideView 的复用关系、安全模型。领域语言见 `CONTEXT.md` "终端（F-19，P1）" 小节。
>
> 与既有文档关系：本文档不重写 `ui-design.md` / `architecture.md` / `requirements.md`，只补充终端引入后的增量变更，并在文末列出对三份文档的具体修订点。终端与编辑器（F-11）复用 `SideView` 容器，故本文档同时修订 `editor-design.md` §2.1 的布局描述（原"编辑器在对话主区标签条"作废，改为"编辑器/预览/终端都是 SideView 互斥子视图"）。

---

## 1. 设计决策摘要（grill 结论）

| 维度 | 决策 | 关键约束 |
|:---|:---|:---|
| 领域定义 | 纯用户人机交互终端，agent 不碰 | 与 `RunCommand` 工具双路径隔离，对齐「用户保存」vs `WriteFile` 既有二分 |
| 布局复用 | `SideView` 四变体互斥（Editor/Preview/Terminal/None） | `EditorView` 枚举废弃，全用 `SideViewContent` 单一状态源 |
| PTY 库 | `portable-pty`（wezterm 维护） | 同步 API + `spawn_blocking` + channel 桥接 ECS |
| 进程归属 | UI 侧（每窗口一个 PTY 集合） | 对齐 AGENTS.md §5.1 交互类在 UI 侧；daemon 不参与 |
| crate 拓扑 | 新建独立 crate `xgent_terminal`（不依赖 Bevy） | 对齐 `xgent_tools`/`xgent_context` 纯逻辑层模式；UI 层只见 trait |
| 实现抽象 | `TerminalBackend` trait，MVP `LocalPtyBackend` | 将来 Web/多窗口共享时换 `DaemonPtyBackend`，调用方不改 |
| 行编辑 | UI 侧行编辑，回车发送整行 | PTY raw 模式 + echo off；控制字符即时发；不做历史/补全 |
| 渲染 | `vte` 解析 ANSI + 行模型 + 虚拟滚动 | 非屏幕字符网格；不支持全屏 TUI（vim/top） |
| Shell | Win powershell、Unix `$SHELL`、fallback sh | 不在 settings 暴露配置（留后续） |
| cwd | 项目根（`ProjectRoot`） | 允许 `cd` 越界，不告警 |
| 安全 | 无命令拦截、不脱敏、不进 agent 上下文 | 用户自主负责，对齐「用户主动行为默认信任」 |
| 多 tab | 不限上限，PTY 退出后 tab 保留 | 窗口关 = 全 kill；最后 tab 关 = 收起 SideView |
| 焦点 | 终端激活吞键，全局键仍生效 | Ctrl+`/Ctrl+\ /Ctrl+Shift+P /Ctrl+B 不被吞 |
| F 编号 | F-19，P1 | 需求文档新增 |

---

## 2. UI 界面

### 2.1 布局（SideView 四变体互斥）

`SideView` 是右侧分屏容器（`SideViewMarker`），默认收起（`display:none`）；展开时与对话主区（`ChatPanelMarker`）并排各占一半（均 `flex:1`）。内部承载三种**互斥**子视图，由 `SideViewContent` 四变体状态机切换：

```
┌──────────────────────────────────────────────────────────┐
│ 顶栏（高 40px）                                           │
│  项目名 · provider 标签 · 新建会话 · 🖥终端 · ⚙设置      │
├────────────┬─────────────────────────────────────────────┤
│            │                                             │
│ 文件面板    │   对话主区（flex:1）                        │
│（宽 240px）│   ┌─────────────────────────────────────┐   │
│ 可折叠      │   │  消息列表 + 工具卡片 + 输入框       │   │
│            │   │                                     │   │
│            │   └─────────────────────────────────────┘   │
│            │┌──────────────────────────────────────┐    │
│            ││  SideView（flex:1，互斥四选一）       │    │
│            ││  ┌─ EditorView（编辑器多标签）       │    │
│            ││  ├─ FileView（文件只读预览）          │    │
│            ││  └─ TermView（终端多 tab）  ← 本文档  │    │
│            │└──────────────────────────────────────┘    │
├────────────┴─────────────────────────────────────────────┤
│ 状态栏（高 24px）                                         │
└──────────────────────────────────────────────────────────┘
```

`SideViewContent` 枚举（扩自 `editor` 模块原 `EditorView`/`SideViewContent`，合并单一状态源）：

```rust
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SideViewContent {
    #[default]
    None,       // 分屏收起，对话独占
    Editor,     // 编辑器视图（代码文件）
    Preview,    // 文件预览（非代码文件）
    Terminal,   // 终端视图
}
```

切换规则（统一由 `apply_side_view_visibility` 系统据 `SideViewContent` 写各 Marker 节点显隐，避免 B0001）：

| 触发 | `SideViewContent` | `SideViewCollapsed` |
|:---|:---|:---|
| 默认 | `None` | `true`（收起） |
| 点击代码文件 | `Editor` | `false`（展开） |
| 点击非代码文件 | `Preview` | `false` |
| `Ctrl+\`（已展开且 Editor → 收起；否则切 Editor） | 翻转 | 据内容 |
| `Ctrl+``（已展开且 Terminal → 收起；否则切 Terminal） | 翻转 | 据内容 |
| `Ctrl+Shift+E` | `Editor` | `false` |
| `Ctrl+Shift+D` | `None` | `true` |
| 关闭最后一个 editor tab | `None` | `true` |
| 关闭最后一个 terminal tab | `None` | `true` |

### 2.2 终端视图布局

```
┌─────────────────────────────────────────────────────────┐
│ 🖥 终端  [●cargo build] [●shell #2] [＋]    ＋ 清屏 ✕  │ ← tv-head
├─────────────────────────────────────────────────────────┤
│ ~/ws/xgent on main                                      │
│ ❯ cargo build --release 2>&1                            │ ← tv-body（历史）
│    Compiling xgent_core v0.1.0                          │
│    ...                                                  │
│ ✓ build 成功                                            │
│ ❯ cargo test -p xgent_agent▌                           │ ← tv-inputline（行编辑器）
├─────────────────────────────────────────────────────────┤
│ ● cargo test 运行中 · shell: zsh · cwd: ~/ws/xgent      │ ← tv-statusbar
│              [⏹ 中断] [↵ 发送] [⨯ 终止]                 │
└─────────────────────────────────────────────────────────┘
```

| 元素 | 尺寸 / 行为 |
|:---|:---|
| tv-head | 高 32px，左标题 + tab 区 + 右动作（新建/清屏/关闭分屏） |
| tv-tabs | tab 宽自适应，活跃 tab 高亮边框；`dot` 绿=运行、蓝=忙碌、灰=已退出 |
| tv-body | `flex:1`，等宽字体，虚拟滚动（复用 `xui::VirtualList` 思路），历史上限 10k 行超丢头部 |
| tv-inputline | 行编辑器：`❯` prompt + typed 文本 + 光标块；回车固化进 tv-body 历史 + 发 PTY |
| tv-statusbar | 高 24px，运行状态 + shell + cwd + exit code + 动作按钮（中断/发送/终止） |

### 2.3 终端视图快捷键

| 快捷键 | 动作 |
|:---|:---|
| `Ctrl+`` | 切换终端视图（已展开且 Terminal → 收起；否则唤起终端） |
| `Ctrl+\` | 切换编辑器视图（与终端互斥复用 SideView） |
| `Ctrl+Shift+P` | 命令面板（终端激活时仍生效） |
| `Ctrl+B` | 切换文件面板（终端激活时仍生效） |
| `Esc` | 终端激活时：退终端视图（→ `None` + 收起），**不中断对话** |
| `Enter` | 发送当前行编辑器内容 + `\n` 给 PTY，固化进历史 |
| `Ctrl+C` | 即时发 `\x03` 给 PTY（不等回车） |
| `Ctrl+D` | 即时发 `\x04` 给 PTY（EOF；空行时退出 shell） |
| `←` `→` `Home` `End` | 行内光标移动 |
| `Backspace` `Delete` | 删除字符 |
| `Ctrl+A` / `Ctrl+E` | 行首 / 行末（对齐 Home/End） |
| `Ctrl+U` | 清空当前行编辑器内容 |

终端激活时**被吞**（不触发全局动作）的键：普通字符（进编辑器）、`Ctrl+Shift+D`（退终端而非切对话）、`Ctrl+W`（不关 editor tab）、`Ctrl+Tab`（不循环 editor tab）。

### 2.4 与编辑器视图的关系（互斥复用）

终端与编辑器/文件预览**不能同时显示**——三者都是 `SideView` 的互斥子视图。原型图 `toggleSideView(kind)` 状态机语义：已展开且 kind 相同再按 → 收起；否则展开并切到该 kind。

点击文件节点：按文件类型自动切 `Editor`/`Preview` 并展开（**让出 Terminal**）。终端运行中的命令**不被打断**——PTY 进程继续，只是视图切走；切回 Terminal 时输出历史仍在（tab 保留）。

---

## 3. 架构边界

### 3.1 crate 归属

| 组件 | crate | 依赖 | 可发布性 |
|:---|:---|:---|:---|
| `TerminalBackend` trait + `LocalPtyBackend` | `xgent_terminal`（新） | `xgent_core` + `settings_core` + `portable-pty` + tokio + async-trait + thiserror + `vte` | 纯逻辑层，不依赖 Bevy |
| `TermView` UI + 行编辑器 + 输出渲染 | `xgent_ui::terminal` | bevy + xui + xgent_core + xgent_terminal | 业务层 |
| `TerminalTabs` / `TerminalInput` 等 ECS 类型 | `xgent_ui::terminal` | bevy + xgent_core | 业务层 |

**关键反转依赖**：`xgent_ui` 依赖 `xgent_terminal` 的 trait + 类型，**不直接依赖 `portable-pty`**。实现注入对齐 `xgent_settings::Localizer` impl `StringSource` 的既有模式：`xgent_app` 启动时注入 `LocalPtyBackend`（或 `xgent_ui` 默认注册）。将来 Web 端换 `DaemonPtyBackend`（走 JSON-RPC），`xgent_ui` 零改动——对齐 AGENTS.md §5.1 "可上移职责用 trait 抽象，切换不破坏调用方"。

### 3.2 ECS 契约

| Event / Message | 方向 | 载荷 | 说明 |
|:---|:---|:---|:---|
| `TerminalSpawnRequest` Message | UI → backend | `{tab_id, shell, cwd, cols, rows}` | 新 tab 触发 PTY spawn |
| `TerminalInput` Message | UI → backend | `{tab_id, bytes: Vec<u8>}` | 整行（回车）或单控制字节（Ctrl+C/D），统一载荷 |
| `TerminalResize` Message | UI → backend | `{tab_id, cols, rows}` | 窗口/SideView resize 时发 |
| `TerminalKillRequest` Message | UI → backend | `{tab_id}` | 关 tab / 终止 |
| `TerminalOutput` Message | backend → UI | `{tab_id, bytes: Vec<u8>}` | PTY stdout/stderr 数据流，高频，缓冲 |
| `TerminalExited` Message | backend → UI | `{tab_id, exit_code}` | PTY 进程退出，tab 保留标灰 |

**Resource**：
- `TerminalTabs` — 活跃 tab 集合（`{tab_id → Entity, TerminalId, RenderHistory, status}`）
- `ActiveTerminalTab` — 当前激活 tab id
- `TerminalIoRuntime` — tokio task handle + channel 句柄（对齐 `EditorIoRuntime`）

**Marker 组件**：
- `TerminalViewMarker` — 终端视图容器（`SideViewMarker` 子节点，初始隐藏）
- `TerminalTabMarker(tab_id)` — 每个 tab 的 Entity

### 3.3 数据流：用户输入

```
用户在 tv-inputline 敲键
  → 终端行编辑器系统更新行编辑器状态（光标/文本）
  → 若是控制字符（Ctrl+C/D）：即时发 TerminalInput{bytes:[0x03/0x04]}
  → 若是 Enter：
      1. 把 "❯<整行>" 作为 RenderLine 追加进 tv-body 历史
      2. 发 TerminalInput{bytes: line + "\n".as_bytes()}
      3. 清空行编辑器
  → backend TerminalBackend::write(tab_id, bytes) → PTY stdin
```

PTY 设为 **raw 模式 + echo off**——shell 不回显用户输入，UI 行编辑器是输入的唯一显示源（避免双份）。

### 3.4 数据流：PTY 输出渲染

```
PTY stdout/stderr 字节流
  → backend read 循环（spawn_blocking task）
  → 经 mpsc channel → TerminalOutput Message 回 ECS
  → 终端输出消费系统每帧 drain 全部 Message + 合并
  → vte 解析 ANSI 转义序列 → 累积成 RenderLine（Vec<StyledSpan>）
  → 追加进 tab 的 RenderHistory（Vec<RenderLine>，上限 10k 行超丢头部）
  → 虚拟滚动只渲染可见行区间（复用 xui::VirtualList 思路）
```

SGR 参数（颜色码）映射到 Bevy Text span 的 `TextColor`。非 SGR 转义（光标移动/清屏）MVP 简化处理：`\x1b[H`/`\x1b[2J` 识别为"历史分隔标记 + 滚到新段"，非真改屏幕网格——故 `vim`/`top` 全屏 TUI 显示混乱（MVP 明确不支持，见 §6 边界）。

非 UTF-8 字节用 `String::from_utf8_lossy` 兜底（替换为 U+FFFD），MVP 不做编码探测。

### 3.5 数据流：PTY 生命周期

```
新建 tab（用户点 ＋ 或 Ctrl+` 首次唤起）
  → spawn TerminalTabMarker Entity
  → TerminalSpawnRequest → backend.spawn(shell, cwd, cols, rows)
  → PTY 进程 + 读写 task 起来 → tab 标活跃

PTY 进程自己退出（exit 命令 / Ctrl+D / shell 崩溃）
  → read task 收 EOF → TerminalExited{exit_code}
  → tab 标灰 + tv-statusbar 显示 exit code（tab 不自动消失）

关 tab（用户点 ×）
  → TerminalKillRequest → backend.kill(TerminalId)
  → PTY kill + 读写 task 结束
  → despawn tab Entity；若是最后一个 tab → SideViewContent=None + 收起

窗口关闭（bevy_window exit）
  → 遍历所有 tab kill（kill_on_drop 或显式循环），无孤儿
```

---

## 4. 状态机

### 4.1 TerminalTab 状态

```
        spawn request
Created ───────────────────→ Running
  ↑                           │
  │                           │ PTY 退出
  │                           ↓
  │                       Exited
  │                           │
  │ 用户关 tab                │ 用户关 tab
  └───────────────────────────┘
        → kill + despawn
```

- `Created`：tab Entity 已 spawn，PTY 尚未就绪（瞬态）。
- `Running`：PTY 活跃，tv-tab dot 绿/蓝（忙碌）。
- `Exited`：PTY 已退出，tab 保留标灰，tv-statusbar 显示 exit code。用户手动关才 despawn。

### 4.2 SideView 视图状态（四变体）

| 状态 | 进入 | 退出 |
|:---|:---|:---|
| `None`（收起） | 默认 / `Ctrl+Shift+D` / 关最后一个 tab | 点击文件 / `Ctrl+\` / `Ctrl+`` |
| `Editor` | 点击代码文件 / `Ctrl+\` / `Ctrl+Shift+E` | 切到其他变体 / 收起 |
| `Preview` | 点击非代码文件 | 切到其他变体 / 收起 |
| `Terminal` | `Ctrl+`` / 顶栏 🖥 | 切到其他变体 / 收起 / `Esc` |

---

## 5. xgent_terminal crate 设计

### 5.1 模块职责

`xgent_terminal` 是 PTY 抽象层，纯依赖 tokio + portable-pty + vte，不依赖 Bevy。

提供：
- `TerminalBackend` async trait：spawn/read/write/resize/kill
- `LocalPtyBackend`：portable-pty 实现
- PTY 读循环（spawn_blocking task）经 mpsc channel 桥接 ECS

不提供：
- UI 渲染（`xgent_ui::terminal` 负责）
- 行编辑器（`xgent_ui::terminal` 负责）
- 多 tab 管理（`xgent_ui::terminal` 负责）

### 5.2 关键类型（草签）

```rust
// crates/xgent_terminal/src/lib.rs

#[async_trait]
pub trait TerminalBackend: Send + Sync {
    async fn spawn(&self, req: SpawnRequest) -> Result<TerminalId, TerminalError>;
    async fn write(&self, id: TerminalId, bytes: Vec<u8>) -> Result<(), TerminalError>;
    async fn resize(&self, id: TerminalId, cols: u16, rows: u16) -> Result<(), TerminalError>;
    async fn kill(&self, id: TerminalId) -> Result<(), TerminalError>;
    /// 订阅输出流（bytes）+ 退出通知（exit_code），经 channel 回 ECS。
    async fn subscribe(&self, id: TerminalId, tx: mpsc::Sender<TerminalEvent>) -> Result<(), TerminalError>;
}

pub enum TerminalEvent {
    Output(Vec<u8>),
    Exited(Option<i32>),
}

pub struct SpawnRequest {
    pub shell: ShellSpec,
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
}

pub enum ShellSpec {
    Powershell,
    FromEnv,     // Unix $SHELL，fallback sh
}

pub struct TerminalId(pub u64);

#[derive(thiserror::Error, Debug)]
pub enum TerminalError {
    #[error("spawn 失败: {0}")]
    Spawn(String),
    #[error("write 失败: {0}")]
    Write(String),
    #[error("resize 失败: {0}")]
    Resize(String),
    #[error("kill 失败: {0}")]
    Kill(String),
}
```

### 5.3 LocalPtyBackend 实现

- `portable-pty::PtySystem::openpty()` 创建 PTY 对。
- `spawn`：用 `CommandBuilder` 设 shell + cwd + env，`slave.spawn`，PTY 设 raw 模式（`set_raw_mode()`）+ echo off。
- `subscribe`：`spawn_blocking` 起读循环 task，`reader.read_to_end`/循环 `read` 读 PTY master 输出，经 `tx` 发 `TerminalEvent::Output(bytes)`；EOF 时发 `Exited`。
- `write`/`resize`/`kill`：操作 master 句柄，`kill` 杀 slave 进程组。
- 同步 API 全包在 `spawn_blocking`，不阻塞 tokio async 上下文。
>
> **落地偏离**（MVP）：`portable-pty` 无跨平台 raw 模式 API（Unix termios 可设、Windows ConPTY 自管 echo），`LocalPtyBackend` **未设 raw 模式**——保持 shell cooked 模式，shell 回显用户输入。UI 行编辑器降级为「输入框 + 回车提交整行」，shell 回显产生可见命令行；控制字符 Ctrl+C/D 即时单字节发送。代价是输入框与历史区短暂双显（输入框清空即消失），收益是跨平台一致 + shell 自带历史/补全（设计本列为「不支持」，cooked 模式反而白送）。raw 模式 + echo off 留后续（需 Unix 走 termios、Windows 走 ConPTY input mode 调整，跨平台抽象复杂）。

### 5.4 Cargo.toml

```toml
# crates/xgent_terminal/Cargo.toml
[package]
name = "xgent_terminal"
version = "0.1.0"
edition = "2024"

[dependencies]
xgent_core = { path = "../xgent_core" }
xgent_settings_core = { path = "../xgent_settings_core" }
portable-pty = "0.8"
vte = "0.13"
tokio = { workspace = true }
async-trait = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }
```

注：版本以实际发布时最新稳定为准，上述版本号为占位。`portable-pty` / `vte` 加进 `[workspace.dependencies]`。

---

## 6. MVP 能力边界

**支持**：
- 普通 CLI（cargo / git / ripgrep 等），输出 ANSI 彩色 + 滚动历史
- 多 tab，每 tab 一个独立 PTY 会话，不限上限
- UI 侧行编辑：左右/Backspace/Delete/Home/End/Ctrl+A/E/U、回车发送、粘贴
- 控制信号 Ctrl+C / Ctrl+D 即时发
- PTY resize 响应（SideView 展开/窗口 resize 时发 TerminalResize）
- PTY 退出后 tab 保留 + 显示 exit code
- 焦点状态机：终端激活吞键，全局键仍生效
- Windows（ConPTY）/ Unix（pty）跨平台

**不支持（留后续）**：
- 全屏 TUI 程序（vim / top / htop）——行模型不支持 alternate screen + 光标定位
- 上下历史（↑↓）——需 UI 侧存历史缓冲，跨 shell 语义不一
- Tab 补全——需和 shell readline 协作，与"UI 侧行编辑"语义冲突
- 多行输入
- 编码探测/配置（GBK 等）
- `terminal.shell` / `terminal.cwd` / `terminal.args` settings 配置项
- tab 持久化 / 跨会话恢复
- 多窗口共享终端（需上移 daemon）
- PTY 持久化重连（daemon 重启重连）
- 命令拦截 / 输出脱敏
- `cd` 越界告警

---

## 7. 对既有文档的修订点

### 7.1 `doc/design/requirements.md`

- **新增 F-19**：行 "F-19 | 内置终端 | 嵌入式用户人机交互终端，多 tab PTY，UI 侧行编辑，与编辑器/预览复用 SideView。详见 `doc/design/terminal-design.md`。 | P1" 加入 §4.2 功能表，并加入 §7.2 P1 清单。

### 7.2 `doc/design/architecture.md`

- **4. crate 划分**：crate 表新增 `xgent_terminal`（lib，依赖 `xgent_core` + `settings_core` + `portable-pty` + `vte`，不依赖 Bevy）。依赖关系图加 `xgent_terminal ← xgent_ui`。
- **5.1 进程模型**：明确"终端 PTY 在 UI 侧（每窗口独立集合），不进 daemon 职责"，对齐既有"agent loop 放 UI 侧"。

### 7.3 `doc/design/editor-design.md`

- **§2.1 布局增量作废重写**：原"编辑器作为对话主区内可切换的标签视图"作废。改为"编辑器/文件预览/终端都是 `SideView`（右侧分屏）的互斥子视图，由 `SideViewContent` 四变体切换"。`EditorView` 枚举废弃，全用 `SideViewContent`。
- **§2.3 快捷键**：`Ctrl+Shift+E`/`Ctrl+Shift+D` 语义更新为操作 `SideViewContent`（设 `Editor`/`None`），而非 `EditorView`。
- **§3.2 ECS 契约**：`EditorCommand` Event 不变；`SideViewContent` 从 3 变体扩到 4 变体（加 `Terminal`）。

### 7.4 `doc/design/ui-design.md`

- **2. 布局**：SideView 容器描述补充"终端"作为第三种子视图。
- **2.4 焦点管理**：新增焦点目标"终端视图"，进入 `Ctrl+``，退出 `Esc`（回 `None` + 收起）或切其他变体。
- **11. 快捷键表**：追加 `Ctrl+`` 切终端。
- **14. MVP 不做**：终端 F-19 是 P1（非 MVP），可备注"详见 terminal-design.md"。

### 7.5 `AGENTS.md`

- **§4 crate 划分**：表新增 `xgent_terminal` 行；依赖关系图加 `xgent_terminal ← xgent_ui`。
- **§7 当前实现状态**：补"F-19 内置终端（P1）设计中"。

### 7.6 `doc/dev-tutorial.md`

- **crate 拓扑章节**：新增 `xgent_terminal` 描述（PTY 抽象 + LocalPtyBackend，不依赖 Bevy，UI 层经 trait 调用）。

---

## 8. 验证方法（P1 实现时）

1. **编译检查**：`cargo check -p xgent_terminal`、`cargo check -p xgent_ui`。
2. **独立性**：`cargo tree -p xgent_terminal` 输出含 portable-pty / vte，不含 Bevy。
3. **PTY 基础测试**：spawn shell，write `echo hello\n`，断言收到 `hello` 输出。
4. **ANSI 渲染测试**：write `printf '\e[31mred\e[0m'`，断言 `RenderLine` 含红色 span。
5. **行编辑测试**：输入 `abc` + 左移 + Backspace + Enter，断言发送 `ac\n`。
6. **控制信号测试**：跑 `cargo build` 中按 Ctrl+C，断言 PTY 收到 `\x03` 且进程中断。
7. **多 tab 测试**：新建多个 tab，各跑独立命令，断言互不干扰。
8. **生命周期测试**：shell `exit` 后 tab 保留标灰；关 tab 后 PTY kill 无孤儿；窗口关后全 kill。
9. **焦点测试**：终端激活时按 Ctrl+B 切文件面板（生效），按 Ctrl+W 不关 editor tab（被吞）。
10. **跨平台测试**：Windows（powershell + ConPTY）与 Unix（$SHELL + pty）各跑一次。

---

## 9. 未决与后续

- **全屏 TUI 支持**：若用户强需 vim/top，评估屏幕字符网格模型（alternate screen + 光标定位），届时终端从"行模型"升"屏幕模型"。
- **Tab 补全**：评估"哑终端 + shell readline"退化路径（放弃 UI 侧行编辑换补全），或混合模型（行编辑 + 补全时临时让 shell 接管）。
- **上下历史**：UI 侧历史缓冲，跨 shell 一致性需设计。
- **多语言扩展**：shell 配置项 `terminal.shell` / `terminal.args` / `terminal.cwd` 进 settings。
- **多窗口共享终端**：Web 端 / 多开场景需 PTY 上移 daemon，`DaemonPtyBackend` 走 JSON-RPC（`terminal.spawn`/`input`/`resize`/`kill` + `terminal.output` 事件流）。
- **PTY 持久化重连**：daemon 重启后重连既有 PTY（需 PTY 进程脱离 daemon 生命周期，复杂）。
