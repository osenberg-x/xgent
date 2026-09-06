# UI v7（Linear 视觉基准）重构实现方案

> 状态：待实施（v1.7，七轮自查修订，修订记录见 §15）｜ 关联：[ADR-0014](../decisions/0014-ui-视觉基准采用-linear-设计系统-v7-原型.md) ｜ 视觉蓝本：[ui-prototype-v7.1.html](../design/ui-prototype-v7.html) ｜ 规范来源：`.agents/skills/design-md/library/linear.app/DESIGN.md`（经 [design-md skill](../../.agents/skills/design-md/SKILL.md) 映射）

---

## 1. 目标与非目标

### 1.1 目标

把 v7.1 原型的视觉语言与信息架构落到 Bevy 实现（`xgent_ui` + `xui`），达成：

1. **视觉对齐**：全部界面配色、排版、圆角、边框、间距符合 Linear 规范（ADR-0014 含五项已裁剪）。
2. **信息架构对齐**：布局调整为 `顶栏(52) / 图标轨(52) + 会话区(flex) + 分隔条(6) + 上下文面板(默认 720) / 状态栏(32)`，右侧面板可拖拽调宽、可折叠。
3. **可移植性**：零渐变、零 box-shadow 依赖（浮层用「描边环 + 深黑底」近似），全部色值收敛进 `Theme`，为后续主题化（亮色、K-01）留好结构。
4. **交互补齐**：hover 反馈全量化、消息操作栏、面板 tooltip/badge、双击复位拖拽等原型交互。

### 1.2 非目标（明确不做，防止范围膨胀）

- **亮色主题**：Theme 结构预留能力，但本期只实现暗色预设（延续 `theme.rs` 现状「MVP 仅暗色」约定，亮色为 P1）。
- **搜索抽屉 / Git 抽屉**：依赖 F-05（检索）、F-10（Git），本期不做；图标轨不放对应按钮。文件树抽屉、历史抽屉**做**（现有功能迁壳）。
- **搜索/git 文件徽章（M/A/U）**：依赖 git 数据，不做，留接口。
- **宠物/陪伴逻辑**：companion 按钮仅视觉占位 + 本地开关状态（xgent_pet 为 P1）。
- **通用动画框架**：不做 tween 库，只实现方案列出的 6 处具体动效。
- **xui 公共组件库扩展**：新视觉组件放 `xgent_ui::kit`（依赖 Theme），xui 保持「纯 bevy + xui_i18n」不动（仅扩展 EditorTheme 注入字段）。

## 2. 现状基线（调研结论）

现状代码事实（详细调研数据见本文附录 A）：

| 维度 | 现状 | v7.1 目标 | 差距等级 |
|---|---|---|---|
| 布局 | 顶栏 48 / 活动栏 48 / 文件面板 240 常驻 / 会话 / 侧视图(编辑器/终端，默认隐藏) / 状态栏 28 | 顶栏 52 / 图标轨 52 / 会话 / 分隔条 6 / 上下文面板 720(预览/差异/终端) / 状态栏 32 | **结构级** |
| 主题 | `Theme::dark()` 唯一，slate+emerald 绿 | Linear 近黑 + 靛紫，四级文字、明度阶梯 | **重写级** |
| 字体 | 系统 Menlo 全局替换（`xgent_app/startup.rs:49`），无字重/字距/特性 | Inter 可变字体 + FontWeight(510/590) + cv01/ss03 + mono 分离 | **新增系统** |
| 图标 | emoji 文本（📁📝🖥⚙🕐🔍⚙ 等） | 单色矢量图标（tint 染色） | **新增系统** |
| hover | 仅 file_panel 有 hover，其余瞬时无反馈 | 全交互元素 hover（瞬时变色起步） | 补齐 |
| 拖拽 | resize.rs 已有完整手柄机制（6px、Interaction+AccumulatedMouseMotion+钳制） | 复用同机制，右侧手柄对齐 v7.1 视觉与默认值 720 | **低成本复用** |
| overlay | 命令面板/确认弹窗/会话历史/设置面板已有 | 视觉对齐 + 会话历史改抽屉 | 中 |
| 动效 | 3 处逐帧（状态点脉冲/流式光标/红边闪烁） | 保留 3 处 + 新增 3 处（pill 脉冲/光标块/companion 环） | 小 |

## 3. 信息架构对齐（原型 IA ↔ 现有 IA）

原型的浏览器 IA 与现状 Bevy IA 有结构差异，对齐决策如下：

| 原型元素 | 现有对应物 | 决策 |
|---|---|---|
| 顶栏（brand / 面包屑 / 新建会话 / agent pill / model / 主题 / 命令） | top_bar.rs（logo/provider pill/按钮组） | 保留模块，按 §8.2 重排元素 |
| 图标轨 rail（对话/文件/搜索/Git/历史/终端/插件 + companion） | activity_bar.rs（48px emoji 导航） | 改造为 rail：52px、矢量图标、tooltip、badge、companion 占位；**不放搜索/Git 按钮**（非目标） |
| 上下文 chips 条（会话上下文文件列表） | 无（`@` 引用仅编辑器内） | **新增** `context_scope.rs`；MVP 只读展示编辑器已打开文件（agent 侧暂无上下文文件集概念，语义依据见 §8.10） |
| 会话流 + 工具卡 + 输入区 | chat_panel.rs + tool_panel.rs | 保留模块，视觉重做（§8.4/8.5） |
| 右侧面板 tabs：预览/差异/终端 | side_view（编辑器/终端双 tab） | 改造为三分页：预览=现编辑器（改名 PreviewTab）、**差异=新增**（只读行级 diff，复用 confirm_dialog 的 line_diff 算法，数据=当前 buffer vs 磁盘）、终端=现有 |
| 文件树抽屉（overlay） | file_panel.rs（240px 常驻） | **抽屉化**：改为左侧 overlay drawer（320px），由 rail 文件按钮/快捷键开关；模块保留，渲染位置与样式改 |
| 历史抽屉 | session_history.rs（居中弹窗） | 改为抽屉样式（结构上仍是 overlay，视觉对齐 drawer） |
| 欢迎仪表盘 | chat_panel 空态（简单提示） | **新增** welcome 视图（快捷卡 3 枚 + 最近会话，复用 `SessionSummary` 数据） |
| 设置 | settings_panel.rs（原型未画） | 保留现有功能，仅视觉对齐（按钮/输入框/圆角） |
| 状态栏 | status_bar.rs | 视觉对齐 + 可点击项 |
| 命令面板 / 确认弹窗 | command_palette.rs / confirm_dialog.rs | 保留，视觉对齐 |

**布局节点树目标形态**（替换 layout.rs 现结构）：

```
UiRoot (Column, bg=theme.bg)
├── TopBarMarker            (h=52, bg=surface, border-bottom=line)
├── MainAreaMarker          (Row, flex_grow=1)
│   ├── RailMarker          (w=52, bg=surface, border-right=line)      ← 原 ActivityBarMarker
│   ├── ChatAreaMarker      (flex_grow=1, bg=bg)                        ← 原 ChatPanelMarker
│   ├── ContextResizerMarker( w=6, 透明 Button, hover/拖拽高亮 )        ← 原 RightResizeHandle
│   └── ContextPanelMarker  (w=PanelWidths.side_view, bg=surface)       ← 原 SideViewMarker
└── StatusBarMarker         (h=32, bg=surface, border-top=line)
```

抽屉/drawer 与 overlay 不在网格内，为 Absolute 悬挂节点（同现有 command_palette 模式）。

**M2 过渡形态（重要，防时序矛盾）**：文件面板抽屉化发生在 M5，因此 M2 的重排保留文件面板列，为五列过渡结构 `Rail(52) / FilePanel(240) / ChatArea(flex) / Resizer(6) / ContextPanel(720)`；M5 完成抽屉化后才收为上面的四列目标形态——避免文件面板在 M2–M5 之间无处安放。

## 4. 设计令牌：`Theme` v3

`theme.rs` 整体重写。字段命名尽量保持旧名以减少机械改动，语义值全部换成 v7.1；新槽位按 ADR-0014 清单补齐。

### 4.1 新 `Theme` 结构定义（暗色预设，完整）

```rust
/// Linear 视觉基准主题（v7.1，暗色预设）。规范见 doc/decisions/0014。
#[derive(Resource, Debug, Clone, Copy)]
pub struct Theme {
    // ===== 表面：明度阶梯 =====
    /// L0 会话画布（最深层）          #08090A
    pub bg: Color,
    /// L1 顶栏/图标轨/上下文面板       #0F1011
    pub surface: Color,
    /// L2 浮层/下拉/命令面板           #191A1B
    pub elevated: Color,
    /// 代码/终端/预览底（比画布略升）   #0D0E10（原 deep 改名，唯一调用点 confirm_dialog 同步改）
    pub code_bg: Color,

    // ===== 交互面（半透明白叠加，亮色主题时换半透明黑）=====
    /// 卡片/工具卡底                   rgba(255,255,255,0.02)（新增；原 panel 字段删除）
    pub subtle: Color,
    /// 输入类底                        rgba(255,255,255,0.02)
    pub input_bg: Color,
    /// 悬停步进                        rgba(255,255,255,0.05)（原 hover_bg 更名）
    pub hover: Color,
    /// 按下/选中步进                   rgba(255,255,255,0.08)
    pub active: Color,
    /// 图标底/小徽标                   rgba(255,255,255,0.06)（原 handle_active 更名复用）
    pub icon_bg: Color,

    // ===== 边框：半透明白丝线 =====
    /// 弱分隔（面板边界）              rgba(255,255,255,0.05)
    pub line: Color,
    /// 标准边框                        rgba(255,255,255,0.08)
    pub border: Color,
    /// 悬停边框                        rgba(255,255,255,0.14)
    pub border_hover: Color,

    // ===== 文字：四级灰阶 =====
    /// 标题/主文字                     #F7F8F8（禁用纯白）
    pub text: Color,
    /// 正文/次要                       #D0D6E0
    pub text_dim: Color,
    /// 弱化/placeholder                #8A8F98
    pub text_muted: Color,
    /// 最弱（时间戳/行号/禁用）         #62666D
    pub text_faint: Color,

    // ===== 强调色：唯一彩色系统 =====
    /// 主色（CTA/主按钮/品牌块）        #5E6AD2
    pub accent: Color,
    /// 交互强调（链接/active/选中）     #7170FF
    pub accent_interactive: Color,
    /// 强调悬停                        #828FFF
    pub accent_hover: Color,
    /// 强调薄底                        rgba(94,106,210,0.14)（原 accent_bg 更名复用）
    pub accent_bg: Color,
    /// 强调辉光（focus 环/拖拽条高亮）   rgba(113,112,255,0.16)
    pub accent_glow: Color,
    /// 强调色上的文字                   #FFFFFF
    pub accent_text: Color,

    // ===== 状态色（Radix 深色阶，外推值见 ADR-0014 裁剪#1）=====
    pub st_pending: Color,   // 待确认    #FFB224
    pub st_running: Color,   // 执行中    #7170FF
    pub st_ok: Color,        // 成功      #30A46C
    pub st_fail: Color,      // 失败      #E5484D
    pub st_deny: Color,      // 已拒绝    #E5484D（与 fail 同色，可调）
    /// 状态薄底（tint 0.12）：success_bg / warning_bg / error_bg / info_bg
    pub st_ok_bg: Color,        // rgba(48,164,108,0.12)
    pub st_pending_bg: Color,   // rgba(255,178,36,0.12)
    pub st_fail_bg: Color,      // rgba(229,72,77,0.12)
    pub st_info: Color,         // #0091FF
    pub st_info_bg: Color,      // rgba(0,145,255,0.12)

    // ===== 陪伴暖色（全界面唯一暖色例外，ADR-0014 裁剪#3）=====
    pub warm: Color,         // #FFB224（亮色主题 #D97706）
    pub warm_bg: Color,      // rgba(255,178,36,0.15)

    // ===== 反色浮层（tooltip/toast，两主题恒暗底）=====
    pub tooltip_bg: Color,   // #28282C
    pub tooltip_text: Color, // #F7F8F8

    // ===== 半透明覆盖 =====
    /// 抽屉/弹窗遮罩 rgba(0,0,0,0.5)（原 overlay 更值）
    pub overlay: Color,

    // ===== 代码语法（冷调低饱和）=====
    /// 代码正文 #D0D6E0 / 关键字 #B3A5FF / 函数 #7FB3FF / 字符串 #56C08D
    /// 数字 #E2A35C / 类型 #6FD3C7 / 注释 #62666D / 标点 #8A8F98
    pub code_text: Color,
    pub kw: Color,
    pub fn_: Color,
    pub str_: Color,
    pub num: Color,
    pub ty: Color,
    pub com: Color,
    /// 标点——原型 CSS 的 --pn 因无消费者而删，但 Bevy 侧编辑器高亮链（§5.4）是真实消费者，必须保留
    pub punc: Color,

    // ===== 排版 =====
    /// 正文基准字号（逻辑像素）
    pub font_size: f32,      // 14.0（原 13.5）
}

/// 主题预设入口。MVP 仅暗色；`light()` 预留（P1，结构已按「亮色时半透明叠加换黑色系」设计）。
impl Theme {
    pub fn dark() -> Self { /* 逐字段按上行注释赋值 */ }
}
impl Default for Theme { fn default() -> Self { Self::dark() } }
```

### 4.2 v7.1 CSS 变量 → Theme 字段映射总表（实现时逐项对照）

| CSS 变量 | 值（暗色） | Theme 字段 | 对应旧字段/处置 |
|---|---|---|---|
| `--bg-canvas` | `#08090A` | `bg` | 保留，换值 |
| `--bg-surface` | `#0F1011` | `surface` | 保留，换值；**`bar` 字段删除**（实际引用点：confirm_dialog×2、terminal×3、file_panel×1，均为头/底栏底色语义，逐点改 `surface` 或 `elevated`） |
| `--bg-elevated` | `#191A1B` | `elevated` | 保留，换值 |
| `--bg-code` | `#0D0E10` | `code_bg` | 原 `deep` 更名（仅 confirm_dialog.rs:244 一处调用） |
| `--bg-subtle` | `white@0.02` | `subtle` | **新增**（原 `panel` 语义由 `surface` 承担，`panel` 字段删除，chat_panel/file_panel/tool_panel/terminal 的 `panel` 引用改 `surface`） |
| `--bg-input` | `white@0.02` | `input_bg` | 新增 |
| `--bg-hover` | `white@0.05` | `hover` | 原 `hover_bg` 更名 |
| `--bg-active` | `white@0.08` | `active` | 新增 |
| `--bg-icon` | `white@0.06` | `icon_bg` | 原 `handle_active` 更名（resize.rs:173-186 同步） |
| `--border` | `white@0.08` | `border` | 保留，换值 |
| `--border-light` | `white@0.05` | `line` | 保留，换值 |
| `--border-hover` | `white@0.14` | `border_hover` | 新增 |
| `--t0~t3` | 四级灰 | `text / text_dim / text_muted / text_faint` | 原 text_dim/text_muted 保留，`text_faint` 新增 |
| `--accent` 系列 | 靛紫三值 | `accent / accent_interactive / accent_hover / accent_bg / accent_glow / accent_text` | `accent_bg` 保留更值，其余新增 |
| `--success/warning/error/info`(+`-bg`) | Radix 深阶 | `st_ok/st_pending/st_fail/st_info`(+`_bg`) | 换值 + 新增 `_bg` 组 |
| `--warm/--warm-light` | 琥珀 | `warm / warm_bg` | 新增 |
| `--tooltip-*` | `#28282C/#F7F8F8` | `tooltip_bg / tooltip_text` | 新增 |
| `--overlay` | `black@0.5` | `overlay` | 保留，换值 |
| `--kw/fn/str/num/ty/com` | v7.1 冷调 | `kw/fn_/str_/num/ty/com` | 保留字段，换值（本期起真正被消费，见 §5.4） |
| `--code-text` | `#D0D6E0` | `code_text` | 新增 |

**删除字段**（0 使用或被替代）：`bubble_user`、`bubble_assistant`（0 引用）、`bar`（引用点改 `surface`）、`panel`（引用点改 `surface` 或 `subtle`，逐点判定见 §8）、`deep`（更名 `code_bg`）、`handle_active`（更名 `icon_bg`）、`hover_bg`（更名 `hover`）。

### 4.3 字号与尺寸常量

```rust
/// 排版阶梯（原型唯一字号集合；新界面禁止就地发明字号；行高档见 line_height）
pub mod type_scale {
    pub const DISPLAY: f32 = 24.0;  // welcome 标题（配 LetterSpacing -0.29、行高 TIGHT）
    pub const H3: f32 = 15.0;       // 弹窗标题/顶栏品牌（行高 UI）
    pub const BODY: f32 = 14.0;     // 消息正文/输入框（行高 BODY）
    pub const BODY_SM: f32 = 13.0;  // 次要正文/按钮（行高 CTRL）
    pub const SMALL: f32 = 12.5;    // 工具名/按钮小字/pill（行高 CTRL）
    pub const CAPTION: f32 = 12.0;  // chips/参数/代码（行高 CTRL）
    pub const MICRO: f32 = 11.0;    // 时间戳/元信息/kbd（行高 UI）
    pub const TINY: f32 = 10.0;     // 大写分区标签/徽标（行高 UI）
    pub const MONO: f32 = 12.0;     // 代码（行高 CTRL；终端实际 font-2、行高 TERM）

    /// 行高档（LineHeight 是 Text 必带组件，text.rs:192；原型正文 1.6/1.7、UI 1.4-1.5、代码 1.5）
    pub mod line_height {
        pub const TIGHT: f32 = 1.2;
        pub const UI: f32 = 1.4;
        pub const CTRL: f32 = 1.5;
        pub const BODY: f32 = 1.6;
        pub const TERM: f32 = 1.6;
    }
}

/// 尺寸常量改动（theme::size 模块）
pub const TOP_BAR_H: f32 = 52.0;        // 48 → 52
pub const STATUS_BAR_H: f32 = 32.0;     // 28 → 32
pub const RAIL_W: f32 = 52.0;           // 原 ACTIVITY_BAR_W 48 → 52（改名）
pub const CONTEXT_W_DEFAULT: f32 = 720.0; // 新：上下文面板默认宽（原 CHAT_SIDEBAR_W 380 弃用）
pub const CONTEXT_W_MIN: f32 = 380.0;     // 新
pub const CHAT_MIN: f32 = 520.0;          // 原 240 → 520（拖拽钳制用）
pub const CONTEXT_TABS_H: f32 = 38.0;     // 新：面板页签条高
pub const DRAWER_W: f32 = 320.0;          // 新：抽屉宽
pub const RESIZER_W: f32 = 6.0;           // 新：分隔条宽（与 resize.rs handle_bundle 现值一致）
// 间距 space 模块维持现状（4/8/12/16/20/24/32 已覆盖原型 8px 栅格）
// 圆角新增常量：RADIUS_MICRO=2 / SMALL=4 / CTRL=6 / CARD=8 / PANEL=12（原型 9999 胶囊用 BorderRadius::MAX）
```

## 5. 字体与排版系统

### 5.1 资产

- **UI 字体**：Inter 可变字体（wght 100–900，OFL 许可），文件 `crates/xgent_app/assets/fonts/Inter-Variable.ttf`（从 rsms/inter release 取 `InterVariable.ttf`），**连同 `OFL.txt` 许可文件一并入库**（fonts/ 目录）。仓库当前无任何字体资产（字体来自系统 Menlo），这是首个入库字体。
- **等宽字体**：继续用现有系统字体机制（macOS Menlo，`xgent_app/src/startup.rs:49-67` 的 `load_system_font`），语义改名 `load_mono_font`。跨平台再议 vendor JetBrains Mono（非本期）。

### 5.2 加载与注入（改造 `xgent_app/src/startup.rs`）

```rust
/// UI 字体句柄资源（xgent_ui 消费；放 xgent_ui::fonts 更合适，见下）
#[derive(Resource)]
pub struct UiFonts {
    pub ui: Handle<Font>,    // Inter Variable
    pub mono: Handle<Font>,  // 系统 Menlo（现状机制保留）
}
```

- 新模块 `xgent_ui/src/fonts.rs`：定义 `UiFonts` Resource + `FontPlugin`（Startup 加载 Inter，失败时 `warn!` 并回退 `Handle::default`，**不 panic**——库代码无 unwrap 约定）。`xgent_app` 改为注册 `FontPlugin` 并保留 Menlo 加载注入 `mono`。
- **资产根目录前提（v1.6 勘误补，阻断级）**：工程未配置 `AssetPlugin` 资产根（默认 CWD 相对 `assets/`，从 workspace 根运行时该目录不存在），且现有代码全部绕开 AssetServer（字体直读 startup.rs:56-59、插件目录 main.rs:182 用 `CARGO_MANIFEST_DIR`）。`main.rs` 的 `AssetPlugin` 须设 `file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets")`，此后 Inter 与图标（§6）统一走 AssetServer；Menlo 为系统文件，保持直读注入。
- 现有「全局默认字体替换」（`AssetId::default()` 覆盖）保留：把默认替换为 Inter，使未显式指定字体的 `TextFont` 自动获得 Inter；mono 场景显式传 `fonts.mono`。
- `TextFont::font` 字段类型是 `FontSource`（handle / family name / generic 均可，`text.rs:383`），handle 直接 `Into`（`text.rs:348`）。

**CJK 回退（关键前提，勿省略）**：Inter 不含中文字形，中文渲染依赖 fontique 的系统字体回退，其前提是 bevy feature `system_font_discovery`——workspace **已启用**（根 `Cargo.toml:7`，且项目已处理过 parley CJK 分词问题）；现状「Menlo 默认 + 中文显示正常」即回退链路可用的实证。M1 强制 spike：默认字体换 Inter 后，验证中文回退（macOS 为 PingFang）与中英混排的渲染、换行、字号观感正常，**通过后才允许铺开各模块**（验收项见 §12）。

### 5.3 字重 / 字距 / 特性用法（已核实 bevy 0.19 API）

```rust
// 字重：TextFont.weight 接受任意 1–1000（text.rs:396,597），510/590 直接可用
TextFont { font: fonts.ui.clone().into(), font_size: FontSize::Px(type_scale::SMALL), weight: FontWeight(510), ..default() }

// OpenType 特性（text.rs:406）：builder 方法是 enable（text.rs:861，非布尔特性才用 set）
TextFont::default().with_font_features(FontFeatures::builder()
    .enable(FontFeatureTag::new(*b"cv01")).enable(FontFeatureTag::new(*b"ss03")).build())

// 负字距：LetterSpacing 是 Text 的必带组件（text.rs:192 require，经 pipeline.rs 桥接 parley）
// 统一携带、非展示档传 0.0（Rust 无法条件组装 Bundle）
( LetterSpacing(-0.29), ) // welcome 24px 标题；正文/控件档 LetterSpacing(0.0)（规范：<24px 趋于 normal）
```

**落地约束**（防 atlas 膨胀）：`TextFont` 的 atlas 按「字体 handle × 字号 × weight」组合生成（`text.rs:390-391` 注释明示）。全项目锁定：**字号只用 `type_scale` 8 档、字重只用 400/510/590 三值**，组合数可控。为此新增 `xgent_ui::fonts` 里的构造器函数统一出口：

```rust
/// 全部业务文本经此构造（杜绝散落 TextFont 字面量）；行高随档位显式传入
pub fn ui_text(size: f32, weight: u16, color: Color, line_height: f32) -> impl Bundle;   // Text + TextFont(ui,weight,features) + TextColor + LetterSpacing(0/-0.29) + LineHeight
pub fn mono_text(size: f32, color: Color, line_height: f32) -> impl Bundle;              // Text + TextFont(mono,400) + TextColor + LineHeight
```

各模块现有 `Text::new(...)` + 裸 `TextFont` 调用点分批迁移到此构造器（**排期：M2 起随 §8 各模块改造同步迁移**；M6 收尾时全仓清点，不得残留裸 `TextFont` 字面量）。

- **span 级背景**：内嵌底色（inline code 等）用 `TextBackgroundColor`（text.rs:1089，per-span 组件，"background color of the text for this section"）——inline code = `TextSpan` + mono/`TextColor` + `TextBackgroundColor(icon_bg)`；工程内 span 模式已在 xui 编辑器验证（render.rs:86）。

### 5.4 语法高亮消费链（顺带修复现状「Theme 语法色 0 消费」）

现状：`xui/text_editor/highlight.rs:224-240` 的 `span_color_for` 硬编码 12 色（VSCode Dark+），`Theme.kw/...` 全项目 0 引用。改造：

1. `xui::EditorTheme` 增加语法色字段组（`kw/fn_/str_/num/ty/com/punc/plain`，`Option` 缺省 None 时回落现硬编码——保持 xui 可独立使用）。
2. `xgent_ui/editor/mod.rs:250-262` 的 `sync_editor_theme` 注入 `Theme` 对应值。
3. `span_color_for` 改为优先读 `EditorTheme`。
4. 上下文面板预览/diff 的静态着色（非 tree-sitter）直接用 `Theme.kw` 等正则级高亮（MVP 可先整段 `code_text` 单色，语法色列为 P1 打磨项——**本期预览按单色 code_text 实现**，与原型视觉差距很小）。

## 6. 图标系统（emoji → 单色矢量）

### 6.1 方案：构建期 SVG → PNG，运行时 `ImageNode` + tint 染色

- bevy_ui 无 SVG 渲染能力；v3 已有图标源文件 `doc/design/icons/*.svg`（34 个，见 `icon-preview.html`）与历史切图 `ui-v3-icons/`。
- 新增脚本 `doc/design/icons/export_png.py`（cairosvg，dev 依赖不入 workspace）：`@1x/@2x` 双倍导出到 `crates/xgent_app/assets/icons/{name}@2x.png`（统一 2x，UI 按 14/16/18/20/22px 显示）。
- **导出必须替换描边色（必修坑）**：现有 SVG 源为 `stroke="currentColor"`（渲染器默认解析为**黑**），而 `ImageNode.color` 是**乘法染色**——黑像素乘任何色仍为黑，整个「白图标 + tint」方案会静默失效。脚本渲染时统一替换为 `stroke="#FFFFFF"` 再出 PNG。验收：抽查导出 PNG 描边为白、底透明，运行时染色后非黑。
- 缺失图标按原型补画（线性 2px stroke、24 viewBox，与现有 icons 风格一致）：`agent-x`(品牌 X 可继续用 Text 字符)、`spark`(companion 用现 star)、`panel-right`、`chevron-down`、`diff`、`shield`、`dollar`、`circle-dot` 等——逐个盘点见 §8.3 清单。
- 图标经 AssetServer 加载，依赖 §5.2 的资产根配置（v1.6 勘误：工程现无可用资产根，须先配 `AssetPlugin.file_path`）。

### 6.2 运行时封装（`xgent_ui/src/kit.rs`）

```rust
/// 图标资源表：启动加载 assets/icons/ 全部 PNG
#[derive(Resource)]
pub struct IconAssets { pub map: HashMap<&'static str, Handle<Image>> }

/// 单色图标节点：ImageNode + color 染色（ImageNode.color 支持乘色）
pub fn icon(icons: &IconAssets, name: &str, px: f32, color: Color) -> impl Bundle;
```

禁止再新增 emoji 图标（现状 activity_bar.rs 📁📝🖥⚙、top_bar.rs 🕐🔍⚙、tool_panel 🔧 等，随各模块任务替换）。

## 7. 交互/动效规范

### 7.1 hover（全量化，数据驱动单系统）

- **通用组件 + 单一全局系统**——全 UI 可交互元素约 30 个，禁止「每个组件写一个专用系统」的重复劳动：

```rust
/// 挂在任意可交互节点上：hover/按下自动换底色与边框色（单系统遍历 Changed<Interaction>）
#[derive(Component)]
pub struct HoverTint { pub base_bg: Color, pub hover_bg: Color, pub base_border: Color, pub hover_border: Color }
```

- 单系统三态规则：`hover → hover_bg/hover_border`，`pressed → theme.active`，`none → base`。取值一般 `base_bg=subtle、hover_bg=hover、hover_border=border_hover`，由组件数据驱动，无需逐组件写系统；file_panel 现有 `update_file_entry_style`（file_panel.rs:1085-1118）迁入该机制。
- **不做** hover 插值动画（P1 再议帧插值）；特殊态组件（agent pill、rail active、tooltip 延迟）仍走各自专用系统。
- 光标：沿用现状（bevy 0.19 `Interaction` 不改系统光标，与现状一致）。

### 7.2 动效清单（全部逐帧系统，无框架）

| 动效 | 实现位置 | 方式 |
|---|---|---|
| agent pill 脉冲点（thinking/generating） | kit::agent_pill | 仿 status_bar.rs:199-228 正弦 alpha，周期 1.2s/0.8s |
| 流式光标 | chat_panel（现有 709-732 改造） | `▍` 字符 + `accent_interactive`，`t%0.8<0.4` 通断；替换现 `▋` |
| 状态栏状态点脉冲 | status_bar（保留现状） | 不动 |
| 空输入红边闪烁 | chat_panel（保留现状机制） | 边框色换 `st_fail` |
| companion 激活环 | kit::companion | 按钮外圈嵌套 2px 描边节点，每帧 scale 1.0→1.3 + alpha 0.4→0 循环（2s）；P2 装饰，最后做 |
| 拖拽分隔条高亮 | resize.rs | hover=`accent_glow` 底 + 2px `accent_interactive` 线（v7.1 视觉），无动画 |

### 7.3 浮层深度（替代 box-shadow）

- 命令面板/确认弹窗/抽屉/toast/tooltip/dropdown：`BackgroundColor(elevated)` + `BorderColor(border)` + `BorderRadius`；遮罩 `theme.overlay`。不再需要额外描边环（elevated 与 surface 本身差一级明度，原型同款效果）。

## 8. 模块逐个改造清单

> 每项含：改动点 → 验收标准。涉及文件均在 `crates/xgent_ui/src/`（另注明者除外）。

### 8.1 theme.rs — 重写
- 按 §4 全量重写；`space`/`size` 常量更新；新增 `type_scale`、圆角常量。
- 同步编译修复：`bar/panel/deep/hover_bg/handle_active` 的全部引用点（top_bar、status_bar、chat_panel、file_panel、tool_panel、terminal、confirm_dialog、layout、resize）。
- 提交拆分：① 新增 v3 字段（旧字段暂留并存）→ ② 各模块引用迁移 → ③ 删除旧字段（§4.2 清单），控制单提交爆炸半径。
- **验收**：`cargo check -p xgent_ui -p xgent_app` 过；`cargo test -p xgent_ui` 过（含 §12 新测试）。

### 8.2 top_bar.rs — 顶栏重排（52px）
- 高度 52（`size::TOP_BAR_H`）；`bg=surface`、底边 `line`。
- 元素左→右：① 品牌块 28×28 `accent` 底白字 "X"（圆角 8）+ "XGent"（H3/590）；② 项目名（`text_dim`，本期纯文本非下拉，点击 toast 预留）；③ 「新建会话」ghost 按钮（`subtle` 底 + `border` + 6px 圆角 + plus 图标 14px，hover→`hover` 底）；④ spacer；⑤ **agent pill**（新增，见 §9，状态源已核实——`ConversationStatus` 五态一一映射：Idle→就绪(ok 点)、Thinking→思考(accent 脉冲)、Streaming→生成(accent 脉冲)、ToolRunning→工具(warning)、等待确认→确认(warning)，conversation.rs:298-308；error 态由 `ErrorMessage` 驱动）；⑥ provider/model pill（现有内容，样式改 `subtle` 底胶囊 + 图标块 `accent` 底 16px）；⑦ 主题按钮（本期隐藏或仅命令面板入口，亮色未实现）；⑧ 命令面板按钮（command 图标）；⑨ **设置按钮（保留现状）**——top_bar.rs:35 标记与行为不变，是设置的常驻入口，rail 不放设置（现状的 🕐 历史按钮移除，迁 rail 历史抽屉 §8.3）。
- emoji 全部替换为 kit::icon。
- **验收**：截图对比原型顶栏；五类按钮交互保持（top_bar.rs:266-306 逻辑不动，仅样式层重排）。

### 8.3 activity_bar.rs → rail（52px 图标轨）
- 宽 52（`RAIL_W`）；`bg=surface`、右边框 `line`；按钮 40×40 圆角 6。
- 按钮（自上而下）：对话 / 文件 / **历史** /（分隔线）/ 终端；（底部 spacer）/ **展开面板钮**（仅上下文面板折叠或响应式收起时显示，点击恢复——对应原型 context-expand，是 §8.8 响应式折叠的恢复入口）/ **companion**（40 圆形 `warm` 底深色 star 图标，激活态 `warm_bg` 外环）。**不加搜索/Git/插件按钮**（非目标）。
- active 态：`accent_bg` 底 + `accent_interactive` 图标 + 左侧 3px 圆角竖条（现 border-left 模拟改独立 3px 节点，activity_bar.rs:222-232）。
- **tooltip**（kit §9.3）：hover 500ms 后浮现于按钮右侧（`tooltip_bg` 底 + 名称 + kbd 小徽标）。
- **badge**（kit §9.3）：右上角 14px 圆点数标（本期用例：历史未读数，恒空可先不挂）。
- 图标替换：chat/folder/history/terminal/star（复用 doc/design/icons 现有 svg，缺的在 §6.1 补）。
- 交互逻辑保留（activity_bar.rs:155-207 视图切换/面板开关），仅视图集合调整（搜索/Git 移除）。
- **验收**：截图对比 rail；切换/折叠行为不回归。

### 8.4 chat_panel.rs — 会话区
- 移除「视图标签条」（VIEW_TABS_H 36px 那排——由新增 context_scope 取代，见 §8.10）。**注意**：标签条右侧挂有 `ConversationInfoMarker`，其真实内容是**轮数 + token 数**（chat_panel.rs:739-758，非会话 id）——token 数与状态栏 TokenUsage 纯重复，**舍弃**；轮数并入状态栏会话 id 位（形如 `#a3f2 · 8 轮`，§8.9 承接）。
- 消息列表（ScrollArea+StickToBottom 保留）：
  - 列宽：消息组容器 `max_width=820` 居中（原型 .msg-group）。
  - 消息头：28×28 圆角 6 头像（user=`icon_bg` 底 "你"/agent=`accent` 底 "X"）+ 角色（SMALL/510）+ 时间（MICRO/text_faint）。
  - 正文：BODY/400 `text_dim`；inline code = `TextSpan` + mono + `accent_interactive` + **`TextBackgroundColor(icon_bg)`**（per-span 背景，机制见 §5.3）；**代码块**（现有实现若有）= `code_bg` 底 + `border` + 圆角 6 + mono CAPTION `code_text`。
  - **消息操作栏**（新增）：agent 消息 hover 时右上浮现 26px 方钮组，`elevated` 底 + `border`。按钮：**复制 + 重试**（复用 `RetryMessage`）。复制基于 **bevy 自带剪贴板**——已核实 `Clipboard::set_text`（bevy_clipboard/lib.rs:299）、`ClipboardPlugin` 在 DefaultPlugins 自动注册（default_plugins.rs:69，app 用 DefaultPlugins，main.rs:202）、workspace 已启用 `system_clipboard`，零新依赖。**实体稳定性已核实**：流式 delta 累加到既有消息节点（chat_panel.rs:420-422），仅 compact/clear 全量重建列表（:794,:800 `despawn_related`）——操作钮持久附着于消息实体即可，无流式期间被冲掉的问题；消息容器以 `Button` + `Interaction` 做 hover 检测。
  - **回到底部浮钮**（新增）：列表滚动偏离底部 >200px 时浮现于输入区上方居中（`elevated` 底 + `border`），点击滚回底部；`StickToBottom` 现机制保留。
  - **welcome 空态**（新增子模块 `welcome.rs`）：会话空时显示——64px `accent` 圆角 12 品牌块、DISPLAY 标题、3 枚快捷卡（`subtle` 底+`border`，图标 tint 用 `st_info_bg/accent_bg/st_ok_bg`）、最近会话 3 条（复用 `ListSessionsMessage` 数据；点击走 `RestoreSessionMessage`）。**进入空态时发送 `ListSessionsMessage` 拉取列表**（`SessionListMessage` 回填）。发首条消息即隐藏。
  - **qa chips 行**（输入区上方）：现有快捷键提示工具栏（chat_panel.rs:220-249，现状为**纯静态文本**、无点击行为）改造为胶囊 chips（9999 圆角、透明底+`border`、hover→`accent_bg`+`accent_interactive`），文案走 i18n；**点击填入输入框是新增行为**——可行性已核实：经 `ChatInputMarker`（chat_panel.rs:33,215）定位输入框，`EditableText.editor` 为 pub `PlainEditor`（editing.rs:117），可程序化赋值（set_text/driver 插入，注意 generation 刷新触发重排）。
- 输入卡：`input_bg` 底 + `border` + 圆角 12，focus 环用 **`Outline` 组件**（bevy_ui 内建 `ui_node.rs:2369`，字段 width/offset/color 已核实，现工程 0 使用——勿用外层节点包边）：focus 时 `Outline` 宽 3px、色 `accent_glow`，边框 `border→accent_interactive`。**Outline 与 BorderRadius 的圆角跟随需 M4 实测**（bevy 历史上有直角渲染问题），不随圆角则回退外层节点方案；底部工具行：模式按钮（`subtle` 底 6px）、安全提示（`st_pending` 图标+文案）、Spacer、Shift+Enter 提示（kbd 小徽章 kit §9.3）、token 计数（MICRO `text_faint`）、发送按钮（`accent` 底 6px 圆角，流式中变 `st_fail` 停止态——现有 abort 逻辑保留）。
- **验收**：流式对话全链路不回归（Delta/Done/Error/Retry 消息处理不动）；截图对比原型消息区。

### 8.5 tool_panel.rs — 工具卡（对话内嵌）
- 卡片：`subtle` 底 + `border` + 圆角 8；head 行 hover→`hover`；展开/折叠逻辑保留（现有 Interaction）。
- head：24px 圆角 4 图标块（read=`st_info_bg/st_info`、write=`st_pending_bg/st_pending`、search=`accent_bg/accent_interactive`、exec=`st_fail_bg/st_fail`）+ 工具名（SMALL/510）+ 参数（mono CAPTION `text_muted` 截断）+ 状态（MICRO 510，ok/pending/fail 色）+ 旋转指示（`▶`→chevron 图标，展开 rotate 90°——ImageNode 旋转需 Transform，MVP 用两枚图标切换或字符 `▸/▾`）。
- 结果条：`st_ok_bg/st_ok`（或 fail）圆角 4，MICRO。
- emoji 🔧 替换为矢量图标。
- **验收**：真实工具调用流（ToolCallMessage/ToolResultMessage）展示不回归。

### 8.6 context panel（editor/ + terminal/ → 三页签）
- 面板头改为 **38px 页签条**（`CONTEXT_TABS_H`）：预览 / 差异 / 终端 三 tab（SMALL/510，active=`accent_interactive` + 底部 2px 线）+ 右侧折叠钮。
- 状态机扩展（事实修正）：`SideViewContent` 实为四态 `{None, Editor, Preview, Terminal}`（editor/mod.rs:62-72，`Preview` 为非代码文件预览，此前调研漏计）——增 `Diff` 变体；「预览」页签 = Editor/Preview 归一承接；rail 终端按钮 → 切到 Terminal 页并展开面板。
- **预览页**：现编辑器主体保留（tab 条、buffer、滚动），外框底改 `code_bg`、`sync_editor_theme` 注入新色（§5.4）。**双 tab 条决策**：面板 38px 页签条 + 编辑器 32px 文件 tab 条会叠两层——MVP 保留两层但编辑器 tab 条降调（高 28px、底色 `code_bg`、去 emoji、仅文件名+关闭钮），合并为单层的最终形态标 P1。
- **差异页**（新增 `editor/diff_view.rs`）：只读行级 diff 列表——数据源=当前编辑 buffer（**已核实 `TextEditor.rope` 为 pub 字段**，text_editor.rs:80，xgent_ui 直读即可，无需改 xui）vs 磁盘文件（io.rs `FileReadRequest` 机制复用）；diff 算法抽取 confirm_dialog.rs:53 的 `line_diff`（调用点 :232）到共享模块；渲染：`code_bg` 底、行号 `text_faint`、add=`str_` 绿、del=`st_fail`、行底 tint；无差异时空态「无未保存更改」（i18n）。
- **终端页**：tab 条改 38px；`term-view` 视觉——底 `code_bg`、正文 mono CAPTION `text_dim`、prompt `st_ok`、命令 `text`；ANSI 256 色调色板保留（output.rs:183-230，终端语义色不受 Theme 管）。
- 拖拽宽度：复用 resize.rs 右侧手柄（§8.8）。
- **验收**：三页签切换、编辑器读写、终端 PTY 全链路不回归；拖拽 380↔(inner-52-520) 钳制生效。

### 8.7 file_panel.rs → 抽屉化
- 渲染位置：从常驻列改为 **左侧 drawer**（Absolute，宽 `DRAWER_W`，`surface` 底 + 右边框 + `overlay` 遮罩，滑入动画可先瞬时）；由 rail 文件按钮 / `filepanel.toggle` 快捷键（shortcuts.rs 现有）开关；新 Resource `FileDrawerOpen(bool)` 替代原常驻逻辑。
- 文件树视觉：条目 6px 圆角、hover=`hover`、选中=`accent_bg`+`accent_interactive`；目录/文件图标矢量化；M/A/U 徽章**不做**（无 git 数据）。
- 预览读（tokio 异步 PreviewReadResult）逻辑不动；预览区随差异页/编辑器承接（点击文件 → 打开 context panel 预览页并加载——**行为变化**：原内嵌预览区取消，改为发 `OpenFileRequest` 走编辑器页签）。
- **验收**：开抽屉→浏览→点文件→context 预览打开，全链路可用。

### 8.8 resize.rs — 分隔条对齐 v7.1
- 右手柄（ContextResizer）：宽 6、透明；hover/拖拽 → `accent_glow` 底 + 居中 2px `accent_interactive` 竖线（现 `handle_active` 纯色高亮改造，resize.rs:173-186）。
- 钳制：`CONTEXT_W_MIN=380`、`CHAT_MIN=520`（resize.rs:23-29 常量更新）；新增**双击复位**：`Interaction` 双击检测（自实现 300ms 内两次 Pressed）→ `PanelWidths.side_view=CONTEXT_W_DEFAULT`。
- `PanelWidths.side_view` 默认值 380→720；`CHAT_SIDEBAR_W` 常量弃用；**`SideViewCollapsed` 默认值 `true`→`false`**（layout.rs:52-53 现默认收起、SideView 节点 `Display::None`）——漏改这条，M2 完成后面板默认仍是隐藏的，表现为「改了没生效」。
- **窗口尺寸钳制与响应式（新增）**：① Startup 与窗口 resize 时统一钳制 `PanelWidths`——五列过渡期面板可用上限 = innerWidth − 52 − 240 − 520，1440px 窗口仅 628px，720 默认值会被收窄，**必须做启动钳制避免初始溢出**；② 窗口宽 <1100px 时自动折叠上下文面板（对应原型 media query），恢复入口 = rail 展开钮（§8.3）；③ `WindowPlugin` 处设最小窗口尺寸 1024×640（现工程未设，main.rs:203）。
- 左手柄（file_panel 用）随抽屉化**移除**（布局无此列）。
- **验收**：拖拽平滑、边界钳制、双击复位、折叠展开（layout.rs:222-269 `toggle_panel_visibility` 适配四列新结构）。

### 8.9 status_bar.rs — 状态栏（32px）
- 高 32；`surface` 底 + 顶边 `line`；条目改 pill 分段（右边框 `line` 分隔，v7.1 样式）。
- 内容：daemon 状态点（ok）+ provider·tokens（现 pill 合并样式对齐）+ 成本（占位 `$0.00`）+ spacer + companion 开关 + 会话 id（承接轮数：`#a3f2 · 8 轮`，见 §8.4）。**两处现状内容的处置**：① 会话状态文本（`status-ready` 等，status_bar.rs:171）**移除**——由顶栏 agent pill 承担，避免双处显示；② 编码/语言段（`status-encoding`，status_bar.rs:132-134）**保留**、样式对齐。**可点击范围本期仅 companion 开关**（切换 `warm` 点亮态），其余只读——防实现时自由发挥。
- **验收**：TokenUsage/会话状态联动不回归。

### 8.10 context_scope.rs — 新增（上下文 chips 条）
- 位置：会话区顶部（ChatArea 内第一行，38px，`surface` 底 + 底边 `line`）。
- 内容：「上下文」标签（TINY 大写 `text_muted`）+ chips（胶囊 9999、透明底+`border`：文件图标 tint `st_pending`/目录 `accent_interactive`、名称 CAPTION、× 删除钮 hover 显现）+「+ 添加」虚线胶囊。
- 数据源 MVP：当前会话编辑器已打开文件（tabs 状态聚合），**只读展示**。原型中 chip 的 × 删除钮本期不做——查实 `UserInputMessage { text, editor_queries }`（xgent_agent/events.rs:17-20），agent 侧没有独立「会话上下文文件集」概念，chips 的真实语义是「工作文件」，删 chip 关 tab 会误伤用户。`+ 添加` 按钮本期触发文件抽屉。真·会话上下文（@ 引用聚合展示、随消息携带）待 `UserInputMessage` 扩展，另立项不在本期。
- **验收**：打开/关闭文件联动 chips 增减。

### 8.11 command_palette.rs / confirm_dialog.rs / session_history.rs / settings_panel.rs — overlay 视觉对齐
- 命令面板：底 `elevated`（原 panel）、圆角 12、**选中态改中性 `hover`**（Linear 惯例，改 handle_palette_click 重绘逻辑）、条目图标块 `icon_bg`、kbd 徽章（`icon_bg` 底 + `border` + mono MICRO）。
- 确认弹窗：圆角 12、`overlay=black@0.5`、diff 区 `code_bg` 底 + `border`（现 `deep` 引用改 `code_bg`）、add/del 色换 `str_/st_fail`、按钮规范（拒绝=ghost 文本钮、确认=`accent` 底）。
- 会话历史：改为**抽屉视觉**（左侧 320px overlay，同 §8.7 结构），条目 active=`accent_bg`+边框 `accent_interactive`。
- 设置面板：输入框（EditableText）外框 `input_bg`+`border` 圆角 6、focus `accent_interactive`；按钮统一 §9.2 规格。
- **验收**：四类 overlay 截图对比原型；键盘导航不回归。

### 8.12 layout.rs — 骨架
- 按 §3 节点树重排；高度/宽度常量替换；折叠逻辑适配（FilePanelCollapsed 语义变为抽屉开关；SideViewCollapsed 保留）。
- **验收**：启动即四列结构；无遗留常驻文件列。

### 8.13 xui 侧改动（最小化）
- `EditorTheme` 增语法色字段（可 None）；`Scrollbar` 默认色参数化（`text_faint` 可注入）。
- `span_color_for` 优先读 EditorTheme（§5.4）。
- **验收**：`cargo tree -p xui` 仍无 xgent_* 依赖；`cargo check -p xui` 过。

### 8.14 xgent_app 侧
- 注册 `FontPlugin`；`load_system_font` 改 mono 语义；assets 增加 fonts/icons；`sync_editor_theme` 调用点更新。
- **验收**：`cargo run -p xgent_app` 启动即新 UI；字体加载失败可运行（warn 回退）。

## 9. 新增组件清单（`xgent_ui/src/kit.rs`，新模块）

| 组件 | 规格（v7.1） | 使用方 |
|---|---|---|
| `ghost_button` | `subtle` 底+`border`+6px 圆角+SMALL/510+图标槽；hover→`hover`/`border_hover` | 顶栏/工具行/弹窗 |
| `primary_button` | `accent` 底+`accent_text`+6px；hover→`accent_hover` | 发送/提交/确认 |
| `pill` / `chip` | 9999 圆角、透明或 `subtle` 底+`border` | agent pill/model pill/ctx chips/qa chips/搜索过滤 |
| `agent_pill` | 状态机（ready/thinking/generating/tool/confirm/error）×（底色/点色/脉冲） | 顶栏 |
| `kbd_badge` | `icon_bg` 底+`border`+圆角 2+mono MICRO | 快捷键提示/tooltip/palette |
| `tooltip` | 悬停 500ms 延迟浮现，`tooltip_bg` 底+`border`+圆角 4+MICRO | rail 按钮 |
| `badge` | 14px 圆点数标 `accent` 底 | rail |
| `icon_button` | 34/40px 方形圆角 6，图标 tint `text_muted`→hover `text` | 顶栏/面板头 |
| `icon()` | §6.2 ImageNode 染色 | 全局 |
| `section_label` | TINY 大写 `text_muted` +0.05em（LetterSpacing 正值） | 分区标题 |
| `companion_button` | 40 圆形 `warm` 底深色图标+激活环 | rail 底 |
| `toast` | 底部居中浮层：`tooltip_bg` 底 + `border` + 圆角 8 + MICRO 文案，2.2s 自动消失（原型的操作反馈模式；现状无统一轻提示） | 全局操作反馈 |

## 10. i18n / 快捷键 / 无障碍

- **新增 i18n key**（fluent，zh-CN/en-US 同步）：welcome 标题/副标题/3 快捷卡/最近会话、上下文条（上下文/添加上下文）、差异页空态、agent pill 五状态、companion 开/关、工具「复制/重试」（若展示）。各模块 `tr()` 现状机制沿用（i18n.rs）。失效键随模块清理（视图标签条移除后 `chat-tab-label` 等），避免死键累积。
- **快捷键**：保留现有 12 个；**语义跟随页签重映射**：`editor.view`（Cmd+E）→ 切上下文面板预览页签、`chat.view`（Cmd+D）→ 聚焦会话区、`settings.open`（Cmd+,）不变（设置的第四入口，加固 §8.2 保留决策）、`filepanel.toggle` → 文件抽屉开关（语义不变）；上下文面板折叠沿用 terminal.toggle 体系。新增项注册进 `HotkeyRegistry`（自动冲突检测）。
- **无障碍**：沿用现状水平（Bevy UI 无内建 ARIA）；保证键盘可达（palette/confirm 已有），不新增承诺。

## 11. 分期实施（每期可编译、可运行、可验收）

| 期 | 内容 | 涉及 | 验收 |
|---|---|---|---|
| **M1 令牌与字体** | theme.rs 重写 + 全引用点编译修复；fonts.rs + Inter 资产 + `ui_text/mono_text` 构造器（先并行提供，逐模块迁移）；xui EditorTheme 语法色注入 | theme.rs、fonts.rs(新)、startup.rs、editor/mod.rs、xui render.rs/highlight.rs | `cargo check --workspace`；启动全 UI 换新配色/字体；`cargo test -p xgent_ui`；**CJK spike 通过（§12.6）** |
| **M2 骨架与拖拽** | layout.rs **五列过渡**重排（52/文件/会话/6/720，见 §3）；resize.rs 钳制/双击复位/高亮/**窗口钳制与响应式折叠（§8.8）**；折叠适配；status_bar 32px | layout.rs、resize.rs、status_bar.rs、theme::size、xgent_app(main.rs 最小窗口) | 拖拽/双击复位/折叠实测；会话区 ≥520 钳制；**1440px 窗口初始无溢出；<1100px 自动折叠** |
| **M3 图标与顶轨** | export_png.py + IconAssets + kit 基础件（icon/ghost_button/pill/kbd/section_label）；top_bar、rail（tooltip）重做 | kit.rs(新)、icons.py(新)、top_bar.rs、activity_bar.rs | 截图对比原型顶栏/rail；无 emoji 残留 |
| **M4 会话区** | 消息视觉、消息操作栏（复制+重试）、回底浮钮、工具卡、welcome、context_scope（只读）、qa chips（点击填入）、输入卡 Outline focus 环、agent pill | chat_panel.rs、tool_panel.rs、welcome.rs(新)、context_scope.rs(新) | 流式对话/工具调用/空态三场景截图对比；消息链路回归；**Outline 随圆角实测（不随则回退外层节点）** |
| **M5 上下文面板** | 三页签条；diff 页新实现；终端/预览样式；file_panel 抽屉化 + 点文件开预览；**布局收为四列目标形态（文件列移除）** | editor/、terminal/、file_panel.rs、layout.rs | 三页切换/编辑/终端/diff 空态+有差异两态；抽屉开关 |
| **M6 overlay 层** | palette/confirm/history 抽屉/settings 视觉对齐；**kit::toast** | 四个 overlay 模块、kit.rs | 键盘导航回归；截图对比；toast 出现/消失 |
| **M7 动效与收尾** | pill 脉冲/光标块/companion 环；toast（若需）；dev-tutorial.md 同步；全量截图对照原型走查 | 分散 | 对照 v7.1 原型逐区走查清单过一遍 |

依赖关系：M1 → M2 → M3 →（M4、M5 可并行）→ M6 → M7。每期结束跑 `cargo fmt && cargo clippy --workspace && cargo test`。

## 12. 测试

1. **令牌单测**（theme.rs）：`dark()` 关键字段值断言（bg==#08090A 等）；`text` 与 `bg` 亮度比 > 4.5:1 的简易 luma 断言（防将来调色破坏可读性）。
2. **钳制单测**（resize.rs）：clamp 函数纯函数化后测边界（380 / inner-52-520 / 双击复位值）。
3. **diff 纯函数单测**（editor/diff_view.rs）：抽取的 line_diff 用例（从 confirm_dialog 现测试迁移）。
4. **截图验收**：新增 debug feature `ui-snapshot`（`cfg(feature)`）下注册快捷键 `F12` → `Screenshot` 组件截主窗口存 `target/snapshots/`，与原型浏览器截图人工对照（Bevy 0.19 内置 Screenshot，无新依赖）。
5. **回归**：每期跑全量 `cargo test --workspace`；会话/工具/终端/编辑器四链路手工冒烟。
6. **CJK spike 验收**（M1，见 §5.2）：默认字体换 Inter 后，中文回退（PingFang）与中英混排渲染/换行正常是进入 M1 验收的前置条件；不通过则回退默认字体并升级风险。

## 13. 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| Inter 可变字体 atlas 膨胀（字号×字重组合） | 显存/首启卡顿 | 字号锁 type_scale 8 档 + 字重 3 值（§5.3）；首启预热点常用组合（后续） |
| 全局默认字体替换（AssetId::default()）与 FontSource 行为耦合 | 字体不生效/panic | fonts.rs 失败回退 warn 不 panic；M1 先在 xgent_app 验证再铺开 |
| file_panel 抽屉化改动面大（交互/快捷键/预览链路） | 回归 | M5 单独一期；预览链路改为 OpenFileRequest 后全链路手测；保留旧常驻实现一个提交便于回滚 |
| diff 页数据源在 buffer 未保存场景的语义（何时算「差异」） | 用户困惑 | MVP 明确语义：buffer vs 磁盘（未保存更改）；空态文案说明；git diff 留 F-10 |
| emoji→图标工作量超预估 | 排期 | M3 只做本期可见的 ~20 枚；缺的先字符占位并列清单 |
| 现状消息为整段 Text 无 markdown 结构 | 代码块/inline code 视觉受限 | 本期先做整段+inline code 检测（` 简单分段），完整 markdown 渲染列 P1（与 agent 输出结构化联动） |
| 350ms 过渡类微动画缺失观感差异 | 与原型观感差 | 已声明非目标（§1.2）；动效仅 §7.2 清单 |

## 14. 完成定义（DoD）

- [ ] `cargo check --workspace && cargo clippy --workspace && cargo test --workspace` 全绿
- [ ] §11 七期验收全过；`ui-snapshot` 截图与 v7.1 原型逐区对照无明显偏差
- [ ] 全仓无 emoji 图标、无游离硬编码色（editor/terminal 语义色除外）、字号全部经 `type_scale`
- [ ] xui 依赖纯净（无 xgent_*）保持
- [ ] `doc/dev-tutorial.md` 同步（新 kit/fonts/diff_view 模块、Theme v3 字段、ADR-0014 链接）
- [ ] 本方案文档状态更新为「已完成」，遗留项（亮色/完整 markdown/搜索 Git 抽屉）移交 requirements P1/P2

---

## 附录 A：现状调研要点（实现时对照）

- 布局根：`layout.rs:73-219`（spawn_layout）；折叠：`layout.rs:222-269`；`PanelWidths`：`resize.rs:49-64`；手柄：`resize.rs:89-108`、拖拽状态机 `resize.rs:150-233`、钳制常量 `resize.rs:23-29`。
- Theme：结构 `theme.rs:11-89`、`Theme::dark()` `theme.rs:93-141`、space/size `theme.rs:151-179`、`px()` `theme.rs:182-184`。未消费字段：kw/fn_/str_/num/ty/com、bubble_*。
- 字体：`xgent_app/src/startup.rs:49-67`（Menlo 全局替换）；仓库无字体资产；全仓无 FontWeight/LetterSpacing/FontFeatures 使用。
- hover 唯一实现：`file_panel.rs:1085-1118`。动效三处：`status_bar.rs:199-228`、`chat_panel.rs:709-732`、`chat_panel.rs:674-707`。
- 编辑器主题注入：`editor/mod.rs:250-262`；硬编码色：`editor/tabs.rs:261,276`（tailwind GRAY_400/AMBER_400，随 M5 改 Theme）；xui 高亮硬编码：`xui/text_editor/highlight.rs:224-240`。
- 终端 ANSI 调色板（保留不动）：`terminal/output.rs:183-230`；行渲染 `output.rs:106-170`。
- overlay 参考：`command_palette.rs`（Absolute+遮罩+键盘导航）、`confirm_dialog.rs`（line_diff 定义 :53、调用 :232）。
- 剪贴板：`ClipboardPlugin` 在 bevy DefaultPlugins（default_plugins.rs:69），`Clipboard::set_text`（bevy_clipboard/lib.rs:299）；workspace 已启用 `system_clipboard`。
- 窗口：`WindowPlugin`（main.rs:203）未设最小尺寸，全工程无 ResizeConstraints。
- 硬编码尺寸散点（M3-M5 顺带收编 type_scale/size）：top_bar 26/30px、chat_panel 10.5/11/12px、tool_panel 11.5/12px、palette 500/400px 等。
- xui 能力边界：ScrollArea+StickToBottom ✓、虚拟列表未接线、scrollbar 只读、button/text_input 用官方底座（EditableText）、CommandRegistry 纯逻辑。

---

## 15. 修订记录

**v1.1（自查修订）**——评审方式：子代理调研的承重断言逐条亲自核查 bevy 源码与工程事实，并按 v7.1 原型逐面覆盖检查。

1. **§3/§11 时序矛盾修正**：M2 由「四列重排」改为「五列过渡」（文件列暂留），M5 抽屉化后才收四列——消除文件面板在 M2–M5 间无处安放的缺陷。
2. **§5.3 API 修正**：`FontFeatures::builder()` 的方法名是 `.enable(tag)`（text.rs:861），原写的 `.tag()` 会编译失败；补充 `LetterSpacing` 是 Text 必带组件（text.rs:192）、`Handle<Font>→FontSource`（text.rs:348）两个已核实事实。
3. **§5 CJK 回退写透**：中文渲染依赖 `system_font_discovery`（workspace 已启用，根 Cargo.toml:7），M1 增加强制 spike（§12.6）；字体入库补 OFL.txt。
4. **§8.4 复制按钮重估**：workspace 已启用 `system_clipboard`，「无剪贴板依赖故砍掉复制」的决策依据失效，改为基于 bevy_clipboard 实现（保留降级 fallback）。
5. **§8.10 context_scope 语义修正**：查实 `UserInputMessage { text, editor_queries }`（events.rs:17-20），agent 无上下文文件集概念，chips 改只读展示（删 × 钮），真上下文另立项。
6. **覆盖补漏**：§8.4 增「回到底部浮钮」；§9/§11 toast 定论为 M6 交付；focus 环改用内建 `Outline` 组件（ui_node.rs:2369）替代外层节点。
7. **§8.1 提交拆分**：theme 重写按「加新字段 → 迁移 → 删旧字段」三步提交。
8. **勘误**：line_diff 定义在 confirm_dialog.rs:53（原引 231-252 为调用点）。

**v1.2（第二轮深度修订）**——新增核查：v1.1 文档自身一致性、真实窗口尺寸下的钳制演算、横切面（行高/hover 规模/响应式）。

1. **§8.8 窗口维度补齐（本轮最大缺口）**：工程未设最小窗口尺寸（main.rs:203）；五列过渡期最小需求 ≈1918px，1440px 窗口下 720 默认值必被钳——补 Startup/resize 统一钳制、<1100px 响应式折叠（配 rail 展开钮 §8.3）、最小窗口 1024×640；M2 验收同步。
2. **§4.3/§5.3 行高维度补齐**：LineHeight 是 Text 必带组件，type_scale 新增 line_height 五档（TIGHT/UI/CTRL/BODY/TERM），构造器签名加行高参数。
3. **§7.1 hover 改数据驱动**：`HoverTint` 组件 + 单一全局系统，替代「每组件一个系统」（约 30 处重复劳动）。
4. **§8.4 两处纠正**：qa chips「点击填入」是新增行为而非保留现状（chat_panel.rs:220-249 现为静态文本）；Outline 需实测圆角跟随（bevy 历史直角问题），不随则回退外层节点（M4 验收同步）。
5. **§8.4 复制按钮实锤升级**：`Clipboard::set_text`（bevy_clipboard/lib.rs:299）+ ClipboardPlugin 在 DefaultPlugins（default_plugins.rs:69），零依赖确认。
6. **§8.6 双 tab 条决策**：MVP 两层保留 + 编辑器 tab 条降调，合并标 P1。
7. **§8.9 statusbar 可点击范围定死**：仅 companion 开关，其余只读。
8. **§4.2 勘误**：`bar` 字段实际引用为 confirm_dialog×2/terminal×3/file_panel×1（原写 top_bar/status_bar 有误）；§4.1 清理 subtle 注释残留。
9. **§11 M1 验收补 CJK spike**（与 §12.6 对齐）。

**v1.3（第三轮修订：执行可行性/阻断性/闭环专项）**——按「施工图模拟执行」逐条核实数据、API、实体结构与插件注册。

1. **§6.1 图标导出必修坑（唯一准阻断项）**：SVG 源为 `stroke="currentColor"`（默认解析为黑），`ImageNode.color` 乘法染色对黑像素无效——导出脚本必须替换为 `#FFFFFF` 描边，否则 M3 染色方案静默失效、全量返工。
2. **§8.2 agent pill 状态源实锤**：`ConversationStatus` 五态（Idle/Thinking/Streaming/ToolRunning/等待确认，conversation.rs:298-308）一一映射，error 态由 ErrorMessage 驱动。
3. **§8.4 三项悬置全部落定为可行**：复制按钮（app 用 DefaultPlugins→ClipboardPlugin 自动注册，main.rs:202）、qa chips 填入（`ChatInputMarker` 定位 + `EditableText.editor` 为 pub PlainEditor 可程序化赋值）、消息操作栏实体稳定（delta 累加至既有节点，仅 compact/clear 重建）。
4. **§8.6 diff 数据源实锤**：`TextEditor.rope` 为 pub 字段（text_editor.rs:80），无需给 xui 加 API。
5. **FontSource 迁移零破坏确认**：全工程无显式 `TextFont.font` 设置。
6. **闭环判定通过**：里程碑依赖链、M4/M5 并行交叉点（context_scope 读 tabs 状态自然衔接）、过渡期布局归宿、验收基础设施（CJK spike/snapshot/钳制演算）全部成立；无「依赖不可用」项。

**v1.4（第四轮修订：整读源码专项）**——对方案改造最重的源文件整读（layout.rs/resize.rs 全文、theme.rs 尾段、chat_panel/activity_bar/editor 关键段），找 grep 式调研会漏的结构性事实。

1. **§4.1 补 `punc` 字段（字段级 bug）**：原型 CSS 的 `--pn` 零消费者故删，但现 `Theme.punc`（theme.rs:133）被编辑器高亮链（§5.4）真实消费——「死令牌」判断对 CSS 成立、对 Bevy 不成立。
2. **§8.2 设置入口找回**：现状设置三入口（activity bar ⚙ / top_bar ⚙ / palette `settings.open`），方案清单漏掉后只剩 palette——top_bar 明确保留设置按钮（⑨），rail 不放；🕐 历史按钮移除迁 rail。
3. **§8.8 补 `SideViewCollapsed` 默认翻转**：现默认 `true`（收起、`Display::None`，layout.rs:52-53,192），不置 `false` 则 M2 后面板默认隐藏、「改了没生效」。
4. **§8.6 状态机事实修正**：`SideViewContent` 实为 `{None, Editor, Preview, Terminal}` 四态（mod.rs:62-72，调研漏 Preview）——预览页签有现成落点，Editor/Preview 归一 + 增 Diff。
5. **§8.4/§8.9 会话信息迁移**：视图标签条右侧挂 `ConversationInfoMarker`（chat_panel.rs:107-158,738），随条移除须迁状态栏会话 id 位，不得一删了之。
6. **读码利好确认**：resize 右手柄钳制公式已天然扣除文件面板占用（resize.rs:229），五列过渡演算与现码一致；折叠机制（layout.rs:254-268）可整体复用；输入卡现状与目标接近，M4 是换肤非重写。

**v1.5（第五轮修订：自由方向）**——核查方案自身一致性、被波及未处置模块、排期模糊地带。

1. **§3 命名不一致修正**：节点树 `PanelWidths.context_view` 为不存在的字段，统一为 `side_view`（与 resize.rs:54 及 §8.8 一致）。
2. **§8.2 引用修正**：「见 kit §9.1」→「见 §9」（无 9.1 子节）。
3. **§10 热键语义重映射**：`editor.view`（Cmd+E）→ 预览页签、`settings.open`（Cmd+,）为设置第四入口——方案此前未提热键跟随。
4. **§8.4/§8.9 ConversationInfo 处置修正**：查实其内容为轮数+token 数（chat_panel.rs:739-758）而非会话 id——token 与状态栏重复应舍弃，轮数并入状态栏会话 id 位；v1.4 的「迁移」表述据此修正。
5. **§8.9 现状内容处置定案**：会话状态文本移除（agent pill 承接）、编码/语言段保留并对齐——此前方案未提。
6. **排期补漏**：文本构造器迁移定为「M2 起随模块、M6 清点收尾」；welcome 进入空态时发 `ListSessionsMessage`；i18n 失效键（`chat-tab-label` 等）随模块清理。

**v1.6（第六轮修订：资产加载物理前提）**——核查 AssetServer 可用性，发现阻断级前提缺失。
1. **§5.2/§6.1 资产根勘误（阻断级）**：工程未配置 AssetPlugin 资产根（默认 CWD 相对 `assets/`，workspace 根运行不存在），现有代码全部绕开 AssetServer（字体直读 startup.rs:56-59、插件 CARGO_MANIFEST_DIR 扫描 main.rs:182）——方案的字体/图标 AssetServer 加载路径会静默失败。修复：`AssetPlugin.file_path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets")`；Inter 走 AssetServer、Menlo 保持直读；任务侧 M1-T2 增先行子任务（tasks v1.3）。
2. 顺带核实：`crates/xgent_app/assets/` 不在 gitignore（仅忽略插件 wasm 构建产物），字体/图标入库无障碍；字体二进制入库为 ADR 已接受决策。

**v1.7（第七轮修订：微补与实证）**——文本内嵌样式底层承诺实证，产出收敛信号。
1. **§5.3/§8.4 补 `TextBackgroundColor` 机制**（text.rs:1089 实证，per-span 背景）：inline code 底色承诺可行，机制此前未记录，防实施者凭直觉误砍。
2. M4-T6（tasks v1.4）失效键清单补 `conversation-tokens`。
3. 其余无新发现；收敛信号明确，评审阶段收工，转入实施。
