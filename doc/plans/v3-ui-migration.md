# XGent UI v3 迁移实现计划

> 依据：`ui-prototype-v3.html`（原型）· `ui-v3-design-tokens.md`（令牌）· `ui-v3-requirements-brief.md`（需求摘要）· `ui-v3-icons/`（32 个 SVG 图标）
> 现状基线：v2 UI 全量落地（MVP step1~12 + O1~O10 + F-11 编辑器 + F-19 终端），约 22k 行 Rust，`cargo check --workspace` 通过
> 目标：将 v3 原型的八处结构性改动逐条落地到 Bevy ECS UI，不破坏现有功能链路

---

## 0. 迁移总览

### 0.1 v3 相对 v2 的八处结构性改动

| # | 改动 | 影响范围 | 优先级 |
|:--|:-----|:---------|:-------|
| ① | 陪伴锚点：状态栏常驻暖色胶囊 + 输入卡回声，五态联动 | `status_bar` `chat_panel`（新模块 `companion`） | P0 |
| ② | 导航收敛 6→3 层：删除 viewtabs / sv-tabs，统一内容标签 | `layout` `chat_panel` `editor/tabs` `terminal/mod`（新模块 `content_tabs`） | P0 |
| ③ | 强调色与语义状态分离：brand 冷电蓝取代绿色 accent | `theme`（全局波及） | P0 |
| ④ | 排版三档分级 + 对话留白上调 | `theme` `chat_panel` `tool_panel` | P1 |
| ⑤ | 微交互 ≤200ms + prefers-reduced-motion | `theme`（动效令牌）+ 各交互模块 | P1 |
| ⑥ | 待确认升级为界面级信号：对话流信号卡 + 弹窗状态条 + 锚点转色 | `tool_panel` `confirm_dialog` `companion` | P0 |
| ⑦ | 状态栏升级为运行时仪表 | `status_bar` | P0 |
| ⑧ | 图标与令牌统一：32 个 SVG 1.75px 几何线性 | `xui`（新模块 `icon`）+ 全局替换 | P0 |

### 0.2 实施阶段

```
Phase 0 — 地基：令牌迁移 + 图标系统        （①③⑧ 的前置）
Phase 1 — 骨架：导航收敛 + 内容标签统一     （②，一次完成避免标签系统改两遍）
Phase 2 — 陪伴：陪伴锚点 + 输入卡回声       （①）
Phase 3 — 仪表：状态栏运行时仪表            （⑦）
Phase 4 — 对话：排版分级 + 留白 + Markdown  （④ + Phase D）
Phase 5 — 信号：待确认界面级信号            （⑥）
Phase 6 — 动效：微交互体系                  （⑤）
Phase 7 — 收尾：i18n 同步 + 清理 + 验证
```

> **修订记录**：
> - 2026-08-08 review：原案 Phase 1 与 Phase 6 合并；图标技术路线修正；状态映射/数据源/Markdown 等 17 处修正。
> - 2026-08-08 bsn：引入 Bevy 0.19 的 BSN（`bsn!` 声明式 UI 构建），新模块全量 BSN、重写模块 spawn 改 BSN；见 0.5。

### 0.3 关键约束

- **不破坏现有功能**：每个 Phase 完成后 `cargo check --workspace` 必须通过，已有功能（对话/工具/编辑器/终端/命令面板）行为不变。
- **ECS 通信契约不变**：所有子系统仍只通过 Events/Messages 通信，新增模块遵守此约束。
- **i18n 内置**：所有新增用户可见字符串走 fluent，不硬编码。
- **暗色优先**：v3 令牌为暗色主题，不引入亮色主题切换。
- **宠物是独立窗口**：主 GUI 内不含任何宠物形象，陪伴锚点仅用抽象几何信号。
- **资源内嵌**：图标、.ftl 等静态资源一律 `include_str!` / `include_bytes!` 嵌入二进制，不依赖运行时文件路径。
- **BSN 声明式构建**：新模块和重写模块的 spawn 代码统一使用 `bsn!` 场景函数（`impl Scene`），ECS update 系统保持不变。具体见 0.5。

### 0.4 图标技术路线（bevy_resvg 修订版）

**调研结论（bevy_resvg v2.5.0）**：
- `bevy_resvg` v2.5.0 直接依赖 `bevy = "0.19"`（Cargo.toml verified），基于 resvg 库一次性光栅化 SVG。
- 提供 `UiSvg(handle)` 组件直接在 bevy_ui 节点中渲染 SVG（底层 ImageNode），`SvgColor(color)` 组件做颜色着色。
- 一站式 `bsn!` 支持：`UiSvg("icons/icon-send.svg")  SvgColor(theme.brand)`。
- 约 1000 行 Rust，MIT/Apache-2.0 双许可，代码轻量。

**推荐方案（Phase 0 采用）— bevy_resvg 直接加载 SVG**：

```
AssetServer::load("icons/icon-send.svg")
        │
        ▼
  SvgFile 资产（resvg 光栅化，加载时一次性完成）
        │
        ▼
  UiSvg(handle) + SvgColor(Color)   ← bevy_ui 原生 ImageNode + bevy_resvg 着色
  Node { width/height = 16|20|24 }  ← 由父级 Node 控制尺寸
```

- **着色原理**：`SvgColor` 组件直接控制渲染颜色，SVG 源文件中 `stroke="currentColor"` 可保留（SvgColor 覆盖默认 currentColor 回退）。
- **尺寸**：SVG viewBox 天然支持任意缩放；`UiSvg` 本身是 ImageNode，尺寸由包裹的 UI Node 控制（IconSize 16/20/24）。
- **资产组织**：32 个 SVG 从 `doc/design/ui-v3-icons/` 直接复制到 `crates/xui/assets/icons/`，构建期自动包含（无需构建脚本、无需 PNG 预处理、无需 include_bytes!）。
- **依赖**：运行时仅增加 `bevy_resvg` 一个 crate（含 `resvg` 库），约 3 个额外编译单元。

### 0.5 BSN 声明式 UI 构建（从 v3 迁移引入）

#### 0.5.1 背景

**现状**：项目所有 UI 构建采用 Bevy 标准 imperative spawn 模式：

```rust
// 当前 v2 风格（xgent_ui 各处）
commands
    .spawn((Node { .. }, BackgroundColor(theme.surface), BorderColor::all(theme.line)))
    .with_children(|tabs| {
        tabs.spawn((Node { .. }, BackgroundColor(theme.elevated), Text::new(".."), TextFont { .. }, TextColor(theme.text)));
    })
    .id();
```

**Bevy 0.19 提供了 BSN（Bevy Scene Notation）**——基于 `bsn!` proc macro 的声明式 UI 场景格式（`bevy_scene` crate，feature-gated by `"bevy_scene"`）。已验证：
- `../bevy/crates/bevy_scene/macros/src/bsn/` 存在完整的 parser + codegen
- `bevy_feathers`（官方 UI 组件库）**所有控件和容器**全部用 BSN 构建（button, checkbox, slider, text_input, menu, pane, subpane, scrollbar, label, icon 等 20+ 模块）
- `bevy_scene::Scene` trait + `bsn!` + `Children [...]` 内嵌 + `#[derive(SceneComponent)]` 模板/继承/覆盖体系

**BSN 等价改写对比**：

```rust
// ── imperative（当前 v2） ──
commands.spawn((Node { width: px(100.0), height: px(S6), ..default() },
    BackgroundColor(theme.panel), BorderColor::all(theme.line)))
    .with_children(|p| {
        p.spawn((Node { padding: UiRect::all(px(S2)), ..default() },
            Text::new("标签"), TextFont { font_size, .. },
            TextColor(theme.text)));
    });

// ── BSN（v3 采用） ──
bsn! {
    Node { width: px(100.0), height: px(S6), ..default() }
    BackgroundColor(theme.panel)
    BorderColor::all(theme.line)
    Children [
        (Node { padding: UiRect::all(px(S2)), ..default() }
         Text::new("标签")  TextFont { font_size, .. }  TextColor(theme.text))
    ]
}
```

BSN 将嵌套 spawn 变成视觉上的缩进树，消除 `commands` / `.with_children()` / `.id()` 样板。

#### 0.5.2 Adoption 策略

**原则：v3 迁移趁机引入 BSN，不要求一次性重写所有 v2 代码。**

| 代码类别 | BSN 策略 |
|:---------|:---------|
| **新模块**（companion, content_tabs, pend_card） | 100% BSN 构建 |
| **重写模块**（status_bar, chat_panel spawn 部分, tool_panel, confirm_dialog, activity_bar） | spawn 函数全量改写为 BSN，update 系统不动 |
| **轻改模块**（file_panel, editor, terminal, settings_panel, session_history） | 保持现有 imperative 代码，仅在触及的 spawn 节点改用 BSN |
| **xui 通用组件**（icon, markdown, animation） | icon 模块用 BSN 封装 spawn_icon 为 scene function；markdown 块渲染用 BSN；animation 保持系统层不变 |
| **ECS 系统**（event 处理、状态同步、update 循环） | **不动**——BSN 只替换 spawn，不改变 ECS 架构 |

**编码约定**：

```rust
// 场景函数签名：fn xxx(props) -> impl Scene
pub fn status_pill(
    dot_color: Color,
    label: &str,
) -> impl Scene {
    bsn! { .. }
}

// 复杂模块：暴露 SceneComponent（即可 spawn 的"组件蓝图"）
#[derive(SceneComponent, Default, Clone)]
#[scene(CompanionAnchorProps)]
pub struct CompanionAnchor;
// 然后 spawn 时：commands.spawn_related(&parent, CompanionAnchor { ..props });

// 涉及动态数据的场景函数：参数化 props struct
pub struct ContentTabProps { pub kind: TabKind, pub name: String, pub dirty: bool, .. }
pub fn content_tab(props: ContentTabProps) -> impl Scene { bsn! { .. } }
```

#### 0.5.3 依赖变更

| crate | 改动 |
|:------|:-----|
| `xui/Cargo.toml` | bevy features 加 `"bevy_scene"`；新增依赖 `bevy_resvg = "2.5"`（图标渲染核心） |
| `xgent_ui/Cargo.toml` | bevy features 加 `"bevy_scene"`（BSN 声明式构建） |
| `xui/src/lib.rs` | `use bevy_scene::prelude::*; use bevy_resvg::prelude::*;` |
| `xgent_ui/src/lib.rs` | `use bevy_scene::prelude::*;` |

> **注意**：`xgent_ui` 不直接依赖 `bevy_resvg`——图标统一通过 `xui::spawn_icon()` 间接使用，`SvgPlugin` 由 `xui::IconPlugin` 注册。

`bevy_resvg` v2.5.0 依赖 `bevy = "0.19"`（verified），约 3 个传递依赖（`pastey`, `resvg ^0.42-0.47`, `serde`/`thiserror` 已存在）。

#### 0.5.4 BSN 编码规则（团队约定）

1. **每个 `bsn!` 块对应一个 entity**，Component 按行罗列，**Children 内嵌子 entity**。
2. **场景函数返回 `impl Scene`**，函数名 snake_case，对应 UI 语义（`companion_anchor`, `status_pill`, `content_tab`, `chat_message`, `code_block`）。
3. **可复用的跨模块场景**放 `xui` crate（如 `pill`, `card`, `kbd`, `section_header`）；业务独有场景放 `xgent_ui`。
4. **Props struct** 统一命名：`XxxProps`（如 `StatusPillProps`, `ChatMessageProps`）。
5. **保留 marker 组件**：BSN 块内的 entity 仍可带 marker（如 `StatusDotMarker`），方便系统 query。
6. **UiSvg 使用规范**：图标统一通过 `spawn_icon(kind, size, color)` 场景函数生成（内部封装 `UiSvg + SvgColor + Node`），不在业务代码中直接写 `UiSvg`。

---

## Phase 0 — 地基：令牌迁移 + 图标系统

> 对应改动 ③⑧。这是所有后续 Phase 的基础，必须最先完成。

### 0.1 Theme 令牌迁移（`crates/xgent_ui/src/theme.rs`）

**现状**：`Theme` 结构体约 35 个颜色字段，`accent` = 绿色 `#22C55E`，同时承担 CTA/聚焦/选中/成功四重语义。

**目标**：将 `ui-v3-design-tokens.md` 的 48 个令牌平移为 Rust 常量，核心动作是 **brand/semantic/warm 三套色系分离**。

#### 新增字段

```rust
// ===== 品牌冷电色（取代 v2 的绿色 accent） =====
pub brand: Color,           // #2D9DFF  主品牌色（CTA/聚焦环/选中/发送键）
pub brand_hover: Color,     // #54B4FF
pub brand_press: Color,     // #1E84E0
pub brand_bg: Color,        // rgba(45,157,255,0.12)  选中态半透明底
pub brand_glow: Color,      // rgba(45,157,255,0.28)  聚焦光晕
pub brand_2: Color,         // #7C6CF5  靛紫，仅装饰渐变

// ===== 边框聚焦色（从绿改冷电，v3 关键分离） =====
pub line_focus: Color,      // rgba(45,157,255,0.55)

// ===== 陪伴层暖色（专供陪伴锚点） =====
pub warm: Color,            // #FF8A5B
pub warm_hover: Color,      // #FFA279
pub warm_bg: Color,         // rgba(255,138,91,0.12)
pub warm_glow: Color,       // rgba(255,138,91,0.30)
pub warm_line: Color,       // rgba(255,138,91,0.28)

// ===== 语义状态色（与品牌色彻底分离） =====
pub ok_bg: Color,           // rgba(52,211,153,0.12)
pub pend_bg: Color,         // rgba(245,181,68,0.12)
pub fail_bg: Color,         // rgba(240,97,109,0.12)
```

#### 字段迁移策略（review 细化）

| v2 字段 | v3 处理 |
|:--------|:--------|
| `accent` (#22C55E 绿) | → `brand` (#2D9DFF 冷电蓝)。**分两步**：先在结构体新增 `brand` 字段并赋值，所有引用点全局替换后再删 `accent`（避免一步到位编译爆炸）。 |
| `accent_bg` (绿色半透明) | → `brand_bg`，同上两步法。 |
| `border` | → 新增 `line_strong`（与令牌文档对齐）；`border` 暂保留为 `line_strong` 的 deprecated alias（`#[allow(dead_code)]` + 注释），Phase 7 收尾时统一替换删除。 |
| `bubble_user` / `bubble_assistant` | 废弃（v3 对话流无气泡），Phase 7 删除。 |
| `bar` | 废弃（顶栏/状态栏统一用 `bg`），Phase 7 删除。 |
| `handle_active` | 改用 `brand` 半透明。 |

#### 排版令牌（新增 `theme::typo` 模块）

```rust
pub mod typo {
    // 字体族
    pub const FONT_UI: &str = "Inter, PingFang SC, Microsoft YaHei, system-ui, sans-serif";
    pub const FONT_MONO: &str = "JetBrains Mono, Fira Code, Cascadia Code, SF Mono, Consolas, monospace";

    // 三档字号/行高
    pub const FS_BODY: f32 = 13.5;  pub const LH_BODY: f32 = 1.65;
    pub const FS_CODE: f32 = 12.5;  pub const LH_CODE: f32 = 1.6;
    pub const FS_META: f32 = 11.5;  pub const LH_META: f32 = 1.4;

    // 三级标题
    pub const FS_H1: f32 = 15.0;  pub const FW_H1: u16 = 700;
    pub const FS_H2: f32 = 13.0;  pub const FW_H2: u16 = 600;
    pub const FS_H3: f32 = 12.5;  pub const FW_H3: u16 = 600;
}
```

#### 动效令牌（新增 `theme::motion` 模块）

```rust
pub mod motion {
    pub const DUR_FAST: f32 = 0.120;  // 120ms
    pub const DUR_BASE: f32 = 0.180;  // 180ms
    pub const DUR_SLOW: f32 = 0.240;  // 240ms
    // 缓动：Bevy 内置 EaseFunction 位于 bevy_math::curve::easing（已验证存在）
    // ease       = cubic-bezier(0.4, 0, 0.2, 1) → EaseFunction::CubicInOut
    // ease_out   = cubic-bezier(0.16, 1, 0.3, 1) → EaseFunction::OutCubic
}
```

#### 间距/圆角令牌（扩展现有 `space` 模块）

```rust
// 间距：8pt 栅格，补充 v3 新增档位
pub const S1: f32 = 4.0;   pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;  pub const S4: f32 = 16.0;
pub const S5: f32 = 20.0;  pub const S6: f32 = 24.0;
pub const S8: f32 = 32.0;  pub const S10: f32 = 40.0;

// 圆角
pub const R_SM: f32 = 4.0;
pub const R_MD: f32 = 6.0;
pub const R_LG: f32 = 8.0;
pub const R_XL: f32 = 12.0;
pub const R_PILL: f32 = 999.0;
```

#### 尺寸常量调整

| 常量 | v2 | v3 | 说明 |
|:-----|:---|:---|:-----|
| `STATUS_BAR_H` | 28 | 32 | 仪表信息增多，加高 |
| `SIDEBAR_W` | 240 | 242 | 微调对齐 8pt 栅格 |
| `TOP_BAR_H` | 48 | 48 | 不变 |
| `ACTIVITY_BAR_W` | 48 | 48 | 不变 |

#### 实施步骤

1. 在 `Theme` 结构体中新增所有 v3 字段；`line_strong` 新增、`border` 保留 alias。
2. 修改 `Theme::dark()` 初始化所有新字段为 v3 令牌值。
3. 全局替换 `theme.accent` → `theme.brand`（两步法，先加后删）。
4. 新增 `typo`、`motion`、`radius` 模块。
5. `cargo check --workspace` 验证；删除 `accent`/`accent_bg` 遗留引用。
6. 全局 emoji/Unicode 图标盘点：`grep -rn "📁\|📝\|⚙️\|🔍\|📄\|➤\|✓\|✕\|▶\|▼" crates/xgent_ui/src/` 列出全部待替换点（Phase 0 只盘点登记，替换随各 Phase 落地）。
7. **BSN + bevy_resvg 初始化**（见 0.5）：
   - `xui/Cargo.toml`：bevy features 加 `"bevy_scene"`；依赖加 `bevy_resvg = "2.5"`。
   - `xgent_ui/Cargo.toml`：bevy features 加 `"bevy_scene"`（不直接依赖 bevy_resvg）。
   - `xui/src/lib.rs`：`use bevy_scene::prelude::*; use bevy_resvg::prelude::*;`。
   - `xgent_ui/src/lib.rs`：`use bevy_scene::prelude::*;`。
   - `xui/assets/icons/` 创建目录，复制 32 个 SVG 文件。
   - 在 `IconPlugin` 中注册 `SvgPlugin`。

### 0.2 图标系统（`crates/xui/src/icon.rs` — 新模块）

**方案**：bevy_resvg 直接加载 SVG + `SvgColor` 着色（详见 0.4）。

#### 模块设计

```rust
// crates/xui/src/icon.rs

/// 图标语义名枚举（32 个）
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconKind {
    Logo, ChevronDown, ChevronLeft, ChevronRight,
    Plus, X, Search, Check, Gear, Play, Pause, Eye,
    Globe, Split, Folder, FolderOpen, File, FileNew,
    Box, Edit, Refresh, Clear, Send, Info, Pet,
    Sliders, Clock,
    CpIdle, CpThink, CpTool, CpFail, CpOff,
}

/// 图标尺寸档
#[derive(Clone, Copy)]
pub enum IconSize { Sm, Md, Lg }  // 16 / 20 / 24 px

impl IconKind {
    fn asset_path(self) -> &'static str {
        match self {
            IconKind::Logo => "icons/icon-logo.svg",
            IconKind::Send => "icons/icon-send.svg",
            IconKind::CpIdle => "icons/icon-cp-idle.svg",
            // ... 共 32 个映射
        }
    }
}

/// BSN scene：直接在父级 spawn 一个图标（一次 AssetServer::load + UiSvg + SvgColor）
/// 使用方式：在 bsn! 块内写 spawn_icon(IconKind::Send, IconSize::Md, theme.brand)
pub fn spawn_icon(kind: IconKind, size: IconSize, color: Color) -> impl Scene {
    let dim = match size {
        IconSize::Sm => px(16.0),
        IconSize::Md => px(20.0),
        IconSize::Lg => px(24.0),
    };
    bsn! {
        Node { width: dim, height: dim, ..default() }
        UiSvg(kind.asset_path())
        SvgColor(color)
    }
}
```

> **assets 组织**：32 个 SVG 从 `doc/design/ui-v3-icons/` 复制到 `crates/xui/assets/icons/`，AssetServer 自动索引。无需构建脚本、无需 `include_bytes!`。

#### 图标-令牌着色映射

| 图标分类 | 默认色 | 特殊态色 |
|:---------|:-------|:---------|
| 品牌标识 (logo) | `--brand` | — |
| 导航/操作通用 | `--t2` (继承) | hover → `--t0`；active → `--brand` |
| 发送键 | `--brand` | hover → `--brand-hover` |
| 完成勾 | `--ok` | — |
| 待确认标记 | `--pend` | — |
| 陪伴锚点常规态 | `--warm` | pend → `--pend`；fail → `--fail` |
| 陪伴锚点关闭态 | `--t3` | — |

#### 实施步骤

1. 复制 32 个 SVG 文件：`doc/design/ui-v3-icons/*.svg` → `crates/xui/assets/icons/`。
2. 实现 `IconKind` 枚举 + `asset_path()` + `spawn_icon()` 场景函数（见上方代码）。
3. 实现 `IconPlugin`：注册 `SvgPlugin`，可选预加载手柄缓存。
4. 编写测试：spawn 每个 IconKind，验证不 panic、`SvgFile` 资产加载成功。
5. 各 Phase 落地时按映射表替换现有 emoji/Unicode。

---

## Phase 1 — 骨架：导航收敛 + 内容标签统一

> 对应改动 ②。**合并原案的 Phase 1 + Phase 6 一次完成**：ContentTabs 从建骨架到 editor/terminal 迁移一次性落地，避免标签系统被改两次。

### 1.1 现状分析：v2 的 6 层导航

```
1. 顶栏 (TopBar)           — 品牌 / provider / 全局动作
2. 活动栏 (ActivityBar)    — Files / Editor / Terminal / Settings
3. 视图标签 (ViewTabs)     — Chat / Editor / Terminal 视图切换     ← 与活动栏重复
4. 侧视图标签 (SideViewTabs)— 编辑器文件 tabs                      ← 与 ed-tabs 重复
5. 编辑器标签 (EditorTabs)  — 文件 tabs
6. 终端标签 (TerminalTabs)  — 终端 tabs
```

### 1.2 v3 目标：3 层导航

```
1. 顶栏 (TopBar)           — 品牌 / provider / 全局动作（不变）
2. 活动栏 (ActivityBar)    — Files / Search / Editor / Terminal / Settings（唯一的视图切换入口）
3. 内容标签 (ContentTabs)  — 统一一条标签栏，标签自带类型图标（文件 / 预览 / 终端）
```

### 1.3 活动栏 ↔ 内容标签联动状态机（review 补充，原型 L1258-1285 的完整逻辑）

**活动栏按钮 = toggle 语义**，active 态由当前标签类型决定：

| 操作 | 前置条件 | 行为 |
|:-----|:---------|:-----|
| 点 Files | — | toggle 侧栏（`FilePanelCollapsed`） |
| 点 Search | — | 打开命令面板（v3 无独立搜索面板，Search 即命令面板入口） |
| 点 Editor | 侧视图显示 且 当前 tab 为 file/preview | 收起侧视图 |
| 点 Editor | 否则 | 激活一个 file 标签（优先 file，其次 preview；无则打开默认文件） |
| 点 Terminal | 侧视图显示 且 当前 tab 为 term | 收起侧视图 |
| 点 Terminal | 否则 | 激活一个 term 标签（无则新建终端） |
| 点 Settings | — | 打开设置面板 |

**活动栏 active 态同步规则**（`sync_activity_bar`）：

| 当前状态 | Files 按钮 | Editor 按钮 | Terminal 按钮 |
|:---------|:-----------|:------------|:--------------|
| 侧栏可见 | active | — | — |
| 侧视图显示 + file/preview tab | — | active | — |
| 侧视图显示 + term tab | — | — | active |
| 侧栏隐藏 + 侧视图隐藏 | 无 active | — | — |

### 1.4 具体改动

#### 删除 ViewTabs（`chat_panel.rs`）

- 移除 `spawn_views_tab_bar()` 及相关代码。
- 会话元信息（会话 ID、轮次）上移至状态栏仪表。
- 对话面板直接从消息列表开始，无标签栏。

#### 新建 ContentTabs 模块（`crates/xgent_ui/src/content_tabs.rs`）

统一管理侧视图中的所有标签（文件 / 预览 / 终端），取代 v2 的 `EditorTabs` + `TerminalTabs` 双系统。

```rust
/// 内容标签类型
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TabKind { File, Preview, Terminal }

/// 标签数据
#[derive(Clone)]
pub struct ContentTab {
    pub id: String,
    pub kind: TabKind,
    pub name: String,
    pub dirty: bool,
    pub running: bool,  // 终端运行中
}

/// 标签状态 Resource
#[derive(Resource, Default)]
pub struct ContentTabs {
    pub tabs: Vec<ContentTab>,
    pub active: Option<String>,
}

/// 标签操作事件（保持 ECS 事件契约）
#[derive(Event)]
pub enum ContentTabRequest {
    Open { kind: TabKind, name: String },
    Close { id: String },
    Switch { id: String },
}
```

#### 标签 UI 细节（对应原型 `.ct-bar`）

- 标签栏高 34px，`bg` 底色 + 底部 `line_strong` 边框。
- 每个标签：顶部 2px 透明边框（active 变 `brand`）+ 类型图标 (13px) + 名称 + 状态指示器（dirty=琥珀 6px 圆点 / running=蓝色脉冲圆点）+ 关闭按钮（默认 28% 透明，hover 全显）。
- 标签栏右侧：新建终端按钮 (`+`) + 工具区（清屏 `icon-clear` / 关闭分屏 `icon-x`）。

**BSN 渲染风格**（`render_content_tabs` 系统）：

```rust
/// 单个内容标签场景函数
fn content_tab_scene(tab: &ContentTab, active: bool, theme: &Theme) -> impl Scene {
    let kind_icon = match tab.kind {
        TabKind::Terminal => IconKind::ChevronRight, // 终端图标
        TabKind::Preview  => IconKind::Eye,
        TabKind::File     => IconKind::File,
    };
    let top_border = if active { theme.brand } else { Color::NONE };
    let bg = if active { theme.panel } else { Color::NONE };

    bsn! {
        Node {
            display: Display::Flex,
            align_items: AlignItems::Center,
            column_gap: px(5.0),
            padding: UiRect::horizontal(px(9.0)),
            height: px(34.0),
            border: UiRect::top(px(2.0)),
            ..default()
        }
        BackgroundColor(bg)
        BorderColor::all(top_border)
        ContentTabMarker
        Children [
            // 类型图标（spawn_icon 返回 impl Scene，用 {} 嵌入）
            {spawn_icon(kind_icon, IconSize::Sm, theme.text_dim)}
            // 名称
            (Text::new(tab.name.clone())  TextColor(if active { theme.text } else { theme.text_muted }))
            // dirty / running 指示器
            {if tab.running { bsn_list!((Node { width:px(6.0), height:px(6.0), border_radius: BorderRadius::all(px(3.0)), ..default() }
                                       BackgroundColor(theme.st_running)  RunningDotMarker)) }
             else if tab.dirty { bsn_list!((Node { width:px(6.0), height:px(6.0), border_radius: BorderRadius::all(px(3.0)), ..default() }
                                           BackgroundColor(theme.st_pending)  DirtyDotMarker)) }
             else { bsn_list!() }}
            // 关闭按钮
            (Node { width:px(15.0), height:px(15.0), ..default() }
             {spawn_icon(IconKind::X, IconSize::Sm, theme.text_muted)}
             TabCloseButtonMarker)
        ]
    }
}
```

> `bsn_list!()` 返回空 children（无节点），用于条件分支。`{spawn_icon(...)}` 返回 `impl Scene`，在 Children 中以 `{expr}` 语法嵌入。

#### 文件类型检测

```rust
fn detect_tab_kind(filename: &str) -> TabKind {
    if filename.ends_with(".md") || filename.ends_with(".toml")
        || filename.ends_with(".lock") || filename.ends_with(".json")
        || filename.ends_with(".yaml") || filename.ends_with(".yml") {
        TabKind::Preview
    } else if is_code_file(filename) {
        TabKind::File
    } else {
        TabKind::Preview
    }
}
```

#### 活动栏调整（`activity_bar.rs`）

- 新增 **Search** 图标项（`icon-search.svg`），点击打开命令面板。
- 活动指示条颜色从绿色改为 `brand`（Phase 0 已完成令牌迁移）。
- 点击 Editor / Terminal 按 1.3 状态机执行。
- 终端图标增加 badge（运行中终端数量，对应原型 `ab-badge.run`）。

#### 布局调整（`layout.rs`）

- 状态栏高度从 28px → 32px。
- 侧栏宽度从 240px → 242px。
- 顶栏底色从 `panel` → `bg`（v3 令牌：顶栏/活动栏/状态栏统一用 `bg`）。

#### 编辑器/终端标签逻辑迁移（关键：不丢功能）

- **EditorTabs Resource 保留为编辑器内部数据源**（tab 顺序、dirty、关闭确认、conflict 检测、CycleTab 等逻辑不动），仅**删除其标签栏渲染层**；渲染统一由 ContentTabs 承担。
  - `EditorDirtyChanged` / dirty 关闭确认弹窗：标签栏不再承载关闭按钮的视觉，但关闭动作仍经 `CloseTabRequest` → 确认弹窗 → `ContentTabRequest::Close`。
- **TerminalTabs Resource 同理保留**（PTY 会话映射、SpawnTab/CloseTab/SwitchTab/ClearTab 事件不变），删除 `TerminalTabBarMarker` 渲染层。
- 新增桥接系统：`ContentTabs.active` 变化 → 通知 editor/terminal 切换 pane；tab 的 dirty/running 状态由 editor/terminal 回写 `ContentTabs`。

### 1.5 实施步骤

1. 新建 `content_tabs.rs`，实现 `ContentTabs` Resource + 标签操作事件 + `ContentTabsPlugin`。
2. 在 `layout.rs` 中为 SideView 顶部预留 `ContentTabsBarMarker` 容器。
3. 实现 `spawn_content_tabs_bar()`：渲染标签栏 UI（图标/名称/指示器/关闭按钮/工具区）。
4. 实现标签 CRUD 系统：响应 `ContentTabRequest`。
5. 删除 `chat_panel.rs` 中的 ViewTabs 代码。
6. 修改 `editor/mod.rs` + `editor/tabs.rs`：删除标签栏渲染，保留数据模型与事件，加 ContentTabs 桥接。
7. 修改 `terminal/mod.rs`：删除 `TerminalTabBar` 渲染，保留 PTY 逻辑，加 ContentTabs 桥接。
8. 修改 `activity_bar.rs`：新增 Search 项，实现 1.3 状态机。
9. `cargo check --workspace` 验证 + 编辑器/终端/预览功能回归。

---

## Phase 2 — 陪伴：陪伴锚点 + 输入卡回声

> 对应改动 ①。这是 v3 最核心的新增特性。

### 2.1 陪伴锚点状态机

五态状态机，同时驱动三个 UI 落点：

```
                    ┌─────────────────┐
                    │  AgentState     │
                    │  (Resource)     │
                    └────┬────┬────┬──┘
                         │    │    │
              ┌──────────┘    │    └──────────┐
              ▼               ▼               ▼
    ┌─────────────────┐ ┌──────────┐ ┌──────────────┐
    │ 状态栏胶囊       │ │ 输入卡回声 │ │ 输入卡边框色  │
    │ (companion)     │ │ (echo)   │ │ (input-card) │
    └─────────────────┘ └──────────┘ └──────────────┘
```

#### Agent 状态定义

```rust
// crates/xgent_ui/src/companion.rs

/// Agent 五态（v3 原型的 STATE_META）
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentState {
    #[default]
    Idle,       // 待命
    Think,      // 思考中（含流式生成期）
    Tool,       // 执行工具中
    Pend,       // 待确认
    Fail,       // 出错
}

/// 陪伴锚点全局状态
#[derive(Resource)]
pub struct CompanionState {
    pub state: AgentState,
    pub pet_enabled: bool,  // 宠物窗口开关（初始开启）
}

impl Default for CompanionState {
    fn default() -> Self {
        Self { state: AgentState::default(), pet_enabled: true }
    }

/// 状态变更事件
#[derive(Event, Clone, Copy)]
pub struct AgentStateChanged(pub AgentState);
```

#### ConversationStatus 8 态 → AgentState 5 态映射（review 补全）

`xgent_agent::conversation::ConversationStatus` 实际为 8 态（Idle/Thinking/Streaming/ToolRunning/Confirming/Aborting/Error），映射如下：

| ConversationStatus | AgentState | 锚点显示 | 说明 |
|:-------------------|:-----------|:---------|:-----|
| `Idle` | Idle | 待命 | — |
| `Thinking` | Think | 思考中 | — |
| `Streaming` | Think | 思考中 | 原型做法（replayStream 期间显示"思考中"）；流式结束后转回由下一事件驱动 |
| `ToolRunning` | Tool | 执行工具中 | — |
| `Confirming` | Pend | 待确认 | 界面级信号 |
| `Aborting` | Think | 思考中 | **决策**：中断是瞬时态，映射 Think 并让状态文字短暂显示"中断中"（可复用 v2 的 Aborting token hint 文案）；500ms 后若无新事件回到 Idle |
| `Error` | Fail | 出错 | — |

**驱动实现**：监听 `ConversationStatusChanged`（现有事件）+ `ToolCallMessage`，在 `sync_agent_state` 系统内做映射。注意用 `is_changed()` 避免每帧重复触发。

#### 待确认三处联动（落点预览）

| 落点 | 行为 |
|:-----|:-----|
| 陪伴锚点胶囊 | 整枚转 `pend` 色 + 脉冲动画 |
| 对话流信号卡 | `pend-card` 出现（Phase 5 实现） |
| 输入卡边框 | 转 `pend` 半透明色 |

### 2.2 状态栏陪伴胶囊

UI 结构（对应原型 `.companion` 元素）：

```
┌─────────────────────────────────────────┐
│ [glyph] XGent · 待命 ●                  │
└─────────────────────────────────────────┘
  ↑ 暖色    ↑ 暖色  ↑暖色  ↑语义色点
```

- 暖色底 (`warm_bg`) + 暖色描边 (`warm_line`) + 圆角药丸 (`R_PILL`)。
- 左侧：陪伴锚点字形（`IconKind::CpIdle/Think/Tool/Pend/Fail`），暖色。
- 字形外层有呼吸光晕动画（`warm_glow`，2.8s 周期）。
- 中间：名称 "XGent" + 分隔符 "·" + 状态文字。
- 右侧：语义色小圆点（idle=灰 / think=蓝 / tool=蓝 / pend=琥珀 / fail=红）。
- **待确认/出错时**：整枚胶囊转为语义色（pend_bg + pend 描边 + pend 文字 + 脉冲动画）。
- **宠物关闭时**：退暖为中性色（`elevated` 底 + `line_strong` 描边），字形换为 `CpOff`。

```rust
// crates/xgent_ui/src/companion.rs
// spawn 函数返回 impl Scene，由调用处 spawn_related

pub fn companion_anchor(theme: &Theme) -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(6.0),
            height: px(22.0),
            padding: UiRect::horizontal(px(9.0)),
            border_radius: BorderRadius::all(px(R_PILL)),
            ..default()
        }
        BackgroundColor(theme.warm_bg)
        BorderColor::all(theme.warm_line)
        CompanionAnchorMarker
        Children [
            // 字形（含呼吸光晕 + 初始 idle 图标；update 系统按状态切换图标和光晕）
            (Node { ..default() }
             CompanionGlyphMarker
             Children [
                 (Node { position_type: PositionType::Absolute, ..default() }
                  BackgroundColor(theme.warm_glow)  CompanionGlowMarker)
                 {spawn_icon(IconKind::CpIdle, IconSize::Sm, theme.warm)}
             ])
            // 名称
            (Text::new("XGent")  TextColor(theme.warm)  CompanionNameMarker)
            // 分隔符
            (Text::new("·")  TextColor(theme.warm_line))
            // 状态文字
            (Text::new("待命")  TextColor(theme.text_dim)  CompanionStateTextMarker)
            // 语义色点
            (Node { width: px(6.0), height: px(6.0), border_radius: BorderRadius::all(px(3.0)), ..default() }
             BackgroundColor(theme.text_muted)  CompanionDotMarker)
        ]
    }
}
```

> **注意**：BSN 版本中 `Children [...]` 内每个 `(...)` 是一个子 entity；组件裸列（无括号）属于父 entity。与 imperative 版本语义等价。

### 2.3 输入卡回声 + 快捷键提示条（review 补充）

输入卡工具栏左侧布局（对应原型 L832-842）：

```
┌──────────────────────────────────────────────────────────────┐
│ ● 待命    Ctrl+↵ 发送  Esc 中断  Ctrl+K 命令  Ctrl+\ 分屏  │
└──────────────────────────────────────────────────────────────┘
  ↑ 回声       ↑ 4 个 kbd 快捷键提示（v3 新增，替代 v2 的 token hint）
```

- 回声：暖色 pill（`warm_bg` + `warm` 文字 + 小色点），与状态栏胶囊同状态源，宠物关闭时退中性色。
- 快捷键提示条：`<kbd>` 样式（`elevated` 底 + `line_strong` 描边 + 9.5px mono 字），4 组提示。
- token hint 移除（token 信息上移至状态栏仪表，Phase 3）。

### 2.4 输入卡边框联动

- `Pend` 态：输入卡边框转为 `pend` 半透明色。
- `Fail` 态：输入卡边框转为 `fail` 半透明色。
- 其他态：正常 `line_strong` 边框。
- 聚焦时：`line_focus`（冷电蓝）+ `brand_glow` 光晕，不受状态影响。

### 2.5 宠物开关

- 状态栏右侧新增宠物开关按钮（`icon-pet.svg`）。
- 点击切换 `CompanionState.pet_enabled`。
- 关闭时：胶囊退暖为中性色，字形换为 `CpOff`，回声同步退化。
- Toast 提示切换结果。

### 2.6 实施步骤

1. 新建 `crates/xgent_ui/src/companion.rs`。
2. 定义 `AgentState`、`CompanionState`、`AgentStateChanged` 事件。
3. 实现 `CompanionPlugin`：注册 Resource + 事件 + 系统。
4. 实现 `spawn_companion_anchor()`：在状态栏 spawn 胶囊。
5. 实现 `update_companion_anchor()`：状态变更时更新字形/颜色/动画。
6. 实现 `sync_agent_state()`：按 2.1 映射表从 `ConversationStatusChanged` / `ToolCallMessage` 驱动状态。
7. 实现 `spawn_input_echo()` + `spawn_input_hints()`：输入卡工具栏。
8. 实现 `update_input_echo()` / `update_input_card_border()`。
9. 实现宠物开关按钮 + `toggle_pet()`。
10. `cargo check --workspace` 验证。

---

## Phase 3 — 仪表：状态栏运行时仪表

> 对应改动 ⑦。将状态栏从简单的 pill 布局升级为运行时仪表。

### 3.1 v3 状态栏布局

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ [陪伴胶囊] │ ● openai / gpt-4o-mini │ ⚙ 12.4k↑ 3.1k↓ │ ≈$0.0182 │ ● daemon 12ms │     │ 宠物 开 │ 会话 #a3f9c1 · 3轮 │ UTF-8 │ LF │ Rust │
└──────────────────────────────────────────────────────────────────────────────┘
  ← 左侧：陪伴 + 运行时信息                              右侧：会话 + 文件元信息 →
```

### 3.2 各仪表数据源（review 核实版）

#### Token 用量仪

- **数据源已存在**：`xgent_core::chat::TokenUsage { prompt: u32, completion: u32 }`（verified），经 `DoneMessage.usage` 到达 UI。
- UI 侧 `status_bar::TokenUsage`（现仅 `total: u64`）扩展为 `{ prompt: u64, completion: u64 }`，**字段命名对齐 core 的 prompt/completion**（不要另造 input/output），`track_token_usage` 从 `DoneMessage.usage` 累加。
- 显示格式：`12.4k↑ 3.1k↓`（prompt ↑ / completion ↓），`icon-sliders.svg`。

#### 预估成本仪

- **现状：项目内无任何定价数据**（settings_core / core 均无 price 字段，verified）。
- **决策（open question，建议 MVP 方案 B）**：
  - 方案 A：`xgent_settings_core` 增加 `provider_pricing` 配置（模型 → input/output 每 M token 价格），daemon 经 IPC 下发。可扩展、可配，但跨 crate 改动大。
  - 方案 B（**推荐 MVP**）：UI 侧内置常见模型价格表（gpt-4o-mini / gpt-4o / claude-3-5-sonnet 等），`CostEstimate` Resource 按 `model` 字段查表，未收录模型显示 `≈ —`。架构上预留 `CostProvider` trait，后续可切方案 A。
- 实施前需与用户确认选 A 或 B。

#### Daemon 连接状态仪

- **现状：ipc_client.rs 无心跳/延迟测量**（verified）。
- 最小改动方案：在 `xgent_app/ipc_client.rs` 增加 RTT 测量——记录最近一次 IPC 请求的往返时长（复用现有请求/响应通道，不新增 ping 协议）；daemon 侧零改动。
- UI 侧：`DaemonStatus { connected: bool, latency_ms: u64 }` Resource，色点 ok=绿 / fail=红，文本 `daemon 12ms`。
- 首次请求前显示 `daemon —`。

#### 会话信息仪

- 会话 ID：`Conversation.id: SessionId` **已存在**（verified），但注意它是**时间戳型**（`session_id.parse::<u64>()`），不是原型的 `#a3f9c1` 短格式——显示时取低 6 位十六进制即可，或直接显示时间戳，**与用户确认展示格式**。
- 轮次：**无现成字段**（verified），由消息数推导（user 消息数即轮次数），`update_session_meter` 系统监听 `Conversation` 变化后计算。

### 3.3 实施步骤

1. 扩展 UI 侧 `TokenUsage` 为 `{prompt, completion}`，修改 `track_token_usage`。
2. 新增 `CostEstimate` Resource + 内置价格表（方案 B）或 settings 配置（方案 A，待确认）。
3. 新增 `DaemonStatus` Resource + RTT 测量（改 `xgent_app/ipc_client.rs`）。
4. 重写 `spawn_status_bar()`：**改用 BSN 声明式构建**，按 v3 布局 spawn 所有仪表组件。每个仪表封装为独立 scene function（`provider_pill()` / `token_meter()` / `cost_meter()` / `daemon_meter()` / `session_meter()`），便于复用和测试。示例：
   ```rust
   fn token_meter(prompt: u64, completion: u64, theme: &Theme) -> impl Scene {
       bsn! {
           Node { column_gap: px(S1), align_items: AlignItems::Center, ..default() }
           Children [
               {spawn_icon(IconKind::Sliders, IconSize::Sm, theme.text_muted)}
               (Text::new(format!("{}↑", fmt_token(prompt)))
                TextColor(theme.text)  TextFont { font_size: FontSize::Px(FS_META), .. } TokenInputMarker)
               (Text::new(format!("{}↓", fmt_token(completion)))
                TextColor(theme.text)  TextFont { font_size: FontSize::Px(FS_META), .. } TokenOutputMarker)
           ]
       }
   }
   ```
5. 重写 `update_status_segments()`：更新各仪表文本和色点。
6. 将陪伴胶囊（Phase 2 产出）集成到状态栏左侧。
7. 会话信息仪：ID 格式 + 轮次推导。
8. `cargo check --workspace` 验证。

---

## Phase 4 — 对话：排版分级 + 留白 + Markdown

> 对应改动 ④ + Phase D（v2 遗留的 Markdown 渲染）。

### 4.1 排版调整

#### 对话流

- 消息块间距从 `--s5` (20px) 上调至 `--s6` (24px)。
- 正文字号 13.5px / 行高 1.65（v3 `FS_BODY` / `LH_BODY`）。
- 代码字号 12.5px / 行高 1.6（v3 `FS_CODE` / `LH_CODE`）。
- 元信息（时间戳、状态标签）字号 11.5px（v3 `FS_META`）。
- 对话流最大宽度 820px，居中。

#### 消息行式布局（v2 已有，微调）

- 用户头像：`elevated` 底 + `line_strong` 描边。
- 助手头像：`brand` → `brand_2` 渐变底（冷电蓝 → 靛紫）。
- 角色名：`FS_H3` (12.5px) / `FW_H3` (600)。
- 时间戳：`FS_META` (11.5px) / `t3` 色。

### 4.2 Markdown 渲染（Phase D 落地，review 修正版）

**现状**：助手消息为纯文本，无 Markdown 渲染。

**解析器选型（修正）**：不手写解析器。采用 `pulldown-cmark`（成熟、纯 Rust、CommonMark 兼容、轻量），加入 `xui` 依赖。项目已依赖 tree-sitter 等 crate，新增一个解析库不违背任何原则；手写会引入大量边界 bug（fence 转义、嵌套列表、HTML 块等）。

**流式策略（拍板）**：**流式期间纯文本 + 流式光标**，流式结束后一次性解析渲染 Markdown。不做增量 markdown 渲染——增量会导致代码块高度变化、布局抖动，与 `StickToBottom` 冲突，且原型本身就是流式纯文本（`data-full` 流式完成后才显示完整内容）。

#### 架构设计

```
LLM 流式输出
    │ 流式期间：纯文本节点 + caret（现有 accumulate_delta 逻辑不变）
    ▼
Done / finalize
    │
    ▼
MarkdownParser::parse(text) → Vec<MarkdownBlock>   （pulldown-cmark）
    │
    ├── Paragraph { text, inline_spans: Vec<(Range, InlineKind)> }
    ├── CodeBlock { language, code, highlighted_spans }
    ├── OrderedList { items: Vec<ListItem> }
    └── UnorderedList { items: Vec<ListItem> }
    │
    ▼
render_markdown(blocks) → spawn ECS 节点（Column 容器）
```

#### 新模块：`crates/xui/src/markdown.rs`

```rust
/// Markdown 块类型
pub enum MarkdownBlock {
    Paragraph { text: String, spans: Vec<InlineSpan> },
    CodeBlock { language: String, code: String },
    OrderedList { items: Vec<String> },
    UnorderedList { items: Vec<String> },
}

/// 行内样式
pub struct InlineSpan {
    pub range: std::ops::Range<usize>,
    pub kind: InlineKind,  // Code, Bold, Link
}

/// 解析（pulldown-cmark → 自有块结构）
pub fn parse_markdown(text: &str) -> Vec<MarkdownBlock> { ... }

/// 渲染为 ECS 节点
pub fn spawn_markdown(commands: &mut Commands, blocks: &[MarkdownBlock], theme: &Theme) -> Entity { ... }
```

#### 代码块渲染

- 深色底 (`deep`) + `line` 描边 + `R_LG` 圆角。
- 头部：终端图标 + 标签文本 + 语言标签 + 复制按钮。
- 主体：等宽字体 + 语法高亮。
- **高亮策略（review 补充）**：`xui::text_editor::highlight` 基于 tree-sitter-rust（**仅 Rust grammar**）。因此：`rust`/`rs` 语言代码块用 tree-sitter 高亮；**其他语言 fallback 为纯文本**（MVP 从简，不做多语言 grammar 分发——与 AGENTS.md D-06 待决策点一致）。
- 流式光标：`--run` 蓝色闪烁竖线，仅流式期间显示。

#### 实施步骤

1. `xui/Cargo.toml` 添加 `pulldown-cmark` 依赖。
2. 新建 `crates/xui/src/markdown.rs`，实现 `parse_markdown()` + `spawn_markdown()`。
3. 在 `XuiPlugin` 中注册 `MarkdownPlugin`（可选，若纯函数则不必注册）。
4. 修改 `chat_panel.rs` 的 `finalize_on_done`：从纯文本 spawn 改为 Markdown spawn。
5. 流式期间保持纯文本逻辑不变（只改 `finalize_on_done`）。
6. 实现复制按钮功能。
7. `cargo check --workspace` 验证。

### 4.3 流式光标改进

- v2 用 Unicode `▮` 字符闪烁。
- v3 改为：在文本末尾 spawn 一个 6px×14px 的 `BackgroundColor(theme.st_running)` 小矩形节点。
- 1Hz 闪烁（显示 500ms / 隐藏 500ms）。
- `prefers-reduced-motion` 下：不闪烁，常亮。

---

## Phase 5 — 信号：待确认界面级信号

> 对应改动 ⑥。将"待确认"从时间线上的一个黄点升级为界面级信号。

### 5.1 三处联动落点

```
┌─────────────────────────────────────────────────────────┐
│  对话流                                                   │
│  ┌─ 时间线节点 ─┐                                        │
│  │ ⏸ WriteFile  │ ← 落点 1：时间线图标 + 卡片边框转琥珀  │
│  └──────────────┘                                        │
│  ┌─ 待确认信号卡 ──────────────────────────────────────┐ │
│  │ ● 等待你的确认                                      │ │
│  │   Agent 请求写入 doc/notes/xxx.md（+9 −1）         │ │
│  │                          [拒绝] [查看差异并确认]    │ │
│  └──────────────────────────────────────────────────────┘ │
│                          ↑ 落点 2：对话流内的醒目信号卡    │
│                                                          │
│  ┌─ 确认弹窗 ────────────────────────────────────────┐  │
│  │ ═════════════════════════════════════════════════ ═ │ ← 落点 3：弹窗顶部状态条
│  │ ⚠ 确认执行                                         │  │
│  │   待确认 · 高危写入 · agent 已暂停                  │  │
│  │   ...diff...                                       │  │
│  │                        [拒绝 Esc] [允许执行 ↵]     │  │
│  └────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
       ↑ 陪伴锚点同步转琥珀色（Phase 2 已实现）
```

### 5.2 待确认信号卡（对话流内）

新增 `pend_card.rs` 模块（或在 `tool_panel.rs` 中新增）：

```rust
/// 待确认信号卡标记
#[derive(Component)]
pub struct PendingCardMarker;

fn spawn_pending_card(
    commands: &mut Commands,
    chat_inner: Entity,
    theme: &Theme,
    tool_name: &str,
    file_path: &str,
    diff_summary: &str,  // "+9 −1"
) { ... }
```

- 琥珀色边框 + `pend_bg` 半透明底 + `R_LG` 圆角。
- 左侧：8px 脉冲圆点（pendPulse 动画）。
- 中间：标题 "等待你的确认"（`FS_H2` / `pend` 色）+ 描述（含 `code` 路径）。
- 右侧：[拒绝]（btn-quiet）[查看差异并确认]（btn-pend）。
- 触发：`ToolCallMessage` 带 `NeedsConfirmation` 时 spawn；用户确认/拒绝后 despawn，时间线节点转 done/deny 态。

### 5.3 确认弹窗顶部状态条

修改 `confirm_dialog.rs`：

- 弹窗顶部新增 2px 高的 `pend` 色状态条（`modal-signal`），1.6s 透明度呼吸动画。
- 弹窗头部大号待确认标记：`icon-info.svg`（24px）在 `pend_bg` + `pend` 描边的 34px 圆角容器中。
- 按钮文案对齐原型：`拒绝 Esc` / `允许执行 ↵`（v2 是 Deny/Allow，需替换 + i18n）。

### 5.4 时间线节点升级（含连接脊，review 补充）

修改 `tool_panel.rs`：

- `Pend` 态图标：`pend_bg` 底 + `pend` 半透明描边 + `pendPulse` 动画。
- `Pend` 态卡片：`pend` 半透明边框 + `pend_bg` 背景叠加。
- **时间线连接脊**（原型 `.timeline::before`）：相邻两个工具节点之间显示 1px 竖线（`line_strong`），在第一个节点下方 30px 起、跨过间距到下一个节点，避免悬空线头。

### 5.5 实施步骤

1. 新建 `pend_card.rs`（或在 `tool_panel.rs` 中新增 `spawn_pending_card`）。
2. 实现 `spawn_pending_card()` + `despawn_pending_card()`。
3. 修改 `tool_panel.rs`：Pend 态视觉升级 + 脉冲动画 + 时间线连接脊。
4. 修改 `confirm_dialog.rs`：顶部状态条 + 大号标记 + 按钮文案。
5. 实现联动逻辑：信号卡 → 弹窗 → 时间线节点 → 陪伴锚点 四处同步。
6. `cargo check --workspace` 验证。

---

## Phase 6 — 动效：微交互体系

> 对应改动 ⑤。统一所有微交互的时长、缓动和降级策略。

### 6.1 动效实现基础（review 核实版）

**验证结论**：Bevy 0.19 的 `bevy_ui` **没有 LayoutTransition**（grep 无结果）——UI 属性过渡没有官方支持，因此需自建轻量动画系统。缓动枚举 `EaseFunction` 位于 `bevy_math::curve::easing`（存在，已验证），可复用其 `ease()` 函数，不必自写缓动曲线。

```rust
// crates/xui/src/animation.rs (新模块)

/// 简易动画状态组件
#[derive(Component)]
pub struct AnimationState {
    pub start: f32,
    pub end: f32,
    pub elapsed: f32,
    pub duration: f32,
    pub easing: EaseFunction,  // bevy_math::curve::easing::EaseFunction
    pub playing: bool,
}

/// 通用动画推进系统
fn advance_animations(
    time: Res<Time>,
    mut q: Query<(&mut AnimationState, &mut ...)>,
) { ... }
```

> 注意：`EaseFunction` 的 `ease(t)` 返回插值进度，颜色过渡需自写 Color 线性插值（`Color::mix`），位置/尺寸过渡直接用数值插值。每种动画目标（BackgroundColor / Node 尺寸 / TextColor / 透明度）各写一个泛化系统或用 trait 抽象，MVP 阶段可先按目标类型各写一个系统（约 4-5 个）。

### 6.2 需要动画的交互点

| 交互 | 时长 | 缓动 | 效果 |
|:-----|:-----|:-----|:-----|
| 状态文字过渡 | 120ms (fast) | ease | 旧文字 fade out (90ms) → 新文字 fade in |
| 工具卡展开/折叠 | 180ms (base) | ease | 内容区高度 0→内容高度 |
| 陪伴锚点状态切换 | 180ms (base) | ease | 背景色/边框色/文字色渐变 |
| 待确认脉冲 | 1.6s | ease | 圆点外发光扩散（pendPulse） |
| 呼吸光晕 | 2.8s | ease | 透明度 + 缩放（breathe） |
| 弹窗淡入 | 240ms (slow) | ease_out | 背景遮罩 opacity + 弹窗 translateY(8px→0) |
| 命令面板淡入 | 240ms (slow) | ease_out | 同弹窗 |
| Toast | 180ms (base) | ease | opacity + translateY |
| 流式光标 | 1s | steps(2) | 显示/隐藏切换 |
| 输入卡 shake | 240ms (slow) | ease | translateX(-3px→3px) |
| 标签激活 | 120ms (fast) | ease | 顶部边框色 + 背景色 |

### 6.3 prefers-reduced-motion 降级

**验证结论**：Bevy 0.19 未桥接 winit 0.30 的 `ApplicationHandler::prefers_reduced_motion` 回调（bevy_winit 中 grep 无结果），accesskit 也无现成暴露。因此 MVP 采用：

```rust
#[derive(Resource, Default)]
pub struct ReducedMotion(pub bool);

/// 启动时从环境变量读取（MVP 手段）
fn detect_reduced_motion(mut commands: Commands) {
    let reduced = std::env::var("XGENT_REDUCED_MOTION").is_ok();
    commands.insert_resource(ReducedMotion(reduced));
}
```

> 后续改进路径（P1）：winit 0.30 的 `EventLoop::prefers_reduced_motion` 或桥接 `ApplicationHandler` 回调注入 Bevy Resource。

降级规则：
- 所有动画 duration → 0.01ms（等价瞬切）。
- 脉冲/呼吸/闪烁 → 停止，保持静态。
- 过渡 → 直接设置最终值。

### 6.4 实施步骤

1. 新建 `crates/xui/src/animation.rs`，实现 `AnimationState` + 颜色/尺寸/透明度推进系统。
2. 在 `XuiPlugin` 中注册 `AnimationPlugin`。
3. 逐个交互点替换硬编码动画为令牌驱动（对照 6.2 表）。
4. 实现 `ReducedMotion` 检测 + 降级逻辑。
5. `cargo check --workspace` 验证。

---

## Phase 7 — 收尾：i18n 同步 + 清理 + 验证

### 7.1 i18n 新增字符串

所有 v3 新增的用户可见字符串需添加到 `crates/xgent_ui/i18n/*.ftl`：

```ftl
# companion
companion-state-idle = 待命
companion-state-think = 思考中
companion-state-tool = 执行工具中
companion-state-pend = 待确认
companion-state-fail = 出错
companion-state-aborting = 中断中
companion-pet-on = 开
companion-pet-off = 关
companion-pet-toggle-on = 宠物窗口已开启 · 陪伴锚点恢复暖色
companion-pet-toggle-off = 宠物窗口已关闭 · 陪伴锚点退化为状态指示器

# status bar
status-token-input = {$count}↑
status-token-output = {$count}↓
status-cost = ≈ ${$amount}
status-cost-unknown = ≈ —
status-daemon = daemon {$latency}ms
status-daemon-pending = daemon —
status-session = 会话 #{$id} · {$rounds}轮

# pending
pending-card-title = 等待你的确认
pending-card-desc = Agent 请求写入 {$path}（{$diff}）。授权后会继续执行队列中剩余 {$remaining} 个工具调用；在此之前 agent 已暂停。
pending-card-reject = 拒绝
pending-card-confirm = 查看差异并确认

# content tabs
content-tab-new-terminal = 新建终端
content-tab-clear = 清屏 / 重置
content-tab-close-split = 关闭分屏

# confirm dialog
confirm-dialog-reject = 拒绝
confirm-dialog-allow = 允许执行
```

### 7.2 废弃代码清理

- 删除 `theme.rs` 中的 v2 废弃字段（`accent`、`accent_bg`、`bubble_user`、`bubble_assistant`、`bar`、`border` 旧名）。
- 删除 `chat_panel.rs` 中的 ViewTabs 相关代码（Phase 1 已删，此处确认无残留）。
- 删除 `editor/tabs.rs` 和 `terminal/tabs.rs` 中的标签栏渲染代码（Phase 1 已删）。
- 全局 emoji/Unicode 图标替换验收（Phase 0 盘点清单逐项核对）。
- 清理所有 `theme.accent` / `theme.border` 遗留引用。

### 7.3 文档同步

- 更新 `doc/dev-tutorial.md`：
  - 新增 v3 令牌系统说明
  - 新增陪伴锚点模块说明
  - 新增内容标签模块说明
  - 新增 Markdown 渲染模块说明
  - 新增图标系统说明
  - 更新 crate 拓扑图
- 更新 `doc/design/ui-gap-plan.md`：标记 Phase D（Markdown）完成。

### 7.4 验证清单

**构建与静态检查：**
- [ ] `cargo check --workspace` 通过
- [ ] `cargo clippy --workspace` 无警告
- [ ] `cargo test --workspace` 通过

**视觉对比（review 补充）：**
- [ ] 每个 Phase 完成后，浏览器打开 `ui-prototype-v3.html` 对照截图/肉眼对比（布局、颜色、间距、图标、状态色），差异登记为待办

**v2 功能回归：**
- [ ] 多轮对话 + 流式输出
- [ ] 工具调用 + 确认弹窗
- [ ] 文件树浏览 + 文件预览
- [ ] 编辑器多标签 + 语法高亮 + 查找替换 + dirty 关闭确认 + 冲突检测
- [ ] 终端多标签 + PTY 交互
- [ ] 命令面板 + 快捷键
- [ ] 设置面板
- [ ] 会话历史

**v3 新功能验证：**
- [ ] 陪伴锚点五态切换正确（含 Streaming/Aborting 映射）
- [ ] 陪伴锚点宠物开关降级正确
- [ ] 输入卡回声与状态栏胶囊同步
- [ ] 输入卡边框随状态变色
- [ ] 状态栏仪表信息正确（token ↑↓ / 成本 / daemon 延迟 / 会话轮次）
- [ ] 活动栏-内容标签联动状态机（1.3 表逐项验证）
- [ ] 内容标签统一切换正确（file/preview/term 三类互斥）
- [ ] 待确认信号卡 + 弹窗 + 时间线 + 锚点联动
- [ ] Markdown 代码块渲染 + Rust 语法高亮 + 非 Rust fallback
- [ ] 流式光标显示
- [ ] 所有微交互 ≤240ms（reduced-motion 下瞬切）

---

## 附录 A：文件变更清单

### 新增文件

| 文件 | 说明 |
|:-----|:-----|
| `crates/xui/assets/icons/*.svg` | 32 个图标 SVG（从 `doc/design/ui-v3-icons/` 复制入库） |
| `crates/xui/src/icon.rs` | 图标系统（IconKind / IconSize / spawn_icon 场景函数） |
| `crates/xui/src/markdown.rs` | Markdown 解析 + 渲染（pulldown-cmark） |
| `crates/xui/src/animation.rs` | 通用动画系统 |
| `crates/xgent_ui/src/companion.rs` | 陪伴锚点 + 输入卡回声 |
| `crates/xgent_ui/src/content_tabs.rs` | 统一内容标签 |
| `crates/xgent_ui/src/pend_card.rs` | 待确认信号卡 |

### 修改文件

| 文件 | 改动摘要 |
|:-----|:---------|
| `crates/xgent_ui/src/theme.rs` | 令牌迁移：brand/warm/semantic 分离 + typo/motion/radius 模块 |
| `crates/xgent_ui/src/layout.rs` | 状态栏高度 32px + 顶栏底色改 bg + SideView 顶部预留 ContentTabsBar |
| `crates/xgent_ui/src/status_bar.rs` | 重写为运行时仪表布局（token ↑↓ / 成本 / daemon / 会话 / 宠物开关） |
| `crates/xgent_ui/src/chat_panel.rs` | 删除 ViewTabs + 排版调整 + Markdown 渲染 + 流式光标改进 + 输入卡快捷键提示条 |
| `crates/xgent_ui/src/tool_panel.rs` | Pend 态视觉升级 + 脉冲动画 + 时间线连接脊 |
| `crates/xgent_ui/src/confirm_dialog.rs` | 顶部状态条 + 大号标记 + 按钮文案对齐原型 |
| `crates/xgent_ui/src/activity_bar.rs` | 新增 Search 项 + 指示条改 brand 色 + badge + 联动状态机 |
| `crates/xgent_ui/src/top_bar.rs` | 底色改 bg + 图标替换 |
| `crates/xgent_ui/src/file_panel.rs` | 宽度 242px + 图标替换 |
| `crates/xgent_ui/src/editor/mod.rs` | 删除标签栏渲染 + ContentTabs 桥接（保留 EditorTabs 数据模型与事件） |
| `crates/xgent_ui/src/editor/tabs.rs` | 标签渲染移除 + 事件桥接 |
| `crates/xgent_ui/src/terminal/mod.rs` | 删除标签栏渲染 + ContentTabs 桥接（保留 PTY 逻辑） |
| `crates/xgent_ui/src/shortcuts.rs` | 新增 Ctrl+Shift+F（搜索）等快捷键 |
| `crates/xui/src/lib.rs` | 注册 IconPlugin + AnimationPlugin + `use bevy_scene::prelude::*; use bevy_resvg::prelude::*;` |
| `crates/xui/Cargo.toml` | 新增 `pulldown-cmark` 和 `bevy_resvg = "2.5"` 依赖；bevy features 加 `"bevy_scene"` |
| `crates/xgent_ui/src/lib.rs` | 注册 CompanionPlugin + ContentTabsPlugin + `use bevy_scene::prelude::*;` |
| `crates/xgent_ui/Cargo.toml` | bevy features 加 `"bevy_scene"`（不直接依赖 bevy_resvg） |
| `crates/xgent_ui/i18n/*.ftl` | 新增 v3 字符串 |
| **`crates/xgent_app/src/ipc_client.rs`** | **（review 补充）RTT 延迟测量，驱动状态栏 daemon 仪表** |

### 不变文件

`xgent_core`、`xgent_provider`、`xgent_daemon`、`xgent_tools`、`xgent_context`、`xgent_agent`、`xgent_settings_core`、`xgent_settings`、`xgent_terminal` — 这些 crate 不受 v3 UI 迁移影响（成本估算若选方案 A 则 `xgent_settings_core` 需加 pricing 字段，另议）。

---

## 附录 B：Phase 依赖关系

```
Phase 0 (令牌+图标)
  ├──→ Phase 1 (导航收敛 + 内容标签统一)
  ├──→ Phase 2 (陪伴锚点)
  │       └──→ Phase 5 (待确认信号) ← 依赖陪伴锚点联动
  ├──→ Phase 3 (状态栏仪表) ← 依赖陪伴锚点集成 + ipc RTT
  └──→ Phase 4 (排版+Markdown)
           └──→ Phase 6 (动效) ← 依赖所有视觉模块就位
                    └──→ Phase 7 (收尾)
```

**建议实施顺序**：0 → 1 → 2 → 3 → 4 → 5 → 6 → 7

Phase 4 可与 Phase 2/3 并行（不同文件，无冲突）；Phase 1 必须独立完成（改动面最大）。

---

## 附录 C：图标-令牌-UI 落点映射表

| 图标 | 令牌色 | 落点模块 | 替换的 v2 元素 |
|:-----|:-------|:---------|:---------------|
| logo | brand→brand_2 渐变 | top_bar | 品牌方块 |
| chevron-down | t2 (继承) | file_panel, top_bar | ▼ Unicode |
| chevron-left | t3 (继承) | file_panel, content_tabs | ◀ Unicode |
| chevron-right | t1 (继承) | activity_bar, content_tabs | ▶ Unicode |
| plus | t1 (继承) | top_bar, content_tabs | + 文字 |
| x | t2 (继承) | confirm_dialog, content_tabs | ✕ Unicode |
| search | t2 (继承) | activity_bar, command_palette | 🔍 emoji |
| check | ok | tool_panel | ✓ Unicode |
| gear | t2 (继承) | top_bar, activity_bar | ⚙️ emoji |
| send | brand | chat_panel | ➤ Unicode |
| info | pend | pend_card, confirm_dialog | ⚠️ emoji |
| folder | t1 (继承) | file_panel | 📁 emoji |
| folder-open | t1 (继承) | file_panel | 📂 emoji |
| file | t1 (继承) | content_tabs, file_panel | 📄 emoji |
| edit | t1 (继承) | activity_bar | 📝 emoji |
| refresh | t1 (继承) | file_panel | 🔄 emoji |
| clear | t1 (继承) | terminal | 🗑️ emoji |
| cp-idle | warm | companion | (新) |
| cp-think | warm | companion | (新) |
| cp-tool | warm | companion | (新) |
| cp-fail | fail | companion | (新) |
| cp-off | t3 | companion | (新) |
| pet | t1 (继承) | status_bar | (新) |
| sliders | t2 (继承) | status_bar | (新) |
| clock | t1 (继承) | tool_panel | (新) |
| play | t1 (继承) | command_palette | (新) |
| pause | t1 (继承) | tool_panel | (新) |
| eye | t1 (继承) | content_tabs | (新) |
| globe | t1 (继承) | command_palette | (新) |
| split | t1 (继承) | command_palette | (新) |
| box | t1 (继承) | command_palette | (新) |
| file-new | t1 (继承) | file_panel | (新) |
