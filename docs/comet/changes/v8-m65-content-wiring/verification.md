---
generated_from_state_version: 12
---

# 验证

## 当前结果

- 结果: **未通过**
- 验证情况: **修复未通过的验收项后重新验证**
- 目标周期: 1
- 迭代: 3
- 验证器尝试次数: 1
- 完成时间: 2026-10-29T04:09:46.228Z
- 摘要: A1/A5/A7/A10 通过；A4/A6/A8 失败；A2/A3/A9 受阻（缺真实窗口证据，A2 另有 copy i18n 缺失）。

## 验收

| 编号 | 结果 | 来源 | 验收项 | 原因 |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | A1: 流式助手回复中插入工具卡，`Transcript` 的顺序键 `(turn_index, block_index)` 单调；`chat_panel`/`tool_panel`/`undo` 不再用 `add_child` 挪动已有消息节点。 | undo 直接追加已删；写入链 Done 先于提交；渲染契约按实际子节点顺序断言。 |
| A2 | blocked | brief.md | A2: 含标题、段落、列表、代码块、粗体、链接的样张在真实窗口正确渲染；代码块使用 `surface_code` 底、`mono`、语言标签与复制按钮，复制内容等于代码块全文。 | 解析/复制/链接代码已修，但无真实窗口样张；copy 文案缺 i18n 键。 |
| A3 | blocked | brief.md | A3: 流式期间显示纯文本，完成后替换为 Markdown；替换只作用于当前消息，不重建整段历史（消息实体保持稳定）。 | 终稿替换代码正确，但测试从 finalized 状态开始，未覆盖流式→终稿的实体稳定性。 |
| A4 | failed | brief.md | A4: 窄窗输入 `@` 弹出 `DROPDOWN` 层补全浮层，方向键选择、Enter 接受、Esc 关闭；接受后输入框包含所选项。 | Esc 关闭后 deactivate 未清 trigger_position，下一帧 sync 会重新激活；方向键未消费；无键盘路由/窄窗真实证据。 |
| A5 | passed | brief.md | A5: 移除 `ContextSet` 中某 chip 后，下一轮发给 LLM 的请求不包含该项内容；chip 与 `ContextSet` 状态一致。 | pinned_context 经 StartLoop/bridge 合并；removed 不被 tab 同步加回。 |
| A6 | failed | brief.md | A6: 上下文 chip 显示 token 占比，数值与 `ContextSet::total_tokens` 一致；会话恢复后 ContextSet 与恢复前一致（会话内持久化）。 | 新会话不清 ContextSet；旧数组格式 sidecar 读为空；token 标签签名不含 token 数；无磁盘往返测试。 |
| A7 | passed | brief.md | A7: 长会话（≥200 条消息）滚动时只渲染可见行；消息顺序不倒退。 | 200 块只渲染 <40 块 + 占位块。 |
| A8 | failed | brief.md | A8: `docs/design/m6-5-content-wiring.md` 在实现前存在并经用户确认，内容覆盖数据流、迁移顺序、回滚点与逐任务验收映射。 | 设计确认记录为后补；实现提交早于文档提交，无独立预实现确认证据。 |
| A9 | blocked | brief.md | A9: 真实窗口走查证据覆盖 Markdown 样张、工具卡插入顺序、`@` 浮层、chip 删除；`cargo test -p xgent_ui -p xui -p xgent_terminal` 全绿。 | 测试全绿，但真实窗口截图/走查缺失。 |
| A10 | passed | brief.md | A10: 台账/交接/AGENTS/dev-tutorial 不再写「M6 已全部完成」；mac/Linux 硬门槛 A 有明确闸门与执行顺序。 | 文档无“M6 已全部完成”误导；门槛 A 有闸门。 |

## 检查

_没有记录 Runtime 检查。_

### Builder 报告的证据

以下为 Builder 报告，不等同于 Runtime 检查凭据或独立验收结果。

- cargo check --workspace: passed — 2026-10-29，-j 2，通过
- cargo test -p xgent_ui: passed — 2026-10-29，-j 2；lib 121 + 全部集成 target 通过
- cargo test -p xui -p xgent_terminal: passed — xui 103 + xgent_terminal 33/6 通过
- cargo test -p xgent_agent --lib -- --skip max_parallel_tools_written_to_handle_bounds_next_turn: passed — 74 passed；跳过既有并发时序 flake
- xgent_app startup smoke: passed — target/debug/xgent_app.exe 运行 12s，零 panic
- 已知限制: 真实窗口截图/走查待用户完成（A2/A4/A9 的可视部分）
- 已知限制: 虚拟窗口在可见区与当前轮不相邻时中间间隙塌陷
- 已知限制: @ 补全仅文件候选
- 已知限制: sidecar 磁盘往返无自动化测试
- 已知限制: xgent_agent 并发测试为既有 flake

## 阻塞项

_无。_

## 风险与跳过的工作

- A4 Esc 重开与方向键未消费
- A6 新会话继承旧 ContextSet / 旧 sidecar 格式
- A2/A3/A9 缺真实窗口证据
- A8 设计确认时序记录不足

## 之前的迭代

| 目标周期 | 迭代 | 尝试 | 结果 | 未解决项 | 摘要 | 完成时间 |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 1 | recovery | — | 独立验证发现 undo 重复渲染、Markdown 粗体/链接缺失、复制按钮未注册、chip 删除回弹、sidecar 会话 ID 不一致、终稿竞态；已按验证意见修复，返回 Build 重新提交候选。 | 2026-10-29T02:12:17.190Z |
| 1 | 2 | 1 | recovery | — | 第二次独立验证仍发现 Markdown 内联顺序、链接 tooltip、补全浮层断言、sidecar removed 回放与 token 标签泄漏；已修复并补充测试，返回 Build 重新提交。 | 2026-10-29T02:59:35.567Z |
| 1 | 3 | 1 | fail | A2, A3, A4, A6, A8, A9 | A1/A5/A7/A10 通过；A4/A6/A8 失败；A2/A3/A9 受阻（缺真实窗口证据，A2 另有 copy i18n 缺失）。 | 2026-10-29T04:09:46.228Z |



## 结论

A1/A5/A7/A10 通过；A4/A6/A8 失败；A2/A3/A9 受阻（缺真实窗口证据，A2 另有 copy i18n 缺失）。
