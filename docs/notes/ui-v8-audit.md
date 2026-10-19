# UI v8 审计：as-built 现状与缺陷台账

> 目的：为 v8「交互架构层重建」提供可核对的证据基线。
> 方法：逐文件读实现，关键结论在本会话内 grep 复核（复核通过项标 ✅）。
> 时间：2026-10-16。基线 commit `d61e54b`。
> 范围：`crates/xui`、`crates/xgent_ui`、`crates/xgent_app` 的 UI 侧。

---

## 一、结论摘要

1. **v7 视觉基线成立，不需要推翻。** Linear 基准的明度阶梯 + 单一靛紫 + 零投影层级哲学，是 bevy_ui 无阴影/无渐变约束下的正确选择；ADR-0014 的五条有意裁剪已在 `theme.rs` 落地。v8 不动色板。
2. **真正的不合理集中在三个层面**，且都能追溯到「缺少原语」而非「写错了值」：
   - **层级与模态**：无统一分层策略，`GlobalZIndex` 仅 8 处各写各的，三类浮层根本没有 z 值 → 抽屉会盖住设置面板并吞点击。
   - **滚动与溢出**：全仓 9 处把 `Overflow::clip_y()` 当滚动容器用，而 clip 轴不响应滚轮 → 这些列表实际滚不动；引擎层**没有省略号能力**（bevy 0.20 无 `TextTruncate`）→ 长文本无截断。
   - **区域模型**：`SideViewContent` 是 closed 5 变体枚举，五个内容互斥；可见性由 5 个独立系统各自写 `Node.display` → Git/MCP/插件面板无处安放。
3. **一个平台级 P0**：等宽字体只在 macOS 加载 Menlo，Windows/Linux 落回 Inter（比例字体），而终端按 `font × 0.6` 排格、编辑器行号用 `LineHeight::Px` 对齐 → **这两个平台上编辑器/终端/diff 的等宽对齐整体失效**。三平台一阶前提下这是阻断项。
4. **BSN 重构（声明式 UI + resvg 图标）已设计完但被放弃**，其评审 r3/r4 的 10 条修复只改进了 prose、从未落到代码。v8 **不复用 BSN**，但 r4 提出的 A-1/A-3/A-8（层级、按钮态、state→render 管线）是真实架构缺口，v8 正面回答——它们在 v7 路线里至今无人认领。
5. **测试只断言存在性与状态机，全仓没有任何布局几何断言。** 裁剪、重叠、滚不动这类缺陷在现有测试体系里不可能被发现。

---

## 二、现状基线

### 2.1 设计世代与权威源

| 世代 | 文档 | 贡献 | 状态 |
|:---|:---|:---|:---|
| G1 | `docs/design/ui-design.md`（草案 v1） | MVP 基线：顶栏 40 / 文件面板 240 / 对话 / 状态栏 24，暗色 only | 已落地后被重塑 |
| G2 | `docs/design/ui-gap-plan.md` | 原型对齐阶段 A–H | 已落地，**仅 D（Markdown）未做** |
| G3 | `docs/design/ui-v3-requirements-brief.md` + `ui-v3-design-tokens.md` | 首套令牌系统；品牌冷电蓝 `#2D9DFF`；导航 6→≤3 层 | 令牌被 v7 取代；陪伴暖色思路存续 |
| G4 | `docs/design/ui-bsn-refactor-plan.md` + 4 份评审 | BSN = Bevy Scene Notation：声明式 `bsn!{}` + resvg 图标 + 新 `XuiTheme`，对标原型 v6 | **设计完 + 评审完，零代码落地，已放弃** |
| G5 | `docs/plans/ui-v7-migration.md` + `ui-v7-tasks.md` + ADR-0014 | Linear 视觉基准、四列布局、`kit.rs`/`fonts.rs`/`type_scale` | **已落地（M1–M7）＝当前权威** |

世代关系：**v7 是在 G1/G2 上演进（累积）**；**BSN 与 v7 竞争且被放弃**——`ui-v7-migration.md:25` 明确「xui 纯 bevy + xui_i18n 不动，不引 BSN」，`docs/notes/bevy-0.20-migration.md:29` 记「xgent 未使用 BSN」，仅把 BSN 留作未来「新界面试点」（`:116`）。

⚠ 文档陈旧处：`docs/plans/README.md:85` 仍把 v7 标为「待实施」，但代码与 git 历史显示已完成。v8 落地时一并勘误。

### 2.2 当前布局树（as-built）

```
Window  min 1024×640，未设默认尺寸                     xgent_app/src/main.rs:229-238
UiRoot  100%×100% Column  bg=theme.bg                  layout.rs:80-90
├─ TopBar        h=52 固定  Row  padX16                 layout.rs:92-108
├─ MainArea      flex_grow1 Row  Overflow::clip()       layout.rs:111-121
│  ├─ ActivityBar    w=52 固定 Column                    layout.rs:124-139
│  ├─ ChatPanel      flex_grow1 Column  clip             layout.rs:142-154
│  │  ├─ ContextScope   h=38 固定                        context_scope.rs:66-122
│  │  ├─ MessageList    ScrollArea(y=Scroll) 行宽 820    chat_panel.rs:173-179
│  │  ├─ InputBar       min_h38 / max_h200 无滚动        chat_panel.rs:182-219
│  │  └─ BackToBottom   Absolute bottom=150              chat_panel.rs:364-393
│  ├─ ResizeHandle  w=6（全应用唯一手柄）                layout.rs:157
│  └─ SideView      w=PanelWidths.side_view(720/380)
│     ├─ PageTabs      h=38 注入 index 0                 editor/mod.rs:665-732
│     ├─ EditorView    100%×100%（自带 28px tab 条）      editor/mod.rs:180-259
│     ├─ TerminalView  100%×100%                         terminal/mod.rs:259-464
│     └─ DiffView      100%×100%                         editor/diff_view.rs:50-79
└─ StatusBar     h=32 固定                               layout.rs:179-194
浮层：FileDrawer z40/41 w320 · History z40 w320 · Palette w560 z无
      · Settings w640 z无 · Confirm z50 w560 · Conflict z无 · Toast z400
```

区域划分：顶栏/状态栏/图标轨固定像素，对话 flex，SideView 显式像素且**仅横向可拖**（`resize.rs:205`）。无纵向分割，抽屉不可调宽，`PanelWidths` 只有一个字段（`resize.rs:47-50`）。

---

## 三、缺陷台账

分级：**P0** 功能阻断／错乱；**P1** 明显不合理的体验或缺口；**P2** 可维护性与一致性。
「所需原语」列指 v8 架构层要新增的能力，非就地补丁。

### A. 平台与渲染正确性

| ID | 级别 | 现象 | 证据 | 所需原语 |
|:---|:---|:---|:---|:---|
| A1 | **P0** | 非 macOS 平台等宽字体落回 Inter（比例字体），编辑器行号/终端格子/diff 行无法对齐 | ✅ `xgent_app/src/startup.rs:79-90`（`else { Handle::default() }`）、`xgent_ui/src/fonts.rs:19-20`、终端按 `font×0.6` 排格 `terminal/output.rs:380` | 跨平台字体装配层（按平台解析候选栈 + 内嵌兜底资产） |
| A2 | **P1** | 主窗未设默认尺寸，仅 min 1024×640；首帧尺寸由 WM 决定 | `main.rs:229-238` | 窗口规格与 DPI 基线（`scale_factor` 感知的尺寸令牌） |
| A3 | **P0** | **CJK 双倍宽未建模**：终端按单一常量 `font × 0.6` 算所有字符宽，无 wcwidth 概念；中文输出占约 2 格却按 1 格排版 → 即使 A1 修好，中文表格/`ls -l`/进度条仍错位。对本项目「前期以中文为主」的定位是主路径缺陷 | `terminal/output.rs:379-380,400-402`；✅ 全文件无 CJK/双宽分支。另注：Inter 无中文字形，CJK 靠 fontique `system_font_discovery` 回退到**另一字体**（`ui-v7-migration.md:280`），跨字体更不可能共享等宽假设 | 单元格宽度模型（等宽基准 + 宽字符按格数分配 + 按格定位而非依赖 shaper） |

### B. 层级与模态

| ID | 级别 | 现象 | 证据 | 所需原语 |
|:---|:---|:---|:---|:---|
| B1 | **P0** | z 值无策略：仅 8 处 `GlobalZIndex`（confirm 50、file drawer `DRAWER_Z`/+1、toast 400、history `DRAWER_Z`）；**设置面板、命令面板、冲突弹窗、脏关闭弹窗无 z 值** → 抽屉绘制在其上并吞点击 | ✅ `confirm_dialog.rs:91`、`file_panel.rs:127,148`、`kit.rs:550`、`session_history.rs:147`；`settings_panel.rs:683-712`/`command_palette.rs:130-208`/`editor/conflict.rs:148-171` 无 z | 分层常量表（5 类浮层）+ 模态栈（唯一权威） |
| B2 | **P1** | 抽屉互斥靠 rail 点击路径手工维持，其它入口（快捷键、`context_scope` 的「+ 添加」）不受同一约束 | `activity_bar.rs:274-287`、`context_scope.rs:202-209` | 模态/浮层栈注册，显隐由栈深派生 |
| B3 | **P1** | tooltip 是宿主按钮的**子节点**且置于右侧 → 被祖先 `Overflow::clip` 裁掉，右缘按钮还会出窗 | `kit.rs:235`；裁剪祖先 `layout.rs:117,149`、`context_scope.rs:75` | 浮层独立于裁剪子树（顶层浮层根 + 定位器） |
| B4 | **P1** | 设置面板无 Esc、无点击外部关闭，只能按其按钮；且它开着时 Esc 触发 `chat.abort` | `settings_panel.rs:1308-1325`、`shortcuts.rs:143-157` | 模态栈的键盘路由与 dismiss 策略 |

### C. 滚动与溢出

| ID | 级别 | 现象 | 证据 | 所需原语 |
|:---|:---|:---|:---|:---|
| C1 | **P0** | **9 处**用 `Overflow::clip_y()` 配 `ScrollPosition`，clip 轴不响应滚轮 → 实际滚不动：文件树、历史列表（×2）、命令面板列表、工具结果、确认弹窗 diff、编辑器正文、设置模型列表 | ✅ `file_panel.rs:186`、`session_history.rs:161,221`、`command_palette.rs:157,201`、`confirm_dialog.rs:221`、`editor/mod.rs:252`、`settings_panel.rs:796`、`tool_panel.rs:211` | 统一滚动容器契约（`OverflowAxis::Scroll` + 视口 + 滚动条） |
| C2 | **P1** | 设置面板模型列表 `max_height:150` 且**完全没有** `ScrollPosition` | `settings_panel.rs:791-800` | 同上 |
| C3 | **P0** | 长路径/长标题/长状态文本溢出或换行而非省略：文件树路径、tab 标题、状态栏 provider 文本 | ✅ 未截断点 `file_panel.rs:349`、`editor/tabs.rs:509`、`status_bar.rs:208`。**根因是引擎缺口**：bevy 0.20 **无 `TextTruncate` 组件**（✅ 全仓 grep `truncat` 仅命中 `bevy_text::TextBounds` 文档注释与 `stack.rs`），只有 `TextBounds`（换行 + 按字符硬裁，官方自陈「only characters completely out of the bounds will be truncated, so this is not a reliable limit」`bounds.rs:9-12`）→ 省略号无法靠引擎，必须自建 | **自建 `xui::Ellipsis`**（测量 + 前缀二分 + `…`）与 clamp 原语 |
| C4 | **P1** | 输入框 `min_h 38 / max_h 200` 但无内部滚动：多行写到 200 后继续增长被裁，看不见当前行 | `chat_panel.rs:182-219` | 自适应高度 + 内部滚动契约 |
| C5 | **P1** | 快捷问答 chips 一行 5 个，无换行无滚动 → 窄窗口下被裁 | `chat_panel.rs:301-339` | wrap/滚动原语 |

### D. 区域模型与信息架构

| ID | 级别 | 现象 | 证据 | 所需原语 |
|:---|:---|:---|:---|:---|
| D1 | **P0** | `SideViewContent` 是 **closed 5 变体枚举** `{None,Editor,Preview,Diff,Terminal}`，插件/新面板无法注册页签 | ✅ `editor/mod.rs:69-81` | 槽位注册表（`PanelSlot` 描述符 + 有序列表） |
| D2 | **P1** | SideView 每个页签都是 `height:100%`，而同一列还有 38px 页签条 → **底部 38px 永久掉到视口外**（终端状态条、编辑器最后一行） | `editor/mod.rs:668` vs `:184`、`terminal/mod.rs:262`、`diff_view.rs:54` | 区域尺寸契约（`flex_grow` + `min_height:0`，禁 `height:100%` 于带兄弟的列） |
| D3 | **P1** | 两套 tab 条并存，且两个返回按钮都用同一个 marker `EditorBackButtonMarker` | `editor/mod.rs:220-239` 与 `:706-731` | 单一页签条所有者 |
| D4 | **P1** | `ActivityKind{Chat,Files,History,Terminal}`：前三只翻布尔值，只有 Terminal 真正换视图；`ActiveActivity` 会停在陈旧值（连点 Files 关掉抽屉后仍「激活」） | `activity_bar.rs:20-30,251-306` | 活动项由注册表驱动 + 选中态从可见性派生 |
| D5 | **P1** | 会话历史是**抽屉浮层**而非面板（与文件抽屉争层级，见 B1） | `session_history.rs:136-251` | 同 D1 |
| D6 | **P1** | 全应用只有 1 个横向手柄；无纵向分割；抽屉不可调宽 | `resize.rs:47-50,205`、`layout.rs:157` | 分割模型（可声明的 split 树） |

### E. 会话区与流式渲染

| ID | 级别 | 现象 | 证据 | 所需原语 |
|:---|:---|:---|:---|:---|
| E1 | **P0** | **无 Markdown/代码块渲染**：助手回复是一个扁平 `Text`（R3-5、ui-gap-plan 阶段 D 遗留，最大单块缺口） | `chat_panel.rs:588`；✅ 全仓 `markdown` 仅 2 处命中且非渲染 | 消息块模型 + 富文本排版（含代码块、行内码、列表、引用） |
| E2 | **P1** | 流式顺序靠命令式挪子节点：用户消息 `add_child(current)`，工具卡/压缩通知/撤销各自 append → **流式文本排在自身工具卡之上**，定稿副本落最后 | `chat_panel.rs:489,724`、`tool_panel.rs:85`、`undo.rs:286` | 有序消息树（state 权威 + 顺序键），渲染按序派生 |
| E3 | **P1** | `@file:`/`@cursor` 仅在提交时静默解析，**无补全 UI** | `chat_panel.rs:776`、`editor/at_syntax.rs:10` | 输入内联补全浮层 |
| E4 | **P1** | **无发送/停止按钮**（注释仍写「发送按钮区」）；发送＝Enter、停止＝Esc，鼠标用户无可点目标，不可用状态也不可见 | `chat_panel.rs:221-298`、`xui/src/input.rs:109`、`shortcuts.rs:51,143` | 组合区动作条 + 状态可见性 |
| E5 | **P1** | 上下文 chips 只是打开的编辑器 tab 的只读镜像，无法单独移除/持久化 | `context_scope.rs:1-7` | 上下文集合模型 |
| E6 | **P2** | 工具参数按 50 字符手工截断（非按宽度、无省略号语义） | `tool_panel.rs:380-387` | 用 C3 的省略原语替代 |

### F. 快捷键与焦点

| ID | 级别 | 现象 | 证据 |
|:---|:---|:---|:---|
| F1 | **P0** | `palette.open_files` 注册了（`shortcuts.rs:44`）但**全仓零消费者** → 快捷键什么都不做 ✅ | `shortcuts.rs:44`；dispatch 无该分支 |
| F2 | **P0** | `input.focus => {}` **空实现** → Ctrl+I 号称聚焦输入，实际无事 | `shortcuts.rs:196` |
| F3 | **P1** | 每个输入框都是 `TabIndex(0)` → 无遍历顺序，Tab 行为不确定 | `settings_input_focus.rs:88,154-167` 断言的正是这个错误不变量 |
| F4 | **P1** | 焦点无可见环（theme 有 `accent_glow` 但无人消费） | `theme.rs:62-63` |

### G. 主题与令牌纪律

| ID | 级别 | 现象 | 证据 |
|:---|:---|:---|:---|
| G1 | **P1** | `Theme` 在 Startup 烘焙一次，**永不随设置驱动** → 无法换肤、无法改字号（R3-14：`preferences.theme` 无读取方） | `layout.rs:60`、`theme.rs:6`；ledger `docs/notes/agent-gap-review.md:237` |
| G2 | **P2** | `diff_view.rs:217` 现场 new 一个 `Theme::dark()` 而非取活的 Theme | ✅ |
| G3 | **P2** | 约 40 处魔法像素（34²/40²/28²/38/150/200/220/360/440/520/560/640/820）与绕过 `radius` 的圆角字面量 | `chat_panel.rs:734`、`tool_panel.rs:118,179`、`welcome.rs:112,247`、`editor/mod.rs:228` |
| G4 | **P2** | `820.0` 重复三次；两个同名 `fn px()` 影子定义；最小宽度两处定义可自由漂移 | `chat_panel.rs:434,545` 等、`confirm_dialog.rs:45`、`settings_panel.rs:1251`、`theme.rs:230-232` vs `resize.rs:25-27` |
| G5 | **P2** | 色板纪律尚可（`theme.*` 读约 373 处 vs 非 theme 色 24 处，后者多为 ANSI 表与个别漏网）——**几何/排版纪律才是失守点** | ✅ `grep` 计数 24；漏网例 `activity_bar.rs:207,395`（`0x1C1917` 星形）、`editor/tabs.rs:429`（`Color::WHITE`） |
| G6 | **P2** | `hover_tint_system` 每帧轮询所有节点 | `kit.rs:125-157` |

### H. i18n（NF-05 违例）

| ID | 级别 | 现象 | 证据 |
|:---|:---|:---|:---|
| H1 | **P1** | 用户可见字符串硬编码：rail tooltip 对话/文件/历史会话/终端/展开面板、顶栏 命令面板/设置、页签 预览/差异/终端 | `activity_bar.rs:130-167`、`top_bar.rs:222,225`、`editor/mod.rs:659-664`（`let _ = loc;` 把 Localizer 丢弃） |
| H2 | **P2** | 占位与非文字符号：`"$0.00"`、`"▼"` | `status_bar.rs:140`、`chat_panel.rs:385` |
| H3 | **P1** | `xui` 的 i18n 桥接形同虚设（R3-7 / D5） | ledger `docs/notes/agent-gap-review.md:193,379` |
| — | — | **测试为何没拦住**：`settings_panel_i18n.rs:55,72` 只扫 settings 面板，所以 rail/页签的字面量存活 | ✅ |

### I. 健壮性与可测性

| ID | 级别 | 现象 | 证据 |
|:---|:---|:---|:---|
| I1 | **P1** | `ChatPanelEntities` 把 Entity 缓存进 Resource；多处 `q.single_mut()` 在数量≠1 时静默早退 → 布局重建后失效且无诊断 | `chat_panel.rs:85-92,796,889`、`top_bar.rs:293` |
| I2 | **P1** | **零布局几何断言**：现有 UI 测试只断言 marker 存在与 `Display` 状态机；chat/palette/confirm/status/topbar/resize/滚动/重叠完全无测 | `tests/layout.rs:28-33,64-68`、`interaction_model.rs:160-283`（合成节点，非真实面板） |
| I3 | **P2** | 可见性由 5 个独立系统各自写 `Node.display`，无单一权威 → 状态与呈现可自由背离 | `layout.rs:199`、`file_panel.rs:199`、`editor/mod.rs:342`、`terminal/mod.rs:475`、`welcome.rs:178` |
| I4 | **P2** | `VirtualList`（xui）零消费者（R3-6）→ 长会话与深文件树不虚拟化 | ledger `agent-gap-review.md:186` |

---

## 四、路线图功能对布局的诉求

v8 的价值不在修 30 条缺陷，而在**修完之后新界面有地方长出来**。以下为逐个功能对架构层的具体要求。

| 功能 | 阶段 | 状态 | UI 诉求 | 依赖的 v8 原语 |
|:---|:---|:---|:---|:---|
| Markdown 渲染 | P1 差距 | 未实现（R3-5） | 消息行成为块容器：代码块/行内码/列表/引用/表格 | E2 有序消息树、C3 省略、A1 等宽字体 |
| F-10 Git 集成 | P1 | **已实现为插件，UI 缺位** | rail 需 Git 图标槽、源代码控制面板、提交对话框；现 `SideViewContent::Diff` 是 buffer-diff 不是 git-diff | D1 槽位注册表、B1 分层 |
| F-13 MCP 支持 | P1 | 仅 trait 占位 | 新设置分区（server 列表/增删启用/鉴权状态）、状态栏段、每 MCP 工具卡与策略行 | G1 主题热驱动、C1 滚动、D1 |
| F-14 自定义工具/插件管理 | P2 | 部分（插件系统即路径） | 设置内插件管理页（装卸/启停）+ `plugin.install/uninstall/reload` 面板命令 + 权限对话框 | D1、B4 模态栈、`docs/design/plugin-system-design.md:898-899,657-662` |
| F-12 成本统计 | P1 | 未实现（R4-1，状态栏占位） | 会话/模型维度分解面板；展示形态未定（OQ-10） | D1、G1 |
| 插件 UI 面板 | P2 | **扩展点已声明未实现**（D-P3） | `ui_panel` 扩展点：manifest 带 `id/title/icon/默认位置`，宿主按槽位渲染 | D1、E2、B1 |
| 编辑器查找替换 | — | R3-21 不可达（`xui::FindState` 零消费者） | 查找条浮层（需浮层定位 + 键盘路由） | B3、B4、F3 |
| 主题系统 | P1 | R3-14 单预设，K-01 亮色延后 | 亮色主题 + 字号密度切换 | G1、A1 |
| F-15 虚拟宠物 | P1 | `xgent_pet` crate 未建 | **第二系统窗口**：置顶透明无边框（K-04），不属主窗布局 | 窗口层（与主窗布局态解耦） |
| 守护进程升级 | B 阶段/胖后台 | 中后台索引上移、胖后台工具执行上移 | 索引进度态、跨 IPC 取消、批量确认队列、`on_update` 流式（**D-P8 至今未接通，执行器恒传 `None`**） | E2（渲染须源自异步状态，不得假设本地执行） |

**多开模型**（架构 `docs/design/architecture.md:416-418`）：一项目一 UI 进程、一主窗，**不是** Bevy MultiWindow。故布局态天然进程内；但项目配置、provider/model 选择、文件态是 daemon 共享且同项目客户端需同步（`requirements.md:118`）。宠物是进程内唯一真正的第二窗口。

---

## 五、bevy_ui 能力边界（不得承诺项）

来自 `docs/notes/bevy-ui-mode-assessment.md` 与本次核对：

- **保留模式**（`:20`）：「每帧重绘」类担忧不成立（`:76-81`）。
- 真实短板（`:64-72`、`:131`）：**无虚拟滚动**（→ `xui::VirtualList`，I4）；**无命令面板**（→ xui 自建，已有）；多行输入/发送语义需包装（K-05）；**富文本与代码高亮偏弱——唯一被标为 P1 的风险，必要时局部混合 egui**（`:101-105`）；a11y 不成熟；**透明/置顶系统窗口受限**（K-04）。
- IME 由官方 `text_input` 支持（`:69`）。
- **无文本截断/省略号**：✅ bevy 0.20 无 `TextTruncate`，只有 `TextBounds`（`bounds.rs:9-12` 自陈按字符硬裁「not a reliable limit」）→ 省略号与多行 clamp 是**必须自建**的原语，不能指望升级引擎解决。
- **无 z-index 默认策略**，`GlobalZIndex` 必须集中分配（`docs/design/ui-bsn-refactor-review-r4.md:80-105` 给出五类浮层）。
- `Overflow::Scroll` / `Clip` / `Hidden` **不可互换**，`ScrollPosition` 只在 Scroll 轴生效（`docs/notes/scroll-abstraction-design.md:19`）——这是 C1 的根因。
- picking **忽略 `FocusPolicy`**，缺 `Pickable` 会挡下层；焦点需 `TabIndex`；overlay 必须挂在确切滚动容器之下否则盖住兄弟（`docs/dev-tutorial.md:565-568`）。
- **无渐变/无 box-shadow**：层级只能靠明度阶梯 + 丝线边框（ADR-0014:13,24）；尚无全局 rem 缩放（`docs/notes/bevy-0.20-migration.md:102`）。

**不得承诺**：阴影/渐变式高程、自由绝对图层栈、插件任意面板、面板拖拽重排（`docs/design/ui-design.md:443`）。

---

## 六、与既有 ledger 的映射

v8 不新开功能账，把架构缺口与既有账对齐，避免双轨：

| ledger | 内容 | v8 承接项 |
|:---|:---|:---|
| R3-5 | 助手消息无 Markdown | E1 |
| R3-6 | `VirtualList` 零消费者 | I4 |
| R3-7 / D5 | xui i18n 桥接形同虚设 | H3 |
| R3-12 | diff 仅公共前后缀（无 LCS/Myers） | 由 E1/C1 承载展示，算法另账 |
| R3-14 | 主题只一套预设 | G1 |
| R3-16 | 插件工具确认弹窗无法展示真实 diff | B1/E1 |
| R3-21 | 编辑器查找替换不可达 | B3/B4 |
| R4-1 / D-P3 / D-P8 | 成本统计 / 插件面板 / `on_update` 未接通 | 第四节 |

来源：`docs/notes/agent-gap-review.md:177-300`，`docs/design/plugin-system-design.md:948-996`。

---

## 七、BSN 复盘：v8 为何仍不引声明式 UI

r4 评审的结论是「10 条 r3 修复从未真正应用」（`ui-bsn-refactor-review-r4.md:280-293`，C-1..C-10 标「未修复」），并把 **A-4 按钮状态**列为最大功能缺口、**A-2 动态插入**列为最大文档缺口（`:352-360`）。这些修复只存在于被回写的 prose 里，**无一行代码验证过**。

v8 的判断：**问题是真的，BSN 不是解**。

- A-1/A-5（主题重复、双主题并存）→ v8 用单一 `Theme` 权威 + 热驱动解决（G1），不引入第二套 `XuiTheme`。
- A-2（动态内容插入）→ v8 用**有序消息树 + 顺序键**解决（E2），比通用 SceneComponent 小得多。
- A-3（z-index）→ v8 用**分层常量 + 模态栈**解决（B1/B2）。
- A-8（state→render 管线）→ v8 明确「状态资源是权威、可见性由系统派生」（I3），不引入可查询 SceneComponent 层。
- A-7（resvg 破坏 xui 纯净性）→ v8 保持 v7 的构建期 SVG→PNG 方案，不动 xui 依赖边界。

结论：把 r4 的七个架构问题当作**需求**，把 BSN 当作**已被否决的一种实现**。图标沿用 v7 白描 PNG 管线（`ui-v7-migration.md:320-341`）。

---

## 八、优先级汇总（v8 必修面）

**P0（9 条，阻断级）**：A1 跨平台等宽字体 · A3 终端 CJK 双倍宽 · B1 分层与模态 · C1 滚动契约 · C3 省略原语 · D1 槽位注册表 · E1 Markdown · E2 有序消息树 · F1/F2 死快捷键（并入模态与焦点栈一并修）。

**P1**：A2 D2 D3 D4 D5 D6 B2 B3 B4 C2 C4 C5 E3 E4 E5 F3 F4 G1 H1 H3 I1 I2。

**P2**：E6 G2 G3 G4 G5 G6 H2 I3 I4。

**验收口径**（回应 I2 的「测不到」）：v8 引入**布局几何断言**——对每个区域断言「视口内可见高度 = 分配高度，无子节点越界」「截断节点宽度 ≤ 父宽」「浮层命中测试不被下层吞掉」，并以 `ui-snapshot` feature（`ui-v7-tasks.md` M2-T6 已建）出图与原型对照。
