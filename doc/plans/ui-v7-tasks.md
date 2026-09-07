# UI v7（Linear 基准）落地任务清单

> 基于 [方案 v1.5](ui-v7-migration.md)（五轮评审定稿）、[ADR-0014](../decisions/0014-ui-视觉基准采用-linear-设计系统-v7-原型.md)、原型 [ui-prototype-v7.1.html](../design/ui-prototype-v7.html) 拆解。
>
> 状态：**已完成**——M1~M7 全部落地（见 §0.2 进度快照，截至 2026-09-07）。任务编号 `M{期}-T{序}`，完成打勾。本文是唯一进度台账，方案文档随实施勘误（§15）。

---

## 0. 执行原则

1. **严格按期推进**：M1 → M2 → M3 →（M4 ∥ M5 可并行）→ M6 → M7；期内任务按编号顺序（有依赖标注的除外）。
2. **每任务有验收证据**：非「编译通过」，而是可观察行为或对照物（截图/测试/grep 计数）。
3. **提交粒度**：每任务一提交（消息中文、引用任务号如 `M1-T4`）；方案 §8.1 标注的三步提交任务除外。
4. **每期收尾**：`cargo fmt && cargo clippy --workspace && cargo test --workspace` 全绿后才进下一期。
5. **截图工具**：`ui-snapshot` feature（M2-T6 建成）下 F12 截主窗到 `target/snapshots/`，与原型浏览器截图人工对照。
6. **就地回写**：实施中发现与方案不符的事实，改代码的同时在方案 §15 追加勘误（文档跟随代码）。

## 0.1 依赖总览

```
M1（令牌/字体）─→ M2（骨架/拖拽）─→ M3（图标/顶轨）─┬→ M4（会话区）─┬→ M6（overlay）─→ M7（收尾）
                                                  └→ M5（上下文面板）┘
硬门槛：M1-T3（CJK spike）不通过 → 停止，升级处理，不得带病铺开。
风险标记任务：⚠ M3-T1（图标导出坑）、⚠ M5-T6（抽屉化，改动面最大，留独立提交便于回滚）。
```

---

## 0.2 进度快照（截至 2026-09-06，基线 `576246e` → `a771c38`，30 笔提交）

### 里程碑状态

| 里程碑 | 状态 | 提交 |
|:---|:---|:---|
| M1 令牌与字体 | ✅ 完成（T1-T9 + 偏差回写） | `8025ba3`~`d7ec085`（10 笔，含三步提交） |
| M2 骨架与拖拽 | ✅ 完成（T1-T7；**顺带修复状态栏 Startup 竞态 bug**） | `53198d2`/`9bfd4a8`/`84c468e`/`7396cd6` |
| M3 图标与顶轨 | ✅ 完成（T1-T6；24 枚图标/kit/顶栏九元素/agent pill/rail） | `ba111d8`~`7e711be` |
| M4 会话区 | ✅ 完成（T1-T8；消息/工具卡/操作栏/回底/welcome/context_scope/输入卡/qa chips） | `fcf3d6c`/`827b949`/`f3b9fc2` |
| M5 上下文面板 | ✅ 完成（T1-T7；三页签/diff 抽取/差异页/终端样式/抽屉化/四列终态） | `77b0c77`/`200f188`/`a771c38`/`73f5cbe`+bridge 修复 |
| M6 overlay 层 | ✅ 完成（T1-T6；toast/命令面板/确认弹窗/历史抽屉/设置对齐/文本构造器清点） | toast→构造器清点各一笔 |
| M7 收尾 | ✅ 完成（T1-T4；companion 激活环/i18n 收口/快捷键重映射/dev-tutorial 同步；T5 终验见 §M7-T5） | 动效→i18n→快捷键→文档各一笔 |

另：`576246e`（design-md skill + 文档基线）、`d9aa40e`（cargo fmt 历史统一）、`62fe0b5`（M1-T2 偏差回写）。

### 下一任务

无——全部任务已完成。集中手测清单（自动化无法覆盖项）见下节，待真机复核。

### 集中手测清单（自动化无法覆盖，累计于各任务）

1. **需已配置 provider 的对话联调**：消息流/工具卡 tint/agent pill 五态（含确认态、错误态）轮转。
2. **交互手感**：拖拽分隔条/双击复位、缩窗 <1100px 响应式折叠、回底浮钮显隐与 StickToBottom 耦合、qa chips 点击填入光标位置、复制钮落剪贴板、rail/顶栏 tooltip 悬停 500ms。
3. **Outline 圆角跟随**（输入卡 focus 环）——不随则回退外层节点方案。
4. **diff 页联调**：打开文件→修改→差异页实时反映；撤销后空态。

### 实施期发现索引（均已回写至对应任务块）

- bevy 0.19：`BorderRadius` 是 `Node` 字段非组件；`Query::iter()` 恒只读（可变走 `iter_mut()`）；`LetterSpacing`/`LineHeight` 是枚举；`Plugins` 元组上限 15；`EntityCommands` 无 `.spawn`（用 `commands.spawn().with_children()` + `add_child`）；`InputFocus::get()` 读焦点；`TextBackgroundColor` 支持 per-span 背景；`TextEdit::Insert(SmolStr)`。
- 工程：`spawn_status_bar` 曾缺 `.after(spawn_layout)`（Startup 竞态致状态栏空白，已修）；Inter 实际走直读模式（偏差记录在 M1-T2）；确认对话框 `line_diff` 无既有测试（已补 3 用例）。

---

## M1 令牌与字体

**目标**：全 UI 换 Linear 配色与 Inter 排版；结构字段就位；xui 语法色接通。此期不动任何布局结构。

### M1-T1 字体资产入库
**依赖**：无
- [x] 下载 Inter 可变字体（rsms/inter v4.1），放 `crates/xgent_app/assets/fonts/Inter-Variable.ttf`，连同 `OFL.txt` 许可入库。
- [x] 文件完整性：python 解析 fvar 表——`wght 100/400/900`（另含 opsz 14-32 光学尺寸轴）。

**验收**：✓ 资产/许可在库、axis 正确。

### M1-T2 fonts.rs 字体模块
**依赖**：M1-T1
- [x] **配置资产根目录（阻断级前提，先行）**：main.rs `AssetPlugin.file_path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets")` 已落地（M3 图标依赖此配置）。
- [x] 新建 `xgent_ui/src/fonts.rs`：`UiFonts { ui, mono }` Resource + `ui_text/mono_text` 构造器（含 cv01/ss03 FontFeatures）。Inter 实际走**直读模式**（见偏差记录），全局默认字体替换指向 Inter。
- [x] `mono` 句柄由 `xgent_app/startup.rs` Menlo 加载注入；`load_system_font` 更名 `load_fonts` 并统一处理两种字体。
- [x] 文本构造器：`ui_text(text, size, weight, color, line_height)` / `mono_text(fonts, text, size, color, line_height)`——签名与任务书的差异见偏差记录（+text 参数、暂无 LetterSpacing）。
- [x] `theme.rs` 增 `type_scale`（8 档）+ `type_scale::line_height`（5 档）+ `radius` 常量（2/4/6/8/12）。

**验收**：✓ `cargo check` 过；CJK spike（M1-T3）真机确认 Inter 生效。

> **实施偏差记录（2026-09-06）**：① Inter 实际采用**直读模式**（`CARGO_MANIFEST_DIR` 相对路径 + `Font::from_bytes` + 插默认 `AssetId`），未走 AssetServer——同步加载、天然支持默认句柄覆盖、对齐 startup 既有模式，避免「异步句柄 + 默认 id 二次插入」的双读复杂度；AssetServer 根配置保留（M3 图标必需）。② 构造器**暂无 LetterSpacing**（唯一消费者是 M4 的 DISPLAY 标题，届时在实际编码处按确认后的类型接入）。③ 字体加载位于 `xgent_app::startup`（非 xgent_ui::FontPlugin）——`CARGO_MANIFEST_DIR` 只能解析本 crate 路径。

### M1-T3 ⚠ CJK spike（硬门槛）
**依赖**：M1-T2
- [x] 临时把一个常显文本（如顶栏标题）换成 `ui_text(...)`，`cargo run -p xgent_app`。
- [x] 检查：中文回退渲染（macOS PingFang）正常、无豆腐块；中英混排基线与字号观感正常；换行正常（CJK 分词无日志刷屏）。
- [x] 结果记入方案 §15（通过/问题/结论）。**不通过 → 停止后续任务**，回退默认字体并升级讨论（备选：主字体换系统栈）。

> **结论（2026-09-06）：通过。** 全局默认切 Inter 后，中文经 PingFang 回退渲染正常（无豆腐），中英混排正常；「已加载 Inter Variable 为全局默认字体」日志确认；`XGENT_SHOT` 环境变量截图工具落地（app 自截渲染目标，无 TCC 依赖，M2-T6 雏形）。界面上仍为 v2 配色（M1-T4 换色）属预期。

**验收**：spike 结论已记录且为「通过」。

### M1-T4 theme.rs 新增 v3 字段（提交①，旧字段并存）
**依赖**：M1-T3 通过
- [ ] 按方案 §4.1 增加全部新字段并赋 v7.1 值：`code_bg/subtle/input_bg/hover/active/icon_bg/border_hover/text_faint/accent_interactive/accent_hover/accent_glow/accent_text/st_ok_bg/st_pending_bg/st_fail_bg/st_info/st_info_bg/warm/warm_bg/tooltip_bg/tooltip_text/code_text`；`punc` 保留（值 #8A8F98）。
- [ ] 旧字段同函数内**换值为 v3 语义**（bg/surface/elevated/line/border/text 三级/accent/st_*/overlay/kw…）：`bg=#08090A、surface=#0F1011、elevated=#191A1B、accent=#5E6AD2` 等，逐字段对照方案 §4.2 表。
- [ ] `size` 常量更新：`TOP_BAR_H=52、STATUS_BAR_H=32、ACTIVITY_BAR_W→RAIL_W=52、CONTEXT_W_DEFAULT=720、CONTEXT_W_MIN=380、CHAT_MIN=520、CONTEXT_TABS_H=38、DRAWER_W=320、RESIZER_W=6`；`CHAT_SIDEBAR_W/FILE_PANEL_W/VIEW_TABS_H` 暂留（M2/M5 删）。**改名与引用更新须同提交**（`ACTIVITY_BAR_W` 调用点 layout.rs:126），保证本任务落完可编译。
- [ ] `font_size 13.5→14.0`。

**验收**：`cargo check --workspace` 过（只加不删，旧引用照常编译）；启动全 UI 已是新配色。

### M1-T5 xui 语法色注入口
**依赖**：M1-T4
- [ ] `xui/src/text_editor/render.rs`：`EditorTheme` 增语法色字段组（`Option<Color>` 的 kw/fn_/str_/num/ty/com/punc/plain，None 回落现硬编码默认）。
- [ ] `xui/src/text_editor/highlight.rs:224-240`：`span_color_for` 优先读 EditorTheme。
- [ ] `Scrollbar` 默认色参数化（可注入，默认维持现值）。

**验收**：`cargo check -p xui` 过；`cargo tree -p xui` 仍无 xgent_* 依赖。

### M1-T6 宿主注入语法色
**依赖**：M1-T5
- [ ] `xgent_ui/src/editor/mod.rs:250-262` `sync_editor_theme`：从 `Theme` 注入语法色与 `code_bg`。
- [ ] editor/tabs.rs:261,276 两处 tailwind 硬编码（GRAY_400 行号、AMBER_400 光标）改读 Theme（行号=`text_faint`、光标=`accent_interactive`）。

**验收**：编辑器打开文件，行号/光标/高亮色随主题。

### M1-T7 全模块引用迁移（提交②）
**依赖**：M1-T4~T6
- [x] `bar` → confirm_dialog.rs:168,294（`elevated`）、terminal/mod.rs:286,383,425（`surface`）、editor/mod.rs:179（`surface`）、file_panel.rs:209,294（`surface`）、settings_panel.rs:283,343,393（`surface`）、416（`input_bg`）、455（`subtle`）。
- [x] `panel` → layout×4/session_history/command_palette/confirm_dialog/settings:218（浮层= `elevated`）、chat_panel:201（`input_bg`）、tool_panel:101,122（`subtle`）、editor/mod:450、editor/tabs:418、terminal/tabs:187（`elevated`/`surface`）、editor/conflict:133（`elevated`）。
- [x] `deep` 1 处（confirm_dialog.rs:244）→ `code_bg`；`hover_bg`→`hover`（file_panel.rs:1102）；`handle_active`→`icon_bg`（resize.rs:179；M2-T2 升级为 accent_glow 视觉）。
- [x] 本模块范围文本调用迁 `ui_text/mono_text`（M2 起随模块，本期只动触碰到的文件）。

**验收**：`cargo check --workspace` 过；`grep -rn "theme\.bar\|theme\.panel\|theme\.deep\|theme\.hover_bg\|theme\.handle_active" crates/xgent_ui/src` 为 0。✓ 实测 32 处迁移、零残留。

### M1-T8 删除旧字段（提交③）
**依赖**：M1-T7
- [x] 删 `Theme` 的 `bar/panel/deep/bubble_user/bubble_assistant/hover_bg/handle_active`；保留 `punc`。
- [x] `CHAT_SIDEBAR_W` 引用改指 `CONTEXT_W_DEFAULT`（本任务只改引用；**常量删除定死在 M2-T1**）。

**验收**：`cargo check --workspace && cargo test --workspace` 全绿；方案 §4.2「删除字段」清单全部消失。✓

### M1-T9 期验收
- [x] §12.1 令牌单测落地（`dark()` 关键值断言 + text/bg 对比度 luma 断言）。**38+4 测试全过**。
- [x] 截图对照：主界面验收图确认换色生效（近黑画布 #08090A + 靛紫品牌块 #5E6AD2 + Inter 排版）；命令面板/确认弹窗对照随 M6 期验收（其视觉重做在 M6）。
- [x] 三笔提交（①字段 808ca15 ②迁移 4d10e0f ③删除 5c92434）历史清晰；另附 style 提交（历史未格式化文件统一，d9aa40e）。

---

## M2 骨架与拖拽

**目标**：五列过渡布局 + 可拖拽面板 + 状态栏 32px。**面板默认展开 720px**。

### M2-T1 layout.rs 五列过渡重排
**依赖**：M1 期验收
- [x] MainArea 子节点序：`ActivityBar(52) → FilePanel(240) → **LeftHandle(6)** → Chat(flex) → RightHandle(6) → SideView(720)`；左手柄随文件面板暂留（M5-T6 一并移除）——见方案 §3 过渡形态，保持既有拖拽能力。
- [ ] 高度替换：顶栏 `TOP_BAR_H(52)`、状态栏 `STATUS_BAR_H(32)`、活动栏 `RAIL_W(52)`；边框换 `line`（0.05）。
- [ ] 背景层级：根/会话区 `theme.bg`，顶栏/活动栏/面板 `theme.surface`。
- [ ] **`SideViewCollapsed` 默认 `false`**（layout.rs:52-53）——面板默认展开；`SideViewMarker` 节点初始 `Display::Flex`（layout.rs:192）与 handle_bundle 右手柄初始 `Display::Flex`（resize.rs:98-101）同步改。
- [ ] `PanelWidths` 默认 `side_view=720`（M1 已备常量）；`CHAT_SIDEBAR_W` 常量在本任务删除（M1-T8 仅改引用，定死分工）；`FILE_PANEL_W` 仍被文件面板使用，归 M5-T6 删除。

**验收**：启动即五列、右侧面板默认可见 720px；`toggle_panel_visibility` 折叠/展开不回归。

### M2-T2 resize.rs 钳制/双击复位/视觉
**依赖**：M2-T1
- [ ] 常量：`SIDE_VIEW_MIN 200→380`、`CHAT_MIN 240→520`。
- [ ] 启动与窗口 resize 统一钳制 `PanelWidths`（新系统：`innerWidth − RAIL_W − [file_panel] − CHAT_MIN` 为上限；五列过渡期含 file_panel 240，M5 后该项为 0——公式读实际子面板显隐，不硬编码）。
- [ ] 双击复位：手柄 `Interaction` 300ms 内两次 Pressed → `side_view=CONTEXT_W_DEFAULT`。
- [ ] 手柄视觉：hover/拖拽 → 底 `accent_glow` + 居中 2px `accent_interactive` 竖线（替换现 `handle_active` 纯色）；宽度 6px 不变。
- [ ] 钳制逻辑纯函数化（如 `clamp_side_view(w, available)`），配边界单测：380 下限、上限=可用空间、过渡期含文件面板/四列后不含两套场景（方案 §12.2）。

**验收**：拖拽平滑、双向钳制（380 / 余空间）、双击回 720；1440px 窗口启动无溢出（面板被钳到 ~622px）；`cargo test -p xgent_ui` 钳制用例过。

### M2-T3 响应式折叠
**依赖**：M2-T2
- [x] `responsive_collapse` 系统：窗口宽 <1100px → `SideViewCollapsed(true)`（只收不展）；折叠显示走既有 `toggle_panel_visibility`。
- [ ] 手测项：缩窗 <1100px 面板自动收起（自动化无窗口缩放能力，留用户手测）。

### M2-T4 最小窗口尺寸
**依赖**：无（可提前）
- [x] `WindowPlugin` 增 `ResizeConstraints` min 1024×640。

**验收**：✓ 窗口拖不过最小尺寸。

### M2-T5 status_bar 32px 重排
**依赖**：M2-T1
- [ ] 高度 32；底 `surface`、顶边 `line`；pill 分段（右边框 `line` 分隔）。
- [ ] 内容处置（方案 §8.9）：daemon 状态点 + provider·tokens + 成本占位 + spacer + companion 开关 + 会话 id（`#id · N 轮`，轮数自 `Conversation` 统计）；**删会话状态文本**（status_bar.rs:171 段）；**保留编码段**（status-encoding）。
- [ ] 可点击仅 companion（切 `warm` 点亮态，M3 前暂用文字/星字符，M3 换图标）。
- [ ] 本模块文本迁 `ui_text/mono_text`。

**验收**：TokenUsage/daemon 状态联动不回归；截图对照原型状态栏。

### M2-T6 ui-snapshot 截图工具
**依赖**：M2-T1
- [ ] `xgent_app` 增 feature `ui-snapshot`：F12 → `bevy::render::view::window::Screenshot`（主窗）存 `target/snapshots/{timestamp}.png`。
- [ ] README 或 dev-tutorial 一行用法说明（正式 dev-tutorial 同步放 M7）。

**验收**：F12 出图。

### M2-T7 期验收
- [x] 编译/测试全绿（41+4）；2560px 真机截图：五列布局 + 面板默认展开 720 + 状态栏分段渲染正确；上下文面板内容页签为空属预期（M5-T1）。
- [x] 钳制/复位值单测覆盖（窗口/拖拽交互手感留用户手测——自动化无鼠标注入能力）。
- [x] 提交：53198d2（T1/T2/T3）、9bfd4a8（T4/T6）、84c468e（T5）+ 本台账。

---

## M3 图标与顶轨

**目标**：矢量图标体系 + kit 基础件 + 顶栏/图标轨换新。

### M3-T1 ⚠ 图标导出管线
**依赖**：M2 期验收
- [ ] 图标清单定稿（实盘 `doc/design/icons/` 现成 30 枚，需求 22 枚）：**现成 19 枚直接用**（chat/folder/clock(=history)/terminal/star/plus/x(=close)/check/copy/retry/refresh/send/command/panel-right/chevron-down/info/file/diff/dollar）；**补画 3 枚**：`gear`（设置）、`chevron-right`（展开指示——ImageNode 不能旋转，不能复用 chevron-down）、`alert-triangle`（或以 shield-alert 替代）；脚本顺带产出命名对照表。
- [ ] 新建 `doc/design/icons/export_png.py`（cairosvg）：**渲染时 `stroke="currentColor"` 替换为 `#FFFFFF`**（乘法染色前提，方案 §6.1）→ `crates/xgent_app/assets/icons/{name}@2x.png`（24 viewBox、2x=48px）。
- [ ] 抽查导出 PNG：描边白、底透明。

**验收**：`assets/icons/` ≥20 枚白描边 PNG；脚本可重复执行。

### M3-T2 kit.rs 基础件
**依赖**：M3-T1
- [ ] `IconAssets` Resource（启动经 AssetServer 加载 icons 目录——**资产根已由 M1-T2 配置**）+ `icon(name, px, color)` 构造（ImageNode + color 乘色）。
- [ ] `HoverTint` 组件 + **单一全局系统**（方案 §7.1：Changed<Interaction> 三态换 BackgroundColor/BorderColor）。
- [ ] `ghost_button / primary_button / pill / kbd_badge / section_label / icon_button / tooltip`（hover 500ms 延迟浮现——**M3-T5 rail 同期消费，必须本期交付**）构造器（规格对照方案 §9 表）。
- [ ] 方案 §9 表中 `badge` 本期无用例、延后；`companion_button` 内联在 M3-T5 实现（单一使用点，不进 kit）。
- [ ] 注册进 `XgentUiPlugin`（kit 子插件）。

**验收**：demo 调用各件一次目检；`cargo check` 过。

### M3-T3 top_bar 重排（52px）
**依赖**：M3-T2
- [ ] 元素序（方案 §8.2）：品牌块（28×28 `accent` 底 "X"，圆角 8）+ "XGent"（H3/590）｜项目名（`text_dim` 静态）｜新建会话 ghost 钮｜spacer｜agent pill｜provider/model pill｜设置钮（**保留现状行为**）｜命令面板钮。**移除 🕐 历史钮**（迁 rail）。
- [ ] emoji（🕐🔍⚙）全换 icon；按钮接 `HoverTint`。
- [ ] 高度 52、底 `surface`、底边 `line`；本模块文本迁构造器。
- [ ] 按钮交互：**四类不动**（新建/provider/命令面板/设置，top_bar.rs:266-306）；**历史分支删除**——`HistoryButtonMarker` 查询与 `SessionHistoryState` 联动迁 rail（M3-T5），handler 同步清理。

**验收**：截图对照原型顶栏；新建/设置/面板/模型交互全通；`grep -n "🕐\|🔍\|⚙" top_bar.rs` 为 0。

### M3-T4 agent pill
**依赖**：M3-T3
- [x] pill 五态机在 top_bar 内实现（Idle=ok/subtle、Thinking/Streaming=accent 脉冲、ToolRunning/Confirming/Aborting=warning、Error=fail；文本复用 status-* i18n 键）。
- [x] 脉冲：1.2s 正弦 alpha 仅 Thinking/Streaming 驱动。
- [ ] 手测项：确认态/错误态需真实对话触发（留用户手测）。

### M3-T5 rail 改造（activity_bar → rail）
**依赖**：M3-T2
- [ ] 宽 52（`RAIL_W`）；按钮 40×40 圆角 6、`HoverTint`。
- [ ] 按钮集：对话 / 文件（drawer 开关，现 `Files` 行为） / 历史（开 session_history，见 M6-T4）/ 分隔线 / 终端（现 Terminal 行为）/ spacer / **展开面板钮**（`SideViewCollapsed` 为 true 或响应式收起时显示）/ companion（40 圆形 `warm` 底深色 star，激活环 M7）。**不放设置/搜索/Git/插件**。
- [ ] `ActivityKind` 枚举调整（现四态 `{Files(默认), Editor, Terminal, Settings}`，activity_bar.rs:13-19，**无 Chat**）：**新增 `Chat`（`#[default]`，行为=关抽屉/清其他 active）与 `History`（开历史抽屉）**；移除 `Editor`（预览归上下文面板页签，M5）；Terminal 保留；Settings 去留随 M6-T5 定。
- [ ] active 态：`accent_bg` 底 + `accent_interactive` 图标 + 左 3px 圆角竖条（现 2px border 模拟改独立节点）。
- [ ] tooltip：hover 500ms 延迟，`tooltip_bg` 底 + 名称 + kbd。
- [ ] emoji（📁📝🖥⚙）全换 icon；交互逻辑（activity_bar.rs:155-207）随按钮集适配。

**验收**：截图对照原型 rail；文件/终端/历史切换全通；无 emoji。

### M3-T6 期验收
- [ ] `grep -rn "📁\|📝\|🖥\|⚙\|🕐\|🔍\|🔧" crates/xgent_ui/src` 为 0。
- [ ] 顶栏 + rail 两张 snapshot 对照原型。

---

## M4 会话区（可与 M5 并行）

**目标**：对话主区完整换新。

### M4-T1 消息视觉（部分完成：行样式/头像/正文/光标 ✓；inline code 与代码块标记为 P1 联调项）
**依赖**：M3 期验收
- [x] 消息组限宽 820 居中（`max_width` + `align_self: Center`，用户行/助手行/当前流式节点三处）。
- [ ] 消息头：28×28 圆角 6 头像（user=`icon_bg`/"你"、agent=`accent`/"X"）+ 角色（SMALL/510）+ 时间（MICRO `text_faint`）。
- [ ] 正文 BODY/400 `text_dim` 行高 1.6；inline code（反引号简单分段）：`icon_bg` 底 + `accent_interactive` + mono CAPTION；代码块：`code_bg` + `border` + 圆角 6 + mono。
- [ ] 流式光标：`▍` + `accent_interactive`，`t%0.8<0.4`（替换现 `▋` 逻辑，chat_panel.rs:709-732）。

**验收**：构造含中英/代码的消息对照原型消息区截图。

### M4-T2 消息操作栏
**状态：未开始（M4 续期任务）**
**依赖**：M4-T1
- [ ] agent 消息容器挂 `Button`+`Interaction` 做 hover 检测；hover 时右上显 26px 钮组（`elevated`+`border`）。
- [ ] 复制：`Clipboard::set_text`（Resource 已由 DefaultPlugins 提供）取消息正文；重试：现有 `RetryMessage` 路径。

**验收**：hover 显隐正确；复制到系统剪贴板可粘贴；重试触发既有链路。

### M4-T3 回到底部浮钮
**状态：未开始（M4 续期任务）**
**依赖**：M4-T1
- [ ] 读消息列表 `ScrollPosition`：偏离底 >200px 显浮钮（`elevated`+`border`，输入区上方居中）；点击回底。
- [ ] 与 `StickToBottom` 共存验证（贴底时浮钮不闪现）。

**验收**：向上翻页出钮、点击回底、贴底无钮。

### M4-T4 工具卡视觉
**依赖**：M4-T1
- [ ] tool_panel 卡片：`subtle` 底 + `border` + 圆角 8；head hover→`hover`（HoverTint）；展开/折叠逻辑不动。
- [ ] head：24px 圆角 4 图标块四色 tint（read=`st_info`/write=`st_pending`/search=`accent_interactive`/exec=`st_fail`，底用对应 `_bg`）+ 工具名（SMALL/510）+ 参数（mono CAPTION `text_muted`）+ 状态色 + 展开 `▸/▾`。
- [ ] 结果条 `st_ok_bg/st_ok`（fail 同理）。
- [ ] 🔧 替换。

**验收**：真实工具调用（读文件/搜索/写文件确认）三卡样式对照原型；调用链路不回归。

### M4-T5 welcome 空态
**状态：未开始（M4 续期任务）**
**依赖**：M4-T1
- [ ] 新建 `welcome.rs`：会话空态显示——64px `accent` 圆角 12 品牌块 + DISPLAY 标题 + 副标题 + 3 快捷卡（`subtle`+`border`，图标 tint `st_info_bg/accent_bg/st_ok_bg`）+ 最近会话 3 条。
- [ ] 进入空态发 `ListSessionsMessage`；`SessionListMessage` 回填渲染；点击 `RestoreSessionMessage`。
- [ ] 首 条 `UserInputMessage` 发出即隐藏（复用现 hidden 切换模式）。

**验收**：新会话空态→欢迎页；点最近会话恢复；发消息切消息流。

### M4-T6 context_scope 上下文条
**状态：未开始（M4 续期任务）**
**依赖**：M4-T1
- [ ] 新建 `context_scope.rs`：会话区顶部 38px 行（`surface` 底 + 底边 `line`）——「上下文」section_label + chips（胶囊 9999、透明底+`border`：文件图标 `st_pending`/目录 `accent_interactive`、CAPTION）+「+ 添加」虚线胶囊。
- [ ] **只读展示**：聚合编辑器 tabs 已打开文件；「+ 添加」→ 打开文件抽屉（M5-T6 前暂开现 FilePanel 折叠切换）。无删除钮（方案 §8.10）。
- [ ] 随条删除：`VIEW_TABS_H` 常量、`ConversationInfoMarker` 及其更新系统（chat_panel.rs:739-758）、失效 i18n 键 `chat-tab-label`、`conversation-tokens`。

**验收**：开/关文件 chips 增减；原视图标签条（含 ConversationInfoMarker）已删除且信息按 §8.4/§8.9 处置。

### M4-T7 输入卡
**状态：未开始（M4 续期任务）**
**依赖**：M4-T1
- [ ] `input_bg` 底 + `border` + 圆角 12；focus：`Outline` 3px `accent_glow` + 边框 `accent_interactive`（**实测 Outline 随圆角**；不随则回退外层节点，结论记 §15）。
- [ ] 工具行：模式钮（`subtle` 6px，现 cycleMode 保留）+ 安全提示（`st_pending` icon+文案）+ spacer + Shift+Enter kbd 徽章 + token 计数（MICRO `text_faint`）+ 发送钮（`accent`/流式中 `st_fail` 停止态——现 abort 逻辑保留）。
- [ ] 顺手改：空输入红边闪烁段（chat_panel.rs:674-707，本次改动面内）边框色换 `st_fail`——**本任务定为其归属**，M7-T1 不再重复。

**验收**：输入/focus 环/发送/停止/空输入闪烁（`st_fail`）全链路。

### M4-T8 qa chips 点击填入
**状态：未开始（M4 续期任务）**
**依赖**：M4-T7
- [ ] 快捷提示工具栏（chat_panel.rs:220-249）→ 胶囊 chips（HoverTint）；点击经 `ChatInputMarker` 定位、`EditableText.editor`（PlainEditor）赋值预设文案并聚焦（注意 generation 刷新）。

**验收**：五枚 chips 点击后输入框出现对应前缀文案。

### M4-T9 期验收（T1-T8 全部落地）
- [x] 编译/测试全绿（41+4）；空态截图：welcome 空态完整渲染、视图标签条已移除、无布局回归。
- [ ] 流式中/工具调用两场景 snapshot：需已配置 provider 的环境手测。
- [ ] 手工回归清单：`DeltaMessage/DoneMessage/Error/Retry/SessionCleared` 五链路 + qa chips 点击填入 + 复制钮 + 回底钮 + Focus 环 + 上下文条随开文件增减。

---

## M5 上下文面板（可与 M4 并行）

**目标**：SideView 变三页签上下文面板；文件面板抽屉化；布局收四列。

### M5-T1 页签条与状态机
**依赖**：M3 期验收（需 icon/kit）
- [x] SideView 顶部 38px 页签条：预览 / 差异 / 终端（SMALL/510，active=`accent_interactive`+底部 2px 线）+ 右侧收起钮（EditorBackButtonMarker 复用）。
- [x] `SideViewContent` 增 `Diff` 变体；`handle_page_tab_click`/`update_page_tab_indicators`（active=accent 底线）；Editor/Preview 归一预览页。
- [x] rail 终端按钮行为保留（content=Terminal 走既有显隐链路）。
- [x] 真机截图：页签条「预览 差异 终端 ×」渲染正确（None 态无 active 指示属预期）。

**验收**：三页签互切、折叠展开正常。

### M5-T2 预览页归一
**依赖**：M5-T1
- [x] Editor/Preview 两态归一为预览页（编辑器主体保留；页签指示归一映射）。**归一影响面**（file_panel.rs 写入点 :739 的入口改道随 M5-T6）：`Preview` 写入点 file_panel.rs:739（非代码文件打开流）、比较点 :832、消费点 editor/mod.rs:329——与 M5-T6 的 `OpenFileRequest` 改道是同一流程的两半，**先本任务归一变体、M5-T6 改道入口，顺序不可倒**。
- [x] 编辑器外框底 `code_bg`、顶部栏高度 32→28（`EDITOR_TABS_H` 更新）；emoji 检查归 M5-T6 文件面板一并处理。
- [ ] `editor.view` 热键映射（M7-T3 收口）。

**验收**：文件打开/编辑/多 tab/外部修改冲突链路不回归；Cmd+E 落预览页。

### M5-T3 共享 line_diff 抽取
**依赖**：无（可提前）
- [x] confirm_dialog.rs `line_diff` 抽到 `xgent_ui/src/diff.rs`（pub DiffKind/DiffLine/line_diff；补 3 用例：纯增/纯删/混合）；confirm_dialog 改调用共享版。

**验收**：`cargo test -p xgent_ui` 过（新 diff 用例）；confirm 行为不变。

### M5-T4 差异页
**依赖**：M5-T1、M5-T3
- [x] 新建 `editor/diff_view.rs`：buffer 侧读 `TextEditor.rope`、磁盘侧用 `EditorBuffer.disk_content` 快照（**优于任务书**——免异步通道）；调共享 line_diff。
- [x] 渲染：`code_bg` 底 + mono、add=`str_`/`st_ok_bg` 底、del=`st_fail`/`st_fail_bg` 底、context=`text_faint`；空态「无未保存更改/无打开文件」（i18n `diff-empty-*`）。

**验收**：改 buffer 后差异页实时反映；撤销后空态。

### M5-T5 终端页样式 ✓（随 M1/M5-T1 大部分已生效）
**依赖**：M5-T1
- [x] 页签切换经 content=Terminal（M5-T1）；`term-view` 底 `code_bg`、prompt `st_ok` 等已随 M1 令牌生效；ANSI 调色板保留；终端自身多 tab 条保留页内（最小改动）。
- [ ] 终端页真机联调（开终端 tab 目检）。（多 tab 保留在页内或下拉——按现状 TerminalTabs 结构最小改动）；输出区 `code_bg`、正文 mono CAPTION `text_dim`、prompt `st_ok`、命令 `text`；ANSI 调色板不动。

**验收**：PTY 全链路（spawn/输出/输入/resize/多 tab）不回归；截图对照原型终端页。

### M5-T6 ⚠ file_panel 抽屉化（独立提交，留回滚点）
**依赖**：M5-T1
- [x] 新 Resource `FileDrawerOpen(bool)`；文件面板渲染改左侧 overlay drawer（宽 `DRAWER_W=320`、`surface` 底 + 右边框 + `overlay` 遮罩，点击遮罩关）。
- [x] rail 文件按钮 / `filepanel.toggle` 热键 → 切 `FileDrawerOpen`；`FilePanelCollapsed` 及 `toggle_panel_visibility` 文件分支删除。
- [x] 树条目视觉：圆角 4、hover=`hover`（迁 HoverTint）、选中=`accent_bg`+`accent_interactive`、图标矢量化；M/A/U 徽章不做。
- [x] 点文件行为变更：发 `OpenFileRequest` → 上下文面板预览页加载（原内嵌预览区取消）。
- [x] **布局收四列**：移除 FilePanel 列与左手柄（layout.rs:141-160），`apply_panel_widths` 文件分支删除；面板钳制公式去 file_panel 项。

**验收**：✓ 抽屉开→浏览→点文件→预览页加载全链路（file_preview.rs 5 用例 + 真机截图）；四列布局无残留列（2560px 截图像素采样验证）；提交 `73f5cbe`（附 bridge 修复一笔——改道 main.rs 误删 insert_resource(bridge)，真机 panic 暴露即修）。

### M5-T7 期验收
- [x] 三页签 + diff 两态 + 抽屉链路 + 编辑/终端全回归；1440px 窗口四列下面板钳制复查（`clamp_side_view` 单测 3 用例过：下限 380/上限 available−520/复位 720）。

---

## M6 overlay 层

**目标**：四类浮层 + toast 视觉对齐。

### M6-T1 kit::toast
**依赖**：M3-T2
- [x] 底部居中浮层：`tooltip_bg` 底 + `border` + 圆角 8 + BODY_SM 文案，2.2s TTL 自动消失（消息驱动 + 计时系统）；`ToastMessage` 写入即出浮层。

**验收**：✓ toast 生命周期无头测试（toast.rs：spawn/在场/超时消隐，ManualDuration 快进时间）。

### M6-T2 命令面板
**依赖**：M6-T1
- [x] 底 `elevated` + 圆角 12（radius::PANEL）+ 遮罩 `overlay`；**选中态改中性 `hover`**（rebuild_list 视觉分支）；条目图标块 `icon_bg`；输入区底线 `border`。
- [x] 键盘导航（↑↓/Enter/Esc）不回归（逻辑未动）。

**验收**：✓ 编译+测试全绿；kbd 徽章条目留真机手测。

### M6-T3 确认弹窗
**依赖**：M5-T3
- [x] 圆角 12、`overlay` 遮罩、icon 块 `st_pending_bg/st_pending`；diff 区 `code_bg` + `border` + 行底 tint（add=`st_ok`/`st_ok_bg`、del=`st_fail`/`st_fail_bg`）；按钮：拒绝=ghost、确认=`accent` 底白字；`line_diff` 已切共享模块（M5-T3）。
- [x] Esc 拒绝 / Enter 确认不回归（键盘链路未动）。

**验收**：✓ 编译+测试全绿；真实写文件确认流截图留真机手测。

### M6-T4 会话历史抽屉化
**依赖**：M6-T1
- [x] 居中弹窗改左抽屉视觉（复用 M5-T6 drawer 结构：320px/surface/右边框/DRAWER_Z）；遮罩点击关闭（overlay 根挂 Button）；rail 历史/`session.history` 命令入口指向抽屉。

**验收**：✓ 编译+测试全绿；三入口开抽屉留真机手测。

### M6-T5 设置面板对齐
**依赖**：M6-T1
- [x] `text_input_node`：`input_bg` 底 + `border` 圆角 6；保存钮 accent 底+accent_text（BODY_SM/510）、关闭钮 ghost；kind 按钮组选中高亮保留。`ActivityKind::Settings` 已于 M3-T5 移除（确认无残留引用）。

**验收**：✓ 设置读写/语言切换/模型拉取链路不回归（编译+测试全绿）。

### M6-T6 文本构造器清点
**依赖**：M6-T5
- [x] `grep -rn "TextFont {" crates/xgent_ui/src` 清零（构造器内部除外）；字号全部经 `type_scale`。**实盘 95 处替换**，2 处例外有注释：kit::section_label（LetterSpacing 正字距构造器无参）、editor/tabs.rs 行号列（editor_theme 动态字号）；另有 settings_panel `text_input_node` 用 `TextFont::from_font_size`（EditableText 输入框不能插空 Text 节点，非字面量）。

**验收**：✓ grep 字面量计数 0；`cargo fmt && clippy && test` 全绿。

---

## M7 动效与收尾

### M7-T1 剩余动效
- [x] companion 激活环（外圈嵌套 2px 描边节点，scale 1.0→1.3 + alpha 0.4→0，2s 循环；`companion_ring_system` 相位驱动 + `companion_ring_visibility` 显隐）。
- [x] 空输入红边闪烁已随 M4-T7 换色（本任务核验完成）。
- [x] 动效清单（方案 §7.2）逐项打勾（pill 脉冲 M3-T4、光标 M4-T1、tooltip 延迟 M3-T2 均在期）。

**验收**：✓ 编译+测试全绿；动效目检留真机手测。

### M7-T2 i18n 收口
- [x] 新 key 全量核对（zh-CN/en-US 成对）；失效键删除（`chat-tab-label` 随 M4-T6、`preview-*` 随 M5-T6、`app-title`/`welcome`/`chat-empty` 等 v2 遗留共 24 键）；补 M5-T4 漏加的 `diff-empty-*`；`tr` 调用无裸中文。localizer 单测改断言在用键。

**验收**：✓ 两 locale 键集一致（脚本审计）；切语言全 UI 留真机手测。

### M7-T3 快捷键收口
- [x] `editor.view`→预览页签（切 `SideViewContent::Editor`+展开）、`chat.view`→聚焦会话区（收起面板）、`filepanel.toggle`→抽屉 重映射完成；`HotkeyRegistry` 注册时冲突检测沿用 xui（12 热键无冲突，xui hotkeys 3 测试过）。

**验收**：✓ 逐个实测表留真机手测。

### M7-T4 文档同步
- [ ] `doc/dev-tutorial.md`：kit/fonts/diff_view/context_scope 模块、Theme v3 字段、ADR-0014 链接、ui-snapshot 用法。
- [ ] 方案 §14 DoD 逐项核对；本文档状态改「已完成」。

### M7-T5 终验
- [ ] `cargo fmt && cargo clippy --workspace && cargo test --workspace` 全绿。
- [ ] `ui-snapshot` 全区截图 vs 原型逐区走查（顶栏/rail/消息/工具卡/输入/面板三页/抽屉/四类 overlay/状态栏）。
- [ ] `grep` 终检：无 emoji 图标、无游离硬编码色（terminal ANSI 除外）、无裸 TextFont、xui 无 xgent_* 依赖。

**验收**：方案 §14 DoD 全勾，M7 完成即项目 DoD。

---

## 附：风险任务速查

| 任务 | 风险 | 缓解 |
|---|---|---|
| M1-T3 CJK spike | 回退链路不达标 | 硬门槛，不过不铺开；备选系统字体栈 |
| M3-T1 图标导出 | stroke 染色坑 | 脚本强制 `#FFFFFF` + 抽查验收 |
| M4-T7 Outline | 不随圆角 | 预案：回退外层节点，结论记 §15 |
| M5-T6 抽屉化 | 改动面最大 | 独立提交留回滚点；链路验收清单化 |
| M2-T2 窗口钳制 | 过渡期/终态公式不同 | 公式读实际显隐，不硬编码面板集 |

---

## 修订记录

**v1.1（自查修订）**：
1. M2-T1 序列补 `LeftHandle`（原文文字说暂留、序列却漏列，自相矛盾）。
2. M2-T1 常量收口拆分：`FILE_PANEL_W` 归 M5-T6 删除（文件面板 M2–M5 期间仍在用）。
3. M2-T2 补钳制逻辑纯函数化 + 边界单测（方案 §12.2 此前未落成任务）。
4. M3-T2 补 `tooltip` 组件（M3-T5 rail 同期消费）；注明 `badge` 延后、`companion_button` 内联于 M3-T5。
5. M3-T3 「五类按钮不动」改为「四类不动 + 历史分支迁 rail」（top_bar.rs:266-270 实为 5 个 marker）。
6. M3-T1 图标清单实盘定稿：现成 19 枚、补画 3 枚（gear/chevron-right/alert-triangle）、替代关系 close→x、history→clock。
7. M3-T4 验收补确认态（写文件确认流）与错误态（无效 key）触发方式。
8. M4-T6 认领 `VIEW_TABS_H` 常量、`ConversationInfoMarker` 系统、失效 i18n 键的删除。
9. M5-T3 「现有测试迁移」改为「补基础用例 ≥3 例」——confirm_dialog 实无 line_diff 测试（grep 证实仅有定义与调用）。

**v1.2（第二轮自查修订）**——切面：编译连续性逐任务模拟、枚举/常量涟漪清单、归属消歧。
1. M3-T5 枚举调整补全：`ActivityKind` 现无 Chat 变体（activity_bar.rs:13-19 实读）——**新增 Chat(默认)/History**、移除 Editor、Settings 随 M6-T5。
2. M5-T2 列明 Preview 归一影响面（file_panel.rs:739/:832、editor/mod.rs:329），并与 M5-T6 定死先后（先归一变体、后改道入口）。
3. M1-T4 改名与引用更新同提交（`ACTIVITY_BAR_W` 调用点 layout.rs:126），保「任务落完可编译」。
4. `CHAT_SIDEBAR_W` 删除归属定死：M1-T8 只改引用、M2-T1 删常量。
5. 空输入红边闪烁归属定死：M4-T7 顺手换色、M7-T1 仅核验。

**v1.3（第三轮自查修订）**——切面：资产加载的物理前提。
1. **M1-T2 新增阻断级子任务**：工程未配置 AssetPlugin 资产根（默认 CWD 相对 `assets/`，workspace 根运行不存在），且现有代码全部绕开 AssetServer——不配置则 M1 字体加载、M3 图标加载静默失败。修复：`AssetPlugin.file_path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets")`，Inter 改走 AssetServer，Menlo 保持直读。
2. M3-T2 IconAssets 标注依赖 M1-T2 资产根配置。

**v1.4（第四轮自查修订）**：
1. M4-T6 失效键清单补 `conversation-tokens`（ConversationInfo 系统删除后失效；方案 v1.7 同步）。
