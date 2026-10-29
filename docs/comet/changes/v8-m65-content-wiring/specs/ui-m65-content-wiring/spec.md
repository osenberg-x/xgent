# UI M6.5：会话内容接线

## 能力概述

本能力把 M6 已落地的会话内容模型接入产品路径：`Transcript` 成为消息顺序的唯一权威，渲染层按顺序键差分更新；助手消息在完成后渲染为 Markdown；输入框提供仅文件候选的 `@` 补全；`ContextSet` 成为上下文 chip 的权威数据源并随会话恢复。M6 的模型 API 已存在（`9ac4daf`），本能力只补数据流、渲染、输入与请求组装，不重写模型选型。

## 1. Transcript 数据流与渲染

### 1.1 唯一顺序权威

- `Transcript` 是会话消息顺序的唯一权威。块的顺序键为 `(turn_index, block_index)`，同一轮内只追加、不重排。
- 需要补齐的写入操作：
  - 用户消息：`ChatInputSubmitted` 被接受后追加 `UserMessage`。
  - 助手流式增量：`DeltaMessage` 追加/更新当前 `AssistantMessage` 的文本块；同一轮内可多次增量。
  - 工具调用与结果：`ToolCallMessage`、`ToolProgressMessage`、`ToolResultMessage` 分别创建或更新 `ToolCall`/`ToolResult`，状态按 Pending → Running → Completed/Failed 迁移。
  - 压缩通知：`CompactedMessage` 追加 `CompressionNotice`。
  - 会话清空/恢复：`SessionClearedMessage` 清空；`SessionRestoredMessage` 重建 Transcript。
- 同一 `tool_call_id` 的 progress/result 必须更新已有块，不得新建重复块；未知 id 记 `tracing::warn!` 并忽略，不改变顺序。

### 1.2 差分渲染契约

- 渲染系统只依据 Transcript 的块序列 spawn/update/despawn 对应实体，不把「第几条消息」编码在系统内部状态里。
- 删除 `chat_panel`、`tool_panel`、`undo` 中通过 `add_child` 挪动已有消息节点的写法；需要调整顺序时只改 Transcript，由渲染系统跟随。
- 会话消息列表使用 `xui::VirtualList`（不等高行），流式消息始终位于可见区并保持滚动到底部；历史回看与流式使用同一套渲染路径。
- 实体缺失或查询不到时记 `tracing::warn!` 并跳过，不 panic。

### 1.3 场景

`Scenario: 流式助手回复中插入工具卡，Transcript 顺序键单调递增，界面上的工具卡出现在当前助手消息之后且不倒退`
验收：A1

`Scenario: 源码级检查确认 chat_panel/tool_panel/undo 不再用 add_child 挪动已有消息节点`
验收：A1

`Scenario: 200 条以上消息的会话滚动时只渲染可见行，顺序不倒退`
验收：A7

## 2. Markdown 终稿渲染

### 2.1 渲染时机

- 流式期间助手消息以纯文本显示，不做 Markdown 解析。
- `DoneMessage` 到达后，对当前助手消息执行 `parse_markdown` + `spawn_markdown`，仅替换该消息的渲染子树；已完成的历史消息在恢复时直接以 Markdown 渲染。
- 替换不得重建整段历史；其它消息的实体保持稳定。

### 2.2 元素与样式

- 支持六类元素：标题、段落、列表、代码块、粗体、链接。
- 代码块：`surface_code` 底色 + `mono` 字体 + 语言标签 + 复制按钮；复制内容等于代码块原文。
- 链接：显示文本与 URL tooltip；本期不打开外部浏览器。
- 宽度不足时按 M3 契约滚动/省略，不溢出容器。

### 2.3 依赖评审

- `pulldown-cmark 0.12` 的依赖评审记录落在 `docs/notes/`，包含许可证、维护状态、体积与替代方案。

### 2.4 场景

`Scenario: 含标题、段落、列表、代码块、粗体、链接的样张在真实窗口正确渲染，代码块样式与复制按钮可用`
验收：A2

`Scenario: 流式期间显示纯文本，完成后当前消息替换为 Markdown，历史消息实体保持稳定`
验收：A3

## 3. `@` 补全（文件候选）

### 3.1 触发与候选

- 输入框检测光标前的 `@` 前缀（`detect_at_trigger`）；前缀为空或超过上限字符时不出浮层。
- 候选仅来自 `ProjectRoot` 的文件树，按前缀过滤；无候选时不显示浮层。
- 候选数量设上限，超出时按路径排序截断。

### 3.2 浮层与交互

- 浮层挂在 `DROPDOWN` 层，不留在输入框子树（避免被 `ChatPanel` 裁剪）。
- 方向键上下移动，Enter 接受，Esc 关闭；接受后用文件路径替换 `@query` 片段并保持焦点在输入框。
- 窄窗时浮层按视口翻转/钳制，不被裁切。

### 3.3 场景

`Scenario: 窄窗输入 @ 弹出 DROPDOWN 层补全浮层，方向键选择、Enter 接受、Esc 关闭，接受后输入框包含所选文件路径`
验收：A4

## 4. ContextSet 与上下文 chip

### 4.1 权威数据源

- `ContextSet` 是上下文项的权威来源；`context_scope.rs` 的 chips 从只读镜像改为与 `ContextSet` 双向同步。
- chip 删除即从 `ContextSet` 移除；通过「+ 添加」或编辑器 tab 变化新增时写入 `ContextSet`。

### 4.2 请求组装

- 发起 `AgentCommand::StartLoop` 时，`ContextSet` 中所有项的内容随请求进入 agent 上下文；从集合移除的项不得出现在下一轮请求中。
- 请求组装不改变 `Transcript` 的历史消息；上下文项与对话消息分开。

### 4.3 显示与持久化

- 上下文条显示 `ContextSet::total_tokens` 与上下文窗口的占比；数值随增删即时更新。
- 持久化范围为会话内：用会话级 sidecar（`<session_id>.context.json`）记录 `ContextSet`，恢复会话时读回；不改 JSONL 记录格式，不写项目配置，不跨会话继承。
- 会话恢复后 chips、总量与 token 占比与恢复前一致。

### 4.4 场景

`Scenario: 移除 ContextSet 中某 chip 后，下一轮发给 LLM 的请求不包含该项内容，chip 与集合状态一致`
验收：A5

`Scenario: chip 显示 token 占比且与 total_tokens 一致；会话恢复后 ContextSet 与恢复前一致`
验收：A6

## 5. 技术设计、证据与文档

### 5.1 技术设计

- 实现前产出 `docs/design/m6-5-content-wiring.md`，覆盖：Transcript 数据流、三处命令式渲染的迁移顺序、影子写入与回滚点、Markdown/@补全/ContextSet 接线、验收映射。
- 设计文档经用户确认后才进入代码实现。

### 5.2 证据

- 真实窗口走查并截图：Markdown 六类元素、工具卡插入顺序、`@` 浮层、chip 删除。
- 证据落 `docs/notes/` 或交接文档；`cargo test -p xgent_ui -p xui -p xgent_terminal` 全绿。

### 5.3 文档与门槛

- 台账、交接、AGENTS、dev-tutorial 不再出现「M6 已全部完成」的误导表述。
- mac/Linux 硬门槛 A 定义明确闸门与执行顺序（Windows 收尾后立刻排）。

### 5.4 场景

`Scenario: docs/design/m6-5-content-wiring.md 在实现前存在且经用户确认，覆盖数据流、迁移顺序、回滚点与验收映射`
验收：A8

`Scenario: 真实窗口证据覆盖 Markdown 样张、工具卡顺序、@ 浮层、chip 删除，且三个 crate 的测试全绿`
验收：A9

`Scenario: 台账/交接/AGENTS/dev-tutorial 无「M6 已全部完成」表述，mac/Linux 门槛 A 有闸门与顺序`
验收：A10

## 6. 依赖与约束

- 依赖 M6 模型（`Transcript`/`markdown`/`markdown_render`/`completion`/`context_set`）。
- 依赖 M3 的 `ScrollFrame`/`region_fill`/`Ellipsis`/`VirtualList`、M4 的 `mono`、M5 的 `PanelRegistry`。
- 子系统通信只走 ECS Events/Messages（AGENTS.md §5.2）。
- 用户可见字符串走 i18n，新增文案补 zh-CN / en-US 语料。
- 不修改 ADR-0008 的会话 JSONL 格式；ContextSet 用会话级 sidecar 持久化。

## 7. 非目标

- 流式 Markdown 节流重解析。
- 符号 / `@selection` 候选。
- 跨会话 ContextSet 持久化。
- 语法高亮算法实现。
- macOS / Linux 实机验证（只定义闸门）。
- M8 与 P1/P2 功能。
