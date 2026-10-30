# XGent UI v8 · 设计系统与令牌演进

> 状态：**提案**　时间：2026-10-16（2026-10-19 增补 §九 3D 视口边界、§十 动效令牌）　基线 commit `d61e54b`
> 上游：[需求简报](ui-v8-requirements-brief.md)　证据：[审计报告](../notes/ui-v8-audit.md)　实现细节：[布局架构](ui-v8-layout-architecture.md)　3D：[视口设计](ui-v8-3d-viewport.md)
> 视觉蓝本：**沿用 [ADR-0014](../decisions/0014-ui-视觉基准采用-linear-设计系统-v7-原型.md)（Linear / v7）**，本轮不换色板、不换气质。

---

## 〇、本轮设计立场

v7 已经把「长什么样」定得很好：近黑画布、四级灰阶、单一靛紫、明度阶梯表达层级、零投影。v8 **不改这些**。

v8 改的是「这套语言能不能被机器一致地执行」。v7 落地后失守的不是审美，而是**纪律**——约 40 处魔法像素、`TextTruncate` 零消费、等宽字体只在 macOS 成立、`Theme` 烘焙一次不再变。所以本文的重点是：**把令牌从"值表"升级为"约束"**，让违反规范在编译期或测试期就被挡住，而不是靠 code review 肉眼盯。

### 明确继承（不动）

| 项 | 值 | 出处 |
|:---|:---|:---|
| 画布 | `#08090A` | `theme.rs:133` |
| 表面阶梯 | `#0F1011` → `#191A1B`，代码底 `#0D0E10` | `theme.rs:134-136` |
| 文字四级 | `#F7F8F8 / #D0D6E0 / #8A8F98 / #62666D` | `theme.rs:151-154` |
| 单一彩色 | 靛紫 `#5E6AD2 / #7170FF / #828FFF` | `theme.rs:157-159` |
| 状态色 | Radix 深色阶 `#FFB224 / #7170FF / #30A46C / #E5484D / #0091FF` | `theme.rs:165-174` |
| 陪伴暖色例外 | 琥珀 `#FFB224`（全界面唯一暖色） | `theme.rs:177-178` |
| 层级手法 | 背景明度阶梯 + rgba 白丝线边框，非浮层零投影 | ADR-0014 |
| 有意裁剪 5 条 | 状态色外推／遮罩 0.5／暖色例外／图标方圆角 6px／亮色交互用半透黑 | ADR-0014 裁剪清单，**实现时勿"修正"** |
| 圆角阶梯 | 2/4/6/8/12，胶囊 `BorderRadius::MAX` | `theme.rs:286-297` |
| 间距网格 | 4px：4/8/12/16/20/24/32 | `theme.rs:209-217` |

---

## 一、跨平台字体系统（v8 最大实体变更）

### 1.1 问题

字体装配今天是单平台特权路径（审计 A1/A3）：`mono` 只在 macOS 解析成 Menlo，其余平台拿到 `Handle::default()`，而默认槽位已被 Inter 占据——**Windows/Linux 上的"等宽"是比例字体**，且 Inter 无中文字形，中文靠 fontique 回退到**另一个字体**。两个事实叠加，意味着任何"按字符宽排格"的假设都不成立。

### 1.2 三槽位模型

放弃「一个 ui + 一个 mono」的二元模型，改为**三个语义槽位 + 显式拉丁/CJK 双源**：

| 槽位 | 用途 | 一阶候选（按平台） | 内嵌兜底 |
|:---|:---|:---|:---|
| `ui` | 全部界面文本 | 三平台统一 **Inter Variable**（已内嵌，OFL，cv01/ss03） | 同资产 |
| `ui_cjk` | 中文与中英混排 | Windows `Microsoft YaHei UI` / macOS `PingFang SC` / Linux `Noto Sans CJK SC` → `Source Han Sans SC` → `WenQuanYi Micro Hei` | 系统发现（`system_font_discovery` 已启用，根 `Cargo.toml:7`） |
| `mono` | 代码/终端/diff/行号 | Windows `Cascadia Mono` → `Consolas`；macOS `SF Mono` → `Menlo`；Linux `JetBrains Mono` → `DejaVu Sans Mono` → `Liberation Mono` | **内嵌 JetBrains Mono（OFL 1.1）** |
| `mono_cjk` | 终端/代码内中文 | `Sarasa Mono SC`（等宽 CJK，OFL）→ `Noto Sans Mono CJK SC` | 系统发现 |

**硬约束（替代今天的软兜底）**：

- C-FONT-1 `mono` **必须**解析到真实等宽资产。候选栈全缺 → 用内嵌 JetBrains Mono；**永不**回退到 `ui` 槽位或比例字体。今天的 `else { Handle::default() }`（`startup.rs:88`）是违规实现，v8 删除该分支。
- C-FONT-2 解析结果写入 `tracing::info!`（实际命中的家族名与来源），三平台启动日志可核对；解析失败是**警告**不是静默。
- C-FONT-3 禁止新增 `cfg!(target_os = …)` 特权路径（NU-6）。平台差异只存在于**候选栈顺序**这一处数据里，代码不分叉。
- C-FONT-4 `mono` 与 `mono_cjk` 的 advance 关系必须**实测**而非估算：启动期渲染样本串测出 `latin_advance` 与 `cjk_advance`，校验 `cjk_advance ≈ 2 × latin_advance`，不满足则记警告并调整单元格基准宽（见 1.3）。

### 1.3 终端与代码的宽度模型（回应 A3）

今天终端按 `宽 ≈ font × 0.6` 一个常量算所有字符（`terminal/output.rs:380`）。v8 改为**显式双宽单元格模型**：

```
cell_w = 实测 latin_advance（来自 mono 槽位，非硬编码系数）
宽字符（CJK 全角、emoji、U+FF00-FF60、制表符块）占 2 × cell_w
列数 = floor(视口逻辑宽 / cell_w)
定位 = 按格数 × cell_w 手工摆位，不依赖文本 shaper 连续排版
```

理由：拉丁走 JetBrains Mono、中文走 Sarasa/Noto，两个字体由 fallback 链拼接，**没有共享的连续排版度量**可靠可言。按格定位是唯一在三平台都成立的做法。这也与真实终端（alacritty/kitty）的做法一致。

### 1.4 排版特性与字重

沿用 v7：Inter 开 `cv01`/`ss03`，字重 400（正文）/510（强调）/590（宣告），负字距仅用于展示级。v8 追加：

- 中文**不开**负字距（CJK 无字距概念，负值会让中文挤成一团）。混排串的负字距只作用于拉丁段，或展示级标题避免混排。
- `mono` 槽位**不开** Inter 特性（今天的 `mono_text_weighted` 未附加 `FontFeatures`，`fonts.rs:57-70`，保持）。
- 行号、单元格、diff 行一律走 `mono`，禁止用 `ui_text` 冒充。

---

## 二、密度与排版刻度

### 2.1 两档密度

`type_scale` 与 `size` 不再是一组固定数，而是**基准字号 × 密度系数**派生：

| 档位 | 基准 | 系数 | 用途 |
|:---|:---|:---|:---|
| `Comfort`（默认） | 14.0 | 1.00 | 与今天 v7 完全一致 |
| `Compact` | 13.0 | 0.92 | 小窗口 / 多开并排 / 长会话密集阅读 |

系数作用于**尺寸**（区域高宽、控件高、内边距），字号走 2.2 的整档阶梯。两档都必须三平台可用，由设置面板选择并即时生效（依赖 U-09.1 的主题热驱动通路）。

### 2.2 字号阶梯（保留 8 档，禁就地发明）

沿用 v7 `type_scale`（`theme.rs:250-269`）：`DISPLAY 24 / H3 15 / BODY 14 / BODY_SM 13 / SMALL 12.5 / CAPTION 12 / MICRO 11 / TINY 10 / MONO 12`。

v8 变更两点：

- **`SMALL 12.5` 改为 12**：半像素在非整数 DPI 缩放下产生不稳定模糊，且 12.5 与 12 视觉差不可辨。合并后阶梯为整数集合，`MONO` 与 `CAPTION` 同档。
- 新增 **`CODE`** 独立档（默认 = `MONO`，Compact 下 −1），使终端/编辑器字号与界面字号解耦（今天终端在 `MONO` 上再手减 2，`theme.rs:268` 注释所述）。

行高档沿用 1.2/1.4/1.5/1.6/1.6。中文正文建议 **1.7**（CJK 密排可读性低于拉丁，v7 的 1.6 在长段中文下偏挤），新增 `line_height.CJK = 1.7` 供消息正文按语言选档。

### 2.3 尺寸令牌收敛

审计 G3 列出约 40 处魔法像素。v8 把它们收进 `size`，并规定**区域尺寸只能来自 `size` 或 `PanelMetrics`**：

| 新令牌 | 值（Comfort） | 取代的散落字面量 |
|:---|:---|:---|
| `size::ICON_BTN` | 34 | `top_bar.rs` 命令/齿轮钮 |
| `size::RAIL_ITEM` | 40 | `activity_bar.rs:88-96` |
| `size::BRAND` | 28 | 品牌块 |
| `size::SCOPE_H` | 38 | `context_scope.rs` |
| `size::MSG_MAX_W` | 820 | 三处重复（`chat_panel.rs:434,545` 等） |
| `size::TOAST_BOTTOM` | 150 | `chat_panel.rs:364-393` 回底钮 |
| `size::OVERLAY_PALETTE_W` | 560 | `command_palette.rs` |
| `size::OVERLAY_SETTINGS_W` | 640 | `settings_panel.rs:683-712` |
| `size::OVERLAY_CONFIRM_W` | 560 | `confirm_dialog.rs:77-110` |
| `size::DRAWER_W` | 320（**可调**，min 240 / max 480） | `theme.rs:236` 今天为定值 |
| `size::MODAL_MIN_W` | 360 | `conflict.rs:148-171` |

同时：**`px()` 全仓唯一**（`theme.rs:300`），删除 `confirm_dialog.rs:45`、`settings_panel.rs:1251` 两个影子定义；最小宽度只在 `size` 定义一次，`resize.rs:25-27` 改为引用（消除 G4 漂移）。

---

## 三、层级视觉规范（五类浮层）

bevy_ui 无阴影无渐变（`bevy-ui-mode-assessment.md`），层级只能靠**明度阶梯 + 丝线边框 + 遮罩**。v8 把这件事写成可校验规则。

| 类 | 底 | 边框 | 遮罩 | 圆角 | z 段 |
|:---|:---|:---|:---|:---|:---|
| **基面** 面板/区域 | `surface` `#0F1011` | `line` 白@0.05 | — | 0 | `BASE 0` |
| **抽屉** 文件/历史 | `surface` | `border` 白@0.08 | 无（非阻塞） | 0 | `DRAWER 200` |
| **下拉** 补全/菜单/查找条 | `elevated` `#191A1B` | `border` | 无 | `radius.PANEL 12` | `DROPDOWN 300` |
| **模态** 设置/确认/冲突/命令面板 | `elevated` | `border_hover` 白@0.14 | `overlay` 黑@0.5 | `radius.PANEL 12` | `MODAL 400 + 栈深` |
| **提示** tooltip | `tooltip_bg` `#28282C` | 无 | 无 | `radius.CTRL 6` | `TOOLTIP 500` |
| **通知** toast | `tooltip_bg` | 无 | 无 | `radius.CTRL 6` | `TOAST 600` |

规则：

- **L-1 层级用明度表达，不用投影。** 每上一级底色的相对亮度递增一档：`bg(0.035) → surface(0.063) → elevated(0.102) → tooltip(0.161)`。这条链是 v7 已有的，v8 把它变成**断言**：测试校验四档亮度单调递增且相邻差 ≥ 一个可辨阈值。
- **L-2 遮罩只随模态出现。** 抽屉与下拉**不加遮罩**（非阻塞，允许边看边操作）；今天文件抽屉和历史抽屉带遮罩却又非阻塞，语义混乱。
- **L-3 z 值不得裸写。** 业务代码禁止出现 `GlobalZIndex(<数字>)`；静态类走常量，模态类由模态栈按深度动态分配 `400 + depth`（栈深上限 8，见审计 B1 与架构文档）。
- **L-4 提示与通知不受祖先裁剪。** 二者渲染在顶层浮层根（审计 B3），宿主子树里不放 tooltip。

---

## 四、文本与溢出视觉规范

- **T-1 单行区域必省略。** 任何宽度受限的单行文本（文件树名/路径、页签标题、状态栏段、命令面板行、工具参数）走 `xui::Ellipsis`，尾省略号。截断时完整值进 tooltip（审计 C3/E6）。⚠ **bevy 0.20 无 `TextTruncate` 组件**（✅ 已 grep 全仓证实），`bevy_text::TextBounds` 只按字符硬裁且官方自陈不可靠（`bounds.rs:9-12`）→ 省略号是 v8 必须**自建**的原语，不是配置项。
- **T-2 路径中段省略。** 文件树与面包屑显示路径时保留**末两段**、省略中段（`src/crates/xgent_…/theme.rs`），因为区分度在尾部。今天 `file_panel.rs:349` 完全不截断。
- **T-3 多行内容用 clamp 不用裁。** 消息正文、工具结果、diff 预览给「最多 N 行 + 展开」，而不是裁到视口外（今天靠 `clip_y` 假装能滚，审计 C1）。
- **T-4 中文不拆词换行。** 换行点走已落地的 CJK 分词（R3-8 已闭合），禁止把整句当一个 token。
- **T-5 数字与代码用 tabular 对齐。** 行号、token 数、耗时、成本统计走 `mono`，避免比例字体下数字列抖动。
- **T-6 空/加载/错误三态有专属视觉。** 今天多数列表在空态直接是空白。规范：空态 `text_faint` + 一行说明（i18n key）；加载态用骨架条而非 spinner（无投影体系下 spinner 不显眼）；错误态 `st_fail` 边框 + `st_fail_bg` 薄底。

---

## 五、图标与状态色使用边界

- **I-1 图标沿用 v7 白描 PNG 构建期管线**（`ui-v7-migration.md:320-341`），`ImageNode` + tint 染色。**不引入 resvg 运行时**——审计第七节 A-7：那会破坏 `xui` 纯 `bevy + xui_i18n` 的依赖边界（NU-4）。
- **I-2 描边粗细随字号**：界面图标 24、rail 22、行内 16；`strokeWidth` 1.5，在 12px 下视觉过细则 1.75。
- **I-3 彩色只用三处**：强调（靛紫）、状态（Radix 四色）、陪伴（琥珀）。其余全部灰阶。今天 `activity_bar.rs:207,395` 的 `0x1C1917` 与 `editor/tabs.rs:429` 的 `Color::WHITE` 属违规（G5）。
- **I-4 状态语义固定**：`st_pending` 待确认（琥珀）/ `st_running` 执行中（靛紫，带 pulse）/ `st_ok` 成功（绿）/ `st_fail` 失败与拒绝（红）。工具卡、Agent pill、状态栏段**只能**用这套，不得自造。
- **I-5 焦点环用 `accent_glow`**（`theme.rs:62-63` 有令牌但今天零消费，审计 F4）：2px 描边 `accent_interactive` + 外圈 `accent_glow`。焦点必须可见，这是键盘可达性的下限。
- **I-6 hover/active 步进全量化**：`hover` 白@0.05、`active` 白@0.08，由单一数据驱动系统处理；废除今天每帧轮询所有节点的 `hover_tint_system`（审计 G6）。

---

## 六、双主题令牌结构（本轮只铺结构）

实现范围外（需求简报第五节），但**结构必须先对**，否则下一轮换肤又要改所有面板。做法：把 `Theme` 从「色值表」拆成「**语义令牌 → 调色板**」两层，面板只消费语义名。

```rust
// 语义令牌（面板唯一可消费的一层）——名称与主题无关
surface_base      // 会话画布
surface_panel     // 顶栏/图标轨/侧区
surface_raised    // 浮层/下拉/面板
surface_code      // 代码与终端底
surface_overlay   // 遮罩
text_primary / text_secondary / text_tertiary / text_quaternary
text_on_accent
border_subtle / border_default / border_emphasis
interactive_hover / interactive_active / interactive_selected
accent / accent_hover / accent_active / accent_muted / accent_focus_ring
status_pending / status_running / status_success / status_failure / status_info
status_*_bg
companion / companion_muted          // 陪伴暖色例外
tooltip_bg / tooltip_text
palette_*                           // 底层 raw 色，仅 theme 内部使用
```

亮色预设的映射规则（ADR-0014 裁剪 5）：`interactive_*` 与 `border_*` 的半透明白 → 半透黑；`surface_*` 明度阶梯**反向递减**；`text_*` 四级反向；`accent` 取 Radix 亮阶。`tooltip` 两主题恒暗（`theme.rs:95-99` 已如此设计）。

配套要求：`Theme` 成为可热替换的单一权威（审计 G1/I1），面板经 ECS 事件订阅主题变更并重取色，**禁止**在 spawn 时把色值烘焙进组件后不再更新（今天 `layout.rs:60` 即此违规）。

---

## 七、Do's & Don'ts

**Do**

- 尺寸走 `size` / `PanelMetrics`；色走语义令牌；字走 `type_scale` + `ui_text/mono_text` 构造器。
- 滚动只用统一滚动容器；截断只用统一省略原语。
- 新界面先注册描述符，再实现视图。
- 每个可见文本都有 i18n key，中英文语料同批提交。
- 状态在 Resource、显隐由单一系统派生。

**Don't**

- ❌ `Overflow::clip_y()` + `ScrollPosition` 并存（审计 C1 的九个现场）。
- ❌ 有兄弟节点的列里写 `height: 100%`（审计 D2 的 38px 裁切）。
- ❌ 裸 `GlobalZIndex(<数字>)`。
- ❌ `Theme::dark()` 现场构造（审计 G2）。
- ❌ `cfg!(target_os = "macos")` 决定功能是否可用（审计 A1）。
- ❌ 用 `Color::WHITE` / 十六进制字面量绕过 theme（审计 G5）。
- ❌ 硬编码用户可见字符串（审计 H1；`let _ = loc;` 属故意绕过）。
- ❌ 承诺阴影、渐变、自由图层栈、面板拖拽重排（bevy_ui 边界，审计第五节）。

---

## 八、`theme.rs` 落地增量

| 增量 | 类型 | 说明 |
|:---|:---|:---|
| 语义令牌层 | 新 `enum Semantic` + `Theme::semantic()` | 面板改消费语义名；raw 字段降为 `pub(crate)` |
| `Density` | 新 `enum {Comfort, Compact}` + `Theme::density` | 派生 `size`/`type_scale` 实际值 |
| `type_scale::CODE` | 新常量 | 代码/终端独立字号档 |
| `line_height::CJK = 1.7` | 新常量 | 中文正文 |
| `SMALL 12.5 → 12` | 改值 | 消除半像素 |
| `size::{ICON_BTN, RAIL_ITEM, BRAND, SCOPE_H, MSG_MAX_W, TOAST_BOTTOM, OVERLAY_*_W, MODAL_MIN_W}` | 新常量 | 收敛约 40 处魔法像素 |
| `z::{BASE, DOCK, DRAWER, DROPDOWN, MODAL_BASE, TOOLTIP, TOAST}` | 新模块 | 审计 B1 分层常量 |
| `DRAWER_W` 变区间 | 改类型 | `DRAWER_W / _MIN / _MAX`，配合 U-05.6 可调宽 |
| `mono_*` 实测度量 | 新 `MonoMetrics` Resource | `latin_advance` / `cjk_advance` / `line_height`，供终端与代码排版（1.3） |
| `code_bg` 与 `surface_code` 归一 | 改名 | 现有 `code_bg`（`theme.rs:136`）语义并入 |

保留不变：`space` 网格、`radius` 阶梯、`px()` 唯一化后成为全仓入口。

---

## 九、3D 视口的令牌边界（F-16，2026-10-19 增补）

3D 不是一套新视觉语言，而是**同一套令牌在第三个维度上的投影**。完整设计见 [ui-v8-3d-viewport.md](ui-v8-3d-viewport.md)；本节只写「不能越界的地方」。

### 9.1 定位

- 3D 是 `Slot::Center` 的**默认面板**（MVP-2）；对话在 SideDock；编辑器/终端打开时接管 Center。MVP-1 时期它与对话同槽互斥；聚焦态只是它的布局态，**不是新层**。
- 渲染走**离屏纹理**（3D 摄像机 → `RenderTarget::Image` → UI `ImageNode`），布局、裁剪、模态栈仍由 2D 系统独占权威。
- 阅读交棒 2D：**3D 内不渲染源码文本**，双击文件即打开编辑器。

### 9.2 元素 → 令牌（唯一允许的映射）

| 3D 元素 | 令牌 | 备注 |
|:---|:---|:---|
| 视口底 | `surface_base` + `surface_panel` 6% 径向 | 禁星空贴图 |
| 星体填充 | `text_faint` → `text_dim`（按 LOC 插值） | 禁按目录分色相 |
| 星团核 | `text_faint` + `accent_glow` @30% | 禁 bloom |
| 依赖边 | `border_subtle` @0.05，选中 `border_emphasis` @0.14 | 禁彩色边 |
| 改动环 | `border_emphasis` 细环 | 禁琥珀（陪伴专用） |
| 运行中 / 失败 | `status_running` / `status_failure` | 复用 I-4，禁自造 |
| 深度雾 | 雾色 = `surface_base`，`0.6×dist` → `1.8×dist` | 禁蓝紫雾 |
| 标签 | 2D 叠加层 `text_secondary` + `Ellipsis` | 禁 3D 文本网格 |
| 地面网格 | 白 @0.03，仅 L0 | 禁高对比科技网格 |

### 9.3 硬规则

- **V-1 不新增色相。** 3D 的「层次」只能靠明度、尺寸、雾、边亮度；出现第五种彩色即为违规。
- **V-2 不用后处理。** 禁 bloom / 景深 / 运动模糊 / SSAO / 阴影贴图——与「零投影、明度阶梯」的 v7 气质冲突，且吃掉集显预算。
- **V-3 标签是 2D。** 每帧投影锚点，在视口上方 UI 层摆 label（同屏 ≤ 24，按距离与选中排序）；走 `Ellipsis` 与统一 tooltip。
- **V-4 视口最高明度 ≤ `text_dim`。** 3D 不能比正文更亮，否则注意力被视口吸走。
- **V-5 降质必须可见。** 任何降级（分辨率 ×0.75、关雾、裁边、停渲染）在 HUD 角标显示档位，禁止静默降质。
- **V-6 主题热驱动。** 材质色来自 `Theme`（订阅变更或每帧取），禁止 spawn 时烘焙。

### 9.4 令牌增量

| 增量 | 类型 | 说明 |
|:---|:---|:---|
| `space::FOG_START / FOG_END` | 常量 | 深度雾比例（相对相机距离） |
| `space::LOD_DIST` | 常量表 | L0/L1/L2 的距离阈值 |
| `space::LABEL_MAX` | 常量 | 同屏标签上限 24 |
| `size::VIEWPORT_MIN_H` | 新常量 | 视口最小高度（低于则退回并排） |
| `motion::*` | 新模块 | 见 §十 |

---

## 十、动效令牌（2026-10-19 增补）

v8 此前没有动效层，而 3D 相机、模态进出、hover 都需要统一节拍。补一层最小令牌，避免每个模块自写 `Timer`。

| 令牌 | 值 | 用途 |
|:---|:---|:---|
| `motion::INSTANT` | 0ms | reduced motion 下的所有相机位移 |
| `motion::FAST` | 90ms | hover / 高亮 |
| `motion::BASE` | 120ms | 选中、焦点环、按钮态 |
| `motion::PANEL` | 200ms | 抽屉 / 聚焦进出 / 浮层淡入 |
| `motion::CAMERA` | 320ms | 相机聚焦飞行 |
| `motion::LOD` | 480ms | 层级跳转 |
| `motion::PULSE` | 1600ms | 运行中呼吸（唯一循环动画） |

曲线：全部 `ease-out`，相机与 LOD 用 `cubic ease-in-out`。

规则：

- **M-1 唯一循环动画是「运行中」脉冲。** 禁止自动旋转、待机漂浮、呼吸光晕（陪伴窗口除外，它是独立窗口）。
- **M-2 无惯性。** 相机与滚动松手即停；不做滑行、不回弹。
- **M-3 不做弹跳。** 选中态用描边与明度，不用 scale bounce（与「克制」气质冲突）。
- **M-4 reduced motion 是全局布尔。** 系统开启时：相机瞬移、脉冲改静态描边、面板进出 120ms 淡入淡出。令牌挂在设置里，不是 3D 私有开关。
- **M-5 动效不承担信息。** 任何必须靠动画才能理解的状态变化，都要同时有静态表达（颜色/图标/文本）。
