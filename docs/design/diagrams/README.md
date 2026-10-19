# XGent 架构图集（新人导读）

> 面向第一次接触本仓库的开发者。十张交互图 + 本页文字，目标是让你在 30 分钟内能回答：
> 进程怎么分、一次对话怎么跑完、编辑器与终端各自怎么转、代码放在哪、哪些地方**文档已经跟不上代码**。
>
> 代码基线：`b0771e8`（2026-10-16），18 个 crate，约 40k 行 Rust（含 3.3k 行测试）。
> 图中每个结论都给了 `文件:行`，全部经本次会话逐条复核；未复核的一律只给文件路径。

---

## 一、图集清单与打开方式

| 文件 | 图类型 | 回答的问题 |
|:---|:---|:---|
| `01-system-architecture.html` | architecture | 两个进程各自负责什么，一次请求横跨哪些组件 |
| `02-crate-layers.html` | architecture | 18 个 crate 的编译期分层，以及三条隔离约束 |
| `03-streaming-chat.html` | sequence | 从按下回车到最后一个字渲染出来，中间经过哪些边界 |
| `04-agent-loop.html` | lifecycle | `ConversationStatus` 的七个状态怎么转移，哪里会停下来 |
| `05-tool-execution.html` | workflow | 一次工具调用如何分组、确认、执行、回灌、可撤销 |
| `06-ipc-dataflow.html` | dataflow | JSON-RPC 的真实协议面，配置与文件事件如何流到 UI |
| `07-editor-preview.html` | architecture | 编辑器与预览：buffer 实体怎么建、rope 怎么渲染、冲突怎么协调 |
| `08-editor-save.html` | sequence | 点文件到落盘的完整往返，含 daemon 广播与三选弹窗 |
| `09-terminal.html` | architecture | 终端：PTY 三线程、两级 channel 桥、行模型渲染 |
| `10-terminal-io.html` | sequence | 开 tab → 输出回流 → 攒行提交 → 退出上报 |

用浏览器直接打开任意 `.html` 即可，文件自包含、无网络依赖。查看器自带：深浅主题、缩放平移、
搜索、点击节点追踪关系、右上角 **视图** 按钮切换本图聚焦子集、导出 PNG/SVG。多数图右下角的
说明卡片是**读图重点**，不是装饰；04 版面被状态机占满，那三条结论写在本文 §三的 04 小节里。

建议顺序：**01 → 03 → 05 → 04 → 06 → 02** 建立整体认知，再按你负责的模块挑：
改界面走 **07 → 08**（编辑器/预览）或 **09 → 10**（终端）。

---

## 二、五分钟项目速览

**它是什么**：基于 Bevy 0.20（ECS + 即时模式 UI）的桌面 AI code agent。UI 就是 agent 状态的
投影，不是"调用界面 API"。

**两个进程**

- `xgent_app`（UI 进程）：每项目/每窗口一个。Bevy App + agent loop + 工具执行 + 上下文构建。
- `xgent_daemon`（守护进程）：全局唯一，纯 tokio，**不依赖 Bevy**。provider 连接池、全局配置、
  文件监听、多客户端文件状态同步。
- 启动时序：`crates/xgent_app/src/main.rs:82` 先探测 socket，探不到就 fork daemon，
  之后每 100ms 轮询、10s 超时（`crates/xgent_app/src/daemon.rs`）。超时即 `exit(1)`。
  末个客户端断开后 daemon 延迟 30s 退出（`crates/xgent_daemon/src/lifecycle.rs`）。

**通信的三种载体（硬约束）**——判据是"这条交互有没有返回值"

| 语义 | 载体 | 例子 |
|:---|:---|:---|
| 动作信号（发生了 X） | Bevy **Message** | `UserInputMessage`、`DeltaMessage`、`AbortMessage` |
| 状态投影（当前是 X） | Bevy **Resource** | `Conversation`、`EditorTabs`、`ProviderInfo`、`PanelWidths` |
| 同步查询（要拿结果） | 直接 **trait 调用** | `ContextProvider::retrieve`、`Tool::execute`、`EditorState` |

两条最容易踩的红线：

1. **ECS 主线程永不 `await`**。所有异步都住在 tokio task 里，结果经 mpsc channel 回 ECS，
   ECS 侧每帧 `try_recv` drain（`crates/xgent_agent/src/agent_loop.rs`）。
2. **跨 Plugin 边界禁止直接方法调用**，否则该 Plugin 无法 headless 测试与回放消息流。

**技术栈**：Bevy 0.20（path 依赖 `../bevy`，检出上游 tag）+ tokio + reqwest/eventsource-stream
+ serde + TOML 配置 + JSONL 会话 + fluent i18n + wasmtime 29（WASM 组件插件）+
portable-pty/vte（终端）+ tree-sitter（编辑器高亮，仅 Rust grammar）。

---

## 三、逐图导读

### 01 系统架构总览

- 主路径：`开发者 → xgent_ui → Bevy Message → AgentBridge → JSON-RPC → daemon → ProviderPool → LLM`，
  流式回传沿同一条路反向逐段换载体（SSE chunk → `ChatEvent` → notification → `AgentEvent` → `DeltaMessage`）。
- `AgentBridge` 是**唯一**跨线程边界：`crates/xgent_agent/src/bridge.rs:188`（结构体）、
  `:516`（`agent_loop_task`）、`:709`（`run_agent_loop`）。
- 工具执行与上下文检索都在 UI 进程内（MVP 瘦后台）：`crates/xgent_tools/src/executor.rs:206`、
  `crates/xgent_context/src/on_demand.rs:105`。
- 插件沙箱经 `PluginHostProxy` 反转依赖（`crates/xgent_plugin/src/proxy.rs:75`），
  宿主核心 `xgent_plugin` 不依赖任何业务 crate。

### 03 一次流式对话的端到端时序

- 上下文检索发生在 **tokio 侧、发请求之前**（`crates/xgent_agent/src/bridge.rs:516` 之后的
  `StartLoop` 分支），因为 ECS 系统无法 await `retrieve`。系统提示词被
  `refresh_system_message` 覆盖 `req.messages[0]`（`crates/xgent_agent/src/format.rs`）。
- 工具 schema 在 `AgentBridge::new` 时从 executor 一次性快照（`bridge.rs:445` 附近），
  注入 `ChatRequest.tools`——这是历史上"LLM 从不调用工具"的修复点。
- 三个真实修复留下的约束，写在图的卡片里，改动流式通路前务必先读：subscribe 早于 call、
  Lagged 只能 continue、`Done` 延迟到流末尾发射。
- 为把面板压进首屏，图里省掉了三跳细节：`provider.chat` 先同步返回 `stream_id`，SSE 侧
  `POST` 与 `[DONE]` 的解析、以及 `ToolCallStart/Delta/End` 按 index 聚合发生在 daemon 适配器里
  （`crates/xgent_provider/src/openai_compat.rs`、`anthropic.rs`）。
- 中断语义：Abort 双路取消；steering 在流式期 race 中断当前流，半截文本要**同时**回灌
  `req` 与 `conv` 两侧，否则模型下一轮看不到自己说过的话；只有 `Network` 与 `StreamParse`
  会自动重试，`NotConfigured` / `AuthFailed` 立即失败（`crates/xgent_agent/src/bridge.rs`
  的 `RetryConfig::is_retryable`）。

### 05 工具执行与安全确认

- 默认策略是**所有工具都要确认**（`crates/xgent_tools/src/security.rs:24`），
  顺序：配置 denied → 配置 approved（仍执行 `approval_for` 动态升级）→ tier 推导。
  `UiOnly` 与 `editor.*` 自动放行。
- 并发不是第二条执行路径：`parallel_safe`（`crates/xgent_tools/src/executor.rs:166`，
  调用点 `crates/xgent_agent/src/bridge.rs:980`）三条信号全过才成组——免确认 + `Shared` +
  tier **恰好** `Read`。测试 `executor.rs:757` 明确断言 `UiOnly` 被排除。
- 同组用 `join_all` 在**当前任务里**轮询，不 spawn；结果按模型给出的**原始下标**回灌，
  先跑完的不抢先。
- 检查点的唯一咽喉是 `run_tool`（`crates/xgent_tools/src/executor.rs:106` 写前快照、
  `checkpoint.rs:184` 轮次归属、`:219` 快照实现）。新增写文件工具必须实现
  `Tool::write_targets`，否则它的改动天然不可撤销。
- 确认决策有 Allow / Deny / AllowAll 三种与超时，Deny 也回灌成 `is_error` 结果继续对话；
  但**当前 UI 没有"全部允许"按钮**（见下方偏差清单第 13 条），AllowAll 只能来自配置。
- 撤销的覆盖边界要如实告诉用户：`run_command` 的磁盘副作用（`sed -i`、`git apply`、
  `cargo fmt`）与插件 `host.write_file` 的写入**不在范围内**；撤销前比对写入后哈希，
  被外部改过或已被删除的文件跳过并报名，绝不谎报"没有可撤销的改动"。

### 04 会话状态机

- 状态定义在 `crates/xgent_agent/src/conversation.rs:307`：`Idle/Thinking/Streaming/ToolRunning/Confirming/Aborting/Error`。
- 是否继续内层循环由 `tool_calls.is_empty()` 决定，**不是** `StopReason`（`bridge.rs:870` 附近）；
  `StopReason::Length` 与 `Aborted` 各有一条特例支路。
- Abort 是**双路**的：ECS 侧直接 `token.cancel()`（`crates/xgent_agent/src/agent_loop.rs:95` 附近）
  + 发 `AgentCommand::Abort`。第一条的存在是因为循环可能正 park 在等确认，收不到命令。
- 图里没画的四个止失控上限（都是可配缺省，配置写 `0` 表示**回落缺省而不是不限**，见
  `docs/dev-tutorial.md` §4.5.1）：`max_tool_rounds` 20 轮（超限发 `BoundedMessage`）、
  `max_tokens_per_turn` 200k token、单帧最多 drain 64 条事件（剩余留在 channel，不丢）、
  同轮最多 4 个只读工具并发。
- `stop_reason == Length` 时**不执行** tool_calls（参数可能残缺），改为补占位 skipped result
  并回灌配对，让模型重发；流提前断开（provider 没发 `Done`）算 `StreamParse` 可重试错误，
  不是正常结束。

### 06 IPC 协议与配置 / 文件事件数据流

- 方法常量 `crates/xgent_core/src/methods.rs:6-24`（7 个），通知常量
  `crates/xgent_core/src/notifications.rs:10-16`（4 个），分发点
  `crates/xgent_daemon/src/session.rs:168`。
- 帧格式是**换行分隔的 JSON 行**，不是长度前缀；出站由单写 task 串行化
  （`crates/xgent_daemon/src/session.rs` + `crates/xgent_app/src/ipc_client.rs`），
  `call()` 有 120s 硬超时（`crates/xgent_app/src/ipc_client.rs:123`）。
- 改 `providers*` 会连带 `ProviderPool::invalidate`，否则旧凭据一直服务到重启
  （`session.rs` 内，池见 `crates/xgent_daemon/src/provider_pool.rs:31`）。
- 广播排除写入者（`crates/xgent_daemon/src/session.rs:270`），所以写入窗口靠本地显式刷新
  （`crates/xgent_app/src/config_bridge.rs`，就绪判定 `:288`）。

### 02 crate 编译期分层

- 分层只讲"谁被谁编译进来"，完整依赖表见下节。
- 两个反转依赖枢纽值得记住：`xui_i18n::StringSource`（让 `xui` 零 xgent 依赖）与
  `xgent_core::EditorState`（让 `xgent_context` 不反向依赖 UI）——后者**当前没有消费者**，见 §五 第 19 条。
- `xgent_app` 与 `xgent_daemon` 之间**没有** cargo 依赖，只有运行时进程派生。

### 07 编辑器与预览（SideView）原理

- 建实体：`OpenFileRequest` → `handle_open_file_requests`（`crates/xgent_ui/src/editor/tabs.rs:164`）
  spawn 一个 buffer 实体，组件组合是 `ScrollArea` + `Scrollbar` + `EditorBuffer` + `TextEditor`
  + `HighlightCache` + `PendingRead`（`tabs.rs:239-261`），子节点 `VirtualContentMarker`。
- **它是只读的**：buffer 实体不挂 `EditableText`，bevy 的 text_input 因此没有落点，敲字符不进
  rope；`update_syntax_highlight` 里唯一会置脏的分支（`crates/xui/src/text_editor.rs:280` 附近）
  对业务 buffer 不可达。`xui::TextEditor` 本身支持编辑态，是业务层选了虚拟化只读模式
  （`crates/xgent_ui/src/editor/mod.rs:309`、`io.rs:360` 的注释即如此声明）。
- 高亮是**全量重解析**：内容 hash 变了就整篇重算 spans（`crates/xui/src/text_editor/highlight.rs`），
  只有 Rust 一种 grammar（`Language` 单变体）。
- 行对齐是硬约束：`LineHeight` 不从 `Text` 级联到 `TextSpan`，所以虚拟行、每个 span、行号列
  都要显式写同一个 `LineHeight::Px`（`crates/xui/src/text_editor/virtual_render.rs:182/338/356`、
  `crates/xgent_ui/src/editor/tabs.rs:210`），否则滚得越深行号与内容错位越大。
- 页签互斥由单个 Resource 决定：`SideViewContent`（`crates/xgent_ui/src/editor/mod.rs:69`），
  各容器只写自己的 `display`。

### 08 编辑器：打开 → 保存 → 冲突

- 读盘**不在异步线程**：`EditorIoRuntime.handle` 在 `xgent_app` 里从未注入（只有
  `TerminalIoRuntime` 被注入，`crates/xgent_app/src/main.rs:264`），于是 `handle_file_read_requests`
  走同步回退 `std::fs::read_to_string`（`crates/xgent_ui/src/editor/io.rs:136`）。撤销路径也刻意
  避开它（`crates/xgent_ui/src/undo.rs:84`）。
- 保存是**第二条写盘通路**：`Cmd+S` → `handle_editor_save`（`editor/mod.rs:317`）→ `io.rs` 的
  临时文件 + rename 原子写，不经 `WriteFile` 工具、不进检查点（ADR-0009 的既定设计）。
  落盘成功后 `BufferSavedEvent` → `fs_event_bridge.rs:119` 发 IPC `fs.notify`，daemon 再向同项目
  其他窗口广播 `peer.fileChanged`。
- 外部变更分流在 `crates/xgent_ui/src/editor/conflict.rs:71`：干净 → 静默重读；脏 → 置
  `ConflictDetected` 并弹三选，且冲突未决期间禁止保存（`editor/mod.rs:327`）。
- 冲突未决时**保存被禁用**（`editor/mod.rs:327`），三个选项里"对比合并"目前等价于"保留本地"。

### 09 终端原理

- crate 切分：`xgent_terminal` 只有 `backend.rs`（trait）、`local_pty.rs`（portable-pty）、
  `render.rs`（vte → 行模型）；`PtyBridge`、行编辑、输出区、多 tab 全在
  `crates/xgent_ui/src/terminal/`。
- 每个会话**三个 std 线程**（`MasterPty` 不是 `Sync`，`local_pty.rs:7`）：reader 阻塞读并在
  此处应答 DSR（`:161`）、wait 独占 `child.wait()` 作为 `Exited` 的唯一权威（`:218`）、
  cmd 循环消费 `Write/Resize/Kill/CloseMaster`（`:231`）。killer 句柄放在
  `Arc<Mutex<Option<..>>>` 里由 cmd 循环与 `Drop` 共享，`take` 保证幂等。
- 输出通路两跳：tokio mpsc(256) → crossbeam 无界 → `handle_pty_events`（`terminal/io.rs:182`）
  → `TerminalOutputChunk` → `RenderHistory::feed`（`output.rs:45`）→ `output.rs` 增量追加。
- 与 agent 的 `RunCommand` 工具**零交集**：终端不经 `ToolExecutor`/`resolve_policy`，进程归用户，
  输出不进工具卡片；两条路只共享 Bevy 的消息习惯。

### 10 终端输入输出时序

- 键盘归属由 `SideViewContent::Terminal` + `InputFocus::is_none()` 共同把关
  （`terminal/input.rs:82/87`），切进终端时会清焦点，因此输入行是普通 `Text` 而非 `EditableText`。
- 输入是**本地行编辑**：字符攒进 `TerminalLineBuf`，Enter 提交整行 + `\r`（`input.rs:131`），
  Ctrl+C 清行并即时发 `\x03`，Ctrl+D 仅在空行时发 `\x04`。↑↓ 历史与 Tab 补全是显式 no-op
  （`input.rs:151`）。PTY 保持 cooked 且未关 echo，所以提交后 shell 的回显会作为普通输出再进历史。
- DSR `\x1b[6n` 由 reader 线程识别并回 `\x1b[1;1R`（`local_pty.rs:178-193`，行列为硬编码）——
  不应答则 PowerShell/PSReadLine 启动即卡在探测上，表现为"输入无反应"。
- 退出不自动关 tab：标签点变暗、状态栏显示退出码、光标停止闪烁、后续输入与 resize 被忽略；
  `Spawned` 晚于 tab 销毁时立即 kill 孤儿会话（`terminal/io.rs:196`）。
- 输出是**行模型不是屏幕网格**：光标定位类 CSI 被丢弃，所以 vim/top 这类全屏 TUI 不支持；
  也没有虚拟滚动，切 tab 即全量重建（`output.rs:7`）。

---

## 四、crate 清单与真实内部依赖

内部依赖 = 该 crate 的 `Cargo.toml` 里出现的其他 `xgent_* / xui*` crate（本次实测提取）。

| crate | 类型 | 行数 | 内部依赖 | 职责 |
|:---|:---|---:|:---|:---|
| xgent_core | lib | 1.3k | — | 跨进程共享类型：`ChatEvent`(`src/chat.rs:174`)、`StopReason`(`:234`)、JSON-RPC 信封(`src/proto.rs:15/81`)、方法/通知常量 |
| xui_i18n | lib | <0.1k | — | `StringSource` trait，反转依赖枢纽 |
| xgent_settings_core | lib | 1.7k | core | 配置纯类型 + TOML 读写 + 平台路径，无 Bevy |
| xgent_settings | lib | 0.4k | core, settings_core, xui_i18n | Bevy Resource 包装 + fluent `Localizer` |
| xgent_provider | lib | 3.7k | core, settings_core | `LlmProvider` trait + 4 适配器 + SSE 解析 |
| xgent_daemon | bin | 2.1k | core, provider, settings_core | 守护进程：池 / 配置协调 / 文件监听 / 多客户端 |
| xgent_tools | lib | 6.1k | core, settings_core, **bevy** | `Tool` trait + 策略 + 执行器 + 检查点 |
| xgent_context | lib | 1.8k | core, settings_core, **bevy** | `ContextProvider` + OnDemand 方案 A + `ContextHub` |
| xgent_terminal | lib | 1.3k | **无** | `TerminalBackend` + `LocalPtyBackend`(portable-pty) + vte 解析 |
| xgent_agent | lib | 7.2k | core, provider, tools, context, settings, settings_core | agent loop + ECS 桥 + `SessionStore` |
| xui | lib | 3.9k | xui_i18n(+bevy/tree-sitter/ropey) | 通用组件库，**可独立发布** |
| xgent_plugin | lib | 2.2k | core(+wasmtime) | 宿主核心：`PluginHost`/`WasmHost`/`WasmPlugin`/proxy |
| xgent_plugin_api | lib | 0.2k | —(+wit-bindgen) | 插件作者侧 API + WIT 绑定，编译成 `wasm32-wasip2` |
| xgent_plugin_host | lib | 1.2k | plugin, core, tools, context, agent, **xui**, settings_core, xui_i18n | ECS 桥 + 三类适配器 |
| xgent_plugin_hello | cdylib | <0.1k | plugin_api | 最小验证插件 |
| xgent_plugin_git | cdylib | 0.2k | plugin_api | 参考插件：4 工具 + 4 命令 + git 历史 provider |
| xgent_ui | lib | 15.2k | core, settings, settings_core, terminal, agent, **tools**, xui, xui_i18n | 业务 UI（24 模块，最大 crate） |
| xgent_app | bin | 1.9k | 上述全部 UI 侧 | UI 进程入口：组装插件 / 拉起 daemon / IPC 客户端 |

装配点：`crates/xgent_app/src/main.rs:250`（`add_plugins` 顺序固定：Xui → Settings → Agent →
Context → PluginHost → Ui → ConfigBridge → FsEventBridge），资源注入 `:260-287`，
`load_builtin_plugins` 在 `:292` 被 spawn 到 bridge 的 tokio runtime 上。

---

## 五、代码与既有文档的偏差（本次逐条复核）

新人在 `AGENTS.md` / `docs/design/architecture.md` / `docs/dev-tutorial.md` 里会读到下面这些
**已经不成立**的说法。以代码为准，改这些文档需要单独一次提交。

1. **架构文档 §6.5 的四个通知名不存在**。`provider.delta` / `provider.toolCall` /
   `provider.done` / `provider.error` 在代码里没有；真实只有单个 `provider.event`，
   params 透传整个 `ChatEvent` JSON（`crates/xgent_core/src/notifications.rs:10`）。
   同时 §6.5 漏了真实存在的 `provider.cancel` 与 `fs.notify`（`crates/xgent_core/src/methods.rs:12/24`）。
2. **daemon 的 `session.rs` 不是会话**。它是"每连接一个 IPC 处理器"，与对话 session 无关
   （`crates/xgent_daemon/src/session.rs:168`）。
3. **`@selection` 未实现**。`CONTEXT.md` 说 MVP 三种 @ 引用，代码只识别 `@file:` 与 `@cursor`，
   未知 `@xxx`（含 `@selection`）原样保留当普通文本（`crates/xgent_ui/src/editor/at_syntax.rs:9`
   与测试 `:107`）。`EditorQuery` 只有两个变体。
4. **`ToolTier` 多一个变体**。`AGENTS.md`/架构文档记 `Read/Write/Exec/UiOnly`，代码另有
   `Dangerous`，`severity()` 序为 `UiOnly<Read<Write<Exec<Dangerous`。
5. **三个 crate 的"不依赖 Bevy"不成立**。`xgent_tools` 与 `xgent_context` 的 `Cargo.toml`
   里确有 `bevy`（分别为了 `ToolExecutorResource` 与 `ContextHub` Resource）。
6. **`xgent_terminal` 根本不依赖任何内部 crate**，而 `AGENTS.md` 记它依赖 core + settings_core。
7. **`xgent_plugin_api` 不依赖 `xgent_core`**（只依赖 wit-bindgen / serde），插件侧类型走 WIT 定义。
8. **`xgent_ui` 还直接依赖 `xgent_tools` 与 `xui_i18n`**，`AGENTS.md` 的 crate 表没写。
9. **项目配置不走 daemon**。`config.read/write` 的 `scope=project` 被显式拒绝
   （`crates/xgent_daemon/src/session.rs:211` 与 `:242`），项目配置由 UI 进程本地读。
10. **终端输入模型已改**。`docs/dev-tutorial.md` 的 ADR-0011 仍写"UI 侧透传模式、shell 回显为唯一
    显示源"，代码已改为**本地行编辑**：字符本地回显、Enter 提交整行
    （`crates/xgent_ui/src/terminal/input.rs:1` 明确记录该变更）、`:34` `TerminalLineBuf`）。
    `crates/xgent_terminal/src/backend.rs` 的注释同样过时。
11. **`xui::VirtualList` 没有任何业务消费者**。组件实现完整，对话流用的是"当前助手文本节点原地
    改写 + 每条完成消息新建实体"，不是虚拟列表（`crates/xui/src/virtual_list.rs` 之外无引用）。
12. **`FollowUpMessage` 在 UI 侧无生产者**。agent 侧注册了 reader（`crates/xgent_agent/src/agent_loop.rs:38`），
    但生产它的地方只有测试；停止后的追加输入实际走 `StartLoop` 上回顶层循环。
13. **确认弹窗没有"全部允许"按钮**。`ConfirmDecision::AllowAll` 存在且被执行器尊重
    （见 `crates/xgent_tools/src/executor.rs` 的 `allowed_all`），但 `crates/xgent_ui/src/confirm_dialog.rs`
    里没有对应入口，用户点不到。
14. **插件注册的 ContextProvider 当前到不了 agent**。`crates/xgent_app/src/main.rs:131` 造的
    `Arc<ContextHub>` 注入 bridge，而 `XgentContextPlugin` 用 `init_resource::<ContextHub>()`
    往 World 里放的是**另一个实例**（`crates/xgent_context/src/lib.rs:63`），
    `plugin_poll_system` 注册时取的是 World 那份（`crates/xgent_plugin_host/src/proxy_impl.rs:242`）。
    工具与命令两条通路是通的（executor 经 `Arc` 共享），只有 context 这条断。
15. **`DiffViewPlugin` 定义了但从未 add**（`crates/xgent_ui/src/editor/diff_view.rs:31`），
    其系统由 `EditorPlugin` 注册。
16. **会话文件名里的 id 不是数字**。`SessionId::to_string()` 产出 `session:1`，恢复时
    `parse::<u64>()` 失败回落 0，因此检查点轮次归属特意取 `conv.id.0` 的数值而非反解字符串
    （`crates/xgent_agent/src/bridge.rs:738`）。

第二批（编辑器与终端，本次逐条 grep/读码复核）：

17. **编辑器当前不可键入**。buffer 实体不挂 `EditableText`（只有 chat / 命令面板 / 设置面板挂，
    见 `crates/xgent_ui/src/chat_panel.rs:204`、`command_palette.rs:181`、`settings_panel.rs:442`），
    所以敲字符不进 rope，`update_syntax_highlight` 里唯一置脏的分支对业务 buffer 不可达；
    `editor/mod.rs:309` 与 `io.rs:360` 的注释明确写了"虚拟化只读、不写 EditableText"。
    `xui::TextEditor` 组件本身支持编辑态，是业务层选了只读模式。
18. **`EditorIoRuntime.handle` 从未注入**。`editor/mod.rs:105` 只 `init_resource`，
    `xgent_app` 注入的是 `TerminalIoRuntime`（`main.rs:264`），全仓没有 `EditorIoRuntime::new` 调用
    → 文件读写一律走 `io.rs:136` 的同步 `std::fs` 回退，在主线程上。
19. **`EditorState` / `EditorStateSnapshot` 没有消费者**。它实现了
    `xgent_core::EditorState`（`editor/state.rs:35`）且每帧刷新，但 `xgent_context` /
    `xgent_agent` / `xgent_app` 里都不出现：`@cursor` 只作为字符串 hint 传下去，`@file:` 由
    `on_demand.rs` **从磁盘读**而非从 buffer 读。架构文档 §6.3 的 `EditorContextProvider` 尚未存在。
20. **冲突弹窗的"对比合并"是降级实现**：`conflict.rs:305` 的 `Diff` 分支只调 `keep_local()`，
    源码注释写着"MVP 降级"。
21. **差异页选了也看不见**：`DiffViewMarker` 只在 `diff_view.rs:61` spawn（`display: None`）与
    `:91` 查子节点，没有任何系统切换它的 `Node.display`；`DiffViewPlugin`（`:33`）从未被 `add`。
22. **查找替换在产品里不可达**：`Cmd+F/H` 只翻 `FindState.active`
    （`crates/xui/src/text_editor.rs:238`），没有渲染查找条，`find_all` / `replace_all`
    （`text_editor/find.rs:58/159`）只有测试调用。`docs/dev-tutorial.md` §5 第 28 条已记下同一点。
23. **终端清空按钮无消费者**：`TerminalClearButtonMarker` 只在 `terminal/mod.rs:74` 定义、
    `:340` spawn，没有系统查询它 → `ClearTabRequest` 无生产者，`handle_clear_tab_requests`
    （`tabs.rs:153`）永不执行。
24. **终端输出容器与输入行缓冲是全局单例**：`output.rs:169` 用 `q_output.single_mut()`、
    `mod.rs:205` 把 `TerminalLineBuf` 注册成 Resource，与 `mod.rs:76` "每 tab 一个"的注释矛盾；
    后果是每次切 tab 都全量重建输出区。
25. **`TerminalBackend` 没有 `subscribe`**：只有 `spawn` / `write` / `resize` / `kill`
    四个方法（`crates/xgent_terminal/src/backend.rs:83-93`），`docs/dev-tutorial.md` §2.1 记多了。
26. **"透传模式"注释已过时**：`backend.rs:74-75` 与 `terminal/mod.rs:13-20` 仍描述"按键直接转
    原始字节、shell 回显为唯一显示源"，代码是本地行编辑（`terminal/input.rs:1` 起）。另外
    `input.rs:129` 注释说 Enter 发 `\n`，`:131` 实际 push 的是 `\r`。
27. **`SideViewContent::Preview` 从未被赋值**：只有枚举声明（`editor/mod.rs:76`）与一处读取
    （`:761`），v7 计划里的"Editor/Preview 归一"尚未落地。

---

## 六、这套图没画什么（下一步该读哪些文档）

| 主题 | 去处 |
|:---|:---|
| UI 视觉体系（Linear 令牌、kit、字体、图标管线） | `docs/design/ui-design.md`、`docs/design/ui-v3-design-tokens.md`、`docs/dev-tutorial.md` §5.12 |
| 编辑器的**设计意图**与未画部分（undo/redo 栈细节、tab 拖拽、@ 引用补全 UI） | 结构见本页 07/08；意图见 `docs/design/editor-design.md`、`docs/dev-tutorial.md` §5.9 |
| 终端的**设计意图**与未画部分（伪历史/Tab 补全、全屏 TUI、每 tab 独立缓冲的改造） | 结构见本页 09/10；意图见 `docs/design/terminal-design.md`、ADR-0011/0012 |
| 插件系统（WIT 接口逐条、清单 schema、沙箱权限） | `docs/design/plugin-system-design.md`、`docs/dev-tutorial.md` §5.13 |
| 每个决策"为什么这样选" | `docs/decisions/`（14 条 ADR） |
| 运行参数与配置键全表 | `docs/dev-tutorial.md` §4.5.1 |

---

## 七、改图与重新生成

图的**源**是同目录的 `*.json`（archify 规格，可直接 diff review）。HTML 是产物，不要手改。

```bash
cd ~/.agents/skills/archify          # archify 技能目录
D=<本仓库>/docs/design/diagrams
T=architecture                        # 其余：sequence / lifecycle / workflow / dataflow

# 校验（不写产物）；只有 architecture 支持 --repo-root 做代码证据核验
node bin/archify.mjs validate $t "$D/01-system-architecture.$t.json" \
     --quality showcase --repo-root <本仓库> --json

# 交付（渲染 + 全套 artifact 校验 + 原子写入）
node bin/archify.mjs deliver $t "$D/01-system-architecture.$t.json" \
     "$D/01-system-architecture.html" --quality showcase --repo-root <本仓库> --json
```

约束提醒：`meta.quality_profile` 必须是 `showcase`；01/02/07/09 声明了 `sources` 代码证据，因此
`meta.repository` 的 revision 需要在提交代码后同步更新，否则校验会失败或指向旧提交。
`visual-check` 需要本机有 Chromium 内核浏览器，用 `ARCHIFY_CHROME` 指定可执行文件路径；
它会在同目录留下 `*.visual-check.*` 截图与回执 sidecar，属可再生产物，不必提交。
