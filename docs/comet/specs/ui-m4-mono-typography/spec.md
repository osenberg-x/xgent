# ui-m4-mono-typography — 代码类文本消费 mono 与中文排版约束

## Purpose

让代码类文本（编辑器代码正文、编辑器行号列、diff 行、工具卡参数摘要）一律走 `mono` 槽位（真实等宽），并给中文排版定下两条约束：展示级负字距不作用中文、消息正文按语言选行高档。

- 今天 `type_scale::MONO` 只改了**字号**没改**字体**：四处代码类文本实际用比例字体（Inter）渲染，等于「用 ui_text 冒充等宽」→ 行号数字宽度不等，行号列与代码列错位。
- M4-T1 落地后 `mono` 已是解析出的真实等宽资产，这四处必须接上。
- 中文：展示级负字距（welcome 标题 −0.29）会让 CJK 挤成一团；消息正文固定 1.6 的行高在长段中文下偏挤。

本变更只交付**排版消费侧**（M4-T3 + M4-T4）。终端双宽单元格（M4-T2）独立成 change，且其前置的硬门槛 A（macOS/Linux 启动日志确认）尚未闭环。

## Requirements

### 字号阶梯

- `xgent_ui::theme::type_scale::SMALL` 由 12.5 改为 12.0（半像素在非整数 DPI 缩放下产生不稳定模糊，且 12.5 与 12 视觉差不可辨）。
- 代码档独立为 `type_scale::CODE`（12.0）：原 `MONO` 常量的三个使用点（确认弹窗 diff 行、diff 视图行、终端输出）全是代码场景，改名而非新增同值常量。
- 新增 `type_scale::line_height::CJK = 1.7`（CJK 密排可读性低于拉丁；`BODY = 1.6` 在长段中文下偏挤）。

#### Scenario: 字号令牌收敛

- **Given** 排版阶梯常量表
- **When** 读取代码字号档与中文行高档
- **Then** `type_scale::SMALL == 12.0`、`type_scale::CODE == 12.0`、`type_scale::line_height::CJK == 1.7`

验收：A4

### 代码类文本消费 mono

- `xui::text_editor::EditorTheme` 携带字体来源（`FontSource`）：今天它只有颜色与字号，代码正文因此落回默认字体。
- `xui::text_editor::virtual_render` 创建与更新代码文本的 `TextFont` 时使用该字体来源。
- `xgent_ui::editor::sync_editor_theme` 把 `UiFonts.mono` 注入 `EditorTheme`。
- 编辑器行号列（`xgent_ui::editor::tabs`）的 `TextFont` 使用 `UiFonts.mono`。
- diff 行（`xgent_ui::editor::diff_view`）改用等宽构造器（此前是 `ui_text` + 等宽字号）。
- 工具卡参数摘要（`xgent_ui::tool_panel`）改用等宽版省略构造器（参数是路径/正则这类代码文本）。

#### Scenario: 编辑器代码正文走 mono

- **Given** 装配了 `UiFonts` 与 `Theme` 的 App
- **When** 运行 `sync_editor_theme`
- **Then** `EditorTheme.font` 等于 `UiFonts.mono`

验收：A1

#### Scenario: 行号列与代码列同源

- **Given** 打开一个文件后的编辑器视图
- **When** 读行号实体的 `TextFont`
- **Then** 其 `font` 等于 `UiFonts.mono`（与代码正文同源 → 数字等宽 → 行号列与代码列对齐）

验收：A2

#### Scenario: diff 行与参数摘要走 mono

- **Given** 等宽文本构造器与等宽省略构造器
- **When** 构造 diff 行文本与工具卡参数摘要文本
- **Then** 两者的 `TextFont.font` 等于 `UiFonts.mono`

验收：A3

### 中文排版约束

- `xgent_ui::fonts::display_spacing(text)`：文本含任一 CJK 码位 → 字距 `0`；纯拉丁 → `-0.29`。
- welcome 标题使用该函数，不再硬写负字距。
- `xgent_ui::theme::type_scale::line_height::for_text(text)`：含 CJK → `CJK`（1.7），否则 `BODY`（1.6）。
- 对话消息正文（用户消息与助手正文）使用该函数。
- 检索链路的 CJK 二元组切分（`xgent_context`）保持不变（本轮只做回归确认）。

#### Scenario: 负字距不作用中文

- **Given** 展示级标题的负字距规则
- **When** 对含中文的标题文本与纯拉丁标题文本分别求字距
- **Then** 含中文的返回 0，纯拉丁的返回 -0.29

验收：A5

#### Scenario: 消息正文行高按内容选档

- **Given** 消息正文的行高规则
- **When** 对含中文的正文与纯拉丁正文分别求行高
- **Then** 含中文的返回 1.7，纯拉丁的返回 1.6

验收：A6

#### Scenario: 检索链路 CJK 分词未回归

- **Given** 中文问句的关键词抽取
- **When** 走既有 `xgent_context` 的切分逻辑
- **Then** 仍能抽出中文二元组（既有单测通过）

验收：A7

### 不回归

- `xgent_ui` lib 76 项与既有集成测试全绿；`xui` 全绿。
- `cargo check --workspace` 通过；`cargo clippy -p xui -p xgent_ui -p xgent_app --all-targets` 在本次改动文件零新增警告；`cargo fmt -p xui -p xgent_ui -p xgent_app` 后无 diff。

#### Scenario: 既有测试不回归

- **Given** 实现完成的仓库
- **When** 跑 lib 与集成测试
- **Then** 全部通过（失败数为 0）

验收：A8

### 文档同步

- `docs/plans/ui-v8-tasks.md`：M4-T3 / M4-T4 勾选 + §10 追加本轮勘误。
- `docs/dev-tutorial.md`：新增 M4 排版小节（mono 消费面、CJK 约束、坑）。
- `docs/plans/ui-v8-handoff.md`：交付表 + 基线 + 下一步。

#### Scenario: 文档同步

- **Given** 实现完成的仓库
- **When** 检索 `docs/dev-tutorial.md` 与 `docs/plans/ui-v8-tasks.md`
- **Then** 两处都记录了 M4-T3/M4-T4 的落地内容与本轮的取舍

验收：A9
