# M6.5 会话内容接线技术设计

> 状态：**草案，待用户确认**。按 brief 决策 D6，本文确认前不写实现代码。
> 依据：`docs/comet/changes/v8-m65-content-wiring/brief.md`、`specs/ui-m65-content-wiring/spec.md`、
> `docs/design/ui-v8-layout-architecture.md` §6、2026-10-28 代码实测。
> 基线提交：`9ac4daf`（M6 模型层）、`81d3060`（M7 + M6-T5）。

---

## 1. 目标与边界

把 M6 的四个模型（`Transcript`、Markdown、`@` 补全、`ContextSet`）接入真实产品路径，同时满足：

- 消息顺序由 `Transcript` 唯一决定，渲染层不再靠 `add_child` 挪节点维持顺序。
- 助手消息在完成后渲染 Markdown；流式期保持纯文本，不做每帧重解析。
- `@` 补全本期只做文件候选，浮层挂 `DROPDOWN`。
- `ContextSet` 成为上下文 chip 的权威源，内容随请求进入 agent 上下文，并按会话持久化。
- 长会话只渲染可见行。

不改变：M3/M5 契约、Markdown 解析器选型、会话 JSONL 格式、P1/P2 功能范围。

---

## 2. 现状事实（2026-10-28 实测）

| 位置 | 现状 | 问题 |
|:---|:---|:---|
| `crates/xgent_ui/src/chat_panel.rs:610` | `accumulate_delta` 只累加单个 `CurrentAssistantText` 文本节点 | 没有块序列，工具卡只能插到列表尾部 |
| `chat_panel.rs:515` | `spawn_user_message` 先 `with_children` 追加用户消息，再 `add_child(current)`（:602）把流式节点挪到末尾 | 顺序依赖系统执行顺序 |
| `chat_panel.rs:631` | `finalize_on_done` 复制一份纯文本历史副本（:665） | 无 Markdown、无 Transcript、复制一次多一份节点 |
| `tool_panel.rs:83` | `spawn_tool_card` 直接向消息列表 `with_children` 追加（:98） | 工具卡位置由 `ToolCallMessage` 到达顺序决定 |
| `undo.rs:273` | `append_undo_notice` 直接向消息列表追加（:287） | 同上 |
| `context_scope.rs:1-7` | chip 是编辑器 tab 的只读镜像，无删除 | 删除 chip 无法影响请求 |
| `context_set.rs` | 模型存在，但全仓零消费者 | 无 chips、无请求注入、无持久化 |
| `completion.rs` | 模型与浮层构造函数存在，零消费者 | 无触发、无键盘路由、无候选来源 |
| `markdown_render.rs:29` | `spawn_markdown` 存在，零调用点 | 无终稿渲染 |
| `chat_panel.rs:118` | `Transcript` 只 `init_resource` | 无写入、无渲染 |
| `xgent_agent/src/agent_loop.rs:75` | 请求由 `build_request` 直接组装 | `ContextSet` 内容无处进入 |
| `xgent_agent/src/bridge.rs:612` | 异步侧 `context.retrieve` 后 `refresh_system_message` 覆盖 system | 若只在 UI 侧注入，会被覆盖 |

---

## 3. 总体数据流

```
ChatInputSubmitted ──► forward_input_submission ──► UserInputMessage { text, editor_queries, pinned_context }
                                                        │
                                                        ▼
                                             agent_loop: build_request
                                                        │
DeltaMessage ──┐                                        ▼
DoneMessage ───┤                                 StartLoop { req, pinned_context }
ToolCallMessage┤                                        │
ToolProgress ──┼──► transcript_write systems ──► Transcript ──► render_transcript ──► MessageList
ToolResult ────┤                                        │
Compacted ─────┘                                        ▼
                                             bridge: context.retrieve + merge pinned_context
                                                        │
                                                        ▼
                                             refresh_system_message → ChatRequest

ContextSet ──► chips（双向） ──► UserInputMessage.pinned_context
     │
     └──► SessionStore sidecar（会话级恢复）
```

**核心原则**：写入侧只改 `Transcript` / `ContextSet`；渲染侧只读它们。渲染层不得反向决定顺序。

---

## 4. T1：Transcript 数据流与渲染

### 4.1 模型补齐（`crates/xgent_ui/src/transcript.rs`）

现有模型缺三类操作，接线前必须补齐：

| 新方法 | 语义 |
|:---|:---|
| `append_text_delta(turn_index, &str)` | 把增量追加到该轮 `AssistantMessage` 的最后一个 `ContentBlock::Text`；没有则创建 |
| `set_tool_status(tool_call_id, ToolCallStatus)` | 按 `tool_call_id` 更新 `ToolCall.status`；找不到记 `tracing::warn!`，不改变顺序 |
| `set_tool_progress(tool_call_id, &str)` | 更新 `ToolCall` 的进度文本（`ToolCall` 增加 `progress: Option<String>`） |
| `set_tool_result(tool_call_id, &str, is_error)` | 把结果写到配对块；本期保留独立 `ToolResult` 块，由渲染按 `tool_call_id` 配对，避免改块类型 |
| `finalize_assistant(turn_index)` | 标记该轮助手消息完成（后续 Markdown 渲染依据） |
| `restore(blocks: Vec<BlockRef>)` | 会话恢复时整体替换，重置 `next_turn_index` |

顺序不变量：

1. 只追加，不在中间插入；`block_index` 在追加时分配，分配后不变。
2. 同一 `tool_call_id` 的 progress/result 只更新已有块，不新建重复块。
3. `AssistantMessage` 在同一轮只有一个；工具卡作为独立块，顺序键取追加时的 `block_index`。

### 4.2 写入侧映射

新增 `transcript_write.rs`（或并入 `chat_panel` 的写入系统组），按消息类型写 `Transcript`：

| 消息 | 写入 |
|:---|:---|
| `ChatInputSubmitted`（仅聊天输入框） | `push_user_message` + `begin_assistant_message`（空占位） |
| `DeltaMessage` | `append_text_delta` |
| `DoneMessage` | `finalize_assistant` |
| `ToolCallMessage` | `push_tool_call`（Pending → Running） |
| `ToolProgressMessage` | `set_tool_progress` |
| `ToolResultMessage` | `set_tool_result` |
| `CompactedMessage` | `push_compression_notice` |
| `ErrorMessage` | 不写入 Transcript；保留现有错误节点路径，或作为 `CompressionNotice` 之外的独立块后置 |
| `SessionClearedMessage` | `clear` |
| `SessionRestoredMessage` | 从恢复的消息重建 `Transcript`（见 §7） |

写入系统统一排在 `agent_poll_system` 之后、渲染系统之前。

### 4.3 渲染侧

新增 `transcript_render.rs`：

- 每个块对应一个实体，挂 `TranscriptBlockKey { turn_index, block_index }` 组件；**不把 Entity 缓存进 Resource**（M7-T4 纪律）。
- 每帧收集现有 `TranscriptBlockKey` 到局部 `HashMap`，与 `Transcript.blocks()` 差分：
  - 缺失的块：按顺序 `insert_children(index, &[entity])` 到消息列表，保证顺序。
  - 已存在的块：只更新变化的文本/状态（比对内容或版本号）。
  - 多余的块：`despawn`。
- 在 `MessageListMarker` 上挂 `xui::VirtualList`（不等高模式）：
  - `VirtualList.item_count = blocks.len()`；`item_height` 用估算值（如 120px）；实测高度写回 `measured`。
  - 用 `build_measured_offsets` + `compute_visible_range_measured` 计算可见区间；顶部/底部占位块撑总高（复用文件树做法）。
  - **流式中的最后一轮始终渲染**（即使超出可见区间），避免流式内容被虚拟化裁掉；该轮完成后回到常规虚拟化。
- 滚动：复用 `ScrollFrame`/`StickToBottom`；新块追加时若处于贴底状态则保持贴底，否则显示“回到底部”按钮。

### 4.4 迁移顺序与回滚点

每一步单独提交，回滚以提交为单位（不新增 cargo feature）：

| 阶段 | 动作 | 回滚点 |
|:---|:---|:---|
| P0 影子写入 | `Transcript` 写入系统上线，渲染仍走旧路径；断言顺序键单调 | 删除写入系统，视觉无变化 |
| P1 用户消息 | 渲染器接管 `UserMessage`；删除 `spawn_user_message` 的节点构造（保留消息发送逻辑） | 恢复 `spawn_user_message` |
| P2 工具卡 | 渲染器接管 `ToolCall`/`ToolResult`；`tool_panel` 的 spawn/update 改为只写 Transcript | 恢复 `spawn_tool_card` / `update_tool_result` |
| P3 助手流式与终稿 | 渲染器接管 `AssistantMessage`；删除 `accumulate_delta` 的文本节点与 `finalize_on_done` 的历史副本 | 恢复 `CurrentAssistantText` 路径 |
| P4 通知 | 渲染器接管 `CompressionNotice` 与 undo 通知 | 恢复 `append_undo_notice` |
| P5 虚拟化 | 启用 `VirtualList` + 实测回写 | 关闭虚拟化，全量渲染 |

P0 必须先于 P1；P3 必须晚于 P2（工具卡顺序先在旧渲染上稳定，再切换）。禁止先删旧路径。

### 4.5 删除点

- `chat_panel.rs` 的 `CurrentAssistantText` 单节点路径（`accumulate_delta` / `finalize_on_done` 中的节点构造）。
- `spawn_user_message` 中的节点构造与 `add_child(current)`。
- `tool_panel.rs` 的 `spawn_tool_card` 节点构造与 `update_tool_*` 的 DOM 写入（改为写 Transcript）。
- `undo.rs` 的 `append_undo_notice` DOM 写入。
- 测试里依赖 `CurrentAssistantText` 的断言改为查 `TranscriptBlockKey`。

---

## 5. T2：Markdown 终稿渲染

### 5.1 时机

- 流式期：助手文本块渲染为纯文本（与现在一致），不做解析。
- `DoneMessage` → `finalize_assistant` 后，该块标记为 `is_final`；渲染系统对该块：
  1. `parse_markdown(text)`；
  2. 清空该块的正文子节点，调用 `spawn_markdown` 生成 Markdown 子树；
  3. 保留消息行容器（头像、角色、操作栏）不动。
- 历史恢复：恢复后的已完成助手消息直接走 Markdown 路径。
- 替换只作用于该块；其它块实体不动。

### 5.2 代码块与链接

- 代码块使用 `surface_code` 底 + `mono` + 语言标签 + 复制按钮；注册 `markdown_render::handle_copy_button_click` 系统。
- 复制内容来自 `CodeBlockMarker.code`，等于代码块原文。
- 链接渲染文本 + URL tooltip；本期不打开外部浏览器。

### 5.3 依赖评审

补 `docs/notes/markdown-dependency-review.md`：许可证（MIT/Apache-2.0）、维护状态、依赖体积（`pulldown-cmark 0.12` 引入 `getopts`/`unicase`/`pulldown-cmark-escape`）、与 `comrak` 的对比、退化方案。

### 5.4 高度与虚拟化

Markdown 渲染后高度变化 → `VirtualList` 的实测回写系统下一帧更新；流式块不受虚拟化影响（§4.3）。

---

## 6. T3：`@` 补全（仅文件）

### 6.1 触发与候选

新增 `completion_ui.rs`：

- 读取聊天输入框的 `EditableText` 与光标：`EditableText::value()` + `editor().raw_selection()`（parley `PlainEditor`）。
- `detect_at_trigger(text, cursor_byte)` 返回 `@query` 的起始字节；无 `@` 或 query 超长则关闭。
- 候选来源：`FileTreeItems`（`file_panel.rs:57` 的扁平化文件列表），按路径前缀过滤，按路径排序，上限 50。
- 无候选 → `CompletionState::deactivate()`，不显示浮层。

### 6.2 浮层

- `spawn_completion_popup` 生成的实体挂到 `theme::z::DROPDOWN` 层根（不是输入框子树）。
- 位置由 `editor().cursor_geometry(font_size)` 计算，按视口宽高钳制；窄窗翻转。
- 样式复用 `theme`/`type_scale`，不新增色值。

### 6.3 键盘路由

- 系统 `handle_completion_keys` 在 `PreUpdate` 读取 `ButtonInput<KeyCode>`，当 `CompletionState.active`：
  - `ArrowDown`/`ArrowUp` 调 `select_next`/`select_prev`，并置 `CompletionKeyConsumed`（本帧标记）。
  - `Enter` 调 `accept_completion`：用选中项替换 `@query`（`editor_mut().insert_or_replace_selection(path)`），关闭浮层，置 `CompletionKeyConsumed`。
  - `Esc` 关闭浮层，置 `CompletionEscConsumed`；`handle_hotkey_triggers` 与 `modal` 的 Esc 路由在本帧跳过。
- `forward_input_submission` 读 `CompletionKeyConsumed`：若本帧 Enter 已被补全消费，丢弃对应 `ChatInputSubmitted`，不发送、不渲染用户消息。
- 顺序：`handle_completion_keys` 在 `xui` 文本输入系统之前；`forward_input_submission` 在 `Update` 读标记。标记按 `Time` 帧号失效，避免下一帧误吞。

### 6.4 风险

parley 的 selection 写回接口需要实测；若 `insert_or_replace_selection` 不触发 bevy 的文本变更通知，则改为「设选区 + `queue_edit(TextEdit::Insert(path))`」两条路径，二选一在实现时验证并记录。

---

## 7. T4：ContextSet 与请求

### 7.1 类型与 chip 同步

- `ContextItem` 保持在 `xgent_ui::context_set`（UI 侧类型），不迁入 core。
- `context_scope.rs` 改为：
  - 启动时从 `ContextSet` 渲染 chips；
  - `ContextSet` 变化时整批重建（沿用现有签名比对）；
  - 每个 chip 增加删除钮；删除发 `ContextSetRemove { id }` 消息（遵守 ECS 契约）；
  - `+ 添加` 打开文件抽屉；文件树选择文件时发 `ContextSetAdd`。
- `ContextSet` 的 `total_tokens` 用于显示占比；上下文窗口由 `GlobalConfigRes` 的 `resolve_context_window` 取得。

### 7.2 请求注入

- `UserInputMessage` 增加 `pinned_context: Vec<xgent_context::ContextChunk>`；UI 把 `ContextSet.items()` 映射为 `ContextChunk`（`path`/`content`/`relevance`/`token_estimate`）。
- `AgentCommand::StartLoop` 增加 `pinned_context: Vec<ContextChunk>`；`agent_loop` 从 `UserInputMessage` 透传。
- `bridge.rs:612` 检索完成后：把 `pinned_context` 合并进 `ContextResult.chunks`（按 path 去重，用户显式选择优先），再 `refresh_system_message`。这样不会被异步覆盖。
- 从集合移除的项不会出现在下一次 `pinned_context`，因此不进入下一轮请求。

### 7.3 会话级持久化

- 不改 JSONL 记录格式。新增侧车：`<sessions>/<session_id>.context.json`，内容为 `{ version: 1, items: [...] }`。
- 写入时机：chips 增删后去抖（例如 500ms）写一次；`SessionStore` 增加 `save_context_set` / `load_context_set`。
- 恢复：`RestoreSessionMessage` 处理时读取侧车，`SessionRestoredMessage` 增加 `context_set: Vec<ContextItemSnapshot>`；UI 收到后替换 `ContextSet`。
- 会话清空：删除/清空侧车。
- 这是「会话内持久化」：同一 session id 可跨应用重启恢复；不跨会话继承。

### 7.4 风险

- `ContextChunk.path` 是 `PathBuf`；ContextSet 的符号/选区项（本期不产生）将来需要兼容，`relevance` 字段可承载类型说明。
- 请求侧合并后 token 预算需要复核：`ContextResult.total_tokens` 重新计算，避免超窗。

---

## 8. T5：证据与 T6：文档门槛

### 8.1 证据

- 真实窗口走查并截图：
  1. Markdown 六类元素样张（可临时加入一个测试会话或在开发构建中注入）；
  2. 流式中插入工具卡的顺序；
  3. 窄窗 `@` 浮层；
  4. chip 删除后下一轮请求（可用日志或 mock provider 断言）。
- 自动化：
  - `cargo test -p xgent_ui -p xui -p xgent_terminal`；
  - 新增 Transcript 顺序测试、渲染差分测试、补全键盘路由测试、ContextSet 请求注入测试；
  - `layout_geometry` 增加消息列表不溢出断言。
- 证据落 `docs/notes/m6-5-wiring-evidence.md` 或交接文档。

### 8.2 文档与门槛

- 更新 `docs/plans/ui-v8-tasks.md` §M6.5 勾选、`ui-v8-handoff.md`、`AGENTS.md`、`docs/dev-tutorial.md`。
- mac/Linux 硬门槛 A：定义闸门为「M6.5 交付后的下一个里程碑第一项」，执行内容为三平台 `mono` 启动日志 + 终端 `ls -l`/中文输出/`top`/`vim` 实机确认；不在本变更内执行。

---

## 9. 验收映射

| 验收 | 任务 | 证据 |
|:---|:---|:---|
| A1 顺序单调 / 无 add_child 挪序 | T1 | Transcript 单测 + 源码 grep + 流式插入集成测试 |
| A2 Markdown 六类元素 + 代码块 | T2 | 真实窗口截图 + 复制按钮测试 |
| A3 终稿替换、历史实体稳定 | T2 | 渲染差分测试（实体 ID 稳定） |
| A4 `@` 浮层与键盘 | T3 | 窄窗截图 + 键盘路由测试 |
| A5 chip 删除影响下一轮请求 | T4 | mock provider 请求快照断言 |
| A6 token 占比 + 会话恢复 | T4 | 单测 + 恢复集成测试 |
| A7 长会话虚拟化 | T1 | 200 条消息测试 + 渲染行数断言 |
| A8 技术设计确认 | T0 | 本文 + 用户确认记录 |
| A9 证据与测试 | T5 | 截图 + 测试输出 |
| A10 文档与门槛 | T6 | 文档 diff + 闸门条目 |

---

## 10. 主要风险与缓解

| 风险 | 缓解 |
|:---|:---|
| 三处命令式渲染同时切换导致流式/工具卡/undo 互相踩 | 按 P0→P5 分阶段，每阶段独立提交与回滚 |
| 虚拟化把流式消息裁掉 | 流式中最后一轮始终渲染；完成后回虚拟化 |
| Markdown 高度变化导致滚动跳动 | 贴底状态保持；实测高度下一帧回写；非贴底不强制滚动 |
| 补全 Enter 与发送冲突 | `CompletionKeyConsumed` 帧标记 + `forward_input_submission` 丢弃本帧提交 |
| 上下文注入被 bridge 覆盖 | `pinned_context` 进入 `StartLoop`，在 `refresh_system_message` 前合并 |
| 会话恢复丢上下文 | 侧车读写单测 + 恢复集成测试 |
| 实体缓存回潮 | 渲染器只用 `TranscriptBlockKey` 查询，不把 Entity 存 Resource |
