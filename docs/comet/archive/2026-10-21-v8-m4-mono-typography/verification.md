---
generated_from_state_version: 11
---

# 验证

## 当前结果

- 结果: **已归档**
- 验证情况: **已完成检查，验证结果已确认**
- 目标周期: 1
- 迭代: 2
- 验证器尝试次数: 1
- 完成时间: 2026-10-21T01:55:09.400Z
- 摘要: 修复轮复验通过：上一轮判定的 A1、A6 两处失败均已真正修复且未引入新缺陷。A1 的根因（bevy 0.20 逐 span 解析字体来源、空根 Text 被跳过）经 bevy 源码独立复核成立，修复后代码正文的每个 TextSpan 自带 EditorTheme.font（= UiFonts.mono）；A6 的消息正文三条路径（流式助手正文、固化副本、会话历史回看）均改用 line_height::for_text，行高跳变与历史固定 1.6 两个问题同时消除。A2~A5、A7~A9 独立复核均通过：令牌（SMALL/CODE 12.0、CJK 1.7、MONO 已改名）正确，负字距仅收窄展示级且欢迎标题按中英文案分别落 0 / -0.29，xgent_context 的 CJK bigram 44 项全绿，xui 103 + xgent_ui 76 + 全部集成套件全绿、cargo check/clippy/fmt 干净（全量 workspace 失败项与 handoff 已记录基线逐一吻合且与本变更无依赖路径），三处文档同步齐备。遗留 6 条风险均为文档陈旧、措辞超范围、潜在陈旧路径与非范围内的边角一致性问题，不构成违约。

## 验收

| 编号 | 结果 | 来源 | 验收项 | 原因 |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | A1 编辑器代码正文使用 `mono` 槽位：`sync_editor_theme` 运行后 `EditorTheme.font` 等于 `UiFonts.mono`；虚拟渲染创建的代码文本 `TextFont.font` 等于该值。 | 上一轮失败项已真正修复。独立核对 bevy 0.20 源码：bevy_text/src/pipeline.rs:124 `if text.is_empty() { continue; }`（空根 Text 被排版管线跳过，根 TextFont 对字形无效）+ pipeline.rs:213-232（逐 section/span 各自 `text_font.font.resolve_font_family(fonts)`）+ text_access.rs:174-194（每个 TextSpan 产出自己的 &TextFont）——上一轮判断成立。修复到位：xui/src/text_editor/virtual_render.rs:340,366 两处 span spawn 均带 `TextFont { font: font.clone(), .. }`，font 参数在 virtual_render.rs:266 由 `Res<EditorTheme>` 传入 `&theme.font`；xui/src/text_editor/render.rs:90-98 高亮层 span 同样带 `font: theme.font.clone()`。注入端 xgent_ui/src/editor/mod.rs:282 `editor_theme.font = fonts.mono.clone();`，且 editor/mod.rs:107 `sync_editor_theme.before(xui::TextEditorUpdateSet)` 保证首次 span 重建前 mono 已就位。mono_typography.rs 的 A1 断言已补「span 必须自带字体来源」。 |
| A2 | passed | brief.md | A2 编辑器行号列使用 `mono`：打开文件后行号实体的 `TextFont.font` 等于 `UiFonts.mono`（与代码列同源 → 数字等宽 → 行号与代码列对齐）。 | xgent_ui/src/editor/tabs.rs:200-210 行号实体 TextFont 带 `font: editor_theme.font.clone()`（= UiFonts.mono，与代码正文同源）。行号列是根 Text 且文本非空（xui/src/text_editor/render.rs:195 `*num_text = Text::new(nums)`），故其根 TextFont 确被 bevy 采用（不触发 pipeline.rs:124 的空文本跳过）→ 数字等宽，行号列与代码列同源对齐。 |
| A3 | passed | brief.md | A3 diff 行与工具卡参数摘要使用 `mono`：两处文本的 `TextFont.font` 等于 `UiFonts.mono`。 | diff 行：xgent_ui/src/editor/diff_view.rs:185 改用 `mono_text(&fonts, ...)`；工具卡参数摘要：xgent_ui/src/tool_panel.rs:183 改用 `ellipsis_mono_text(&fonts, ...)`；两者构造器均写 `font: fonts.mono.clone()`（xgent_ui/src/fonts.rs:480 与 528）。确认弹窗 diff 行原本已用 `mono_text_weighted`（confirm_dialog.rs:274，本轮仅 MONO→CODE 改名）。 |
| A4 | passed | brief.md | A4 令牌：`type_scale::SMALL == 12.0`、`type_scale::CODE == 12.0`、`type_scale::line_height::CJK == 1.7`。 | xgent_ui/src/theme.rs:315 `SMALL = 12.0`、:326 `CODE = 12.0`、:339 `line_height::CJK = 1.7`；全仓 grep `type_scale::MONO` 仅剩 theme.rs:324 的一处文档说明，常量已彻底改名（三个使用点 confirm_dialog/diff_view/terminal output 全部改用 CODE）。 |
| A5 | passed | brief.md | A5 展示级负字距不作用中文：`display_spacing` 对含 CJK 的文本返回 0、对纯拉丁返回 −0.29；welcome 标题接线到该函数。 | xgent_ui/src/fonts.rs:354-360 `display_spacing`：has_cjk → LetterSpacing::Px(0.0)，否则 Px(-0.29)；has_cjk（fonts.rs:337-348）覆盖 CJK 标点/假名/扩展A/基本区/兼容表意/扩展B~F/全角。接线在 welcome.rs:137 `crate::fonts::display_spacing(&tr(&loc, "welcome-title"))`；i18n 实测 zh-CN 标题「开始新会话」→ 0、en-US「Start a new session」→ -0.29。全仓其余 LetterSpacing 仅剩 +0.5 两处（kit.rs:632、welcome.rs:156），按 D-4 不动，正字距对 CJK 无害。 |
| A6 | passed | brief.md | A6 消息正文行高按语言选档：`line_height::for_text` 对含 CJK 的文本返回 1.7、对拉丁返回 1.6。 | 上一轮失败项已修好，消息正文三条路径全部走 `line_height::for_text`：流式助手正文 chat_panel.rs:575（每个 delta 按当前累计文本重算 LineHeight::RelativeToFont）、固化副本 chat_panel.rs:665（finalize_on_done 用 for_text(&content)）、会话历史回看 session_history.rs:586（for_text(text)）；用户消息 chat_panel.rs:549 亦为 for_text。for_text 实现 theme.rs:347-355（ASCII 快路径→BODY 1.6；含 CJK→CJK 1.7；否则 BODY）。流式与固化副本对同一内容求值一致 → 「流式 1.7 → 完成 1.6」跳变消除。 |
| A7 | passed | brief.md | A7 检索链路 CJK 分词未被破坏：中文问句仍抽出二元组（`xgent_context` 既有单测通过）。 | xgent_context 未被本变更触碰（git log 显示本轮两个提交均未改 xgent_context）；独立跑 `cargo test -p xgent_context`：44/44 通过，含 on_demand::tests::extract_keywords_splits_cjk_into_bigrams、extract_keywords_drops_function_words、no_bigram_spans_a_separator、retrieve_finds_file_from_chinese_question，CJK 二元组切分未回归。 |
| A8 | passed | brief.md | A8 既有测试不回归：`xgent_ui` lib 76 项与既有集成测试全绿，`xui` 全绿；`cargo check --workspace` 与 clippy 通过、fmt 无 diff。 | 独立复跑：xui lib 103/103 绿；xgent_ui lib 76/76 绿；xgent_ui 全部集成套件绿（mono_typography 9、editor_theme_sync 2、ellipsis_contract 6、scroll_contract 8、file_preview 7、file_tree_refresh 5、density 4、settings_panel_values 19、interaction_model 7、modal_settings 3、toast 1、settings_panel_i18n 3、settings_input_focus 2、virtual_list_contract 3、layout 2、conflict_save_guard 4、font_slots 6）；xgent_context 44/44。`cargo check --workspace` 通过；`cargo clippy -p xui -p xgent_ui -p xgent_app --all-targets` 在本变更触碰的全部文件上零新增告警（残留告警均在未触碰的 xgent_settings_core/xgent_context/xgent_tools/xgent_app 及既有测试文件）；`cargo fmt -p xui -p xgent_ui -p xgent_app -- --check` 无 diff。全量 `cargo test --workspace --no-fail-fast` 的失败项与 handoff §2 已记录基线逐一吻合（xgent_tools 6 个 POSIX shell 用例、xgent_plugin_host ecs_registration 3 + git_plugin 3（WASM 未构建）、xgent_daemon config_store 1、xgent_settings_core daemon_socket_env_override 1、xgent_agent 并发用例 1），且 xgent_agent 不依赖 xui/xgent_ui，判定为既有环境性失败而非本变更回归。 |
| A9 | passed | brief.md | A9 文档同步：`docs/plans/ui-v8-tasks.md` 的 M4-T3/M4-T4 勾选 + §10 勘误、`docs/dev-tutorial.md` 新增小节、`docs/plans/ui-v8-handoff.md` 交付表与下一步。 | docs/plans/ui-v8-tasks.md：M4-T3（:222-223）与 M4-T4（:228-229）均已勾选，且 §10 新增勘误 26/27/28（:409-411，分别记录 bevy 逐 span 解析字体来源、正文三条路径、非整数 DPI 视觉确认待真实窗口）；docs/dev-tutorial.md 新增 §5.22「代码类文本消费 mono 与中文排版」（:822-846）并追加两条坑（bevy 逐 span 解析 / 正文三条路径）；docs/plans/ui-v8-handoff.md §1 交付表新增 M4-T3/T4 行（:26，提交 de2bb68）且 §4.2 下一步表第 2 行标记 ✅ 已交付（:90）。 |

## 检查

| 检查 | 命令 | 工作目录 | 状态 | 退出码 | 耗时 |
| --- | --- | --- | --- | ---: | ---: |
| changed crate tests: xgent_ui / xui / xgent_app (debug info off) | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo test -p xgent_ui -p xui -p xgent_app --no-fail-fast | . | passed | 0 | 115206 ms |

### Builder 报告的证据

以下为 Builder 报告，不等同于 Runtime 检查凭据或独立验收结果。

- cargo test -p xui -p xgent_ui --no-fail-fast: passed — 21 个测试二进制全绿：xgent_ui lib 76 + 全部集成测试（含 mono_typography 9 条）；xui 103
- cargo test -p xgent_context: passed — 44/44 绿（检索链路 CJK 分词未回归）
- cargo check --workspace: passed — —
- cargo clippy -p xui -p xgent_ui -p xgent_app --all-targets: passed — 本次改动文件零新增警告（上一轮的 2 条已清）
- cargo fmt -p xui -p xgent_ui -p xgent_app: passed — 格式化后无 diff
- 已知限制: A1/A2 的断言以源码级为主（span spawn 处与注入处各一行）：打开文件 → 行号实体生成的完整 IO 流程在 headless 下成本高；运行时的字体来源断言由 A3 的构造器测试（真实 TextFont.font）覆盖。
- 已知限制: 「行号列与代码列在 125%/150% 缩放下仍对齐」这一视觉判据仍需真实窗口 + 非整数 DPI：代码侧保证的是同源同字体（等宽 → 数字等宽），台账 §10 勘误 28 已记录。
- 已知限制: CODE 档的 Compact −1 未做（密度系统只缩间距）；逐段字距未做（含中文则整串不施加负字距）。
- 已知限制: M4-T2 终端双宽单元格未做（独立 change），其前置的硬门槛 A macOS/Linux 启动日志确认仍待用户在对应平台各启动一次。

## 阻塞项

_无。_

## 风险与跳过的工作

- docs/plans/ui-v8-handoff.md 的 §0 一句话现状与 §4.1「起点事实」仍写「M4-T3/T4 未开工」「type_scale 仍无 CODE 档、SMALL 仍是 12.5、line_height 无 CJK」（:83），与已交付事实矛盾；§4.2 第 2 行的「见 §1.1」仍指向 M4-T1 硬门槛章节而非 M4-T3/T4 交付条目。属交接文档陈旧（该文档本就未随 M4-T1 更新），不影响 A9 判定的四项硬性内容，但下个会话可能被误导。
- 台账 M4-T3 勾选项措辞「行号、diff 行、代码块、chips 中代码全部走 mono_text」略超实际交付：范围内四个接线点（代码正文/行号列/diff 行/工具卡摘要）确已覆盖，但「代码块」在产品中尚不存在（Markdown 渲染 R3-5 未做），qa chips 仍是 ui_text 的 UI 文案而非代码文本。
- rebuild_visible_spans 仅在可见区间或内容哈希变化时重建；若 EditorTheme.font 在运行期变化，已存在的 span 不会刷新（仅根 TextFont 更新，而 bevy 对空根 Text 不采信）。当前 fonts.mono 在 Startup 一次性解析、sync_editor_theme 每次重写同值，实践上不触发，属潜在陈旧路径。
- 错误提示与重试状态（chat_panel.rs:719-723、770-774）直接覆写流式节点文本而不重算行高档，中文错误文案会沿用上一条正文档位；不属 spec 定义的「消息正文」范围，但残留了行高不一致的小口子。
- A1 的回归护栏是源码文本断言（include_str! + contains）而非对已生成 span 的 TextFont 的运行时断言；本轮 A1 的运行时语义由 Verifier 依据 bevy 源码（pipeline.rs:124/213-232、text_access.rs:174-194）+ 代码逐行核对确认，仓库内尚无同等强度的运行时测试。
- 「125%/150% 缩放下行号列与代码列视觉对齐」仍需真实窗口 + 非整数 DPI 才能确认，代码侧只能保证同源同字体（勘误 28 已如实标注）。

## 之前的迭代

| 目标周期 | 迭代 | 尝试 | 结果 | 未解决项 | 摘要 | 完成时间 |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 1 | fail | A1, A6 | 结论：不通过（fail）。本轮 M4-T3/T4 的令牌收敛（A4）、中文负字距约束（A5）、行号列走 mono（A2）、diff 行与工具卡参数摘要走 mono（A3）、检索链路未回归（A7）均属实并经独立复跑验证（xgent_ui lib 76、xui 103、xgent_context 44 全绿，check/clippy/fmt 通过，失败项与既有基线一致）。但两项关键验收未达成：A1 编辑器代码正文实际仍未走 mono —— sync_editor_theme 与 virtual_render 只把等宽字体写到了「文本为空的容器实体」上，真正承载字形的 TextSpan 子节点仍用默认字体句柄（Inter 比例字体），Bevy 0.20 的 TextPipeline 逐 span 解析字体，根实体空文本不产生任何字形区间，故 mono 对代码正文零作用；这意味着本轮要修的「行号数字宽度不等→行号列与代码列错位」原缺陷未修复，且行号列（mono）与代码列（比例）已不同源。A6 助手正文的最终固化副本（chat_panel.rs:663）与会话历史消息（session_history.rs:585）仍固定 1.6 行高，中文 1.7 约束只落在流式阶段与用户消息上。此外新增验收测试以源码字符串断言为主（A7 仅检查源文件是否含 bigram 字样），强度不足以发现 A1 这类「接线看似完成、运行时不生效」的缺陷；测试文件另有 2 条新增 clippy 警告。文档三处均已同步，但 handoff 交付表把 M4-T3/T4 的提交列写成「见 §1.1」（指向 M4-T1 硬门槛 A）并覆盖掉了验收文案，台账 M4-T3 首条勾选口径亦宽于实际交付。建议修复顺序：先在 xui virtual_render 给 TextSpan 注入 EditorTheme.font 并补运行时断言（A1），再把助手固化副本与会话历史改用 line_height::for_text（A6），然后清理 mono_typography.rs 的 2 条警告与台账/handoff 口径，复验后再提交。 | 2026-10-21T01:33:32.044Z |
| 1 | 2 | 1 | pass | — | 修复轮复验通过：上一轮判定的 A1、A6 两处失败均已真正修复且未引入新缺陷。A1 的根因（bevy 0.20 逐 span 解析字体来源、空根 Text 被跳过）经 bevy 源码独立复核成立，修复后代码正文的每个 TextSpan 自带 EditorTheme.font（= UiFonts.mono）；A6 的消息正文三条路径（流式助手正文、固化副本、会话历史回看）均改用 line_height::for_text，行高跳变与历史固定 1.6 两个问题同时消除。A2~A5、A7~A9 独立复核均通过：令牌（SMALL/CODE 12.0、CJK 1.7、MONO 已改名）正确，负字距仅收窄展示级且欢迎标题按中英文案分别落 0 / -0.29，xgent_context 的 CJK bigram 44 项全绿，xui 103 + xgent_ui 76 + 全部集成套件全绿、cargo check/clippy/fmt 干净（全量 workspace 失败项与 handoff 已记录基线逐一吻合且与本变更无依赖路径），三处文档同步齐备。遗留 6 条风险均为文档陈旧、措辞超范围、潜在陈旧路径与非范围内的边角一致性问题，不构成违约。 | 2026-10-21T01:55:09.400Z |



## 结论

修复轮复验通过：上一轮判定的 A1、A6 两处失败均已真正修复且未引入新缺陷。A1 的根因（bevy 0.20 逐 span 解析字体来源、空根 Text 被跳过）经 bevy 源码独立复核成立，修复后代码正文的每个 TextSpan 自带 EditorTheme.font（= UiFonts.mono）；A6 的消息正文三条路径（流式助手正文、固化副本、会话历史回看）均改用 line_height::for_text，行高跳变与历史固定 1.6 两个问题同时消除。A2~A5、A7~A9 独立复核均通过：令牌（SMALL/CODE 12.0、CJK 1.7、MONO 已改名）正确，负字距仅收窄展示级且欢迎标题按中英文案分别落 0 / -0.29，xgent_context 的 CJK bigram 44 项全绿，xui 103 + xgent_ui 76 + 全部集成套件全绿、cargo check/clippy/fmt 干净（全量 workspace 失败项与 handoff 已记录基线逐一吻合且与本变更无依赖路径），三处文档同步齐备。遗留 6 条风险均为文档陈旧、措辞超范围、潜在陈旧路径与非范围内的边角一致性问题，不构成违约。
