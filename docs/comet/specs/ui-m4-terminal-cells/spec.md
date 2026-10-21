# ui-m4-terminal-cells — 终端双宽单元格与按格定位

## Purpose

把终端输出从「按字体系数估算列宽 + shaper 连续排版」改成**显式双宽单元格模型**：

- 等宽槽位（`mono`）的字符前进宽度**实测**得出（不再用 `font × 0.6` 之类的系数估算）。
- 每个字符**占几格**显式化（wcwidth 语义：CJK 全角 / emoji / U+FF00–FF60 占 2 格）。
- 终端行按「格数 × cell_w」**手工定位**渲染，不再依赖文本 shaper 的连续排版度量。
- PTY 的列数由实测 `cell_w` 派生（`floor(视口逻辑宽 / cell_w)`），与可见宽度一致。

这是 UI v8 交互架构层重建的最后一期（台账 `docs/plans/ui-v8-tasks.md` §M4-T2）。

## Requirements

### 等宽槽位的实测度量

- `xgent_ui::terminal::metrics::MonoMetrics { latin_advance, cjk_advance, line_height, at_size }` 为独立 `Resource`（不塞进 `UiFonts`）。
- 实测方式：启动期在 UI 文本管线上挂探针节点（ASCII 样本串 × N、CJK 样本串 × N），读 bevy 文本管线产出的排版结果反推单字符前进宽度与行高；取数点是 `ComputedTextBlock` 的 parley layout 原始值（`full_width` / `height` / `scale`）——**不是** `TextLayoutInfo.size`，后者是 `ceil` 过的（12px × 1.6 行高会被抬成 20.0 而非 19.2），误差会直接进到单元格基准宽上；探针使用与终端渲染相同的 `mono` 槽位来源、`type_scale::CODE` 字号与 `type_scale::line_height::TERM` 行高。
- 校验 `cjk_advance ≈ 2 × latin_advance`（设计系统 C-FONT-4）：偏差超阈值时记警告，并把单元格基准宽调整为 `max(latin_advance, cjk_advance / 2)`。
- 实测值与校验结论写 `tracing::info!`。

#### Scenario: 启动期实测出等宽度量

- **Given** 装配了 `UiFonts`（`mono` 槽位已解析）与 UI 文本管线的 App
- **When** 跑若干帧
- **Then** `MonoMetrics` 资源存在，`latin_advance > 0`、`cjk_advance > 0`、`line_height > 0`，且 `at_size == type_scale::CODE`

验收：A1

#### Scenario: 双宽比例校验与基准宽调整

- **Given** 一组实测度量
- **When** 校验 `cjk_advance` 与 `2 × latin_advance` 的关系
- **Then** 偏差在阈值内时无告警且基准宽为 `latin_advance`；偏差超阈值时产出告警文案且基准宽为 `max(latin_advance, cjk_advance / 2)`

验收：A2

### 字符占格语义

- `xgent_terminal::render::display_width(ch: char) -> usize`：CJK 全角 / emoji / U+FF00–FF60 → 2；组合记号（Mn / Me）→ 0；其余 → 1。
- `xgent_terminal::render::display_width_str(s: &str) -> usize`：整串占格数。

#### Scenario: display_width 的 wcwidth 边界

- **Given** 各类字符
- **When** 逐个求占格数
- **Then** CJK 全角 / emoji / U+FF00–FF60 得 2，组合记号得 0，ASCII 得 1

验收：A3

### 按格定位的排版内核

- `xgent_terminal::render::layout_cells(spans) -> Vec<CellRun>`：把带样式的 span 序列切成「起始列 + 占格数」的 run 序列。
  - 窄字符（1 格）连续且同风格的相邻字符合并为一个 run。
  - 宽字符（2 格）逐字成 run。
- `xgent_ui::terminal::metrics::CellGrid { cols, rows, cell_w, line_h }`：由视口逻辑宽高与 `MonoMetrics` 派生，`cols = floor(视口逻辑宽 / cell_w)`。

#### Scenario: 按格布局内核的列推进

- **Given** 一行含窄字符与宽字符的 span 序列
- **When** 求按格布局
- **Then** 宽字符逐字成 run，且每个 run 的 `start_col` 等于其之前所有字符占格数之和；窄字符连续同风格者合并为一个 run

验收：A4

### 终端按格定位渲染

- `terminal/output.rs` 的行渲染改为「行容器 + 绝对定位的 run 节点」，run 的 `left = 起始列 × cell_w`；行高用实测 `line_h`。
- `handle_terminal_resize` 的列 / 行数改用实测 `cell_w` / `line_h`；视口宽取输出容器内容盒宽（扣掉横向内边距与滚动条占位）。
- 删除 `font × 0.6` / `font × 1.4` 估算。
- UI 侧不二次换行：PTY 已被告知正确的 `cols`，由 shell 自己折行；超出视口的格子被裁剪。

#### Scenario: 行内 run 按格定位

- **Given** 已装配 `MonoMetrics` 的终端视图，收到一行含中文的输出
- **When** 查询该行内各 run 节点的 `Node.left`
- **Then** 每个 run 的 `left` 等于其起始列乘以 `cell_w`

验收：A5

#### Scenario: PTY 列数来自实测 cell_w

- **Given** 视口逻辑宽与实测 `MonoMetrics`
- **When** 计算给 PTY 的 resize 列数
- **Then** 列数等于 `floor(视口逻辑宽 / cell_w)`；代码中不再存在按字体系数估算列宽的表达式

验收：A6

### 旧路径 A/B 对照

- cargo feature `legacy-terminal-layout`（默认不开启）保留旧的 flex 连续排版渲染路径，用于与新路径做 A/B 截图对照；该 feature 打开时仍能编译通过。

#### Scenario: 旧路径保留在 feature flag 下

- **Given** 开启 cargo feature `legacy-terminal-layout`
- **When** 编译工作区
- **Then** 编译通过，且旧渲染路径仍被保留

验收：A7

### 不回归

- `cargo check --workspace` 通过；`cargo fmt -p xgent_terminal -p xgent_ui` 后无 diff；`cargo clippy -p xgent_terminal -p xgent_ui --all-targets` 在改动文件零新增警告。
- `xgent_terminal` / `xgent_ui` 既有单元测试与集成测试全绿。

#### Scenario: 既有测试不回归

- **Given** 实现完成的仓库
- **When** 跑 `xgent_terminal` 与 `xgent_ui` 的测试
- **Then** 全部通过（失败数为 0）

验收：A8

### 文档同步

- `docs/plans/ui-v8-tasks.md`：M4-T2 勾选 + §10 追加本轮勘误。
- `docs/plans/ui-v8-handoff.md`：交付表 + 基线 + 下一步 + 硬门槛 A 状态。
- `docs/dev-tutorial.md`：新增 M4-T2 小节（单元格模型、实测路径、坑）。

#### Scenario: 文档同步

- **Given** 实现完成的仓库
- **When** 检索 `docs/dev-tutorial.md`、`docs/plans/ui-v8-tasks.md`、`docs/plans/ui-v8-handoff.md`
- **Then** 三处都记录了 M4-T2 的落地内容、平台验证状态与本轮的取舍

验收：A9

### 平台验证状态

- Windows 本机完成实测与实机验证；macOS / Linux 的硬门槛 A 启动日志确认记为**待办**（不阻塞本变更归档）。

#### Scenario: 平台验证状态落纸

- **Given** 本变更完成
- **When** 检索台账与交接文档中的平台验证记录
- **Then** Windows 的验证结论与 macOS / Linux 的待确认状态都在文档中，且后者被明确标记为不阻塞

验收：A10
