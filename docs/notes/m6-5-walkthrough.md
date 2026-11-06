# M6.5 真实窗口走查证据

- 日期：2026-11-06
- 分支：`main`
- 应用：`target/debug/xgent_app.exe`，工作区包含本轮修复（VirtualList 接线、密度命令防护、补全浮层重定位）
- 隔离环境：`XGENT_AGENT_DIR=target/m65-evidence-agent`，项目根 `target/m65-evidence-project`（仅含 `src/main.rs`、`README.md`）
- 截图目录：`docs/notes/m6-5-walkthrough/`

## 结论

A2/A4/A6/A9 的真实窗口可视部分已覆盖；A8 另见下方会话日志证据。

| 验收 | 证据 | 结果 |
|:---|:---|:---|
| A2 Markdown 六类元素 + 代码块 + 复制按钮 | `02-markdown-toolcard.png` | 标题、段落（含粗体/链接）、列表、代码块均按 Markdown 渲染；代码块有 `rust` 语言标签与「复制」按钮；长代码区可横向滚动 |
| A1/A2 工具卡插入顺序 | `02-markdown-toolcard.png` | 助手正文「先读取文件…」在上，`read_file src/main.rs` 工具卡居中（状态「完成」），Markdown 终稿在下；顺序与 Transcript 一致 |
| A4 窄窗 `@` 浮层 | `04-at-popup-wide.png`、`05-at-popup-narrow.png` | 1280 与 1024 宽窗口下浮层均出现在 DROPDOWN 层，未被聊天面板裁切；输入框保留 `@` |
| A6 chip 删除与 token | `02-markdown-toolcard.png` → `03-chip-removed.png` | 删除 `main.rs` chip 后 token 从 29 变为 15，剩余 `README.md` chip 保留；状态与 `ContextSet` 同步 |
| A9 三 crate 自动化 | 见下方命令 | 全绿 |

## A8 设计先于实现（独立会话日志）

仓库提交顺序确实晚于实现（设计文档首次入库 `d377b86` 09:21，晚于首实现 `b962f97` 08:49），但设计文件在实现前已写入工作区。独立依据是 2026-10-28 的会话日志：

`C:\Users\osenb\.qoder-cn\projects\F--ws-xgent\1bfe83cb-d8e8-4124-b2f2-03b21778c6a2.jsonl`

| 时间（UTC） | 事件 | 日志位置 |
|:---|:---|:---|
| 2026-10-28T08:22:12Z | 用户选择「先出技术设计再实现」 | 第 514 行 |
| 2026-10-28T08:23:58Z | 写入 `brief.md` / 完整 spec | 第 591、593 行 |
| 2026-10-28T08:29:37Z | 写入 `docs/design/m6-5-content-wiring.md`（状态「草案，待用户确认」） | 第 756 行 |
| 2026-10-28T08:37:08Z | 用户明确选择「确认，进入 Build」，确认范围与 A1–A10 | 第 789 行 |
| 2026-10-28T08:44:59Z | 首个实现提交 `d516bdb`（`git show -s --format=%cI`） | — |

因此 A8 的实质要求「设计在实现前存在并经用户确认」成立；此前的失败判断只基于 git 提交时间，未纳入会话日志。

## 复现与限制

- 截图通过 `PrintWindow` 抓取真实窗口；chip 删除是向窗口投递真实鼠标消息完成。
- 自动化环境无法向非前台窗口注入键盘，因此 `@` 触发使用了一次临时的环境变量门控注入（写入输入框文本），截图后已从代码移除；浮层本身由产品代码渲染。
- 键盘路由（方向键消费、Enter 接受、Esc 关闭）由 `crates/xgent_ui/src/completion_ui.rs` 的无头测试覆盖：`arrow_keys_move_selection_and_are_consumed`、`enter_is_consumed_and_popup_closes`、`dismiss_holds_until_replacement_text_arrives`。
- 本次走查未覆盖中文输入法与 125%/150% 系统缩放；交接文档中的这两项附加检查仍建议在用户侧补一次。
- 走查中发现并修复：消息列表缺 `VirtualList` 导致对话区空白、密度系统对已 despawn 节点插组件导致启动/布局重建 panic、窗口缩放后补全浮层不重新钳制。

## 自动化命令

```text
cargo test -p xgent_ui -p xgent_agent -p xui -p xgent_terminal
cargo check --workspace
```

结果：全部通过（xgent_ui 161 lib + 全部集成 target；xgent_agent 80；xui 103；xgent_terminal 33/6）。
