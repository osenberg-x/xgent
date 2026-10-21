# 目标

把终端输出从「按字体系数估算列宽 + shaper 连续排版」改成**显式双宽单元格模型**：等宽槽位的字符前进宽度**实测**得出、每个字符**占几格**显式化（wcwidth 语义）、终端行按「格数 × cell_w」**手工定位**渲染，使中文 / emoji / 全角在终端里稳定占 2 格，PTY 的列数与可见宽度一致。

这是 UI v8 交互架构层重建的**最后一期**（台账 `docs/plans/ui-v8-tasks.md` §M4-T2），交付后 M4 收口。

# 范围

## 1. `MonoMetrics` 启动期实测（台账第 1 条）

- 新增 `MonoMetrics { latin_advance, cjk_advance, line_height, at_size }`（`xgent_ui::terminal::metrics`，`Resource`）。
- **实测**而非估算：启动期在 UI 文本管线上挂探针节点（ASCII 样本串 × N、CJK 样本串 × N），读 bevy 的 `TextLayoutInfo`（`size` / `scale_factor`）反推单字符前进宽度与行高。探针走的是**终端渲染同一条 measure 路径**：同一 `mono` 槽位来源、同一 `type_scale::CODE` 字号、同一 `type_scale::line_height::TERM` 行高。
- 校验 `cjk_advance ≈ 2 × latin_advance`（设计系统 C-FONT-4）：偏差超阈值 → **记警告**并把单元格基准宽调整为 `cell_w = max(latin_advance, cjk_advance / 2)`。校验与调整是**纯函数**，可单测。
- 实测值写 `tracing::info!`（家族来源 + 四个数值 + 校验结论），三平台可核对。

## 2. `display_width(char)`：wcwidth 语义（台账第 2 条）

- 落在 `xgent_terminal::render`（终端领域 crate，不依赖 Bevy，可脱离 Bevy 快速单测）。
- 规则：CJK 全角 / emoji / U+FF00–FF60 → **2**；组合记号（Mn / Me）→ **0**；其余 → **1**。
- 附 `display_width_str(&str) -> usize`（整串占格数）。

## 3. 按格定位的排版内核（台账第 3 条前半）

- `xgent_terminal::render::layout_cells(spans) -> Vec<CellRun>`：把带样式的 span 序列切成「起始列 + 占格数」的 run 序列（纯函数）。
  - 窄字符（1 格）连续且同风格的相邻字符合并为一个 run（等宽字体下连续排版与按格定位等价）。
  - 宽字符（2 格）**逐字**成 run——保证每个宽字符的起点严格落在格边界上，不依赖 CJK 回退字体的连续排版度量。
- `CellGrid { cols, rows, cell_w, line_h }`：由视口逻辑宽高 + `MonoMetrics` 派生；`cols = floor(视口逻辑宽 / cell_w)`。

## 4. 终端按格定位渲染（台账第 3 条后半）

- `terminal/output.rs` 的行渲染从「flex 行 + span 连续排版」改为「行容器 + 绝对定位的 run 节点」：run 的 `left = 起始列 × cell_w`。
- 行高改用实测 `line_h`（此前空行用 `theme.font_size × 1.4` 估算）。
- `handle_terminal_resize` 的列 / 行数改用实测 `cell_w` / `line_h`：
  - `cols = floor(视口逻辑宽 / cell_w)`；视口宽取输出容器**内容盒**宽（扣掉 8px 横向内边距与滚动条占位）。
  - **删除** `font × 0.6` / `font × 1.4` 估算；同时修正字号取错的问题（此前用界面正文 `theme.font_size = 14`，终端实际按 `type_scale::CODE = 12` 渲染 → 列数被低估约 17%）。
- UI 侧**不二次换行**：PTY 已被告知正确的 `cols`，由 shell 自己折行；超出视口的格子被裁剪（避免「shell 折一次 + UI 再折一次」的双重折行错位）。

## 5. 旧路径 A/B 对照（台账第 4 条）

- 新增 cargo feature `legacy-terminal-layout`（默认**不开启**）：开启时走旧的 flex 连续排版渲染路径，用于与新路径做 A/B 截图对照；对照通过后删除旧路径与该 feature。

# 非目标

- **不做 M5 / M6 / M7**（槽位注册表、Transcript/Markdown、快捷键/焦点/i18n/几何断言层）、插件与 Git 面板。
- **不改 M4-T1 / T3 / T4 已交付行为**（候选栈、CODE 档、负字距规则、正文行高档）。
- **不做真正的屏幕字符网格**：行模型仍是 `RenderLine`（行 + span），不引入 alternate screen、光标寻址、全屏重绘；按格定位只解决**行内**对齐。
- **不做终端输入行（tv-inputline）的按格定位**：输入行是 UI 侧单行编辑，不参与 PTY 网格。
- **不接 `mono_cjk` 槽位**：终端 CJK 仍走 `mono` 槽位的 fallback 链（本轮只把「回退字体不是 2:1」这件事**测出来并记警告**）。
- **不把 `MonoMetrics` 塞进 `UiFonts`**（见 D-1）。
- **本轮不做 macOS / Linux 实机验证**（见 D-9）。

# 验收示例

- A1 `MonoMetrics` 由**实测**得出：探针经 bevy 文本管线读排版结果反推（取 `ComputedTextBlock` 的 parley layout 原始值，不用 `ceil` 过的 `TextLayoutInfo.size`），四个字段均有值且 `at_size == type_scale::CODE`；实测值写进启动日志。
- A2 双宽校验是纯函数：`cjk_advance ≈ 2 × latin_advance` 时不告警；偏差超阈值时返回告警文案并把基准宽调整为 `max(latin_advance, cjk_advance / 2)`。
- A3 `display_width` 边界：CJK 全角 / emoji / U+FF00–FF60 → 2；组合记号 → 0；ASCII → 1。
- A4 按格布局内核 `layout_cells`：宽字符逐字成 run，`start_col` 严格按累计格数推进；窄字符合并为 run；`CellGrid.cols == floor(视口逻辑宽 / cell_w)`。
- A5 终端按格定位渲染：行内 run 节点的 `left` 等于 `起始列 × cell_w`（含中文的行）。
- A6 `handle_terminal_resize` 的列数来自实测 `cell_w`（`floor(视口逻辑宽 / cell_w)`），代码中不再出现 `font × 0.6` / `font × 1.4` 估算。
- A7 旧渲染路径保留在 cargo feature `legacy-terminal-layout` 下，且该 feature 打开时仍能编译通过。
- A8 既有测试不回归：`cargo check --workspace` 通过；`cargo fmt -p xgent_terminal -p xgent_ui` 无 diff；`cargo clippy -p xgent_terminal -p xgent_ui --all-targets` 在改动文件零新增警告；`xgent_terminal` / `xgent_ui` 既有测试全绿。
- A9 文档同步：`docs/plans/ui-v8-tasks.md` 的 M4-T2 勾选 + §10 勘误、`docs/plans/ui-v8-handoff.md` 交付表 / 基线 / 下一步 / 硬门槛 A 状态、`docs/dev-tutorial.md` 新增 M4-T2 小节。
- A10 平台验证状态落纸：Windows 本机的实机验证结论与 macOS / Linux 硬门槛 A 的**待确认**状态都写进台账与交接文档，且 macOS / Linux 待确认**不阻塞**本变更归档。

# 决策

- D-1 `MonoMetrics` 独立成 `Resource`，**不塞进** `UiFonts`：实测需要 UI 布局管线先跑起来（探针节点要有 `ComputedNode`），而 `UiFonts` 在 Startup 就必须可用——塞进去会形成「等 `UiFonts` → 等布局 → 等 `UiFonts`」的循环。
- D-2 实测走 **bevy 文本管线**（探针 + 排版结果），不直接读 parley / fontique 的字形度量：前者是终端渲染**同一条** measure 路径，自动处理 fallback 链（CJK 由回退字体渲染），与布局架构 §8.2「经同一 measure 路径取」一致；后者要自己复刻缩放与回退逻辑，且不同平台的 `FontMetrics` 可用性不一致。取数点是 `ComputedTextBlock` 的 parley layout **原始值**（`full_width`/`height`/`scale`），不是 `TextLayoutInfo.size`——后者是 `ceil` 过的（12px × 1.6 行高会被抬成 20.0 而非 19.2；32 个 6.6px 字符的 211.2 会变成 212），误差会直接进到单元格基准宽上。
- D-3 `display_width` 与 `layout_cells` 放 `xgent_terminal::render`（终端领域 crate，无 Bevy 依赖）：单元格语义属终端域，且能脱离 Bevy 快速单测。
- D-4 组合记号返回 **0**（真 wcwidth 语义），而非布局架构 §8.3 简写里的「其余 → 1」：终端里组合记号与前一字符**共用一格**，返回 1 会让后续所有列右移一格，与「列对齐正确」这个验收目标直接冲突。
- D-5 宽字符**逐字**成 run：CJK 回退字体多为比例字体（微软雅黑 / 苹方 / 思源黑体的汉字步进 = 1em，而等宽拉丁步进 = 0.6em），连续排版与「2 格」不等宽；逐字定位才能保证宽字符起点严格落在格边界。
- D-6 UI 侧**不二次换行**（超出视口裁剪）：PTY 已被告知正确的 `cols`，由 shell 自己折行；UI 再折一次就是双重折行，必然错位。
- D-7 旧路径保留用 **cargo feature**（`legacy-terminal-layout`，默认关）而非运行时开关：台账原文写「feature flag」；代价是 A/B 对照要两次构建，换来两套渲染路径编译期互斥。
- D-8 列数的视口宽取输出容器**内容盒**宽并扣掉滚动条占位：容器有 8px 横向内边距，滚动条是绝对定位覆盖层，不扣会让最右一列被裁掉。
- D-9 硬门槛 A 的 macOS / Linux 确认**不阻塞**本变更（用户 2026-10-21 决定：先做 Windows 适配，其他平台后续适配）：本变更在 Windows 上完成实测与实机验证，macOS / Linux 的确认状态记入台账与交接，留作后续。
