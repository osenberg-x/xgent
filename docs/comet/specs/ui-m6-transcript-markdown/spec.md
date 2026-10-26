# UI M6: Transcript 有序树 + Markdown 富文本 + 输入区完善

## 能力概述

M6 实现会话内容层，包括：Transcript 有序数据模型、Markdown 解析与富文本渲染、输入区的 @ 补全、发送/停止动作条、ContextSet 建模。依赖已交付的 M3（滚动/省略/填满契约）、M4（字体装配）、M5（槽位注册表）。

---

## 1. Transcript 有序树模型（M6-T1）

### 1.1 数据模型

- `Transcript` Resource：持有 `Vec<BlockRef>` 块序列
- `BlockRef` 枚举：`UserMessage { turn_index, content }` / `AssistantMessage { turn_index, blocks: Vec<ContentBlock> }` / `ToolCall { turn_index, block_index, tool_call_id, status }` / `ToolResult { turn_index, block_index, tool_call_id, content }` / `CompressionNotice { turn_index }`
- 顺序键：`(turn_index, block_index)`，单调递增
- 渲染系统按顺序键 spawn/despawn/update 实体，不手动挪子节点

### 1.2 渲染契约

- 流式助手回复中插入工具卡时，顺序稳定不倒退
- 删除命令式挪子节点代码（`chat_panel.rs:489`、`tool_panel.rs:85`、`chat_panel.rs:724`、`undo.rs:286`）
- 会话消息列表改走 `VirtualList`（不等高行，M3-T6 遗留）

**验收：A1** — 边流式边插工具卡，顺序稳定不倒退（自动化断言 `(turn_index, block_index)` 单调递增）

---

## 2. Markdown 块模型与解析（M6-T2）

### 2.1 依赖评审

- 优先使用 `pulldown-cmark`（走依赖评审，记录落 `docs/notes/`）
- 退化方案：仅支持代码块 + 列表 + 粗体（不引入外部依赖）

### 2.2 块模型

- `MarkdownBlock` 枚举：`Heading { level, text }` / `Paragraph { text }` / `List { items: Vec<ListItem> }` / `CodeBlock { language, code }` / `Bold { text }` / `Link { url, text }`
- 解析器：`parse_markdown(input: &str) -> Vec<MarkdownBlock>`
- 单测覆盖六类元素

**验收：A2** — 评审记录落 `docs/notes/`；解析器单测覆盖六类元素

---

## 3. 富文本渲染（M6-T3）

### 3.1 渲染规则

- 单节点多 `TextSection`：一个 Markdown 块对应一个 `Text` 实体，内含多个 `TextSection`（按样式分段）
- 代码块：`surface_code` 底色 + `mono` 字体 + 语言标签（右上角）+ 复制按钮
- 语法色：接 `xui` 高亮链（不实现高亮算法，只消费已有能力）
- 六类元素（标题、段落、列表、代码块、粗体、链接）混合样张视觉正确

### 3.2 视觉对照

- 对照 `docs/design/ui-prototype-v8.html` 原型截图

**验收：A3** — 代码块使用 `surface_code` 底色 + `mono` 字体 + 语言标签 + 复制按钮

---

## 4. @ 内联补全（M6-T4）

### 4.1 触发与浮层

- 输入 `@` 触发补全浮层
- 浮层挂 `DROPDOWN` 层（不能留在输入框子树，否则被 `ChatPanel` 的 `clip` 裁）
- 候选来源：文件 / 符号 / `@selection`

### 4.2 交互

- 方向键导航候选列表
- `Enter` 接受选中项
- 窄窗不被裁（视口边界翻转，参考 M1-T5 tooltip）

**验收：A4** — 输入 `@` 出现补全浮层，方向键可选择，Enter 接受；浮层挂 `DROPDOWN` 层不被裁

---

## 5. 发送/停止动作条（M6-T5）

### 5.1 按钮

- 发送按钮：`accent` 色，禁用态由 `AgentStatus` 派生（空闲时可点，流式中禁用）
- 停止按钮：流式中显示，`st_running` 色 + pulse 动画
- 删除「发送按钮区」空注释（`chat_panel.rs:221`）

### 5.2 交互

- 不碰键盘可完成一次完整发送（点发送按钮）
- 不碰键盘可完成一次中断（点停止按钮）

**验收：A5** — 不碰键盘可完成一次完整发送与一次中断

---

## 6. ContextSet 建模（M6-T6）

### 6.1 数据模型

- `ContextSet` Resource：独立于 `context_scope.rs` 的只读镜像
- 可增删上下文项（文件、符号、selection）
- 可持久化（进配置）
- 显示 token 占比（UI 上可见）

### 6.2 行为

- 替换 `context_scope.rs:1-7` 的「打开 tab 只读镜像」
- 移除某 chip 后其不再进入下一轮 LLM 请求

**验收：A6** — 移除某 chip 后其不再进入下一轮请求

---

## 7. 依赖与约束

- 依赖 M3 `ScrollFrame` / `region_fill` / `Ellipsis` / `VirtualList`
- 依赖 M4 `mono` 槽位（代码块字体）
- 依赖 M5 `PanelRegistry`（面板 = 注册项）
- 不引入 3D / TUI / Web / 宠物（MVP 仅 2D GUI）
- 所有用户可见字符串走 i18n（`fluent`）

---

## 8. 非目标

- 多行 clamp（M3-T4 遗留，不在 M6 单独作为任务）
- 语法高亮算法实现（只接 `xui` 高亮链）
- macOS / Linux 的 `mono` 启动日志确认（M4 遗留，不阻塞 M6）
