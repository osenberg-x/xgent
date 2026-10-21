---
generated_from_state_version: 6
---

# 验证

## 当前结果

- 结果: **未通过**
- 验证情况: **修复未通过的验收项后重新验证**
- 目标周期: 1
- 迭代: 1
- 验证器尝试次数: 1
- 完成时间: 2026-10-21T01:33:32.044Z
- 摘要: 结论：不通过（fail）。本轮 M4-T3/T4 的令牌收敛（A4）、中文负字距约束（A5）、行号列走 mono（A2）、diff 行与工具卡参数摘要走 mono（A3）、检索链路未回归（A7）均属实并经独立复跑验证（xgent_ui lib 76、xui 103、xgent_context 44 全绿，check/clippy/fmt 通过，失败项与既有基线一致）。但两项关键验收未达成：A1 编辑器代码正文实际仍未走 mono —— sync_editor_theme 与 virtual_render 只把等宽字体写到了「文本为空的容器实体」上，真正承载字形的 TextSpan 子节点仍用默认字体句柄（Inter 比例字体），Bevy 0.20 的 TextPipeline 逐 span 解析字体，根实体空文本不产生任何字形区间，故 mono 对代码正文零作用；这意味着本轮要修的「行号数字宽度不等→行号列与代码列错位」原缺陷未修复，且行号列（mono）与代码列（比例）已不同源。A6 助手正文的最终固化副本（chat_panel.rs:663）与会话历史消息（session_history.rs:585）仍固定 1.6 行高，中文 1.7 约束只落在流式阶段与用户消息上。此外新增验收测试以源码字符串断言为主（A7 仅检查源文件是否含 bigram 字样），强度不足以发现 A1 这类「接线看似完成、运行时不生效」的缺陷；测试文件另有 2 条新增 clippy 警告。文档三处均已同步，但 handoff 交付表把 M4-T3/T4 的提交列写成「见 §1.1」（指向 M4-T1 硬门槛 A）并覆盖掉了验收文案，台账 M4-T3 首条勾选口径亦宽于实际交付。建议修复顺序：先在 xui virtual_render 给 TextSpan 注入 EditorTheme.font 并补运行时断言（A1），再把助手固化副本与会话历史改用 line_height::for_text（A6），然后清理 mono_typography.rs 的 2 条警告与台账/handoff 口径，复验后再提交。

## 验收

| 编号 | 结果 | 来源 | 验收项 | 原因 |
| --- | --- | --- | --- | --- |
| A1 | failed | brief.md | A1 编辑器代码正文使用 `mono` 槽位：`sync_editor_theme` 运行后 `EditorTheme.font` 等于 `UiFonts.mono`；虚拟渲染创建的代码文本 `TextFont.font` 等于该值。 | 接线只做到容器层，真正承载代码字形的 TextSpan 仍是比例字体。sync_editor_theme 确实把 UiFonts.mono 注入 EditorTheme.font（crates/xgent_ui/src/editor/mod.rs:282），virtual_render 也把它写进容器 Text 实体的 TextFont（crates/xui/src/text_editor/virtual_render.rs:179、227）；但可见代码文本由 TextSpan 子节点承载，其 TextFont 为 { font_size, ..default() }（virtual_render.rs:333-345、355-364），font 字段即 FontSource::default()=默认字体句柄→xgent_app/startup.rs:81 覆盖为 Inter Variable（比例字体）。Bevy 0.20 逐 span 解析字体：TextReader 对每个 span 取其自身 TextFont（bevy_text/src/text_access.rs:174-194），TextPipeline::update_buffer 按 section 自身的 text_font.font.resolve_font_family 下发 StyleProperty::FontFamily（bevy_text/src/pipeline.rs:230-232），而根实体文本为空（Text::new(String::new())，virtual_render.rs:171）在 pipeline.rs:124-126 被 continue 跳过 → 根上的 mono 字体不对任何字形生效。后果：原始缺陷（行号数字宽度不等→行号列与代码列错位）未被修复，且行号列（已改 mono）与代码列（仍比例）反而不同源。新增 tests/mono_typography.rs 的 A1/A2 用例只做 include_str!+contains 的源码字符串断言，发现不了该缺陷。修复方向：rebuild_visible_spans 生成的每个 TextSpan 需带 font: theme.font.clone()（由 EditorTheme.font 派生），并补运行时断言而非源码断言。 |
| A2 | passed | brief.md | A2 编辑器行号列使用 `mono`：打开文件后行号实体的 `TextFont.font` 等于 `UiFonts.mono`（与代码列同源 → 数字等宽 → 行号与代码列对齐）。 | 行号列有效走 mono。crates/xgent_ui/src/editor/tabs.rs:200-210 的行号实体是根 Text（非 span），自带 TextFont{ font: editor_theme.font.clone() }，根文本字形按自身字体解析（bevy_text/src/pipeline.rs:230）→ 生效。系统顺序正确：sync_editor_theme 在 xui::TextEditorUpdateSet 之前（editor/mod.rs:107），handle_open_file_requests 在其之后（editor/mod.rs:161），spawn 时 editor_theme.font 已是 mono 槽位。残留低风险：行号实体字体只在 spawn 时写一次，槽位运行时变更不会跟随（mono 槽位启动期固定，实际不触发）。 |
| A3 | passed | brief.md | A3 diff 行与工具卡参数摘要使用 `mono`：两处文本的 `TextFont.font` 等于 `UiFonts.mono`。 | 两处都是根 Text + mono 字体，实际生效。diff 行：crates/xgent_ui/src/editor/diff_view.rs:185 改用 mono_text(&fonts, ...) → fonts.rs:517-533 的 TextFont{ font: fonts.mono.clone() }。工具卡参数摘要：crates/xgent_ui/src/tool_panel.rs:183 改用 ellipsis_mono_text(&fonts, summary, ...) → fonts.rs:469-492；省略测量读同一实体的 TextFont（xui/src/ellipsis.rs:282），字体来源一致。遗留（不在 brief 列出的 4 点内，记入风险）：工具卡结果区 stdout（tool_panel.rs:242）仍是 ui_text + CAPTION 字号，属代码类文本但未走 mono。 |
| A4 | passed | brief.md | A4 令牌：`type_scale::SMALL == 12.0`、`type_scale::CODE == 12.0`、`type_scale::line_height::CJK == 1.7`。 | 令牌按设计系统 §2.2 收敛且实测一致：crates/xgent_ui/src/theme.rs:315 SMALL=12.0（原 12.5）、theme.rs:326-327 CODE=12.0（MONO 改名，值不变）、theme.rs:338-339 line_height::CJK=1.7。与 docs/design/ui-v8-design-system.md:92-101 及 §八（227-229）一致；全仓 grep 确认 type_scale::MONO 常量已无残留引用（仅剩 EMBEDDED_MONO_PATH/EMBEDDED_MONO_FAMILY 资产常量与文档提及）。 |
| A5 | passed | brief.md | A5 展示级负字距不作用中文：`display_spacing` 对含 CJK 的文本返回 0、对纯拉丁返回 −0.29；welcome 标题接线到该函数。 | 规则与接线均到位。crates/xgent_ui/src/fonts.rs:354-360 的 display_spacing：含 CJK→LetterSpacing::Px(0.0)，纯拉丁→Px(-0.29)；has_cjk（fonts.rs:337-348）覆盖 CJK 基本区/扩展A/兼容表意/扩展B~F/假名/CJK 标点/全角形式。接线：crates/xgent_ui/src/welcome.rs:137 标题改用 display_spacing(&tr(&loc, "welcome-title"))，取代原硬写 LetterSpacing::Px(-0.29)，且该实体是根 Text 非空 → 生效。D-4 成立：全仓仅剩两处 LetterSpacing 均为 +0.5（kit.rs:632、welcome.rs:156），对 CJK 无害未动。 |
| A6 | failed | brief.md | A6 消息正文行高按语言选档：`line_height::for_text` 对含 CJK 的文本返回 1.7、对拉丁返回 1.6。 | 函数本身正确但只接到一半，助手正文最终态没走 for_text。theme.rs:344-352 的 for_text 行为正确（ASCII→1.6、含 CJK→1.7、其他→1.6），单测通过。接线：用户消息正文 ✓（chat_panel.rs:549）、流式助手正文 ✓（chat_panel.rs:562-580 每条 delta 更新 LineHeight）。缺口：助手消息的最终固化副本仍用固定 type_scale::line_height::BODY（chat_panel.rs:663，finalize_on_done），而 finalize_on_done 会清空流式节点 → 用户最终读到的助手消息是这份 1.6 副本；会话历史列表同理（session_history.rs:585 固定 BODY）。后果：中文助手消息最终以 1.6 渲染（1.7 约束落不到主界面），且流式时 1.7 → 完成时 1.6 出现行高跳变。spec 明确要求「对话消息正文（用户消息与助手正文）使用该函数」，未达成；修复点为 chat_panel.rs:663 与 session_history.rs:585 改用 for_text。 |
| A7 | passed | brief.md | A7 检索链路 CJK 分词未被破坏：中文问句仍抽出二元组（`xgent_context` 既有单测通过）。 | 检索链路未被破坏，既有单测实跑通过。cargo test -p xgent_context 44/44 全绿，含 on_demand::tests::extract_keywords_splits_cjk_into_bigrams（中文词串切二元组）、no_bigram_spans_a_separator、extract_keywords_drops_single_cjk_char、extract_keywords_drops_function_words、retrieve_finds_file_from_chinese_question（中文问句命中含词片文件）。本变更未触碰 xgent_context（git show --stat de2bb68 无该 crate），实现仍为 crates/xgent_context/src/on_demand.rs 的 cjk_bigrams/extract_keywords 链路。注：新增 mono_typography.rs 的 A7 用例只是 include_str! 查源码里有没有 "bigram"/"CN_FUNC_CHARS" 字样，属弱断言；真正的回归保障来自上述既有单测（该单测未被削弱）。 |
| A8 | passed | brief.md | A8 既有测试不回归：`xgent_ui` lib 76 项与既有集成测试全绿，`xui` 全绿；`cargo check --workspace` 与 clippy 通过、fmt 无 diff。 | 无回归，但有 1 处新增警告瑕疵。cargo check --workspace --all-targets 通过；cargo test --workspace --no-fail-fast：xgent_ui lib 76 全绿、17 个集成测试文件全绿（含新增 mono_typography 8 条）、xui lib 103 全绿、xgent_context 44 全绿；失败项与基线一致（xgent_agent 1 = 已知时序 flake max_parallel_tools_written_to_handle_bounds_next_turn、xgent_plugin_host 6 = git 插件 WASM 未构建、xgent_settings_core 1 = Windows 路径、xgent_tools 6 = 依赖 POSIX shell），xgent_daemon 54 全绿（基线表那 1 项本次未复现）。cargo fmt -p xui -p xgent_ui -p xgent_app --check 无 diff；cargo clippy 同范围退出码 0。瑕疵：spec 要求「本次改动文件零新增警告」，但新增文件 crates/xgent_ui/tests/mono_typography.rs 引入 2 条新警告（:8 unused import LineHeight、:22 dead_code NoopStrings），不阻断 A8 但需清理。 |
| A9 | passed | brief.md | A9 文档同步：`docs/plans/ui-v8-tasks.md` 的 M4-T3/M4-T4 勾选 + §10 勘误、`docs/dev-tutorial.md` 新增小节、`docs/plans/ui-v8-handoff.md` 交付表与下一步。 | 三处文档均已同步。docs/plans/ui-v8-tasks.md：M4-T3 两项、M4-T4 两项均勾选，§10 勘误新增 21-25 条（MONO 只改字号没改字体 / EditorTheme 携带字体来源 / CODE 用改名而非新增 / CJK 判定口径 / CODE 档 Compact −1 未做）。docs/dev-tutorial.md：新增 §5.22「代码类文本消费 mono 与中文排版（v8 M4-T3/T4）」（问题、四个接线点、字号阶梯、中文约束、三个坑）。docs/plans/ui-v8-handoff.md：§1 交付表新增 M4-T3/T4 行，§4.2 下一步表第 2 行标 ✅ 已交付。瑕疵（记入风险）：交付表 M4-T3/T4 行的「提交」列写「见 §1.1」指向的是 M4-T1 硬门槛 A 一节（应为实现提交 de2bb68），且 §4.2 第 2 行原「验收」文案（行号列与代码列在 125%/150% 缩放下仍对齐；中英混排无豆腐块）被替换成「见 §1.1」导致验收口径丢失；另台账 M4-T3 首条勾选声称「代码块、chips 中代码全部走 mono_text」，实际工具卡结果区（tool_panel.rs:242）与上下文 chips（context_scope.rs:189）仍是 ui_text，勾选口径宽于交付。 |

## 检查

| 检查 | 命令 | 工作目录 | 状态 | 退出码 | 耗时 |
| --- | --- | --- | --- | ---: | ---: |
| changed crate tests: xgent_ui / xui / xgent_app (debug info off) | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo test -p xgent_ui -p xui -p xgent_app --no-fail-fast | . | passed | 0 | 148942 ms |

### Builder 报告的证据

以下为 Builder 报告，不等同于 Runtime 检查凭据或独立验收结果。

- cargo test -p xui -p xgent_ui --no-fail-fast: passed — 21 个测试二进制全绿：xgent_ui lib 76 + 全部集成测试（含新增 mono_typography 8 条）；xui 103
- cargo test --workspace --no-fail-fast（与交接文档 §2 既有基线对比：无回归）: passed — xgent_ui / xui / xgent_app 零失败；失败项全部落在未改动的 crate 且与既有基线一致（xgent_agent 1 个时序 flake、xgent_daemon 1、xgent_plugin_host 6、xgent_settings_core 1、xgent_tools 6）
- cargo check --workspace: passed — —
- cargo clippy -p xui -p xgent_ui --all-targets: passed — 本次改动文件零新增警告；其余为改动前既有警告
- cargo fmt -p xui -p xgent_ui -p xgent_app: passed — 格式化后无 diff
- 已知限制: A2/A1 的部分断言是源码级（行号实体与虚拟渲染的接线）：打开文件 → 行号实体生成的完整 IO 流程在 headless 下成本高，接线各只有一行，用源码断言守住；运行时的字体来源断言由 A3 的构造器测试覆盖。
- 已知限制: CODE 档的 Compact −1 未做：密度系统按 M2b 纪律只缩间距、不缩字号，字号缩放要另立机制（台账 §10 勘误 25）。
- 已知限制: 逐段字距未做：采用「含中文则整串不施加负字距」的保守做法（台账 §10 勘误 24）。
- 已知限制: M4-T2 终端双宽单元格未做（独立 change），其前置的硬门槛 A macOS/Linux 启动日志确认仍待用户在对应平台各启动一次。
- 已知限制: 本机（Windows）无法验证「行号列与代码列在 125%/150% 缩放下仍对齐」这一视觉判据：需要真实窗口 + 非整数 DPI，代码侧只能保证两者同源同字体。

## 阻塞项

_无。_

## 风险与跳过的工作

- A1 未闭环是本轮最严重问题：代码正文仍用 Inter 比例字体渲染（TextSpan 自带默认字体），M4-T3 要修的「行号数字宽度不等 → 行号列与代码列错位」原缺陷实际未修复；且行号列（mono）与代码列（比例）现在不同源，方向上与目标相反。需在 xui::text_editor::virtual_render::rebuild_visible_spans 给每个 TextSpan 带上 EditorTheme.font（并补运行时断言），再复验。
- A6 未闭环：助手消息固化副本（chat_panel.rs:663）与会话历史消息（session_history.rs:585）仍固定 1.6，中文正文 1.7 约束落不到主界面；流式 1.7 → 完成 1.6 有可见行高跳变。
- 新增验收测试强度不足：tests/mono_typography.rs 对 A1/A2/A3/A7 用 include_str!+contains 做源码字符串断言，A7 甚至只是查文件里有没有 "bigram" 字样——这类断言挡不住「接线写了但运行时不生效」类缺陷（A1 正是如此）。建议改成构造真实实体/系统后读 TextFont 的运行时断言。
- 规格条款「本次改动文件零新增警告」未满足：crates/xgent_ui/tests/mono_typography.rs 有 2 条新警告（unused import LineHeight、dead_code NoopStrings）。
- 代码类文本仍有漏网点（不在本轮 4 点范围，但与 Purpose 的「一律走 mono」有落差）：工具卡结果区 stdout（tool_panel.rs:242）、上下文 chips 文件名（context_scope.rs:189）、冲突对话框与命令面板中的路径类文本仍是 ui_text/ellipsis_text。
- 文档口径问题：handoff 交付表 M4-T3/T4 行「提交」列写「见 §1.1」（指向 M4-T1 硬门槛 A）且覆盖掉了 §4.2 该行的验收文案；台账 M4-T3 首条勾选范围宽于实际交付。
- 既有基线失败与本变更无关但仍在：xgent_agent 1（时序 flake）、xgent_plugin_host 6、xgent_settings_core 1、xgent_tools 6；xgent_daemon 基线那 1 项本次未复现。
- 本轮未做真实窗口下的视觉判据（非整数 DPI 下的行号列对齐、中英混排豆腐块），该类判据仍需人工/截图确认；本判定完全基于代码路径 + Bevy 0.20 文本管线源码 + 本地测试/clippy/fmt 实跑。

## 之前的迭代

| 目标周期 | 迭代 | 尝试 | 结果 | 未解决项 | 摘要 | 完成时间 |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 1 | fail | A1, A6 | 结论：不通过（fail）。本轮 M4-T3/T4 的令牌收敛（A4）、中文负字距约束（A5）、行号列走 mono（A2）、diff 行与工具卡参数摘要走 mono（A3）、检索链路未回归（A7）均属实并经独立复跑验证（xgent_ui lib 76、xui 103、xgent_context 44 全绿，check/clippy/fmt 通过，失败项与既有基线一致）。但两项关键验收未达成：A1 编辑器代码正文实际仍未走 mono —— sync_editor_theme 与 virtual_render 只把等宽字体写到了「文本为空的容器实体」上，真正承载字形的 TextSpan 子节点仍用默认字体句柄（Inter 比例字体），Bevy 0.20 的 TextPipeline 逐 span 解析字体，根实体空文本不产生任何字形区间，故 mono 对代码正文零作用；这意味着本轮要修的「行号数字宽度不等→行号列与代码列错位」原缺陷未修复，且行号列（mono）与代码列（比例）已不同源。A6 助手正文的最终固化副本（chat_panel.rs:663）与会话历史消息（session_history.rs:585）仍固定 1.6 行高，中文 1.7 约束只落在流式阶段与用户消息上。此外新增验收测试以源码字符串断言为主（A7 仅检查源文件是否含 bigram 字样），强度不足以发现 A1 这类「接线看似完成、运行时不生效」的缺陷；测试文件另有 2 条新增 clippy 警告。文档三处均已同步，但 handoff 交付表把 M4-T3/T4 的提交列写成「见 §1.1」（指向 M4-T1 硬门槛 A）并覆盖掉了验收文案，台账 M4-T3 首条勾选口径亦宽于实际交付。建议修复顺序：先在 xui virtual_render 给 TextSpan 注入 EditorTheme.font 并补运行时断言（A1），再把助手固化副本与会话历史改用 line_height::for_text（A6），然后清理 mono_typography.rs 的 2 条警告与台账/handoff 口径，复验后再提交。 | 2026-10-21T01:33:32.044Z |



## 结论

结论：不通过（fail）。本轮 M4-T3/T4 的令牌收敛（A4）、中文负字距约束（A5）、行号列走 mono（A2）、diff 行与工具卡参数摘要走 mono（A3）、检索链路未回归（A7）均属实并经独立复跑验证（xgent_ui lib 76、xui 103、xgent_context 44 全绿，check/clippy/fmt 通过，失败项与既有基线一致）。但两项关键验收未达成：A1 编辑器代码正文实际仍未走 mono —— sync_editor_theme 与 virtual_render 只把等宽字体写到了「文本为空的容器实体」上，真正承载字形的 TextSpan 子节点仍用默认字体句柄（Inter 比例字体），Bevy 0.20 的 TextPipeline 逐 span 解析字体，根实体空文本不产生任何字形区间，故 mono 对代码正文零作用；这意味着本轮要修的「行号数字宽度不等→行号列与代码列错位」原缺陷未修复，且行号列（mono）与代码列（比例）已不同源。A6 助手正文的最终固化副本（chat_panel.rs:663）与会话历史消息（session_history.rs:585）仍固定 1.6 行高，中文 1.7 约束只落在流式阶段与用户消息上。此外新增验收测试以源码字符串断言为主（A7 仅检查源文件是否含 bigram 字样），强度不足以发现 A1 这类「接线看似完成、运行时不生效」的缺陷；测试文件另有 2 条新增 clippy 警告。文档三处均已同步，但 handoff 交付表把 M4-T3/T4 的提交列写成「见 §1.1」（指向 M4-T1 硬门槛 A）并覆盖掉了验收文案，台账 M4-T3 首条勾选口径亦宽于实际交付。建议修复顺序：先在 xui virtual_render 给 TextSpan 注入 EditorTheme.font 并补运行时断言（A1），再把助手固化副本与会话历史改用 line_height::for_text（A6），然后清理 mono_typography.rs 的 2 条警告与台账/handoff 口径，复验后再提交。
