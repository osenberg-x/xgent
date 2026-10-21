# 目标

让**代码类文本**一律走 `mono` 槽位（真实等宽），并给中文排版定下两条约束：展示级负字距不作用中文、消息正文按语言选行高档。

1. **代码与行号消费 `mono`**（台账 M4-T3）：今天编辑器代码正文、编辑器行号列、diff 行、工具卡参数摘要**全都用比例字体**（Inter）渲染——`type_scale::MONO` 只改了**字号**没改**字体**，等于「用 ui_text 冒充等宽」。M4-T1 落地后 `mono` 已是真实等宽资产，这四处必须接上。
2. **中文排版约束**（台账 M4-T4）：① 展示级负字距（welcome 标题 −0.29）不作用中文（CJK 无字距概念，负值会让中文挤成一团）；② 消息正文按语言选行高档（中文 1.7 / 拉丁 1.6），新增 `line_height::CJK` 供按语言选档。
3. **字号阶梯收尾**：`SMALL 12.5 → 12`（半像素在非整数 DPI 缩放下产生不稳定模糊，且 12.5 与 12 视觉差不可辨）；代码档独立为 `CODE`。

# 范围

## 字号阶梯（`xgent_ui::theme::type_scale`）

- `SMALL: 12.5 → 12.0`（消除半像素模糊）。
- 代码档独立：`MONO` 的三个使用点（确认弹窗 diff 行、diff 视图行、终端输出）**全是代码场景** → 改名为 `CODE`（值仍 12.0），避免留两个同值常量。设计意图（代码字号与界面字号解耦）由此达成。
- 新增 `line_height::CJK = 1.7`（CJK 密排可读性低于拉丁；`BODY = 1.6` 在长段中文下偏挤）。

## 代码类文本走 `mono`

- `xui::text_editor::EditorTheme` 增加字体来源字段（`FontSource`）——今天它只有颜色与字号，代码正文因此落回默认字体。`xui/src/text_editor/virtual_render.rs` 创建/更新代码文本 `TextFont` 时带上它。
- `xgent_ui::editor::sync_editor_theme` 把 `UiFonts.mono` 注入 `EditorTheme` → 编辑器代码正文走等宽。
- 编辑器行号列（`xgent_ui/src/editor/tabs.rs` 的行号实体）：`TextFont` 指定 `UiFonts.mono` → 行号列与代码列同一字体（否则数字宽度不等 → 行号与代码列错位）。
- diff 行（`xgent_ui/src/editor/diff_view.rs`）：`ui_text` + MONO 字号 → 改用 `mono_text`。
- 工具卡参数摘要（`xgent_ui/src/tool_panel.rs`）：参数是路径/正则这类代码文本 → 新增 `fonts::ellipsis_mono_text`（带 `Ellipsis` 的等宽版本）。

## 中文排版约束

- 新增 `fonts::display_spacing(text) -> LetterSpacing`：文本含 CJK → `0`（不施加负字距），纯拉丁 → `−0.29`。welcome 标题（`welcome.rs:136`）改用它。
- 新增 `type_scale::line_height::for_text(text) -> f32`：含 CJK → `CJK`（1.7），否则 `BODY`（1.6）。对话消息正文（用户消息与助手正文）改用它。
- 检索链路的 CJK 分词（R3-8，`xgent_context/src/on_demand.rs` 的 bigram 切分）**已落地**——M4-T4 该项为**回归确认**，本轮补断言守住，不改实现。

# 非目标

- **不做 M4-T2 终端双宽单元格**（`MonoMetrics` / `display_width` / 按格定位）——独立 change；且其前置的硬门槛 A（macOS/Linux 启动日志确认）尚未闭环。
- **不做逐段字距**（拉丁段细分）：采用「含中文则整串不施加负字距」的保守做法。
- **不扩展密度系统去缩放字号**：`CODE` 档的 Compact −1 留待密度系统支持字号缩放后另议（M2b 纪律：密度只缩间距）。
- 不做亮色主题与其它视觉变更；不改 `ui_text` 签名（`ui` 槽位仍走默认字体资产）。

# 验收示例

- A1 编辑器代码正文使用 `mono` 槽位：`sync_editor_theme` 运行后 `EditorTheme.font` 等于 `UiFonts.mono`；虚拟渲染创建的代码文本 `TextFont.font` 等于该值。
- A2 编辑器行号列使用 `mono`：打开文件后行号实体的 `TextFont.font` 等于 `UiFonts.mono`（与代码列同源 → 数字等宽 → 行号与代码列对齐）。
- A3 diff 行与工具卡参数摘要使用 `mono`：两处文本的 `TextFont.font` 等于 `UiFonts.mono`。
- A4 令牌：`type_scale::SMALL == 12.0`、`type_scale::CODE == 12.0`、`type_scale::line_height::CJK == 1.7`。
- A5 展示级负字距不作用中文：`display_spacing` 对含 CJK 的文本返回 0、对纯拉丁返回 −0.29；welcome 标题接线到该函数。
- A6 消息正文行高按语言选档：`line_height::for_text` 对含 CJK 的文本返回 1.7、对拉丁返回 1.6。
- A7 检索链路 CJK 分词未被破坏：中文问句仍抽出二元组（`xgent_context` 既有单测通过）。
- A8 既有测试不回归：`xgent_ui` lib 76 项与既有集成测试全绿，`xui` 全绿；`cargo check --workspace` 与 clippy 通过、fmt 无 diff。
- A9 文档同步：`docs/plans/ui-v8-tasks.md` 的 M4-T3/M4-T4 勾选 + §10 勘误、`docs/dev-tutorial.md` 新增小节、`docs/plans/ui-v8-handoff.md` 交付表与下一步。

# 决策

- D-1 代码档用**改名**（`MONO` → `CODE`）而非新增：三个使用点全是代码场景，留两个同值常量只会让人随便挑一个用。台账写「新增 CODE 档」的意图（代码字号与界面字号解耦）由此达成。
- D-2 `EditorTheme` 的字体字段用 `FontSource`（不是 `Handle<Font>`）：M4-T1 之后槽位解析结果是 `FontSource`（可能是家族名，也可能是内嵌句柄）。
- D-3 中文判定用「文本含任一 CJK 码位」而非「CJK 占比」：负字距一旦作用在 CJK 上就会挤成一团，含即不施加是最保守正确的做法；占比阈值会漏掉「一句中文带一个英文词」的混排串。
- D-4 负字距只收窄「展示级」：welcome 标题是唯一的负字距使用点（`welcome.rs:136`），`+0.5` 的正字距（`welcome.rs:155`、`kit.rs:632`）对 CJK 无害，不动。
- D-5 消息正文的行高按**文本内容**判定（含 CJK → 1.7），不按 UI 语言设置判定：同一条会话里中英文消息混排，按语言设置选档会让一条会话里出现两种行高。
