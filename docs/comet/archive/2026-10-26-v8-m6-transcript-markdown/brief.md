# Outcome

实现 M6 会话内容层：Transcript 有序树模型、Markdown 富文本渲染、输入区完善（@ 补全、发送/停止动作条、ContextSet 建模）。M6 依赖已交付的 M3（滚动/省略/填满契约）与 M5（槽位注册表），是 UI v8 交互架构重建的最后一期功能开发。

# Scope

## 视觉参考

- `docs/design/ui-prototype-v8.html`：UI v8 交互原型（整体视觉参考，非 M6 专属需求源）

## 任务范围（来源：`docs/plans/ui-v8-handoff.md` §4.2 + `docs/plans/ui-v8-tasks.md` §M6）

| 任务 | 范围 | 验收（可机器判定） |
|:---|:---|:---|
| M6-T1 | `Transcript` 有序树（`Transcript` / `BlockRef` 块序列），渲染按顺序 spawn/despawn/update，顺序键 `(turn_index, block_index)`；删除命令式挪子节点（`chat_panel.rs:489`、`tool_panel.rs:85`、`chat_panel.rs:724`、`undo.rs:286`） | 边流式边插工具卡，顺序稳定不倒退（自动化断言序列单调） |
| M6-T2 | Markdown 块模型与依赖评审（优先 `pulldown-cmark`，走一次依赖评审；退化方案为仅代码块 + 列表 + 粗体） | 评审记录落 `docs/notes/`；解析器单测覆盖六类元素 |
| M6-T3 | 富文本渲染（单节点多 `TextSection`；代码块 `surface_code` 底 + `mono` + 语言标签 + 复制按钮；语法色接 `xui` 高亮链） | 六类元素混合样张视觉正确；对照原型截图 |
| M6-T4 | `@` 内联补全（浮层挂 `DROPDOWN` 层；方向键导航 + `Enter` 接受；候选来自文件/符号/`@selection`） | 输入 `@` 出现补全并可键盘选择；窄窗不被裁 |
| M6-T5 | 发送/停止动作条（显式按钮；禁用态由 `AgentStatus` 派生；删除「发送按钮区」空注释） | 不碰键盘可完成一次完整发送与一次中断 |
| M6-T6 | `ContextSet` 建模（独立 `Resource`，可增删、可持久化、显示 token 占比；替换 `context_scope.rs` 的只读镜像） | 移除某 chip 后其不再进入下一轮请求 |

## 依赖

- M3 滚动契约（`ScrollFrame`）、填满契约（`region_fill`）、省略原语（`Ellipsis`）— 已交付
- M5 槽位注册表（`PanelRegistry`）— 已交付
- M4 字体装配（`mono` 槽位）— 已交付

# Non-goals

- 多行 clamp（M3-T4 遗留，待 M6 与 Markdown 一起做 → 实际 M6-T3 的富文本渲染会覆盖，但不单独作为任务）
- 会话消息列表虚拟化（M3-T6 遗留，随 M6-T1 的 Transcript 有序树一并处理）
- 语法高亮的完整实现（M6-T3 只接 `xui` 高亮链，不实现高亮算法本身）
- macOS / Linux 的 `mono` 启动日志确认（M4 遗留，不阻塞 M6）

# Acceptance examples

- A1: 流式助手回复中插入工具卡，Transcript 顺序稳定不倒退（自动化断言 `(turn_index, block_index)` 单调递增）
- A2: Markdown 六类元素（标题、段落、列表、代码块、粗体、链接）混合样张渲染视觉正确
- A3: 代码块使用 `surface_code` 底色 + `mono` 字体 + 语言标签 + 复制按钮
- A4: 输入 `@` 出现补全浮层，方向键可选择，Enter 接受；浮层挂 `DROPDOWN` 层不被裁
- A5: 不碰键盘可完成一次完整发送（点发送按钮）与一次中断（点停止按钮）
- A6: 移除 ContextSet 中某 chip 后，其不再进入下一轮 LLM 请求
