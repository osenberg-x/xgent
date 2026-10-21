---
generated_from_state_version: 27
---

# 验证

## 当前结果

- 结果: **已归档**
- 验证情况: **已完成检查，验证结果已确认**
- 目标周期: 2
- 迭代: 4
- 验证器尝试次数: 1
- 完成时间: 2026-10-21T06:07:41.337Z
- 摘要: 独立复核 HEAD=ea38c88 候选实现，10 条验收项全部通过。A1/A2 复核了本轮新改的实测路径与双宽校验纯函数（含新增单测 warning_wording_matches_actual_cell_width_change 通过）；A2 与日志部分确认本轮三处新改均已如实落地：告警按 CJK 步进偏宽/偏窄分两种措辞（偏宽「由 A 调整为 B」、偏窄「维持 A（= 窄字符步进）」）、info! 行恒含家族+四数值+单元格宽+校验结论（告警细节另走 warn! 且同样带家族名）、家族标签区分「系统家族 X / 内嵌资产（句柄）/ 通用族兜底（Monospace）」。独立复跑 check/fmt/clippy/test 全绿（xgent_ui lib 84 + terminal_cell_layout 6、xgent_terminal lib 33 + integration 6、改动文件 clippy 零告警、legacy-terminal-layout feature 下 check 通过）。文档三处（台账 M4-T2 勾选 + 勘误 29~33、交接交付表/基线/下一步/硬门槛 A、开发指南 §5.23）齐备，Windows 结论与 macOS/Linux 待确认且不阻塞的状态均已落纸。独立发现 1 处文档滞后（交接提交清单缺 41598f0/ea38c88）与 2 处口径细节（CellGrid 的 .max(1.0) 下限、line_height 回退常量），均为非阻塞项。

## 验收

| 编号 | 结果 | 来源 | 验收项 | 原因 |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | A1 `MonoMetrics` 由**实测**得出：探针经 bevy 文本管线读排版结果反推（取 `ComputedTextBlock` 的 parley layout 原始值，不用 `ceil` 过的 `TextLayoutInfo.size`），四个字段均有值且 `at_size == type_scale::CODE`；实测值写进启动日志。 | 独立复核 crates/xgent_ui/src/terminal/metrics.rs：探针 spawn_probe 用同一 mono 槽位（fonts.mono.clone()）、type_scale::CODE 字号、type_scale::line_height::TERM 行高、Visibility::Hidden（保留布局）；collect_mono_metrics 读 ComputTextBlock 的 parley 原始值（block.buffer().full_width()/height()/scale()，未用 ceil 过的 TextLayoutInfo.size），advance=(width/scale)/samples，还原逻辑像素；at_size 固定为 type_scale::CODE(=12.0，见 theme.rs:326)。四字段均有值：单测 mono_metrics_is_measured_at_runtime 实跑 headless App 断言 latin_advance>0、cjk_advance>0、line_height>0、at_size==type_scale::CODE，测试通过。实测值写 tracing::info!（家族+latin+cjk+行高+字号+单元格宽+结论），本轮复核确认 info! 行恒含结论。 |
| A2 | passed | brief.md | A2 双宽校验是纯函数：`cjk_advance ≈ 2 × latin_advance` 时不告警；偏差超阈值时返回告警文案并把基准宽调整为 `max(latin_advance, cjk_advance / 2)`。 | MonoMetrics::check() 是无副作用纯函数（metrics.rs:65-116）：double_width_ok = \|cjk-2*latin\| <= 2*latin*CJK_ADVANCE_TOLERANCE(0.1)；不满足时 cell_w = latin_advance.max(cjk_advance/2.0) 并返回告警文案。三个单测独立通过：double_width_ratio_ok_uses_latin_advance(7.2/14.4→cell_w=7.2 且 warning=None)、double_width_ratio_violation_warns_and_widens_cell(7.2/12.0→告警+cell_w=7.2 即 max 分支)、double_width_ratio_violation_widens_cell_when_cjk_is_wider(7.0/20.0→cell_w=10.0=cjk/2)。本轮新增 warning_wording_matches_actual_cell_width_change 亦通过：CJK 偏窄措辞含「维持」不含「调整为」，偏宽含「调整为」。 |
| A3 | passed | brief.md | A3 `display_width` 边界：CJK 全角 / emoji / U+FF00–FF60 → 2；组合记号 → 0；ASCII → 1。 | crates/xgent_terminal/src/render.rs:531 display_width 按 ZERO_WIDTH_RANGES→0、WIDE_RANGES(含 0xFF00-0xFF60)/EMOJI_NARROW_RANGES→2、其余→1。单测覆盖并通过：ascii_is_one_cell(1)、cjk_is_two_cells(中文/扩展A/扩展B/假名/谚文/顿号=2)、fullwidth_forms_are_two_cells(U+FF00..FF60=2 且 U+FF61 半角=1)、emoji_is_two_cells(U+1F600/U+1F680/U+26A1/U+2B50=2)、combining_marks_are_zero_cells(U+0301/U+FE0F/U+200D/U+200B=0)、display_width_str_sums_cells。xgent_terminal lib 33 项全绿。 |
| A4 | passed | brief.md | A4 按格布局内核 `layout_cells`：宽字符逐字成 run，`start_col` 严格按累计格数推进；窄字符合并为 run；`CellGrid.cols == floor(视口逻辑宽 / cell_w)`。 | render.rs:572 layout_cells 宽字符逐字成 run（cells=2）、窄字符同 style_index 合并（cells 累加）、零宽字符并入当前 run 不推进列；col 仅按 1/2 推进，start_col 严格等于其前累计格数。单测 layout_cells_merges_narrow_runs / layout_cells_splits_wide_chars_one_by_one(中 start_col=0、文 start_col=2) / layout_cells_advances_columns_by_cell_count(ab中cd→(0,2,ab)(2,2,中)(4,2,cd)) / layout_cells_respects_span_boundaries 全通过。CellGrid::from_viewport 用 (width/cell_w).floor() 得 cols，单测 cell_grid_cols_is_floor_of_width_over_cell_width 断言 floor(500/7.2)=69、rows=floor(300/19.2)=15 通过。 |
| A5 | passed | brief.md | A5 终端按格定位渲染：行内 run 节点的 `left` 等于 `起始列 × cell_w`（含中文的行）。 | crates/xgent_ui/src/terminal/output.rs:154-162 新路径 spawn_line 给每个 run 节点写 Node{position_type: Absolute, left: px(run.start_col as f32 * cell_w)}，cell_w 取 metrics.cell_width()（含双宽校验调整）；同时挂 OutputCellRun{start_col, cells} 供断言。集成测试 terminal_runs_are_positioned_by_cell 真实跑 headless App 喂入 ab中cd，断言 run 起始列为 0/2/4 且每个 run 的 Node.left == 起始列×cell_w（误差<0.01px），测试通过。 |
| A6 | passed | brief.md | A6 `handle_terminal_resize` 的列数来自实测 `cell_w`（`floor(视口逻辑宽 / cell_w)`），代码中不再出现 `font × 0.6` / `font × 1.4` 估算。 | output.rs:485-533 handle_terminal_resize 取输出容器 content_box 尺寸×inverse_scale_factor 再扣 Scrollbar 占位宽，交给 CellGrid::from_viewport(width,height,&metrics)（cols=floor(width/cell_w)），不再有字号系数估算；全仓 grep 确认 crates/xgent_terminal 与 crates/xgent_ui/src/terminal 下无 '* 0.6' / '* 1.4' / 'theme.font_size' 残留（仅 xui 编辑器与 chat_panel 的非终端代码有各自的 0.6/1.4，与本变更无关）。单测 legacy_column_width_estimate_is_removed 通过；集成测试 pty_cols_come_from_measured_cell_width 断言 grid.cols == floor((内容盒宽-滚动条)/cell_w) 通过。 |
| A7 | passed | brief.md | A7 旧渲染路径保留在 cargo feature `legacy-terminal-layout` 下，且该 feature 打开时仍能编译通过。 | crates/xgent_ui/Cargo.toml:11 声明 legacy-terminal-layout = []（默认不开启）；output.rs:194-233 旧路径 spawn_line（flex 行 + shaper 连续排版）完整保留在 #[cfg(feature = "legacy-terminal-layout")] 之下，与新路径 #[cfg(not(feature=...))] 编译期互斥。独立复跑 cargo check -p xgent_ui --all-targets --features legacy-terminal-layout → exit 0（仅 xgent_tools 等无关 crate 的既有告警）；测试文件 terminal_cell_layout.rs 对该 feature 下的两条按格定位用例做了 #[cfg(not(feature=...))] 门控，feature 下不产生未用导入告警。 |
| A8 | passed | brief.md | A8 既有测试不回归：`cargo check --workspace` 通过；`cargo fmt -p xgent_terminal -p xgent_ui` 无 diff；`cargo clippy -p xgent_terminal -p xgent_ui --all-targets` 在改动文件零新增警告；`xgent_terminal` / `xgent_ui` 既有测试全绿。 | 独立复跑（非复用 Runtime 回执）：cargo check --workspace → exit 0；cargo fmt -p xgent_terminal -p xgent_ui -- --check → exit 0 无 diff；cargo clippy -p xgent_terminal -p xgent_ui --all-targets → 按路径过滤，本变更改动文件（xgent_terminal/src/render.rs、xgent_ui/src/terminal/{metrics,output,mod}.rs、xgent_ui/tests/terminal_cell_layout.rs）零告警（现存告警全在 xgent_settings_core/xgent_tools/xgent_context 与 settings_* 两个无关集成测试）；cargo test -p xgent_ui --no-fail-fast → 全部测试二进制 0 failed（lib 84 + terminal_cell_layout 6 等）；cargo test -p xgent_terminal → lib 33 passed + integration 6 passed，0 failed。 |
| A9 | passed | brief.md | A9 文档同步：`docs/plans/ui-v8-tasks.md` 的 M4-T2 勾选 + §10 勘误、`docs/plans/ui-v8-handoff.md` 交付表 / 基线 / 下一步 / 硬门槛 A 状态、`docs/dev-tutorial.md` 新增 M4-T2 小节。 | 台账 docs/plans/ui-v8-tasks.md:214-226 M4-T2 四条子项全为 [x] 且验收行标注 Windows 已自动化断言/macOS-Linux 待办；§10 勘误 29-33 齐备（29 TextLayoutInfo.size 取整坑、30 CJK 回退非 2:1、31 硬门槛 A 降级不阻塞、32 headless 视口恒 0、33 绝对定位 max-content 定宽）。交接 docs/plans/ui-v8-handoff.md：§1 交付表 line 29 有 M4-T2 行、§1.1 硬门槛 A 状态、§2 基线（xgent_ui lib 84 / xgent_terminal lib 33 / terminal_cell_layout 6，与我实测数字一致）、§5 M4 尾巴、§7 下次开工 5 步齐全。开发指南 docs/dev-tutorial.md:850-869 有 §5.23 小节（宽度模型/实测度量/按格定位三交付点 + 8 条坑）。AGENTS.md:93/201 已同步 xgent_terminal 双宽单元格与 M4 状态。 |
| A10 | passed | brief.md | A10 平台验证状态落纸：Windows 本机的实机验证结论与 macOS / Linux 硬门槛 A 的**待确认**状态都写进台账与交接文档，且 macOS / Linux 待确认**不阻塞**本变更归档。 | 台账 ui-v8-tasks.md:224-225 明确：Windows（本机）✅ 自动化断言 + 实机视觉确认待人工；macOS / Linux ⏳ 待办（不阻塞本项归档），并在 §10 勘误 31 记录用户 2026-10-21 的降级决定。交接 handoff.md §1.1 line 33-34 同样分列：✅ Windows（本机，latin 6.60px / CJK 12.0px / 行高 19.2px @ 12px）、⏳ macOS/Linux（需各启动一次确认 mono 启动日志），并注明「降级为不阻塞，先做 Windows 适配」。两处均把待确认项显式标记为不阻塞，符合 A10 与 brief D-9。 |

## 检查

| 检查 | 命令 | 工作目录 | 状态 | 退出码 | 耗时 |
| --- | --- | --- | --- | ---: | ---: |
| xgent_terminal 与 xgent_ui 全量测试 | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo test -p xgent_terminal -p xgent_ui --no-fail-fast | . | passed | 0 | 25960 ms |
| 全工作区编译检查 | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo check --workspace | . | passed | 0 | 1179 ms |
| 改动 crate 的 clippy（--all-targets） | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo clippy -p xgent_terminal -p xgent_ui --all-targets | . | passed | 0 | 1215 ms |
| 改动 crate 的 rustfmt 检查（--check） | /c cargo fmt -p xgent_terminal -p xgent_ui -- --check | . | passed | 0 | 454 ms |
| 旧路径 feature（legacy-terminal-layout）编译检查 | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo check -p xgent_ui --all-targets --features legacy-terminal-layout | . | passed | 0 | 4849 ms |

### Builder 报告的证据

以下为 Builder 报告，不等同于 Runtime 检查凭据或独立验收结果。

- cargo check --workspace: passed — M4-T2 三个提交后全工作区编译通过
- cargo fmt -p xgent_terminal -p xgent_ui: passed — git diff 为空，无格式改动（未用 --all，避免改写 ../bevy 检出）
- cargo clippy -p xgent_ui -p xgent_terminal --all-targets: passed — 改动文件零新增警告；仓库既有的 xgent_tools / xgent_settings_core / xgent_context 警告不在本轮范围
- cargo test -p xgent_ui --no-fail-fast: passed — lib 82 passed + 17 个集成测试文件全绿（新增 terminal_cell_layout 5 项）
- cargo test -p xgent_terminal --no-fail-fast: passed — lib 33 passed（M4-T2 新增 12 项）+ 集成 6 passed
- cargo test --workspace --no-fail-fast: passed — 失败集合与既有基线完全一致（15 项：xgent_tools 6 / xgent_plugin_host 6 / xgent_daemon 1 / xgent_settings_core 1 / xgent_agent 1），无新增失败
- cargo check -p xgent_ui --all-targets --features legacy-terminal-layout: passed — A/B 对照用的旧路径 feature 打开时仍能编译通过
- 实机视觉确认（ls -l / 中文输出 / top / vim）: not-run — 需真实窗口 + 人工确认，本轮只交付几何级自动化断言；已作为遗留风险记入交接文档 §6
- macOS / Linux 平台的 mono 启动日志确认（硬门槛 A）: not-run — 本机为 Windows；按用户 2026-10-21 决定该项不阻塞本变更，已记为待办
- cargo test -p xgent_ui --test terminal_cell_layout: passed — 6 passed（验证轮新增 long_run_does_not_wrap_inside_the_run）
- cargo test -p xgent_ui --test terminal_cell_layout --features legacy-terminal-layout: passed — 4 passed 0 failed（A/B 旧路径配置下测试自洽）
- cargo clippy -p xgent_ui --all-targets --features legacy-terminal-layout: passed — 改动文件零新增警告
- cargo test -p xgent_ui --lib terminal::metrics: passed — 7 passed（新增 family_label_distinguishes_sources）
- cargo test -p xgent_ui --lib terminal::metrics: passed — 8 passed（新增 warning_wording_matches_actual_cell_width_change）
- 已知限制: 宽字符的字形填不满那 2 格（不是与相邻窄字符之间的间隙）：CJK 回退字体（雅黑/苹方/思源）的汉字步进是 1em，不是等宽拉丁的 2 倍——宽字符起点严格落在格边界（网格正确），但字形本身比 2 格窄（本机约差 1.2px/字）。根治需接入 mono_cjk 槽位（Sarasa / Noto Sans Mono CJK，真 2:1），本轮未做（见台账 §10 勘误 30）。注：C-FONT-4 的告警与否是平台相关的（本机偏差 9% < 10% 容差 → 不告警）。
- 已知限制: 硬门槛 A 的 macOS / Linux 的 mono 启动日志确认仍待办（用户 2026-10-21 决定降级为不阻塞，先做 Windows 适配，其他平台后续适配）。
- 已知限制: 实机视觉确认（ls -l / cargo test 中文输出 / top / vim 全屏 TUI）未自动化：headless 环境没有渲染管线，无法产出真实终端截图；本轮只交付几何级自动化断言（run 的 left == 起始列 × cell_w、PTY 列数 == floor(宽 / cell_w)）。
- 已知限制: UI 侧不再二次换行：PTY 已被告知正确的 cols 并由 shell 自己折行，超长行（PTY 未折行）会被视口裁掉——与真实终端一致，但与旧路径（flex wrap）行为不同。
- 已知限制: 等宽度量在首帧之后才就绪：实测需要 UI 布局管线跑起来，度量就绪前终端不渲染行（游标不推进，下一帧补齐），PTY 也暂不发 resize（保持默认 80×24）。

## 阻塞项

_无。_

## 风险与跳过的工作

- 交接文档 §1 交付表 line 29 与 §7 下次开工第 1 步 line 142 列出的 M4-T2 实现提交清单仍是 6 个（ff9c203/5e1d4cb/e06f77d/d3a276b/0fd632b/fe96a82），遗漏第三、四轮修复提交 41598f0 与 ea38c88——二者确在 HEAD（git log --oneline 已核）。属文档滞后，不影响 A9 的交付表/基线/下一步三要素存在性，但下轮开工按该清单核对会漏两条提交，建议归档前顺手补齐。
- CellGrid::from_viewport 对 floor 结果再取 .max(1.0)：视口不足一格宽时 cols=1 而非字面 floor(=0)。这是有意防御（0 列对 PTY 无意义，有单测 cell_grid_never_reports_zero 钉住），与 A4/A6 字面表述有细微偏差，正常视口下二者等价。
- MonoMetrics.line_height 在探针布局高度取不到时回退到 type_scale::CODE * line_height::TERM(=19.2) 常量，与实测值在本机恰好相同，故测试断言 line_height>0 无法区分「实测」与「回退」；实际走的是探针路径，但若未来探针失效会静默退化。
- 宽字符字形填不满 2 格（CJK 回退字体汉字步进 1em ≠ 2×等宽拉丁 0.6em，本机约差 1.2px/字）——已在 brief 非目标与台账勘误 30 显式接受，根治需接 mono_cjk 槽位，不阻塞本轮。
- 硬门槛 A 的 macOS / Linux 启动日志确认仍为待办（用户 2026-10-21 决定不阻塞）；实机视觉确认（ls -l / 中文输出 / top / vim）未自动化，已记入台账 line 224 与交接 §5「M4 尾巴」。
- 等宽度量依赖 UI 布局管线首帧后才就绪，度量未就绪前终端不渲染行（游标不推进）且不发 PTY resize（保持默认 80×24）；已在交接风险 8 记录，属 brief 接受的取舍。

## 之前的迭代

| 目标周期 | 迭代 | 尝试 | 结果 | 未解决项 | 摘要 | 完成时间 |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 1 | pass | — | 10/10 验收项全部通过（verdict=pass）。独立复跑证据：xgent_terminal lib 33 / xgent_ui lib 82 / 新增集成测试 terminal_cell_layout 5 及其余既有集成测试全绿（合计 217 passed、0 failed）；cargo fmt 两 crate 无 diff；cargo clippy 零警告落在改动文件；cargo check -p xgent_ui --features legacy-terminal-layout 编译通过。实现与 spec/brief 的核心口径一致：display_width 的 wcwidth 语义、layout_cells 的按格列推进、CellGrid.cols=floor(宽/cell_w)、run 节点 left=起始列×cell_w、PTY 列数来自实测 cell_w 且旧 font×0.6/×1.4 估算已彻底删除、legacy-terminal-layout feature 保留旧路径。文档三处（台账 M4-T2 勾选 + §10 勘误 29~32、交接交付表/基线/下一步/硬门槛 A 状态、开发指南 §5.23）同步到位，macOS/Linux 待确认被明确标记为不阻塞。独立发现 5 条风险，其中两条为实质项：① 勘误 30/§5.23 关于『警告分支是常态路径』的机制描述与本机实测数值（9.1% 偏差 < 10% 容差 ⇒ 实际不告警）自相矛盾，文档口径需订正；② 绝对定位 run 未设 NoWrap，超宽行可能在 run 内换行并压到下一行，与 D-6『不二次换行/裁剪』的描述不一致，无自动化覆盖，建议实机确认。两者均不构成任何一条验收项的失败。 | 2026-10-21T04:06:59.122Z |
| 1 | 1 | 1 | recovery | — | 独立 Verifier 对 10 项验收全部判 pass，但发现两处需修：① 实质缺陷——按格定位的 run 节点未设 TextLayout NoWrap，绝对定位下可用宽 = 容器宽 − left 偏移，靠右的 run 文本会在 run 内折行、行高超出固定 line_h 压到下一行，与已确认的 D-6「UI 侧不二次换行、超出视口裁剪」不符（补 NoWrap + 回归断言）；② 文档口径订正——勘误 30 与开发指南 §5.23 称双宽告警分支是「常态路径」，但按本机实测 latin 6.60 / cjk 12.0，偏差 9.1% < 10% 容差，Windows 实际走「不告警」分支。确认范围不变，返回 Build 修复后重新提交候选。 | 2026-10-21T04:07:56.865Z |
| 1 | 2 | 0 | recovery | — | Native confirmed acceptance criteria changed | 2026-10-21T04:16:21.707Z |
| 2 | 1 | 1 | pass | — | 第二轮独立只读核查（HEAD=0fd632b，工作树除 comet-state.yaml 外干净）：A1~A10 全部通过，verdict=pass。逐条核对了实现代码、bevy 0.20 / parley 0.11 上游 API 取数点、以及本机实测命令（cargo check --workspace EXIT=0；cargo fmt --check EXIT=0；clippy 改动文件零新增告警；cargo test -p xgent_terminal -p xgent_ui --no-fail-fast 全绿，lib 33+82、terminal_cell_layout 6 项；cargo check -p xgent_ui --all-targets --features legacy-terminal-layout EXIT=0）。修复轮三处改动复核结论：① run 显式 TextLayout{NoWrap} 确实落地（output.rs:176-179），且 bevy 0.20 的 TextLayout 默认是 LineBreak::WordBoundary（bevy_text/src/text.rs），改动真实改变了行为；新增 long_run_does_not_wrap_inside_the_run 通过，但它是「不变量守卫」而非 NoWrap 的判别器（taffy 对绝对定位子节点本就 max-content 定宽，勘误 33 已如实说明）；② 文档口径订正（勘误 30 + dev-tutorial §5.23 改为「校验结果平台相关」）已逐字核对，与实现 metrics.rs:75 的 10% 相对容差可复现一致；③ Spec/brief 取数点措辞订正为 ComputedTextBlock parley 原始值，与实现及 bevy pipeline.rs:473 的 ceil 事实一致。独立发现 3 项应清理项（output.rs 被引入 UTF-8 BOM、交接文档基线 terminal_cell_layout 仍写 5、交接文档提交清单漏 d3a276b/0fd632b）与 1 项需知悉项（legacy feature 打开时 2 个集成测试会红），均不影响本轮 10 条验收判定，建议在归档前或下一轮顺带修掉。 | 2026-10-21T04:26:53.102Z |
| 2 | 1 | 1 | recovery | — | 第二轮独立 Verifier 对 10 项验收全判 pass，另发现三处需修并已修完（提交 fe96a82）：① 清理 terminal/output.rs 的 UTF-8 BOM（第二轮验证时用 PowerShell Set-Content 回写引入；rustc 接受 BOM 故编译全绿，但会让剥注释行的静态断言漏掉首行）——已去 BOM 并加「源文件不得带 BOM」断言；② A/B feature 下测试自洽——按格定位专属的两个用例在 legacy-terminal-layout 下会红，加 cfg 门后默认 6 项 / legacy 4 项都全绿；③ 交接文档订正（terminal_cell_layout 用例数 5 → 6；提交清单补 d3a276b / 0fd632b）。因修复发生在第二轮验收之后、候选已变，确认范围不变，返回 Build 重新提交候选并再做一轮独立验证。 | 2026-10-21T04:33:37.703Z |
| 2 | 2 | 1 | pass | — | 第三轮候选（HEAD=fe96a82；工作树除 docs/comet 状态文件外干净，无 BOM、无未提交实现改动）独立只读核查：A1~A10 全部通过，verdict=pass。本轮三处新改动复核结论均为真实落地且有效：① crates/xgent_ui/src/terminal/output.rs 已无 UTF-8 BOM（前三字节实测非 EF BB BF），tests/terminal_cell_layout.rs:275-278 新增的「源文件不得带 BOM」断言是真守卫，注释过滤另在 :282 剥 BOM 兜底；② 两个按格定位专属用例已加 #[cfg(not(feature = legacy-terminal-layout))]（:156 / :308）、OutputCellRun 的 import 同带 cfg 门（:15-16），本人实测默认配置 6 passed / legacy 配置 4 passed，0 failed，两种配置 clippy 改动文件均零告警；③ 交接文档基线用例数已 5→6（:47）、交付表与 §7 已补齐 d3a276b / 0fd632b（:29 / :142）。核心实现经独立核对属实：探针实测取 parley 原始值（已对照 bevy 0.20 上游 pipeline.rs:473 的 ceil 事实）、display_width/layout_cells 区间表有序且二分前提成立、run 的 left=起始列×cell_w、PTY 列数来自实测 cell_w 且旧 font×0.6/×1.4 估算仅存于文档注释。残余风险均为文档滞后或 brief 已显式接受的已知限制，不构成任何一条验收项的失败。 | 2026-10-21T04:38:00.933Z |
| 2 | 2 | 1 | recovery | — | 第三轮独立 Verifier 对 10 项验收全判 pass，另发现三处小项并已修完（提交 41598f0）：① 等宽度量日志补上家族名——brief 范围 §1 要求「家族 + 四个数值 + 校验结论」同在一行（三平台核对不必跨两条日志对照），新增 mono_family_label 区分系统家族/内嵌句柄/通用族兜底并补单测；② 台账顶部状态行仍写「M4 起未开工」，与 §M4「M4 全部交付」冲突（既有滞后）已订正；③ 交接文档基线 xgent_ui lib 82 → 83、M4-T2 提交清单补 fe96a82。因修复发生在第三轮验收之后、候选已变，确认范围不变，返回 Build 重新提交候选并做最后一轮独立验证。 | 2026-10-21T04:42:26.244Z |
| 2 | 3 | 1 | pass | — | 独立复核当前候选（HEAD=41598f0）后，10 条验收项 A1-A10 全部通过。实现侧核实：display_width/layout_cells 为无 Bevy 依赖的纯函数（render.rs:531-631，xgent_terminal lib 33 项全绿）；MonoMetrics 由 bevy 文本管线探针实测（metrics.rs:189-327，at_size==CODE，家族+四值+结论入日志）；双宽校验为纯函数 check()（metrics.rs:65-104）；终端按格定位渲染 left=起始列×cell_w（output.rs:157）；PTY 列数取自实测 cell_w，旧 font×0.6/×1.4 估算在改动文件零残留；旧路径保留在 legacy-terminal-layout feature 下且该配置独立复跑编译并 4 项全过。独立执行的只读验证：cargo fmt --check 无 diff、xgent_terminal lib 33+集成 6、xgent_ui terminal_cell_layout 6、xgent_ui lib metrics 7 全绿，Runtime 的 c1-c5 回执一并采信。文档侧：台账 M4-T2 勾选 + §10 勘误 29-33、交接交付表/基线(83/33/6)/下一步 M5/硬门槛 A 状态、dev-tutorial §5.23、AGENTS.md 全部到位；Windows 结论与 macOS/Linux 待确认且明确不阻塞。本轮三处新改动均已复核并落实，但发现四处低severity 事项：告警分支下「校验结论」不在 info! 行、告警文案「调整为」在无实际调整时略误导、通用族兜底标签仅为 debug 名（Monospace）、交接提交清单未含 41598f0 自身；均不违反任何一条验收项，判 pass 并建议在归档/后续轮次顺带收敛。 | 2026-10-21T04:48:37.606Z |
| 2 | 3 | 1 | recovery | — | 第四轮独立 Verifier 对 10 项验收全判 pass，另指出三处措辞不准并已修完（提交 ea38c88）：① 双宽告警措辞如实化——原警告恒称「基准宽调整为 X px」，但 CJK 步进偏窄时基准宽其实没变，改为按实际分「由 A 调整为 B」/「维持 A」两种措辞并补单测；② info! 行恒带校验结论（此前告警分支的结论只在单独的 warn! 行），现在恒为「家族 + 四个数值 + 单元格宽 + 结论」；③ 家族标签更明确（通用族兜底不再只印 debug 名，改为「通用族兜底（Monospace）」/「系统家族 X」）。因修复发生在第四轮验收之后、候选已变，确认范围不变，返回 Build 重新提交候选并做最后一轮独立验证。 | 2026-10-21T04:52:56.645Z |
| 2 | 4 | 1 | pass | — | 独立复核 HEAD=ea38c88 候选实现，10 条验收项全部通过。A1/A2 复核了本轮新改的实测路径与双宽校验纯函数（含新增单测 warning_wording_matches_actual_cell_width_change 通过）；A2 与日志部分确认本轮三处新改均已如实落地：告警按 CJK 步进偏宽/偏窄分两种措辞（偏宽「由 A 调整为 B」、偏窄「维持 A（= 窄字符步进）」）、info! 行恒含家族+四数值+单元格宽+校验结论（告警细节另走 warn! 且同样带家族名）、家族标签区分「系统家族 X / 内嵌资产（句柄）/ 通用族兜底（Monospace）」。独立复跑 check/fmt/clippy/test 全绿（xgent_ui lib 84 + terminal_cell_layout 6、xgent_terminal lib 33 + integration 6、改动文件 clippy 零告警、legacy-terminal-layout feature 下 check 通过）。文档三处（台账 M4-T2 勾选 + 勘误 29~33、交接交付表/基线/下一步/硬门槛 A、开发指南 §5.23）齐备，Windows 结论与 macOS/Linux 待确认且不阻塞的状态均已落纸。独立发现 1 处文档滞后（交接提交清单缺 41598f0/ea38c88）与 2 处口径细节（CellGrid 的 .max(1.0) 下限、line_height 回退常量），均为非阻塞项。 | 2026-10-21T06:07:41.337Z |



## 结论

独立复核 HEAD=ea38c88 候选实现，10 条验收项全部通过。A1/A2 复核了本轮新改的实测路径与双宽校验纯函数（含新增单测 warning_wording_matches_actual_cell_width_change 通过）；A2 与日志部分确认本轮三处新改均已如实落地：告警按 CJK 步进偏宽/偏窄分两种措辞（偏宽「由 A 调整为 B」、偏窄「维持 A（= 窄字符步进）」）、info! 行恒含家族+四数值+单元格宽+校验结论（告警细节另走 warn! 且同样带家族名）、家族标签区分「系统家族 X / 内嵌资产（句柄）/ 通用族兜底（Monospace）」。独立复跑 check/fmt/clippy/test 全绿（xgent_ui lib 84 + terminal_cell_layout 6、xgent_terminal lib 33 + integration 6、改动文件 clippy 零告警、legacy-terminal-layout feature 下 check 通过）。文档三处（台账 M4-T2 勾选 + 勘误 29~33、交接交付表/基线/下一步/硬门槛 A、开发指南 §5.23）齐备，Windows 结论与 macOS/Linux 待确认且不阻塞的状态均已落纸。独立发现 1 处文档滞后（交接提交清单缺 41598f0/ea38c88）与 2 处口径细节（CellGrid 的 .max(1.0) 下限、line_height 回退常量），均为非阻塞项。
