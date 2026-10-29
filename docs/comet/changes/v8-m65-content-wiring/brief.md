# 目标

把 M6 已落地但未接入产品的会话内容模型（`Transcript`、Markdown、`@` 补全、`ContextSet`）接入真实渲染与请求路径，使 XGent 的对话界面具备有序消息流、Markdown 富文本、`@` 内联补全与可编辑上下文集合；并在实现前先产出一份经用户确认的技术设计，避免在流式、工具卡、undo 通知三处命令式渲染上直接动刀。

# 范围

## 任务范围（来源：`docs/plans/ui-v8-tasks.md` §M6.5）

| 任务 | 范围 | 验收（可机器判定） |
|:---|:---|:---|
| M6.5-T0 技术设计 | `docs/design/m6-5-content-wiring.md`：Transcript 数据流、三处命令式渲染迁移顺序、影子写入与回滚点、Markdown/@补全/ContextSet 接线方式、验收映射 | 文档在实现前经用户确认；含回滚点与逐任务验收 |
| M6.5-T1 Transcript 渲染 | 用户消息、助手流式增量、工具调用/结果、压缩通知写入 `Transcript`；渲染按 `(turn_index, block_index)` 差分 spawn/update/despawn；删除命令式挪节点；会话消息列表虚拟化 | 流式中插工具卡顺序单调；全仓无渲染系统 `add_child` 挪序；长会话只渲染可见行 |
| M6.5-T2 Markdown 接线 | 终稿助手消息走 `parse_markdown` + `spawn_markdown`；代码块复制按钮接线；补 `docs/notes/` 依赖评审 | 六类元素真实样张正确；代码块样式与复制可用；评审记录在库 |
| M6.5-T3 `@` 补全接线 | 输入框触发 `detect_at_trigger`；浮层挂 `DROPDOWN`；方向键 / Enter / Esc；接受后写回 `EditableText` | 窄窗不被裁；键盘完成选择；无候选不出浮层 |
| M6.5-T4 ContextSet 接线 | 替换 `context_scope.rs` 只读镜像；chips 与 `ContextSet` 双向同步；发送请求时内容进入 `AgentCommand::StartLoop`；token 占比显示 | 移除 chip 后下一轮请求不含该内容；chip 与集合状态一致 |
| M6.5-T5 视觉与端到端证据 | 真实窗口走查 + 截图（Markdown 样张、工具卡顺序、`@` 浮层、chip 删除）；把 M6 A1–A6 从「用户接受降级」补成可复跑证据 | 证据落 `docs/notes/` 或交接文档 |
| M6.5-T6 文档与门槛 | 台账 / 交接 / AGENTS / dev-tutorial 同步；mac/Linux 硬门槛 A 定义闸门与执行顺序 | 无「M6 已全部完成」误导；门槛 A 有明确闸门 |

## 依赖

- M6 模型层 `9ac4daf`（`Transcript`/`markdown`/`markdown_render`/`completion`/`context_set`）
- M3 的 `ScrollFrame`/`region_fill`/`Ellipsis`/`VirtualList`、M4 的 `mono` 槽位、M5 的 `PanelRegistry`
- 现有流式事件链：`ChatEvent` → `chat_panel` 命令式渲染；`AgentCommand::StartLoop` 的请求组装

## 约束与不变量

- 通信仍只走 ECS Events/Messages（AGENTS.md §5.2），不引入跨 crate 直接方法调用。
- 用户可见字符串走 i18n；新增文案同时补 zh-CN / en-US 语料。
- 不修改会话存储格式（ADR-0008 的 JSONL）除非 ContextSet 决策要求。
- `Transcript` 必须成为唯一顺序权威；渲染层不得再持有「第几条消息」的隐含顺序。

# 非目标

- 不重做 M3 滚动/省略/填满契约与 M5 槽位注册表。
- 不实现语法高亮算法，只接 `xui` 高亮链。
- 不新增 P1/P2 功能（F-10/F-12/F-13/F-15/F-16）。
- 不在本期执行 macOS / Linux 实机验证，只定义闸门与顺序。
- 不改变 Markdown 解析器选型（`pulldown-cmark` 已引入）；只补评审记录。
- 不做流式 Markdown 节流重解析（见决策 D1）。
- 不做符号 / `@selection` 候选（见决策 D2）。
- 不做跨会话 ContextSet 持久化（见决策 D3）。
- 不修改会话存储格式（ADR-0008 的 JSONL）。

# 决策

- D1 **Markdown 终稿渲染**：流式期间显示纯文本，助手消息完成后一次性 `parse_markdown` + `spawn_markdown`；替换只作用于当前消息，不重建历史。
- D2 **`@` 补全仅文件候选**：候选来自 `ProjectRoot` 文件树；符号与 `@selection` 后置，不纳入本期验收。
- D3 **ContextSet 会话内持久化**：随会话 JSONL 恢复；不改 `ProjectConfig` schema，不写 daemon 白名单。
- D4 **消息列表虚拟化纳入 T1**：Transcript 顺序正确后一并启用，长会话只渲染可见行。
- D5 **迁移策略**：先影子写入 `Transcript`（保留旧渲染），再按流式正文 → 工具卡 → undo 通知逐路径切换，每步可回滚；顺序不可颠倒（详见技术设计）。
- D6 **技术设计先行**：`docs/design/m6-5-content-wiring.md` 在实现前完成并经用户确认。

# 验收示例

- A1: 流式助手回复中插入工具卡，`Transcript` 的顺序键 `(turn_index, block_index)` 单调；`chat_panel`/`tool_panel`/`undo` 不再用 `add_child` 挪动已有消息节点。
- A2: 含标题、段落、列表、代码块、粗体、链接的样张在真实窗口正确渲染；代码块使用 `surface_code` 底、`mono`、语言标签与复制按钮，复制内容等于代码块全文。
- A3: 流式期间显示纯文本，完成后替换为 Markdown；替换只作用于当前消息，不重建整段历史（消息实体保持稳定）。
- A4: 窄窗输入 `@` 弹出 `DROPDOWN` 层补全浮层，方向键选择、Enter 接受、Esc 关闭；接受后输入框包含所选项。
- A5: 移除 `ContextSet` 中某 chip 后，下一轮发给 LLM 的请求不包含该项内容；chip 与 `ContextSet` 状态一致。
- A6: 上下文 chip 显示 token 占比，数值与 `ContextSet::total_tokens` 一致；会话恢复后 ContextSet 与恢复前一致（会话内持久化）。
- A7: 长会话（≥200 条消息）滚动时只渲染可见行；消息顺序不倒退。
- A8: `docs/design/m6-5-content-wiring.md` 在实现前存在并经用户确认，内容覆盖数据流、迁移顺序、回滚点与逐任务验收映射。
- A9: 真实窗口走查证据覆盖 Markdown 样张、工具卡插入顺序、`@` 浮层、chip 删除；`cargo test -p xgent_ui -p xui -p xgent_terminal` 全绿。
- A10: 台账/交接/AGENTS/dev-tutorial 不再写「M6 已全部完成」；mac/Linux 硬门槛 A 有明确闸门与执行顺序。
