---
generated_from_state_version: 17
---

# 验证

## 当前结果

- 结果: **已归档**
- 验证情况: **已完成检查，验证结果已确认**
- 目标周期: 1
- 迭代: 4
- 验证器尝试次数: 1
- 完成时间: 2026-11-06T05:30:49.594Z
- 摘要: A1–A10 全部有证据通过。真实窗口补齐 Markdown 样张、工具卡顺序/状态、宽窄窗 @ 浮层与 chip 删除；A8 由独立会话日志证明设计先于实现并经用户确认。剩余风险为 IME/HiDPI 证据深度与既有 agent 测试时序 flake，不构成产品正确性失败。

## 验收

| 编号 | 结果 | 来源 | 验收项 | 原因 |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | A1: 流式助手回复中插入工具卡，`Transcript` 的顺序键 `(turn_index, block_index)` 单调；`chat_panel`/`tool_panel`/`undo` 不再用 `add_child` 挪动已有消息节点。 | Transcript 为顺序权威；渲染契约与真实窗口截图均显示助手正文→工具卡→Markdown 顺序。 |
| A2 | passed | brief.md | A2: 含标题、段落、列表、代码块、粗体、链接的样张在真实窗口正确渲染；代码块使用 `surface_code` 底、`mono`、语言标签与复制按钮，复制内容等于代码块全文。 | 真实窗口样张含标题/段落/列表/代码块/粗体/链接；代码块有 rust 标签与复制按钮；复制内容取自 CodeBlockMarker 全文。 |
| A3 | passed | brief.md | A3: 流式期间显示纯文本，完成后替换为 Markdown；替换只作用于当前消息，不重建整段历史（消息实体保持稳定）。 | 流式→终稿只替换当前消息正文子树；finalize_replaces_only_current_assistant_row 断言行实体稳定。 |
| A4 | passed | brief.md | A4: 窄窗输入 `@` 弹出 `DROPDOWN` 层补全浮层，方向键选择、Enter 接受、Esc 关闭；接受后输入框包含所选项。 | 1280/1024 窗口浮层均在 DROPDOWN 且未被裁切；方向键消费/Enter/Esc 与关闭保持期均有测试。 |
| A5 | passed | brief.md | A5: 移除 `ContextSet` 中某 chip 后，下一轮发给 LLM 的请求不包含该项内容；chip 与 `ContextSet` 状态一致。 | StartLoop 携带 removed_context，检索与 pinned 合并后剔除已移除路径；tab 同步不会加回。 |
| A6 | passed | brief.md | A6: 上下文 chip 显示 token 占比，数值与 `ContextSet::total_tokens` 一致；会话恢复后 ContextSet 与恢复前一致（会话内持久化）。 | chip 删除后 token 29→15；sidecar 磁盘往返/旧数组/损坏回退与新会话清空均有测试；写盘在后台线程。 |
| A7 | passed | brief.md | A7: 长会话（≥200 条消息）滚动时只渲染可见行；消息顺序不倒退。 | 200 块只渲染可见窗口，段间补占位；HiDPI 几何一致；消息列表已挂 VirtualList。 |
| A8 | passed | brief.md | A8: `docs/design/m6-5-content-wiring.md` 在实现前存在并经用户确认，内容覆盖数据流、迁移顺序、回滚点与逐任务验收映射。 | 独立会话日志：08:29:37Z 写设计、08:37:08Z 用户确认、08:44:59Z 首实现；设计先于实现且经确认。 |
| A9 | passed | brief.md | A9: 真实窗口走查证据覆盖 Markdown 样张、工具卡插入顺序、`@` 浮层、chip 删除；`cargo test -p xgent_ui -p xui -p xgent_terminal` 全绿。 | 真实窗口截图覆盖 Markdown/工具卡顺序/@ 浮层宽窄窗/chip 删除；四 crate 测试与 workspace check 通过。 |
| A10 | passed | brief.md | A10: 台账/交接/AGENTS/dev-tutorial 不再写「M6 已全部完成」；mac/Linux 硬门槛 A 有明确闸门与执行顺序。 | 台账/交接/AGENTS/dev-tutorial 已同步，mac/Linux 门槛 A 有闸门与顺序。 |

## 检查

_没有记录 Runtime 检查。_

### Builder 报告的证据

以下为 Builder 报告，不等同于 Runtime 检查凭据或独立验收结果。

- cargo check --workspace: passed — 2026-11-06，exit 0
- cargo test -p xgent_ui -p xgent_agent -p xui -p xgent_terminal: passed — 2026-11-06，xgent_ui/agent/xui/terminal 全绿
- 真实窗口走查: passed — docs/notes/m6-5-walkthrough.md 与截图目录
- 已知限制: 本次走查未覆盖中文输入法与 125%/150% 系统缩放
- 已知限制: @ 触发在截图中由临时环境变量注入文本，截图后已移除；键盘路由由无头测试覆盖
- 已知限制: 工具卡实时状态修复已用新增集成测试覆盖，但真实窗口未逐步录屏

## 阻塞项

_无。_

## 风险与跳过的工作

- 本次走查未覆盖中文输入法与 125%/150% 系统缩放
- @ 文本由临时环境变量注入，截图后移除；键盘路由由无头测试覆盖
- agent bridge 测试存在并行负载下的时序 flake，串行/重跑通过

## 之前的迭代

| 目标周期 | 迭代 | 尝试 | 结果 | 未解决项 | 摘要 | 完成时间 |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 1 | recovery | — | 独立验证发现 undo 重复渲染、Markdown 粗体/链接缺失、复制按钮未注册、chip 删除回弹、sidecar 会话 ID 不一致、终稿竞态；已按验证意见修复，返回 Build 重新提交候选。 | 2026-10-29T02:12:17.190Z |
| 1 | 2 | 1 | recovery | — | 第二次独立验证仍发现 Markdown 内联顺序、链接 tooltip、补全浮层断言、sidecar removed 回放与 token 标签泄漏；已修复并补充测试，返回 Build 重新提交。 | 2026-10-29T02:59:35.567Z |
| 1 | 3 | 1 | fail | A2, A3, A4, A6, A8, A9 | A1/A5/A7/A10 通过；A4/A6/A8 失败；A2/A3/A9 受阻（缺真实窗口证据，A2 另有 copy i18n 缺失）。 | 2026-10-29T04:09:46.228Z |
| 1 | 4 | 1 | pass | — | A1–A10 全部有证据通过。真实窗口补齐 Markdown 样张、工具卡顺序/状态、宽窄窗 @ 浮层与 chip 删除；A8 由独立会话日志证明设计先于实现并经用户确认。剩余风险为 IME/HiDPI 证据深度与既有 agent 测试时序 flake，不构成产品正确性失败。 | 2026-11-06T05:30:49.594Z |



## 结论

A1–A10 全部有证据通过。真实窗口补齐 Markdown 样张、工具卡顺序/状态、宽窄窗 @ 浮层与 chip 删除；A8 由独立会话日志证明设计先于实现并经用户确认。剩余风险为 IME/HiDPI 证据深度与既有 agent 测试时序 flake，不构成产品正确性失败。
