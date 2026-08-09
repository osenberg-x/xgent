# UI BSN 重构实现方案（v2 细化版）

> 对照 `ui-prototype-v6.html`（对话画布 v6.1）与 `icons/`（31 个 Lucide 风格 24×24 线性图标），将 XGent UI 从当前手写 Node bundle 方式重构为 **Bevy Scene Notation（BSN）** 声明式方式 + **bevy_resvg** 矢量图标加载，并补齐原型全部视觉/交互差距。
>
> **v2 变更摘要**（相对 v1）：
> - 图标方案从「build.rs 光栅化 SVG→PNG」改为 **bevy_resvg 运行时加载 SVG**，保留矢量源文件、支持热重载
> - 修正 `currentColor` 着色问题（黑色底图 tint 无效 → 预处理为白色底图）
> - 明确 `xui` **不依赖 bevy_feathers**，主题系统自建（ThemeToken/ThemedBg/ThemedText/ThemedBorder）
> - BSN 组件代码示例对齐 `bevy_scene::SceneComponent` + `bevy_feathers` 实际 API（已验证源码）
> - 新增 bevy_resvg 集成细节、2x 视网膜渲染、dev 热重载工作流
> - 细化各阶段步骤与验收标准
>
> **v2.1 Review 修正**（对照 `ui-bsn-refactor-review.md`）：
> - **P0-1**: `NodePosition::Absolute` → `PositionType::Absolute`（类型名错误，编译失败）
> - **P0-2**: `update_theme_colors` 中 `q_text.entity(0)` → `(Entity, &ThemedText)` 迭代（运行时崩溃）
> - **P0-3**: build.rs 路径 `../doc/design/icons` → `../../doc/design/icons`（路径少一级）
> - **P0-4**: `ThemedIcon` 与 `UiSvg` 必须在同一实体（否则 bevy_resvg `sync_svg_color_changes` 找不到 `SvgColor`，着色失效）；`ThemedIcon` 加 `#[require(SvgColor)]`
> - **P0-5**: 缺少 `HierarchyPropagatePlugin::<TextColor, With<ThemedText>>::new(PostUpdate)` 注册（`Propagate` 不生效）
> - **P0-6**: `ThemedText(pub ThemeToken)` 混淆了 bevy_feathers 的 `InheritableThemeTextColor`（父令牌+传播）与 `ThemedText`（子标记）两种角色 → 拆为 `InheritableThemedText` + `ThemedTextColor` + `ThemedText` 标记三组件
> - **P1-4**: build.rs 补逐文件 `rerun-if-changed`
> - **P1-5**: Markdown 解析器改用 `pulldown-cmark`
> - **P1-6**: `bevy_resvg` 设为 `xui` 的 feature flag（`default = ["icons"]`）

---

## 1. 背景与动机

### 1.1 当前状态 vs v6 原型目标

| 维度 | 当前实现 | v6 原型目标 |
|:---|:---|:---|
| 构建方式 | 手写 `Node` + `BackgroundColor` + `Text` 内联 bundle | BSN 声明式 `bsn! { ... }` 场景组合 |
| 主题 | 仅暗色，`Theme` 28 字段硬编码 `Color::srgba` | 亮/暗双主题，设计令牌驱动，运行时切换 |
| 图标 | emoji 字符（📁 📄 🖥 ⚙ 🔍 🔧 ✦） | 31 个 24×24 SVG 矢量图标，bevy_resvg 加载，运行时着色 |
| 布局 | 顶栏 + 活动栏 + 文件面板 + 对话 + SideView + 状态栏 | 顶栏 + 活动栏 + 对话 + 右侧上下文面板 + 滑出抽屉 |
| 顶栏 | 品牌 + provider 下拉 + 新建会话 + 设置 | 品牌 + 项目面包屑 + 新建会话 + agent 状态 pill + 模型选择 + 主题切换 + 命令面板 |
| 对话区 | viewtabs + 消息流 + 输入卡片 | 上下文 scope 条 + 欢迎仪表盘 + 消息流（hover 操作栏）+ 快捷操作 + 增强输入 |
| 消息 | 纯 `Text` 节点 | 头像 + 角色名 + 时间 + 正文（markdown）+ hover 操作栏 |
| 工具卡 | 内联工具时间线 | 可折叠卡片 + 复制按钮 + 语法高亮预览 |
| 侧面板 | SideView（编辑器/预览/终端互斥） | 右侧上下文面板（预览/Diff/终端 tab 切换）+ 可折叠 |
| 抽屉 | 无 | 文件/搜索/Git/历史/终端 5 个滑出抽屉 |
| 交互 | 基本点击 + 流式光标 | 消息 hover 操作、代码块复制、滚动到底部、状态栏可点击、命令面板执行 |
| 陪伴 | 无 | 状态栏 + 活动栏底部陪伴开关（暖色脉冲） |

### 1.2 为什么用 BSN

**BSN（Bevy Scene Notation）** 是 Bevy 0.19 引入的声明式场景组合系统（`bevy_scene` crate），核心能力：

1. **场景组合**：`bsn! { ... }` 宏以声明式语法描述实体 + 组件 + 子层级，替代冗长的 `commands.spawn((...)).with_children(...)` 链式调用。
2. **`SceneComponent` 派生**：将一个 Component 转为可继承的「场景组件」，带 `Props` 参数 + `scene()` 工厂函数，实现 UI 组件封装（按钮、卡片、pill 等）。派生自动实现 `Component` + `FromTemplate`，且在 debug 模式下检测「直接 spawn 场景组件而非通过场景 spawn」的错误。
3. **模板补丁**：`~Template` 语法做字段级 override，复用场景时只改个别字段（如只改按钮宽度）。
4. **`SpawnRelated` trait**：为 `Children` 等 entity relationship 提供命名子层级 spawn API。
5. **`bevy_feathers` 全量验证**：官方 UI 组件库已用 BSN 重写全部控件（button/slider/checkbox/menu/...），模式成熟。
6. **bevy_resvg 原生支持**：`UiSvg("path.svg")` 可直接在 `bsn! {}` 中使用，与场景系统无缝集成。

**重构收益**：
- UI 代码量预估减少 40-50%（消除重复 Node bundle 模板）
- 样式与结构分离（令牌 → BSN 组件 → 场景组合三层）
- 组件可复用、可测试、可组合
- 与官方 `bevy_feathers` 模式对齐，降低维护成本
- SVG 图标矢量保真 + 运行时着色 + dev 热重载

---

## 2. 目标架构

### 2.1 分层与依赖

```
xui_i18n (trait, 零依赖)
    ↑
xui (BSN 组件库: 令牌 + 主题 + 基础组件 + bevy_resvg 图标)
    ├── 依赖: bevy, xui_i18n, bevy_resvg, thiserror, tree-sitter, ropey
    └── 不依赖任何 xgent_* crate，可独立发布
    ↑
xgent_ui (业务 UI: 面板 + 场景 + 系统)
    ↑
xgent_app (入口: 组装)
```

> **硬性约束**：`xui` 仅依赖 `bevy` + `xui_i18n` + 工具类 crate（tree-sitter/ropey/thiserror）+ `bevy_resvg`，**不依赖 `bevy_feathers`**（非稳定 API）也**不依赖任何 `xgent_*`**（保证可独立发布）。主题系统在 `xui` 内自建，模式对齐 `bevy_feathers` 但代码独立。

### 2.2 xui crate 新增模块

```
xui/src/
├── lib.rs              # XuiPlugin（注册主题系统 + bevy_resvg SvgPlugin + 基础组件系统）
├── theme.rs            # [新] ThemeToken + XuiTheme + ThemedBg/ThemedText/ThemedBorder + 解析系统
├── tokens.rs           # [新] 设计令牌常量表（对应 CSS 变量，~50 个）
├── palette.rs          # [新] 亮/暗调色板构建函数（token → Color 映射）
├── icon.rs             # [新] bevy_resvg 图标系统（路径常量 + ThemedIcon + 场景函数 + tint 系统）
├── constants.rs        # [新] 尺寸/间距/排版常量（对齐 v6 CSS）
├── components/         # [新] BSN 基础组件库
│   ├── mod.rs
│   ├── button.rs       # XButton（主/次/幽灵变体）
│   ├── icon_button.rs  # XIconButton（图标 + tooltip）
│   ├── pill.rs         # XPill（agent 状态指示，5 状态 + 脉冲动画）
│   ├── chip.rs         # XChip（上下文标签，可关闭）
│   ├── card.rs         # XCard（卡片容器）
│   ├── tab.rs          # XTabs（标签页）
│   ├── tooltip.rs      # XTooltip（悬浮提示 + 快捷键）
│   └── toast.rs        # XToast（通知）
├── scroll_area.rs      # [保留] ScrollArea + StickToBottom
├── scrollbar.rs        # [保留] 滚动条
├── virtual_list.rs     # [保留] 虚拟列表
├── mouse_wheel_scroll.rs # [保留]
├── text_editor/        # [保留] 代码编辑器
├── command_palette.rs  # [保留] 命令面板状态
├── hotkeys.rs          # [保留] 快捷键
├── shortcuts.rs        # [保留]
├── input.rs            # [保留] ChatInput
└── i18n_bridge.rs      # [保留] i18n 桥接
```

### 2.3 xgent_ui crate 重构后结构

```
xgent_ui/src/
├── lib.rs              # XgentUiPlugin（组装子插件）
├── layout.rs           # [重写] BSN 根布局
├── scenes/             # [新] BSN 场景定义
│   ├── mod.rs
│   ├── topbar.rs       # 顶栏场景
│   ├── rail.rs         # 活动栏场景
│   ├── conversation.rs # 对话区场景
│   ├── input_zone.rs   # 输入区场景
│   ├── context_panel.rs# 右侧上下文面板场景
│   ├── statusbar.rs    # 状态栏场景
│   └── drawers.rs      # 滑出抽屉场景
├── panels/             # [重组] 业务面板逻辑
│   ├── mod.rs
│   ├── chat.rs         # 对话面板（消息流 + 流式 + markdown）
│   ├── file_tree.rs    # 文件树
│   ├── file_preview.rs # 文件预览
│   ├── terminal.rs     # 终端
│   ├── editor.rs       # 编辑器
│   ├── git.rs          # [新] Git 抽屉
│   ├── search.rs       # [新] 搜索抽屉
│   └── history.rs      # [新] 历史抽屉
├── overlays/           # [重组] 浮层
│   ├── mod.rs
│   ├── command_palette.rs
│   ├── confirm_dialog.rs
│   ├── settings.rs
│   └── dropdown.rs     # [新] 通用下拉菜单
├── markdown.rs         # [新] 轻量 markdown 解析
├── resize.rs           # [保留] 拖拽手柄
├── shortcuts.rs        # [保留] 快捷键
└── i18n.rs             # [保留]
```

---

## 3. 设计令牌系统（xui 自建，不依赖 bevy_feathers）

### 3.1 主题系统核心类型

参照 `bevy_feathers::theme` 的设计，在 `xui` 内自建等价类型。核心模式：令牌组件 + `On<Insert>` 观察者 + `theme.is_changed()` 全量刷新系统。

```rust
// xui/src/theme.rs
use bevy_app::{Propagate, PropagateOver, HierarchyPropagatePlugin};
use bevy_color::{palettes, Color};
use bevy_ecs::{
    change_detection::DetectChanges, component::Component, lifecycle::Insert,
    observer::On, query::Changed, reflect::*, resource::Resource, system::*,
};
use bevy_log::warn_once;
use bevy_platform::collections::HashMap;
use bevy_reflect::Reflect;
use bevy_text::TextColor;
use bevy_ui::{BackgroundColor, BorderColor};
use smol_str::SmolStr;

/// 设计令牌 — 主题属性的查找键（与 bevy_feathers::ThemeToken 等价）
#[derive(Clone, PartialEq, Eq, Hash, Reflect, Default)]
pub struct ThemeToken(SmolStr);

impl ThemeToken {
    pub const fn new_static(text: &'static str) -> Self {
        Self(SmolStr::new_static(text))
    }
}

/// 主题属性集合
#[derive(Default, Clone, Reflect, Debug)]
#[reflect(Default, Debug)]
pub struct ThemeProps {
    pub color: HashMap<ThemeToken, Color>,
}

/// 当前 UI 主题。覆盖此 Resource 即可切换主题。
#[derive(Resource, Default, Reflect, Debug)]
#[reflect(Resource, Default, Debug)]
pub struct XuiTheme(pub ThemeProps);

impl XuiTheme {
    /// 按令牌查颜色。未找到时告警并返回醒目错误色（品红）。
    pub fn color(&self, token: &ThemeToken) -> Color {
        match self.0.color.get(token) {
            Some(c) => *c,
            None => {
                warn_once!("主题颜色 {} 未找到。", token);
                palettes::basic::FUCHSIA.into()
            }
        }
    }

    pub fn set_color(&mut self, token: &str, color: Color) {
        self.0.color.insert(ThemeToken(SmolStr::new(token)), color);
    }
}

// ===== 令牌组件（On<Insert> 观察者解析为实际颜色组件）=====

/// 背景：插入时解析令牌 → 写入 BackgroundColor
#[derive(Component, Clone, Default)]
#[require(BackgroundColor)]
#[component(immutable)]
#[derive(Reflect)]
#[reflect(Component, Clone)]
pub struct ThemedBg(pub ThemeToken);

/// 边框：插入时解析令牌 → 写入 BorderColor
#[derive(Component, Clone, Default)]
#[require(BorderColor)]
#[component(immutable)]
#[derive(Reflect)]
#[reflect(Component, Clone)]
pub struct ThemedBorder(pub ThemeToken);

/// 可继承文字色令牌 — 放在父实体上，通过 Propagate 向下传播 TextColor。
/// 对应 bevy_feathers::InheritableThemeTextColor。
/// 子实体需加 ThemedText 标记才会接收传播的 TextColor。
#[derive(Component, Clone, Default)]
#[component(immutable)]
#[derive(Reflect)]
#[reflect(Component, Clone)]
#[require(ThemedText, PropagateOver::<TextColor>)]
pub struct InheritableThemedText(pub ThemeToken);

/// 直接文字色令牌 — 放在 Text 实体本身上，直接设置 TextColor（不传播）。
/// 对应 bevy_feathers::ThemeTextColor。
/// 用法：代码高亮 span、状态栏文字等需精确控制且不受父级传播影响的场景。
#[derive(Component, Clone, Default)]
#[component(immutable)]
#[require(TextColor)]
#[derive(Reflect)]
#[reflect(Component, Clone)]
pub struct ThemedTextColor(pub ThemeToken);

/// 文字色继承标记 — 放在需要继承父级文字色的 Text 实体上。
/// 对应 bevy_feathers::ThemedText（单元标记组件，无字段）。
/// HierarchyPropagatePlugin::<TextColor, With<ThemedText>> 仅向带此标记的实体传播。
#[derive(Component, Reflect, Default, Clone)]
#[reflect(Component)]
pub struct ThemedText;  // 无字段标记组件

// ===== 解析系统 =====

/// 主题变更时全量刷新所有令牌组件
pub fn update_theme_colors(
    theme: Res<XuiTheme>,
    mut q_bg: Query<(&mut BackgroundColor, &ThemedBg)>,
    mut q_border: Query<(&mut BorderColor, &ThemedBorder)>,
    mut q_direct: Query<(&mut TextColor, &ThemedTextColor)>,
    q_inheritable: Query<(Entity, &InheritableThemedText)>,
    mut commands: Commands,
) {
    if !theme.is_changed() {
        return;
    }
    // 背景/边框/直接文字色 — 直接修改组件
    for (mut bg, themed) in &mut q_bg {
        bg.0 = theme.color(&themed.0);
    }
    for (mut border, themed) in &mut q_border {
        border.set_all(theme.color(&themed.0));
    }
    for (mut text_color, themed) in &mut q_direct {
        text_color.0 = theme.color(&themed.0);
    }
    // 可继承文字色 — 重新插入 Propagate 触发传播更新
    // （bevy_feathers 缺少此步，导致主题切换时继承色不更新）
    for (entity, themed) in &q_inheritable {
        let color = theme.color(&themed.0);
        commands.entity(entity).insert(Propagate(TextColor(color)));
    }
}

/// 新实体插入 ThemedBg 时立即解析
pub fn on_insert_themed_bg(
    insert: On<Insert, ThemedBg>,
    mut q: Query<(&mut BackgroundColor, &ThemedBg), Changed<ThemedBg>>,
    theme: Res<XuiTheme>,
) {
    if let Ok((mut bg, themed)) = q.get_mut(insert.entity) {
        bg.0 = theme.color(&themed.0);
    }
}

// on_insert_themed_border 同理...

/// InheritableThemedText 插入时 → 解析令牌 → 插入 Propagate(TextColor)
pub fn on_insert_inheritable_text(
    insert: On<Insert, InheritableThemedText>,
    q: Query<&InheritableThemedText>,
    theme: Res<XuiTheme>,
    mut commands: Commands,
) {
    if let Ok(token) = q.get(insert.entity) {
        let color = theme.color(&token.0);
        commands.entity(insert.entity).insert(Propagate(TextColor(color)));
    }
}

/// ThemedTextColor 插入时 → 解析令牌 → 直接设置 TextColor
pub fn on_insert_themed_text_color(
    insert: On<Insert, ThemedTextColor>,
    mut q: Query<(&mut TextColor, &ThemedTextColor)>,
    theme: Res<XuiTheme>,
) {
    if let Ok((mut text_color, themed)) = q.get_mut(insert.entity) {
        text_color.0 = theme.color(&themed.0);
    }
}
```

### 3.2 令牌常量定义

将 v6 原型 `:root` + `[data-theme="dark"]` 的 ~50 个 CSS 变量平移为 Rust 常量：

```rust
// xui/src/tokens.rs
use crate::theme::ThemeToken;

// ===== 背景深度（stone 系中性色）=====
pub const BG_CANVAS: ThemeToken = ThemeToken::new_static("xgent.bg.canvas");
pub const BG_SURFACE: ThemeToken = ThemeToken::new_static("xgent.bg.surface");
pub const BG_ELEVATED: ThemeToken = ThemeToken::new_static("xgent.bg.elevated");
pub const BG_RAIL: ThemeToken = ThemeToken::new_static("xgent.bg.rail");
pub const BG_INPUT: ThemeToken = ThemeToken::new_static("xgent.bg.input");
pub const BG_CODE: ThemeToken = ThemeToken::new_static("xgent.bg.code");

// ===== 边框 =====
pub const BORDER: ThemeToken = ThemeToken::new_static("xgent.border");
pub const BORDER_LIGHT: ThemeToken = ThemeToken::new_static("xgent.border.light");
pub const BORDER_STRONG: ThemeToken = ThemeToken::new_static("xgent.border.strong");

// ===== 文字五档 =====
pub const T0: ThemeToken = ThemeToken::new_static("xgent.text.0");
pub const T1: ThemeToken = ThemeToken::new_static("xgent.text.1");
pub const T2: ThemeToken = ThemeToken::new_static("xgent.text.2");
pub const T3: ThemeToken = ThemeToken::new_static("xgent.text.3");
pub const T4: ThemeToken = ThemeToken::new_static("xgent.text.4");

// ===== 品牌色 =====
pub const ACCENT: ThemeToken = ThemeToken::new_static("xgent.accent");
pub const ACCENT_HOVER: ThemeToken = ThemeToken::new_static("xgent.accent.hover");
pub const ACCENT_LIGHT: ThemeToken = ThemeToken::new_static("xgent.accent.light");
pub const ACCENT_GLOW: ThemeToken = ThemeToken::new_static("xgent.accent.glow");
pub const ACCENT_TEXT: ThemeToken = ThemeToken::new_static("xgent.accent.text");

// ===== 语义状态色 =====
pub const SUCCESS: ThemeToken = ThemeToken::new_static("xgent.success");
pub const SUCCESS_BG: ThemeToken = ThemeToken::new_static("xgent.success.bg");
pub const WARNING: ThemeToken = ThemeToken::new_static("xgent.warning");
pub const WARNING_BG: ThemeToken = ThemeToken::new_static("xgent.warning.bg");
pub const ERROR: ThemeToken = ThemeToken::new_static("xgent.error");
pub const ERROR_BG: ThemeToken = ThemeToken::new_static("xgent.error.bg");
pub const INFO: ThemeToken = ThemeToken::new_static("xgent.info");
pub const INFO_BG: ThemeToken = ThemeToken::new_static("xgent.info.bg");

// ===== 陪伴暖色 =====
pub const WARM: ThemeToken = ThemeToken::new_static("xgent.warm");
pub const WARM_LIGHT: ThemeToken = ThemeToken::new_static("xgent.warm.light");

// ===== 语法高亮 =====
pub const SYN_KW: ThemeToken = ThemeToken::new_static("xgent.syntax.kw");
pub const SYN_FN: ThemeToken = ThemeToken::new_static("xgent.syntax.fn");
pub const SYN_STR: ThemeToken = ThemeToken::new_static("xgent.syntax.str");
pub const SYN_NUM: ThemeToken = ThemeToken::new_static("xgent.syntax.num");
pub const SYN_TY: ThemeToken = ThemeToken::new_static("xgent.syntax.ty");
pub const SYN_COM: ThemeToken = ThemeToken::new_static("xgent.syntax.com");
```

### 3.3 调色板

亮/暗双主题，值取自 v6 原型 `:root` 与 `[data-theme="dark"]`：

```rust
// xui/src/palette.rs
use crate::{theme::*, tokens::*};
use bevy_color::Color;
use bevy_platform::collections::HashMap;

pub fn light_theme() -> XuiTheme {
    let mut color = HashMap::new();
    // 背景（stone 中性色，浅→深）
    color.insert(BG_CANVAS, Color::srgb_u8(0xFA, 0xFA, 0xF9));
    color.insert(BG_SURFACE, Color::srgb_u8(0xFF, 0xFF, 0xFF));
    color.insert(BG_ELEVATED, Color::srgb_u8(0xF5, 0xF5, 0xF4));
    color.insert(BG_RAIL, Color::srgb_u8(0xFA, 0xFA, 0xF9));
    color.insert(BG_INPUT, Color::srgb_u8(0xFF, 0xFF, 0xFF));
    color.insert(BG_CODE, Color::srgb_u8(0x1C, 0x19, 0x17));
    // 边框
    color.insert(BORDER, Color::srgb_u8(0xE7, 0xE5, 0xE4));
    color.insert(BORDER_LIGHT, Color::srgb_u8(0xF5, 0xF5, 0xF4));
    color.insert(BORDER_STRONG, Color::srgb_u8(0xD6, 0xD3, 0xD1));
    // 文字（stone-900 → stone-400）
    color.insert(T0, Color::srgb_u8(0x1C, 0x19, 0x17));
    color.insert(T1, Color::srgb_u8(0x44, 0x40, 0x3C));
    color.insert(T2, Color::srgb_u8(0x78, 0x71, 0x6C));
    color.insert(T3, Color::srgb_u8(0xA8, 0xA2, 0x9E));
    color.insert(T4, Color::srgb_u8(0xD6, 0xD3, 0xD1));
    // 品牌（琥珀 amber-500）
    color.insert(ACCENT, Color::srgb_u8(0xF5, 0x9E, 0x0B));
    color.insert(ACCENT_HOVER, Color::srgb_u8(0xD9, 0x77, 0x06));
    color.insert(ACCENT_LIGHT, Color::srgb_u8(0xFE, 0xF3, 0xC7));
    color.insert(ACCENT_GLOW, Color::srgba(0xF5, 0x9E, 0x0B, 0.15));
    color.insert(ACCENT_TEXT, Color::srgb_u8(0xB4, 0x53, 0x09));
    // 语义色（emerald/amber/red/blue）
    color.insert(SUCCESS, Color::srgb_u8(0x10, 0xB9, 0x81));
    color.insert(SUCCESS_BG, Color::srgb_u8(0xD1, 0xFA, 0xE5));
    color.insert(WARNING, Color::srgb_u8(0xF5, 0x9E, 0x0B));
    color.insert(WARNING_BG, Color::srgb_u8(0xFE, 0xF3, 0xC7));
    color.insert(ERROR, Color::srgb_u8(0xEF, 0x44, 0x44));
    color.insert(ERROR_BG, Color::srgb_u8(0xFE, 0xE2, 0xE2));
    color.insert(INFO, Color::srgb_u8(0x3B, 0x82, 0xF6));
    color.insert(INFO_BG, Color::srgb_u8(0xDB, 0xEA, 0xFE));
    // 陪伴暖色（暖橙渐变）
    color.insert(WARM, Color::srgb_u8(0xFB, 0x92, 0x36));
    color.insert(WARM_LIGHT, Color::srgb_u8(0xFD, 0xE6, 0x8A));
    // 语法高亮（浅色主题：深色文字）
    color.insert(SYN_KW, Color::srgb_u8(0xC0, 0x26, 0xD3));  // fuchsia-600
    color.insert(SYN_FN, Color::srgb_u8(0x25, 0x63, 0xEB));   // blue-600
    color.insert(SYN_STR, Color::srgb_u8(0x05, 0x96, 0x69));   // emerald-600
    color.insert(SYN_NUM, Color::srgb_u8(0xEA, 0x58, 0x0C));   // orange-600
    color.insert(SYN_TY, Color::srgb_u8(0x7C, 0x3A, 0xED));    // violet-600
    color.insert(SYN_COM, Color::srgb_u8(0x78, 0x71, 0x6C));   // stone-500
    XuiTheme(ThemeProps { color })
}

pub fn dark_theme() -> XuiTheme {
    let mut color = HashMap::new();
    // 背景（stone 深色系）
    color.insert(BG_CANVAS, Color::srgb_u8(0x1C, 0x19, 0x17));
    color.insert(BG_SURFACE, Color::srgb_u8(0x29, 0x25, 0x24));
    color.insert(BG_ELEVATED, Color::srgb_u8(0x44, 0x40, 0x3C));
    color.insert(BG_RAIL, Color::srgb_u8(0x1C, 0x19, 0x17));
    color.insert(BG_INPUT, Color::srgb_u8(0x29, 0x25, 0x24));
    color.insert(BG_CODE, Color::srgb_u8(0x0C, 0x0A, 0x09));
    // 边框
    color.insert(BORDER, Color::srgb_u8(0x44, 0x40, 0x3C));
    color.insert(BORDER_LIGHT, Color::srgb_u8(0x29, 0x25, 0x24));
    color.insert(BORDER_STRONG, Color::srgb_u8(0x57, 0x53, 0x4E));
    // 文字
    color.insert(T0, Color::srgb_u8(0xFA, 0xFA, 0xF9));
    color.insert(T1, Color::srgb_u8(0xE7, 0xE5, 0xE4));
    color.insert(T2, Color::srgb_u8(0xA8, 0xA2, 0x9E));
    color.insert(T3, Color::srgb_u8(0x78, 0x71, 0x6C));
    color.insert(T4, Color::srgb_u8(0x57, 0x53, 0x4E));
    // 品牌
    color.insert(ACCENT, Color::srgb_u8(0xFB, 0xBF, 0x24));
    color.insert(ACCENT_HOVER, Color::srgb_u8(0xF5, 0x9E, 0x0B));
    color.insert(ACCENT_LIGHT, Color::srgba(0xFB, 0xBF, 0x24, 0.12));
    color.insert(ACCENT_GLOW, Color::srgba(0xFB, 0xBF, 0x24, 0.2));
    color.insert(ACCENT_TEXT, Color::srgb_u8(0xFB, 0xBF, 0x24));
    // 语义色
    color.insert(SUCCESS, Color::srgb_u8(0x34, 0xD3, 0x99));
    color.insert(SUCCESS_BG, Color::srgba(0x52, 0x4B, 0x3A, 0.12));
    color.insert(WARNING, Color::srgb_u8(0xFB, 0xBF, 0x24));
    color.insert(WARNING_BG, Color::srgba(0x5C, 0x4A, 0x14, 0.3));
    color.insert(ERROR, Color::srgb_u8(0xF8, 0x71, 0x71));
    color.insert(ERROR_BG, Color::srgba(0x5C, 0x2A, 0x2A, 0.3));
    color.insert(INFO, Color::srgb_u8(0x60, 0xA5, 0xFA));
    color.insert(INFO_BG, Color::srgba(0x2A, 0x3A, 0x5C, 0.3));
    // 陪伴暖色
    color.insert(WARM, Color::srgb_u8(0xFB, 0x92, 0x36));
    color.insert(WARM_LIGHT, Color::srgba(0xFB, 0x92, 0x36, 0.15));
    // 语法高亮（深色主题：浅色文字）
    color.insert(SYN_KW, Color::srgb_u8(0xE8, 0x79, 0xF9));
    color.insert(SYN_FN, Color::srgb_u8(0x93, 0xC5, 0xFD));
    color.insert(SYN_STR, Color::srgb_u8(0x6E, 0xE7, 0x96));
    color.insert(SYN_NUM, Color::srgb_u8(0xFB, 0xBF, 0x24));
    color.insert(SYN_TY, Color::srgb_u8(0xC4, 0xB5, 0xFD));
    color.insert(SYN_COM, Color::srgb_u8(0x78, 0x71, 0x6C));
    XuiTheme(ThemeProps { color })
}
```

### 3.4 令牌消费（BSN 中）

```rust
bsn! {
    Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
    }
    ThemedBg(BG_SURFACE)              // → 插入时解析为 BackgroundColor
    ThemedBorder(BORDER)              // → 插入时解析为 BorderColor
    InheritableThemedText(T1)         // → Propagate 向下传播 TextColor
    Children [
        Text("Hello")                 // 子实体
        ThemedText                     // ← 标记：我要继承文字色
    ]
}

// 代码高亮 span — 用 ThemedTextColor 直接设色，不受父级传播影响
bsn! {
    Text("fn")
    ThemedTextColor(SYN_KW)           // 直接色，不需要 ThemedText 标记
}
```

### 3.5 运行时切换

```rust
fn toggle_theme(mut commands: Commands, current: Res<CurrentTheme>) {
    let next = if current.0 == ThemeMode::Light {
        dark_theme()
    } else {
        light_theme()
    };
    commands.insert_resource(next);
    // update_theme_colors 系统检测 is_changed() → 全量刷新所有 ThemedBg/ThemedBorder/ThemedText
    // update_icon_tints 系统同理刷新所有 ThemedIcon → SvgColor
}
```

---

## 4. 矢量图标系统（bevy_resvg）

### 4.1 方案选型

| 方案 | 做法 | 优点 | 缺点 | 决定 |
|:---|:---|:---|:---|:---|
| ~~A. build.rs 光栅化 PNG~~ | 构建期 SVG→PNG，内嵌 `embedded://` | 零运行时开销 | 丢失矢量源、无热重载、build.rs 复杂 | **否**（v1 方案，已废弃） |
| **B. bevy_resvg 运行时加载** | `SvgPlugin` + `UiSvg` + `SvgColor` tint | 保留 SVG 源文件、dev 热重载、BSN 原生支持、API 简洁 | 启动期一次性光栅化开销（31 图标 ≈ <50ms） | **是** |
| C. bevy_vello | GPU JIT 光栅化 | 任意分辨率清晰 | 复杂 SVG 支持差、不支持运行时改色 | 否 |

**采用方案 B**：`bevy_resvg` 2.5+，SVG 一次性光栅化为纹理，渲染为 `ImageNode`，通过 `SvgColor` 组件 tint。

### 4.2 bevy_resvg API 概要（已验证源码）

| 类型 | 角色 | BSN 用法 |
|:---|:---|:---|
| `SvgPlugin` | 注册 SVG 资源管线（AssetLoader + 光栅化） | `app.add_plugins(SvgPlugin)` |
| `SvgFile` | SVG 资产类型 | `asset_server.load("icons/chat.svg")` 返回 `Handle<SvgFile>` |
| `UiSvg` | UI 组件，渲染为 `ImageNode` | `bsn! { UiSvg("icons/chat.svg") }`（字符串路径，BSN 原生） |
| `SvgColor(Color)` | tint 组件，乘法混合到渲染图像 | `bsn! { UiSvg("icons/chat.svg") SvgColor(Color::WHITE) }` |
| `SvgFileLoaderSettings` | 加载设置（`target_render_size`、`usvg::Options.style_sheet`） | `asset_server.load_builder().with_settings(...).load(...)` |

**关键发现**：
- `UiSvg("path")` 在 `bsn! {}` 中直接接受字符串路径，**无需预加载 Handle** — 与 BSN 无缝集成。
- `SvgColor` 可通过系统运行时修改：`Query<&mut SvgColor, With<UiSvg>>`。
- 渲染输出为标准 `ImageNode`，尺寸由父 `Node` 的 `width`/`height` 决定。

### 4.3 currentColor 着色问题与解决

**问题**：31 个 SVG 全部使用 `stroke="currentColor"`（Lucide 风格线性图标），`fill="none"`。bevy_resvg 底层 usvg 将 `currentColor` 解析为 SVG 默认 `color` 属性值 = **黑色** `#000000`。光栅化后得到**黑色描边**图像。`SvgColor` tint 是乘法混合：`黑色(0,0,0) × 任意颜色 = 黑色` — **tint 完全无效**。

**解决**：必须使底图为**白色**描边，这样 `白色(1,1,1) × 目标色 = 目标色`。

三种方案对比：

| 方案 | 做法 | 保留源 SVG | BSN 字符串路径 | 额外代码 | 决定 |
|:---|:---|:---|:---|:---|:---|
| **预处理拷贝** | build.rs 将 `currentColor` → `#ffffff` 拷贝到 assets | ✅ 源不变 | ✅ 支持 | build.rs（纯文本替换，无光栅化） | **是** |
| CSS 注入 | `load_builder().with_settings(style_sheet = "* { color: #fff }")` | ✅ | ❌ 需预加载 Handle | 预加载系统 + Handle 存储 | 否（太重） |
| 直接改源 | 直接修改 `doc/design/icons/*.svg` | ❌ 破坏原型 | ✅ | 无 | 否（破坏原型 `currentColor` 语义） |

**采用预处理拷贝**：`build.rs` 读取 `doc/design/icons/*.svg`，做纯文本替换（`currentColor` → `#ffffff`，`width="24"` → `width="48"` `height="24"` → `height="48"`），写入 `xui/assets/icons/*.svg`。**不做光栅化** — 光栅化由 bevy_resvg 在运行时完成。

### 4.4 2x 视网膜渲染

24×24 的 SVG 在 2x retina 屏上以 24px 显示时光栅化为 24×24 纹理，上采样到 48 物理像素会模糊。解决：预处理时将 `width`/`height` 属性改为 48（viewBox 保持 `0 0 24 24`），bevy_resvg 光栅化为 48×48 纹理，在 24px UI 节点中下采样显示 — 视网膜清晰。

```rust
// build.rs 文本替换规则
// 1. stroke="currentColor" → stroke="#ffffff"
// 2. width="24" → width="48"
// 3. height="24" → height="48"
// viewBox="0 0 24 24" 保持不变
```

### 4.5 build.rs 脚本（纯文本预处理，无光栅化）

```rust
// xui/build.rs
use std::{fs, path::PathBuf};

fn main() {
    // 注意：build.rs 在 crate 目录 crates/xui/ 下执行
    // 需要两级回退到 workspace 根：crates/xui/ → crates/ → workspace 根
    let icons_src = PathBuf::from("../../doc/design/icons");
    let icons_dst = PathBuf::from("assets/icons");

    // 创建输出目录
    fs::create_dir_all(&icons_dst).expect("创建 icons 输出目录失败");

    // 遍历源 SVG，文本替换后拷贝
    let mut count = 0;
    for entry in fs::read_dir(&icons_src).expect("读取 icons 源目录失败") {
        let entry = entry.expect("读取目录条目失败");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("svg") {
            continue;
        }
        // 逐文件 rerun-if-changed — 跟踪文件内容变化
        println!("cargo:rerun-if-changed={}", path.display());

        let content = fs::read_to_string(&path).expect("读取 SVG 失败");
        // currentColor → #ffffff（使 tint 生效）
        let processed = content.replace("currentColor", "#ffffff");
        // 24 → 48（2x retina 光栅化分辨率）
        let processed = processed
            .replace("width=\"24\"", "width=\"48\"")
            .replace("height=\"24\"", "height=\"48\"");

        let dst = icons_dst.join(entry.file_name());
        fs::write(&dst, &processed).expect("写入预处理 SVG 失败");
        count += 1;
    }

    println!("cargo:rerun-if-changed={}", icons_src.display());
    println!("cargo:warning=已预处理 {} 个 SVG 图标到 assets/icons/", count);
}
```

> **注意**：此 build.rs 仅做文本替换（~5 行逻辑），不做光栅化。SVG 光栅化由 bevy_resvg 在应用启动时完成（一次性，缓存为纹理）。build.rs 不依赖 resvg/usvg/tiny-skia。

### 4.6 图标路径常量

```rust
// xui/src/icon.rs
/// 图标 SVG 资产路径（相对于 assets/ 根目录）
pub mod icons {
    pub const CHAT: &str = "icons/chat.svg";
    pub const FOLDER: &str = "icons/folder.svg";
    pub const FILE: &str = "icons/file.svg";
    pub const SEARCH: &str = "icons/search.svg";
    pub const GIT: &str = "icons/git.svg";
    pub const CLOCK: &str = "icons/clock.svg";
    pub const TERMINAL: &str = "icons/terminal.svg";
    pub const PUZZLE: &str = "icons/puzzle.svg";
    pub const COMMAND: &str = "icons/command.svg";
    pub const PLUS: &str = "icons/plus.svg";
    pub const X: &str = "icons/x.svg";
    pub const CHEVRON_DOWN: &str = "icons/chevron-down.svg";
    pub const SUN: &str = "icons/sun.svg";
    pub const MOON: &str = "icons/moon.svg";
    pub const STAR: &str = "icons/star.svg";
    pub const PANEL_RIGHT: &str = "icons/panel-right.svg";
    pub const INFO: &str = "icons/info.svg";
    pub const CODE: &str = "icons/code.svg";
    pub const FLASK: &str = "icons/flask.svg";
    pub const WRENCH: &str = "icons/wrench.svg";
    pub const CHECK_CIRCLE: &str = "icons/check-circle.svg";
    pub const CHECK: &str = "icons/check.svg";
    pub const COPY: &str = "icons/copy.svg";
    pub const REFRESH: &str = "icons/refresh.svg";
    pub const RETRY: &str = "icons/retry.svg";
    pub const DIFF: &str = "icons/diff.svg";
    pub const SEND: &str = "icons/send.svg";
    pub const MIC: &str = "icons/mic.svg";
    pub const SHIELD: &str = "icons/shield.svg";
    pub const SHIELD_ALERT: &str = "icons/shield-alert.svg";
    pub const DOLLAR: &str = "icons/dollar.svg";
}
```

### 4.7 图标场景函数

`UiSvg` 渲染为 `ImageNode`，尺寸由父 `Node` 决定。封装一个带尺寸的图标场景：

```rust
// xui/src/icon.rs
use bevy_scene::prelude::*;
use bevy_ui::prelude::*;
use bevy_resvg::prelude::*;
use crate::{theme::*, tokens::*, constants};

/// 图标着色源：从哪个令牌取色写入 SvgColor。
/// 【关键】require(SvgColor) 确保 SvgColor 与 ThemedIcon 在同一实体。
/// bevy_resvg 的 sync_svg_color_changes 查询 (&SvgColor, &mut ImageNode, With<UiSvg>)
/// — 三者必须在同一实体，否则 tint 不生效。
#[derive(Component, Clone, Copy, Default)]
#[component(immutable)]
#[require(SvgColor)]          // ← 确保 SvgColor 自动插入到此实体
#[derive(Reflect)]
#[reflect(Component, Clone, Default)]
pub struct ThemedIcon(pub ThemeToken);

/// 带尺寸 + 令牌着色的图标场景。
/// 【关键架构修正】UiSvg 和 ThemedIcon 必须在同一子实体上，
/// 这样 SvgColor + ImageNode + UiSvg 三组件共存，bevy_resvg 的 tint 系统才能找到它们。
pub fn icon(path: &'static str, size: f32, tint: ThemeToken) -> impl Scene {
    bsn! {
        Node {
            width: px(size),
            height: px(size),
        }
        Children [
            // UiSvg + ThemedIcon(+SvgColor via require) 都在这个子实体上
            // bevy_resvg 加载 SVG 后在此实体插入 ImageNode
            // sync_svg_color_changes 在此实体找到 (SvgColor, ImageNode, UiSvg) → tint 生效
            UiSvg(path)
            ThemedIcon(tint)
        ]
    }
}
```

### 4.8 Cargo.toml 变更

```toml
# xui/Cargo.toml 新增
[features]
default = ["icons"]
icons = ["dep:bevy_resvg"]

[dependencies]
bevy_resvg = { version = "2.5", optional = true }
smol_str = "1"  # ThemeToken 用
```

```toml
# Cargo.toml [workspace.dependencies] 新增
bevy_resvg = "2.5"
smol_str = "1"
```

> bevy_resvg 2.5 的 MSRV 为 1.95（与 Bevy MSRV 一致）。bevy_resvg 重导出了 `resvg` crate，无需单独添加。
> `bevy_resvg` 设为 `xui` 的可选依赖（feature `icons`），不需要 SVG 图标的项目可不启用此 feature。

### 4.9 Plugin 注册

```rust
// xui/src/lib.rs
use bevy::prelude::*;
use bevy_app::HierarchyPropagatePlugin;
use bevy_text::TextColor;
use bevy_ecs::query::With;
#[cfg(feature = "icons")]
use bevy_resvg::prelude::SvgPlugin;
use crate::theme::*;

pub struct XuiPlugin;

impl Plugin for XuiPlugin {
    fn build(&self, app: &mut App) {
        app
            // bevy_resvg SVG 管线（feature gate）
            #[cfg(feature = "icons")]
            .add_plugins(SvgPlugin)
            // TextColor 层级传播插件 — 使 InheritableThemedText 的 Propagate 生效
            // With<ThemedText> 过滤器：仅向带 ThemedText 标记的子实体传播文字色
            // （防止覆盖代码高亮 span 的自有 TextColor）
            .add_plugins(
                HierarchyPropagatePlugin::<TextColor, With<ThemedText>>::new(PostUpdate)
            )
            // 主题系统（默认暗色）
            .insert_resource(dark_theme())
            // 令牌解析系统
            .add_systems(Update, (
                update_theme_colors,
                #[cfg(feature = "icons")]
                update_icon_tints,
            ))
            // On<Insert> 观察者
            .add_observer(on_insert_themed_bg)
            .add_observer(on_insert_themed_border)
            .add_observer(on_insert_inheritable_text)
            .add_observer(on_insert_themed_text_color)
            // 图标 tint 观察者（feature gate）
            .add_observer(
                #[cfg(feature = "icons")]
                on_insert_themed_icon
            );
    }
}
```

### 4.10 Dev 热重载

启用 Bevy `file_watcher` feature 后，修改 `doc/design/icons/*.svg` → build.rs 触发 → `assets/icons/*.svg` 更新 → bevy_resvg 热重载 SVG 资产，无需重启。

```toml
# 开发时启用
# Cargo.toml [workspace.dependencies] bevy features 含 "file_watcher"
# 或运行时: cargo run --features bevy/file_watcher
```

---

## 5. BSN 基础组件库（xui::components）

### 5.1 设计原则

- 每个组件 = `#[derive(SceneComponent)]` + `#[scene(Props)]` + `fn scene(props) -> impl Scene`
- 颜色全部走 `ThemedBg(token)` / `ThemedText(token)` / `ThemedBorder(token)`
- 图标走 `icon(path, size)` + `ThemedIcon(token)`
- 尺寸走 `constants::size` / `space` 常量
- 子层级通过 `Children [ ... ]` 或 `SceneList` 参数传入
- **场景组件不可直接 spawn**（SceneComponent derive 在 debug 模式会检测并报错），必须通过 `bsn! { @Component(props) }` 或 `spawn_scene` 使用

### 5.2 组件清单

| 组件 | Props | 场景结构 | 对应原型 |
|:---|:---|:---|:---|
| `XButton` | `caption, variant(Normal/Primary/Plain/Ghost), corners` | Node + Button + variant + ThemedBg + ThemedText + Children | `.btn`, `.btn-primary`, `.btn-ghost` |
| `XIconButton` | `icon, icon_size, button_size, tint` | Node(size) + Button + Hovered + icon() + ThemedIcon | `.icon-btn`, `.rail-btn` |
| `XPill` | `label, state(Ready/Thinking/Generating/Tool/Error)` | Node + ThemedBg + dot + Text + Children | `.agent-pill` |
| `XChip` | `label, icon, closable` | Node + icon + Text + close-btn | `.ctx-chip`, `.qa-chip` |
| `XCard` | `children` | Node + ThemedBg + ThemedBorder + radius + Children | `.tool-card`, `.welcome-card` |
| `XTabs` | `tabs, active` | Node(row) + tab items | `.context-tabs` |
| `XTooltip` | `text, kbd` | Node(abs) + Text + kbd | `.rail-tooltip` |
| `XToast` | `text, icon` | Node(abs) + icon + Text | `.toast` |

### 5.3 示例：XButton

```rust
// xui/src/components/button.rs
use bevy_scene::prelude::*;
use bevy_ui::prelude::*;
use bevy_picking::hover::Hovered;
use bevy_input_focus::tab_navigation::TabIndex;
use crate::{theme::*, tokens::*, constants};

#[derive(Clone, Copy, PartialEq, Eq, Default, Reflect)]
pub enum ButtonVariant {
    #[default]
    Normal,   // BG_ELEVATED 底 + T1 字
    Primary,  // ACCENT 底 + T0 字
    Plain,    // 透明底 + T1 字
    Ghost,    // 透明底 + T2 字，hover 显底
}

#[derive(SceneComponent, Default, Clone)]
#[scene(XButtonProps)]
#[derive(Reflect)]
#[reflect(Component, Clone, Default)]
pub struct XButton;

pub struct XButtonProps {
    pub variant: ButtonVariant,
    pub children: Box<dyn SceneList>,
    pub corners: f32,
}

impl Default for XButtonProps {
    fn default() -> Self {
        Self {
            variant: ButtonVariant::Normal,
            children: Box::new(()),
            corners: 8.0,
        }
    }
}

impl XButton {
    fn scene(props: XButtonProps) -> impl Scene {
        let (bg, text) = match props.variant {
            ButtonVariant::Normal => (BG_ELEVATED, T1),
            ButtonVariant::Primary => (ACCENT, T0),
            ButtonVariant::Plain => (BG_SURFACE, T1),
            ButtonVariant::Ghost => (BG_CANVAS, T2),
        };
        bsn! {
            Node {
                padding: UiRect::horizontal(px(constants::space::S3)),
                border_radius: BorderRadius::all(px(props.corners)),
                column_gap: px(constants::space::S2),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            }
            Button
            Hovered
            TabIndex(0)
            ThemedBg(bg)
            ThemedText(text)
            {props.children}
        }
    }
}
```

### 5.4 示例：XIconButton（含图标）

```rust
// xui/src/components/icon_button.rs
use bevy_scene::prelude::*;
use bevy_ui::prelude::*;
use bevy_picking::hover::Hovered;
use bevy_input_focus::tab_navigation::TabIndex;
use crate::{theme::*, tokens::*, icon, constants};

#[derive(SceneComponent, Default, Clone)]
#[scene(XIconButtonProps)]
#[derive(Reflect)]
#[reflect(Component, Clone, Default)]
pub struct XIconButton;

pub struct XIconButtonProps {
    pub icon: &'static str,
    pub icon_size: f32,
    pub button_size: f32,
    pub tint: ThemeToken,       // 图标着色令牌
}

impl Default for XIconButtonProps {
    fn default() -> Self {
        Self {
            icon: "",
            icon_size: 18.0,
            button_size: 34.0,
            tint: T2,
        }
    }
}

impl XIconButton {
    fn scene(props: XIconButtonProps) -> impl Scene {
        // icon() 返回的场景已将 UiSvg + ThemedIcon(+SvgColor) 放在同一子实体
        bsn! {
            Node {
                width: px(props.button_size),
                height: px(props.button_size),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(px(8.0)),
            }
            Button
            Hovered
            TabIndex(0)
            Children [
                {icon(props.icon, props.icon_size, props.tint)}
            ]
        }
    }
}
```

### 5.5 状态 Pill（agent 状态指示）

```rust
// xui/src/components/pill.rs
use bevy_scene::prelude::*;
use bevy_ui::prelude::*;
use crate::{theme::*, tokens::*, constants};

#[derive(Component, Default, Clone, Reflect, Debug, PartialEq, Eq)]
#[reflect(Component, Clone, Default)]
pub enum PillState {
    #[default]
    Ready,       // 灰底灰字
    Thinking,    // 品牌浅底 + 品牌字 + 脉冲动画
    Generating,  // 同上 + 快脉冲
    Tool,        // 暖色底 + 暖色字
    Confirm,     // 暖色底 + 暖色字
    Error,       // 红底红字
}

#[derive(SceneComponent, Default, Clone)]
#[scene(XPillProps)]
pub struct XPill;

pub struct XPillProps {
    pub label: Box<dyn SceneList>,
    pub state: PillState,
}

impl Default for XPillProps {
    fn default() -> Self {
        Self {
            label: Box::new(()),
            state: PillState::Ready,
        }
    }
}

impl XPill {
    fn scene(props: XPillProps) -> impl Scene {
        let (bg, text) = match props.state {
            PillState::Ready => (BG_ELEVATED, T1),
            PillState::Thinking | PillState::Generating => (ACCENT_LIGHT, ACCENT_TEXT),
            PillState::Tool | PillState::Confirm => (WARNING_BG, WARNING),
            PillState::Error => (ERROR_BG, ERROR),
        };
        bsn! {
            Node {
                padding: UiRect::horizontal(px(constants::space::S3)),
                border_radius: BorderRadius::all(px(16.0)),
                column_gap: px(constants::space::S2),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
            }
            ThemedBg(bg)
            ThemedText(text)
            props.state   // 组件标记，供脉冲动画系统查询
            Children [
                // 状态点
                Node {
                    width: px(6.0),
                    height: px(6.0),
                    border_radius: BorderRadius::all(px(3.0)),
                }
                ThemedBg(text)
                // 标签文字
                {props.label}
            ]
        }
    }
}
```

---

## 6. 布局重构（BSN 根场景）

### 6.1 CSS Grid → Bevy Flexbox 映射

v6 原型使用 CSS Grid 三行三列：

```css
.app {
  display: grid;
  grid-template-rows: var(--topbar-h) 1fr var(--statusbar-h);
  grid-template-columns: var(--rail-w) 1fr var(--context-w);
  grid-template-areas: "topbar topbar topbar" "rail main context" "status status status";
}
```

Bevy 无 CSS Grid，用嵌套 flexbox 等价映射：

```
UiRoot (column, 100%×100%)
├── TopBar (row, 100%×52px, flex_shrink:0)         ← grid-area: topbar
├── MainRow (row, 100%×flex_grow:1)                 ← grid-area: rail+main+context
│   ├── Rail (column, 52px×100%, flex_shrink:0)    ← grid-area: rail
│   ├── Main (column, flex_grow:1)                  ← grid-area: main
│   └── ContextPanel (column, 360px, flex_shrink:0) ← grid-area: context
└── StatusBar (row, 100%×32px, flex_shrink:0)        ← grid-area: status
```

### 6.2 BSN 根布局场景

```rust
// xgent_ui/src/layout.rs
use bevy_scene::prelude::*;
use bevy_ui::prelude::*;
use xui::{theme::*, tokens::*, constants};

/// 根布局 — 三行两列嵌套 flexbox
pub fn root_layout() -> impl Scene {
    bsn! {
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
        }
        UiRoot
        Children [
            // ===== 顶栏 =====
            Node {
                width: Val::Percent(100.0),
                height: px(constants::TOPBAR_H),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(constants::space::S3),
                padding: UiRect::horizontal(px(constants::space::S4)),
                flex_shrink: 0.0,
                border: UiRect::bottom(px(1.0)),
            }
            ThemedBg(BG_SURFACE)
            ThemedBorder(BORDER)
            TopBarMarker
            // ===== 中间行 =====
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                min_height: Val::ZERO,
            }
            MainRowMarker
            Children [
                // 活动栏
                Node {
                    width: px(constants::RAIL_W),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    padding: UiRect::vertical(px(constants::space::S3)),
                    row_gap: px(constants::space::XS),
                    flex_shrink: 0.0,
                    border: UiRect::right(px(1.0)),
                }
                ThemedBg(BG_RAIL)
                ThemedBorder(BORDER)
                RailMarker
                // 主区域
                Node {
                    flex_grow: 1.0,
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    min_width: Val::ZERO,
                    overflow: Overflow::clip(),
                }
                ThemedBg(BG_CANVAS)
                MainMarker
                // 右侧上下文面板
                Node {
                    width: px(constants::CONTEXT_W),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    border: UiRect::left(px(1.0)),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                }
                ThemedBg(BG_SURFACE)
                ThemedBorder(BORDER)
                ContextPanelMarker
            ]
            // ===== 状态栏 =====
            Node {
                width: Val::Percent(100.0),
                height: px(constants::STATUSBAR_H),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                border: UiRect::top(px(1.0)),
            }
            ThemedBg(BG_SURFACE)
            ThemedBorder(BORDER)
            StatusBarMarker
        ]
    }
}
```

### 6.3 尺寸常量

```rust
// xui/src/constants.rs
pub const TOPBAR_H: f32 = 52.0;
pub const RAIL_W: f32 = 52.0;
pub const STATUSBAR_H: f32 = 32.0;
pub const CONTEXT_W: f32 = 360.0;
pub const INPUT_MIN_H: f32 = 120.0;

pub mod space {
    pub const XS: f32 = 4.0;
    pub const S2: f32 = 8.0;
    pub const S3: f32 = 12.0;
    pub const S4: f32 = 16.0;
    pub const S5: f32 = 24.0;
    pub const S6: f32 = 32.0;
}

pub mod text {
    pub const XS: f32 = 11.0;   // 状态栏
    pub const SM: f32 = 13.0;   // 次要文字
    pub const BASE: f32 = 14.0; // 正文
    pub const LG: f32 = 16.0;   // 标题
    pub const XL: f32 = 20.0;   // 大标题
    pub const CODE: f32 = 13.0; // 代码
}
```

### 6.4 上下文面板折叠

v6 原型通过 `grid-template-columns` 动画切换实现面板折叠。Bevy 中等价为 `width: 0` + `display: None`：

```rust
fn toggle_context_panel(
    collapsed: Res<ContextPanelCollapsed>,
    mut q: Query<&mut Node, With<ContextPanelMarker>>,
) {
    if !collapsed.is_changed() { return; }
    if let Ok(mut node) = q.single_mut() {
        node.width = if collapsed.0 { Val::Px(0.0) } else { px(constants::CONTEXT_W) };
        node.display = if collapsed.0 { Display::None } else { Display::Flex };
        node.border = if collapsed.0 { UiRect::ZERO } else { UiRect::left(px(1.0)) };
    }
}
```

---

## 7. 面板场景设计

### 7.1 顶栏（TopBar）

```
TopBar (row, 52px)
├── Brand (brand-mark "X" + "XGent")
├── ProjectCrumb (folder icon + project name + chevron) [click → dropdown]
├── NewSessionBtn (plus icon + "新建会话")
├── Spacer (flex_grow:1)
├── AgentPill (dot + state text) [state-driven]
├── ModelSel (model icon + model name + chevron) [click → dropdown]
├── ThemeToggle (sun/moon icon)
└── CommandPaletteBtn (command icon)
```

关键交互：
- `AgentPill`：`PillState` 组件驱动 5 种状态颜色 + 脉冲动画（Thinking/Generating）
- `ProjectCrumb` / `ModelSel`：点击弹出 `Dropdown` 浮层
- `ThemeToggle`：切换 `XuiTheme` Resource

### 7.2 活动栏（Rail）

```
Rail (column, 52px)
├── RailBtn[chat] (active indicator + icon + tooltip)
├── RailBtn[files]
├── RailBtn[search]
├── RailBtn[git]
├── RailBtn[history]
├── Divider
├── RailBtn[terminal]
├── RailBtn[plugins]
├── ContextExpand (仅面板折叠时显示)
├── Spacer
└── CompanionBtn (star icon, warm gradient, pulse ring)
```

关键交互：
- Active 态：左侧 3px 竖条 + `ACCENT_LIGHT` 背景
- Tooltip：hover 时 opacity 0→1，含 `kbd` 快捷键提示
- `CompanionBtn`：暖色渐变背景 + 呼吸光环动画
- Rail 按钮点击触发 `RailAction` 消息 → 对应抽屉开关或视图切换

### 7.3 对话区（Main）

```
Main (column)
├── ContextScope (row, scroll-x) — 上下文 chip 条
│   ├── Label "上下文"
│   ├── CtxChip[file] × N (icon + name + close-x)
│   └── CtxAdd (plus + "添加上下文")
├── ScrollBottomBtn (abs, 仅滚动时显示)
├── Conversation (scroll-y, flex_grow:1)
│   ├── WelcomeDashboard (空状态时显示)
│   │   ├── WelcomeMark (gradient "X")
│   │   ├── H1 "开始新会话"
│   │   ├── WelcomeGrid (3 cards: 解释代码/重构/生成测试)
│   │   └── WelcomeSessions (最近会话列表)
│   └── MsgGroup (消息组)
│       └── Msg × N
│           ├── MsgHead (avatar + role + time)
│           ├── MsgActions (hover 显示: copy/regenerate/retry)
│           └── MsgBody (markdown: p/code/tool-card/ul/ol)
└── InputZone
    ├── QuickActions (row, wrap) — 5 个快捷操作 chip
    └── InputBox (border + radius + shadow)
        ├── InputTextarea (multiline, Enter 发送)
        └── InputToolbar (row)
            ├── InputMode (mode icon + label) [click → cycle]
            ├── InputSafety (shield icon + "工具调用需确认")
            ├── Spacer
            ├── InputHint (kbd Shift+Enter)
            ├── InputCounter (token 计数)
            └── InputSend (send icon + "发送" / abort 态)
```

### 7.4 右侧上下文面板（ContextPanel）

```
ContextPanel (column, 360px)
├── ContextTabs (row, 38px)
│   ├── CtxTab[preview] (active)
│   ├── CtxTab[diff]
│   ├── CtxTab[terminal]
│   ├── Spacer
│   └── CtxCollapse (panel-right icon) [click → collapse]
└── ContextContent (scroll-y, flex_grow:1)
    ├── [preview] FilePreview (head + code with line numbers)
    ├── [diff] DiffView (stats + code with add/del colors)
    └── [terminal] TermView (dark bg + output + input line)
```

### 7.5 状态栏（StatusBar）

```
StatusBar (row, 32px)
├── StatusItem[daemon] (dot + "Daemon 已连接")
├── StatusItem[model] (mic icon + "GPT-4o · 2,341 tokens") [click → model dropdown]
├── StatusItem[cost] (dollar icon + "$0.04") [click → toast]
├── Spacer
├── StatusItem[companion] (star icon + "陪伴已开启") [click → toggle]
└── StatusItem[session] ("会话 #a3f2")
```

### 7.6 滑出抽屉（Drawers）

5 个抽屉（files/search/git/history/terminal），统一结构：

```
DrawerOverlay (abs, full-screen, bg rgba(0,0,0,0.2)) [click → close]
Drawer (abs, left, 320px, slide animation)
├── DrawerHead (icon + title + close-btn)
└── DrawerBody (scroll-y)
    └── [drawer-specific content]
```

抽屉通过 `DrawerState` Resource 管理开关，Rail 按钮点击触发对应抽屉。多个抽屉互斥（同时只开一个）。

---

## 8. 消息渲染与 Markdown

### 8.1 轻量 Markdown 解析器（pulldown-cmark）

使用成熟的 `pulldown-cmark` crate 解析 Markdown，避免手写解析器的功能缺失（链接、引用块、表格、嵌套列表等 AI 回复常见格式）。

```rust
// xgent_ui/src/markdown.rs
use pulldown_cmark::{Parser, Event, Tag, TagEnd};

/// Markdown 渲染块 — 解析后转换为 UI 可渲染的块结构
pub enum MarkdownChunk {
    Paragraph(String),
    CodeBlock { lang: String, content: String },
    InlineCode(String),
    ListItem { ordered: bool, text: String },
    Bold(String),
    Link { text: String, url: String },
    Blockquote(String),
    Header { level: u8, text: String },
    Table { headers: Vec<String>, rows: Vec<Vec<String>> },
    HorizontalRule,
}

/// 将 Markdown 文本解析为渲染块列表
/// 未支持的 Tag 降级为 Paragraph，确保不丢内容
pub fn parse_markdown(text: &str) -> Vec<MarkdownChunk> {
    let parser = Parser::new(text);
    let mut chunks = Vec::new();
    let mut current_text = String::new();

    for event in parser {
        match event {
            Event::Text(t) => current_text.push_str(&t),
            Event::Code(code) => chunks.push(MarkdownChunk::InlineCode(code.into())),
            Event::Start(Tag::Paragraph) => current_text.clear(),
            Event::End(TagEnd::Paragraph) => {
                if !current_text.is_empty() {
                    chunks.push(MarkdownChunk::Paragraph(std::mem::take(&mut current_text)));
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(l) => l.into_string(),
                    _ => String::new(),
                };
                current_text.clear();
                // 收集代码内容直到 End(CodeBlock)
                // （实际实现需状态机跟踪）
                chunks.push(MarkdownChunk::CodeBlock {
                    lang,
                    content: std::mem::take(&mut current_text),
                });
            }
            // ... 其他 Tag 处理，未支持的降级为 Paragraph
            _ => {}
        }
    }
    chunks
}
```

### 8.2 消息渲染

```
Msg (container, relative)
├── MsgHead (avatar + role + time)
├── MsgActions (abs, top-right, hover 显示)
│   ├── CopyBtn
│   ├── RegenerateBtn
│   └── RetryBtn
└── MsgBody (column)
    └── per-chunk:
        ├── Paragraph → Text 节点
        ├── CodeBlock → ToolCard 样式（暗底 + 头部 + 语法高亮）
        ├── InlineCode → Text span（accent 底色）
        └── List → column of Text items
```

### 8.3 工具卡（ToolCard）

```
ToolCard (border + radius, hover shadow)
├── ToolCardHead (click → toggle expand)
│   ├── ToolIcon (read/write/search/exec 颜色变体)
│   ├── ToolName
│   ├── ToolArgs (mono, ellipsis)
│   ├── ToolStatus (icon + "0.3s · 128 行")
│   └── ToolChevron (▶, expand 时旋转 90°)
└── ToolCardBody (max-height 动画)
    └── ToolCardContent
        ├── ToolPreview (dark code bg + line numbers + copy btn)
        └── ToolResult (ok/fail badge)
```

### 8.4 流式渲染

- 流式期间（`accumulate_delta`）：只渲染纯段落 `Text`，末尾追加闪烁光标 `▋`
- `DoneMessage` 后：整体解析 markdown → 重建 `MsgBody` 子层级（despawn 旧 → spawn 新，单帧完成避免闪烁）

---

## 9. 图标着色方案（bevy_resvg SvgColor + 主题令牌）

### 9.1 着色原理

bevy_resvg 的 `SvgColor(Color)` 组件对光栅化后的纹理做**乘法 tint**。预处理将 SVG 描边改为白色（`#ffffff`）后，光栅化结果为白色描边透明底。乘法混合：`白色(1,1,1) × 目标色 = 目标色` — tint 完美生效。

### 9.2 ThemedIcon 组件 + 解析系统

```rust
// xui/src/icon.rs
use bevy_resvg::prelude::*;  // SvgColor
use crate::theme::*;

/// 图标着色源：从哪个令牌取色写入 SvgColor。
/// 【关键】require(SvgColor) 确保 SvgColor 与 ThemedIcon 在同一实体。
/// bevy_resvg 的 sync_svg_color_changes 查询 (&SvgColor, &mut ImageNode, With<UiSvg>)
/// — 三者必须在同一实体，否则 tint 不生效。
#[derive(Component, Clone, Copy, Default)]
#[component(immutable)]
#[require(SvgColor)]          // ← 确保 SvgColor 自动插入到此实体
#[derive(Reflect)]
#[reflect(Component, Clone, Default)]
pub struct ThemedIcon(pub ThemeToken);

/// 新图标插入时立即解析令牌 → 写入 SvgColor
pub fn on_insert_themed_icon(
    insert: On<Insert, ThemedIcon>,
    mut q: Query<(&mut SvgColor, &ThemedIcon), Changed<ThemedIcon>>,
    theme: Res<XuiTheme>,
) {
    if let Ok((mut svg_color, themed)) = q.get_mut(insert.entity) {
        svg_color.0 = theme.color(&themed.0);
    }
}

/// 主题切换时全量刷新所有图标 tint
pub fn update_icon_tints(
    theme: Res<XuiTheme>,
    mut q: Query<(&ThemedIcon, &mut SvgColor)>,
) {
    if !theme.is_changed() {
        return;
    }
    for (themed, mut svg_color) in &mut q {
        svg_color.0 = theme.color(&themed.0);
    }
}
```

### 9.3 BSN 中的用法

使用 `icon()` 场景函数，`UiSvg` 和 `ThemedIcon(+SvgColor)` 已在同一子实体上：

```rust
// 图标默认用 T2（次要文字色）着色
// icon() 内部结构: Node(size) > [UiSvg(path) + ThemedIcon(tint) + SvgColor(require)]
bsn! {
    Node { ... }
    Children [
        {icon(icons::CHAT, 18.0, T2)}
    ]
}

// 活动栏 active 态图标用 ACCENT 着色
bsn! {
    Node { ... }
    Children [
        {icon(icons::FOLDER, 20.0, ACCENT)}
    ]
}

// 警告态图标
bsn! {
    Node { ... }
    Children [
        {icon(icons::SHIELD_ALERT, 16.0, WARNING)}
    ]
}
```

### 9.4 自定义色（非令牌）

某些场景需固定色（如陪伴暖色光环），直接用 `SvgColor` 不加 `ThemedIcon`：

```rust
bsn! {
    Node {
        width: px(20.0),
        height: px(20.0),
    }
    SvgColor(Color::srgb_u8(0xFB, 0x92, 0x36))  // 固定暖橙
    Children [
        UiSvg(icons::STAR)
    ]
}
```

---

## 10. 动画系统

### 10.1 原型动画清单

| 动画 | 触发 | CSS 实现 | Bevy 实现 |
|:---|:---|:---|:---|
| AgentPill 脉冲 | Thinking/Generating | `@keyframes pulse` opacity+scale | 系统 `update_pulse`，基于 `Time` 计算正弦波，写 `BackgroundColor` alpha |
| Companion 光环 | active | `@keyframes companion-ring` scale+opacity | 系统 `update_companion_ring`，scale + BorderColor alpha |
| 流式光标 | streaming | `@keyframes blink` opacity | `update_streaming_cursor`（已有，保留） |
| 工具卡展开 | click | `max-height` transition | `update_tool_card_expand`（Display 切换 + 可选 height 插值） |
| 抽屉滑入 | open | `transform: translateX` transition | `update_drawer_slide`（position 插值） |
| Modal 入场 | open | `transform: scale` + opacity | `update_modal_in`（scale + BackgroundColor alpha 插值） |
| Toast | show | `transform: translateY` + opacity | `update_toast`（position + alpha 插值） |

### 10.2 实现模式

统一用「系统 + 状态组件 + 帧插值」模式，避免引入动画库：

```rust
#[derive(Component)]
pub struct DrawerAnimation {
    pub target: f32,      // 目标 translateX
    pub current: f32,     // 当前 translateX
    pub speed: f32,       // 速度（px/frame）
}

fn update_drawer_animation(
    time: Res<Time>,
    mut q: Query<(&mut DrawerAnimation, &mut Node), Changed<DrawerAnimation>>,
) {
    let dt = time.delta_secs();
    for (mut anim, mut node) in &mut q {
        let diff = anim.target - anim.current;
        if diff.abs() < 0.5 {
            anim.current = anim.target;
        } else {
            anim.current += diff * 10.0 * dt; // 缓动
        }
        // 修正：PositionType（不是 NodePosition），Absolute 是单元变体
        // left/right/top/bottom 是 Node 的独立字段
        node.position_type = PositionType::Absolute;
        node.left = Val::Px(-anim.current);
    }
}
```

---

## 11. 实施分期

### Phase 0: 基础设施（xui 令牌 + 主题系统 + bevy_resvg 图标）

| 步骤 | 内容 | 产出 | 验证 |
|:---|:---|:---|:---|
| 0.1 | `xui/Cargo.toml` 加 `bevy_resvg`、`smol_str` 依赖；workspace `[workspace.dependencies]` 同步 | Cargo.toml 更新 | `cargo check -p xui` |
| 0.2 | `xui/build.rs`：文本预处理脚本（currentColor→#fff, 24→48），输出到 `assets/icons/*.svg` | 预处理 SVG 31 个 | build 日志显示 count=31 |
| 0.3 | `xui/src/theme.rs`：`ThemeToken` + `XuiTheme` + `ThemedBg/ThemedBorder/ThemedText` + 解析系统（`On<Insert>` 观察者 + `is_changed()` 全量刷新） | 主题系统可用 | 单元测试：插入 ThemedBg 后 BackgroundColor 正确 |
| 0.4 | `xui/src/tokens.rs`：全量令牌常量（对应 v6 CSS 变量，~50 个） | 令牌常量表 | 编译通过 |
| 0.5 | `xui/src/palette.rs`：亮/暗调色板构建函数 | 双主题数据 | 单元测试：dark_theme().color(&ACCENT) 返回正确色 |
| 0.6 | `xui/src/icon.rs`：图标路径常量 + `icon()` 场景函数 + `ThemedIcon` + `update_icon_tints` 系统 | 图标可用 | spawn 一个 `UiSvg` + `ThemedIcon` 显示正确色 |
| 0.7 | `xui/src/constants.rs`：space/size/typography 常量 | 尺寸常量表 | 编译通过 |
| 0.8 | `xui/src/lib.rs`：`XuiPlugin` 注册 `SvgPlugin` + 主题系统 + 图标 tint 系统 | 插件可组装 | `cargo run` 显示一个测试图标 |

### Phase 1: BSN 基础组件（xui::components）

| 步骤 | 内容 | 产出 | 验证 |
|:---|:---|:---|:---|
| 1.1 | `XButton`（Normal/Primary/Plain/Ghost 变体 + hover/pressed 系统） | 通用按钮 | BSN `@XButton(...)` 渲染 4 变体 |
| 1.2 | `XIconButton`（icon + tooltip + size 参数） | 图标按钮 | 点击有反馈、hover 显 tooltip |
| 1.3 | `XPill`（5 状态 + 脉冲动画系统） | 状态 pill | 切换 PillState 颜色 + 动画正确 |
| 1.4 | `XChip`（label + icon + closable） | 标签 chip | 关闭按钮触发事件 |
| 1.5 | `XCard`（容器 + hover shadow） | 卡片 | hover 有阴影 |
| 1.6 | `XTabs`（tab 项 + active 指示） | 标签页 | 切换 active 高亮 |
| 1.7 | `XTooltip`（hover 显示 + kbd） | 悬浮提示 | hover 延迟显示 |
| 1.8 | `XToast`（show/hide 动画） | Toast 通知 | show 动画 + 自动消失 |

### Phase 2: 根布局重构（xgent_ui::layout）

| 步骤 | 内容 | 产出 | 验证 |
|:---|:---|:---|:---|
| 2.1 | 删除旧 `xgent_ui/src/theme.rs`，迁移至 xui 令牌系统 | 主题统一 | `cargo check` |
| 2.2 | 重写 `layout.rs` 为 BSN `root_layout()` 场景 | 新布局骨架 | `cargo run` 显示三行两列骨架 |
| 2.3 | 上下文面板折叠系统（`ContextPanelCollapsed` Resource） | 面板可折叠 | 按钮切换面板显隐 |
| 2.4 | 更新所有 marker 组件（`TopBarMarker` 等）适配新布局 | 标记对齐 | 各面板可被 Query 定位 |

### Phase 3: 顶栏 + 活动栏 + 状态栏

| 步骤 | 内容 | 产出 | 验证 |
|:---|:---|:---|:---|
| 3.1 | 顶栏 BSN 场景（brand + project crumb + new session + spacer + agent pill + model sel + theme toggle + palette btn） | 完整顶栏 | 视觉对齐 v6 |
| 3.2 | 活动栏 BSN 场景（7 个 rail btn + divider + companion btn + context expand） | 完整活动栏 | 视觉对齐 v6 |
| 3.3 | 状态栏 BSN 场景（daemon + model + cost + spacer + companion + session） | 完整状态栏 | 视觉对齐 v6 |
| 3.4 | AgentPill 状态驱动系统（订阅 agent 事件 → PillState） | 状态联动 | 发消息时 pill 变 Thinking |
| 3.5 | ThemeToggle 切换系统 | 主题切换 | 点击 sun/moon 全界面换色 |
| 3.6 | CompanionBtn 脉冲光环动画 | 陪伴动效 | 光环呼吸动画 |

### Phase 4: 对话区核心

| 步骤 | 内容 | 产出 | 验证 |
|:---|:---|:---|:---|
| 4.1 | ContextScope 条（chip 列表 + add 按钮 + scroll-x） | 上下文条 | 添加 chip 滚动显示 |
| 4.2 | WelcomeDashboard（空状态：mark + grid + recent sessions） | 欢迎页 | 无消息时显示 |
| 4.3 | 消息渲染重构（MsgHead + MsgBody column + MsgActions hover） | 消息结构 | 消息有头像/角色名/时间 |
| 4.4 | `markdown.rs` 解析器（paragraph/code/inline-code/list/bold） | markdown | 代码块正确渲染 |
| 4.5 | 消息 Done 后 markdown 重建（despawn + spawn chunks） | 富文本消息 | 流式结束后格式正确 |
| 4.6 | 流式光标迁移至 MsgBody 末尾 | 光标修正 | 光标在文字末尾闪烁 |
| 4.7 | 消息 hover 操作栏（copy/regenerate/retry） | 消息操作 | hover 显示操作按钮 |
| 4.8 | ScrollBottomBtn（滚动检测 + 显示/隐藏 + click 滚动） | 滚动按钮 | 滚动时出现、点击回底 |

### Phase 5: 工具卡 + 输入区

| 步骤 | 内容 | 产出 | 验证 |
|:---|:---|:---|:---|
| 5.1 | ToolCard BSN 场景（head + expandable body + chevron） | 可折叠工具卡 | 点击展开/折叠 |
| 5.2 | ToolPreview（dark code bg + line numbers + 语法高亮 + copy btn） | 代码预览 | 复制按钮工作 |
| 5.3 | ToolResult badge（ok/fail/denied） | 结果标记 | 颜色正确 |
| 5.4 | QuickActions 条（5 个 chip: explain/refactor/test/fix/review） | 快捷操作 | 点击填入输入框 |
| 5.5 | InputBox 增强（border + focus glow + shadow） | 增强输入框 | focus 有光晕 |
| 5.6 | InputToolbar（mode + safety + hint + counter + send/abort） | 输入工具栏 | 各元素正确显示 |
| 5.7 | InputMode cycle（Agent/聊天/仅问答） | 模式切换 | 点击循环模式 |
| 5.8 | InputCounter（token 计数实时更新） | token 计数 | 输入时数字更新 |

### Phase 6: 右侧上下文面板

| 步骤 | 内容 | 产出 | 验证 |
|:---|:---|:---|:---|
| 6.1 | ContextTabs（preview/diff/terminal + collapse btn） | tab 切换 | 点击切换内容 |
| 6.2 | FilePreview（head + code with line numbers + 高亮行） | 文件预览 | 代码正确高亮 |
| 6.3 | DiffView（stats + add/del 着色代码） | 差异视图 | 增删行颜色正确 |
| 6.4 | TermView（dark bg + output + input line + stick-to-bottom） | 终端视图 | 输出滚动到底 |
| 6.5 | Tab 切换系统（`ContextTab` enum + `Display` 切换） | 面板切换 | 切换无闪烁 |

### Phase 7: 滑出抽屉

| 步骤 | 内容 | 产出 | 验证 |
|:---|:---|:---|:---|
| 7.1 | 通用 Drawer 容器（overlay + slide animation + close） | 抽屉骨架 | 滑入/滑出动画 |
| 7.2 | FileTreeDrawer（文件树 + 递归展开 + 图标） | 文件浏览器 | 展开/折叠正确 |
| 7.3 | SearchDrawer（input + filters + results） | 搜索面板 | 输入触发搜索 |
| 7.4 | GitDrawer（staged/unstaged sections + stage/unstage + commit） | Git 面板 | stage 操作工作 |
| 7.5 | HistoryDrawer（session list + restore） | 历史面板 | 点击恢复会话 |
| 7.6 | TerminalDrawer（full terminal in drawer） | 终端抽屉 | 终端可交互 |
| 7.7 | DrawerState Resource（互斥开关 + Rail 联动） | 抽屉管理 | 同时只开一个 |

### Phase 8: 浮层与下拉

| 步骤 | 内容 | 产出 | 验证 |
|:---|:---|:---|:---|
| 8.1 | Dropdown 通用组件（abs + items + active） | 下拉菜单 | 点击外部关闭 |
| 8.2 | ModelDropdown（云端/本地/自定义模型列表） | 模型选择 | 选择切换模型 |
| 8.3 | ProjectDropdown（当前 + 最近项目 + 打开其他） | 项目切换 | 选择切换项目 |
| 8.4 | CommandPalette 重写为 BSN（input + results + sections + kbd） | 命令面板 | 搜索/执行命令 |
| 8.5 | ConfirmDialog 重写为 BSN（head + body + diff + foot） | 确认弹窗 | 确认/取消工作 |
| 8.6 | SettingsPanel 重写为 BSN | 设置面板 | 各设置项可编辑 |
| 8.7 | Toast 通知系统（show/hide 动画 + 队列） | Toast | 队列显示 |

### Phase 9: 主题切换 + 打磨

| 步骤 | 内容 | 产出 | 验证 |
|:---|:---|:---|:---|
| 9.1 | 亮色主题调色板全量填充 + 视觉校准 | 亮色可用 | 截图对比 v6 亮色 |
| 9.2 | 主题切换 Resource + 系统 + 持久化（settings.toml） | 切换可用 | 重启保留选择 |
| 9.3 | 编辑器/终端 EditorTheme 同步 XuiTheme | 编辑器主题联动 | 切换时编辑器换色 |
| 9.4 | 全量 emoji → 矢量图标替换（文件树/工具卡/状态栏/活动栏） | 图标统一 | 全界面零 emoji |
| 9.5 | i18n 全量 key 补齐（新组件文案） | i18n 完整 | 无硬编码字符串 |
| 9.6 | 无障碍 ARIA 等价（Tab focus + keyboard navigation） | 键盘可达 | Tab 遍历所有交互元素 |
| 9.7 | 清理旧代码（重复 `px()` 定义、旧 Theme 结构、旧 emoji 引用） | 代码整洁 | 无 dead code warning |

---

## 12. 关键技术决策

### D-BSN-01: bevy_feathers 复用策略

| 选项 | 描述 | 决定 |
|:---|:---|:---|
| A. 直接依赖 bevy_feathers | 复用其 Button/Slider/Checkbox/ThemeToken 等 | **否** — bevy_feathers 是示例 crate，非稳定 API；且 AGENTS.md 约束 xui 不依赖非必要外部 crate |
| B. 复用模式自建 | 照搬 ThemeToken/SceneComponent/On<Insert> 模式，在 xui 自建 | **是** — 代码独立但模式对齐，可独立发布 |

**自建内容**：`ThemeToken`、`XuiTheme`、`ThemedBg/ThemedBorder/ThemedText`、`ThemedIcon` + 对应解析系统。全部在 `xui/src/theme.rs` + `xui/src/icon.rs`，约 150 行。

### D-BSN-02: 主题令牌粒度

v6 原型有 ~50 个 CSS 变量。映射策略：
- 语义令牌（`BG_SURFACE`, `ACCENT`, `T1`...）而非原始色值
- 每个令牌 = 一个 `ThemeToken` 常量 + 亮/暗调色板各一个 Color
- 组件消费令牌，不直接写 `Color::srgba`

### D-BSN-03: 图标加载方案

| 选项 | 描述 | 决定 |
|:---|:---|:---|
| ~~A. build.rs 光栅化 PNG~~ | 构建期 SVG→PNG，内嵌 `embedded://` | **否**（v1 方案，丢失矢量源、无热重载） |
| **B. bevy_resvg 运行时加载** | `SvgPlugin` + `UiSvg` + `SvgColor` tint，SVG 保持矢量源 | **是** |
| C. bevy_vello JIT | GPU JIT 光栅化 | 否 — 不支持运行时改色、复杂 SVG 支持差 |

**采用 bevy_resvg 理由**：
- BSN 原生支持：`bsn! { UiSvg("icons/chat.svg") }` 一行即用
- 运行时改色：`SvgColor` 组件，系统可动态修改
- 热重载：dev 模式修改 SVG 即时生效
- 性能：一次性光栅化（31 图标 <50ms），渲染零开销

### D-BSN-04: currentColor 着色解决

| 选项 | 描述 | 决定 |
|:---|:---|:---|
| **预处理拷贝** | build.rs 文本替换 currentColor→#fff，SVG 拷贝到 assets | **是** — 简洁、保留源、支持 BSN 字符串路径 |
| CSS 注入 | `load_builder().with_settings(style_sheet)` | 否 — 需预加载 Handle，太重 |
| 直接改源 | 修改 doc/design/icons/*.svg | 否 — 破坏原型 currentColor 语义 |

### D-BSN-05: 抽屉实现

v6 原型用 CSS `position: fixed` + `transform` 实现抽屉滑出。Bevy 中：
- 抽屉 = `PositionType::Absolute` + `Node` 宽度 320px
- 滑出动画 = `position.left` 从 -320px → 0px 帧插值
- 遮罩 = `PositionType::Absolute` + `ThemedBg` 半透明 + `GlobalZIndex`

### D-BSN-06: Markdown 渲染深度

| 选项 | 描述 | 决定 |
|:---|:---|:---|
| ~~A. 轻量手写解析~~ | 正则 + 状态机，支持 paragraph/code/list/bold/inline-code | **否** — AI 回复中链接/引用/表格常见，手写解析器功能缺口大 |
| **B. pulldown-cmark** | 成熟 markdown crate，零依赖，性能好 | **是** — 解析器成熟，renderer 可按需实现已支持的 chunk 类型 |
| C. 完整 markdown 渲染 | 含图片/脚注/HTML 转义 | 延后 — B 阶段可扩展 renderer |

### D-BSN-07: 消息 hover 操作

bevy_ui 无原生 hover 事件组件。方案：
- 使用 `bevy_picking::hover::Hovered` 组件（bevy 0.19 内置）
- `Hovered` 变化时系统切换 `MsgActions` 的 `Display`（None ↔ Flex）
- 与 `XIconButton` 的 `Hovered` 复用同一套 picking 系统

---

## 13. 风险与缓解

| 风险 | 影响 | 缓解 |
|:---|:---|:---|
| BSN 宏 + SceneComponent 学习曲线 | 开发效率短期下降 | 先在 xui 基础组件验证模式（Phase 0-1），成熟后再批量迁移 xgent_ui |
| bevy 0.19 BSN API 仍在演进 | breaking change 风险 | path 依赖本地 bevy 源码，可即时排查；限制 BSN 使用范围在场景定义层 |
| bevy_resvg MSRV 1.95 | 需 Rust 1.95+ | 项目已使用 edition 2024，MSRV 满足 |
| bevy_resvg zoom 模糊 | 放大显示模糊 | 预处理时 2x 渲染（48×48），图标用 18-24px 显示无模糊；大尺寸场景用 `target_render_size` |
| `currentColor` 解析为黑色 | tint 无效 | build.rs 预处理 `currentColor` → `#ffffff`，白色底图 tint 生效 |
| `ThemedIcon` → `SvgColor` 联动 | 图标颜色不随主题切换 | `update_icon_tints` 系统监听 `theme.is_changed()`，全量刷新 |
| 亮色主题视觉校准 | 对比度/可读性差 | 逐面板亮色验证，参考 v6 原型 `:root` 值 |
| 动画帧插值性能 | 大量动画系统开销 | 所有动画系统加 `Changed<>` guard，只在状态变化时计算 |
| 消息 markdown 重建闪烁 | Done 后子层级重建闪屏 | 先 spawn 新子层级再 despawn 旧，单帧完成 |
| bevy_resvg 启动光栅化开销 | 首次加载 31 SVG 延迟 | 实测 <50ms，可接受；如需优化可 lazy load |

---

## 14. 与现有代码的兼容策略

### 14.1 保留模块（无需 BSN 改造）

以下 xui 模块逻辑不变，仅可能调整主题引用方式（`Theme` → `XuiTheme`）：
- `scroll_area.rs` / `scrollbar.rs` / `virtual_list.rs` / `mouse_wheel_scroll.rs`
- `text_editor/`（编辑器核心逻辑）
- `command_palette.rs` / `hotkeys.rs` / `shortcuts.rs` / `input.rs` / `i18n_bridge.rs`

以下 xgent_ui 模块逻辑保留，UI 层 BSN 化：
- `resize.rs`（拖拽手柄）
- `shortcuts.rs`（快捷键）
- `editor/`（编辑器业务逻辑，仅 render 层适配新主题）
- `terminal/`（终端业务逻辑，仅 view 层适配新主题）

### 14.2 渐进迁移

- Phase 0-1 完成后，xui 有令牌 + 主题系统 + 图标 + 基础组件
- Phase 2 开始 xgent_ui 逐面板迁移
- 每个面板迁移后 `cargo check` + `cargo run` 验证
- 新旧代码可共存（旧用 `Theme` Resource，新用 `XuiTheme` Resource），最后统一清理（Phase 9.7）

### 14.3 i18n key 补充

新增 UI 文案需注册 i18n key（`crates/xgent_settings/src/locales/`）：

```ftl
# zh-CN
topbar-new-session = 新建会话
topbar-project = 项目
topbar-model = 模型
rail-chat = 对话
rail-files = 文件
rail-search = 搜索
rail-git = Git
rail-history = 历史
rail-terminal = 终端
rail-plugins = 插件
rail-expand-panel = 展开面板
companion-toggle = 陪伴开关
context-label = 上下文
context-add = 添加上下文
welcome-title = 开始新会话
welcome-subtitle = 输入问题，或从下方快捷操作开始
welcome-card-explain = 解释代码
welcome-card-refactor = 重构代码
welcome-card-test = 生成测试
welcome-recent = 最近会话
input-placeholder = 输入你的问题，或按 Enter 发送…
input-mode-agent = Agent 模式
input-safety = 工具调用需确认
input-hint-newline = Shift+Enter 换行
input-send = 发送
status-daemon-connected = Daemon 已连接
status-companion-on = 陪伴已开启
status-session = 会话
```

---

## 15. 验收标准

| 维度 | 标准 |
|:---|:---|
| 编译 | `cargo check --workspace` 通过，无 warning |
| 视觉 | 截图与 v6 原型逐区域对齐（亮/暗双主题） |
| 交互 | 原型全部 13 条交互（消息 hover/代码复制/顶栏增强/欢迎仪表盘/输入打磨/上下文面板/滚动到底部/状态栏可点击/命令面板执行/Git 抽屉/搜索抽屉/无障碍）均可操作 |
| 图标 | 全界面零 emoji，31 个矢量图标正确着色（白底 × tint） |
| 主题 | 亮/暗切换无闪烁，所有面板颜色 + 图标 tint 同步 |
| 性能 | 动画系统均有 `Changed<>` / `is_changed()` guard，帧率 ≥ 60fps |
| i18n | 所有用户可见字符串走 i18n key，无硬编码 |
| 文档 | `doc/dev-tutorial.md` 同步更新 crate 拓扑与 ADR |
| 热重载 | dev 模式修改 SVG 图标即时生效（file_watcher feature） |

---

## 16. bevy_resvg 集成速查

### 16.1 依赖与插件

```toml
# xui/Cargo.toml
[dependencies]
bevy_resvg = "2.5"
```

```rust
// xui/src/lib.rs
use bevy_resvg::prelude::SvgPlugin;

app.add_plugins(SvgPlugin);
```

### 16.2 资产路径

- SVG 源：`doc/design/icons/*.svg`（设计参考，含 `currentColor`）
- 预处理后：`xui/assets/icons/*.svg`（白色描边，48×48 渲染分辨率）
- BSN 引用：`UiSvg("icons/chat.svg")`（相对于 `assets/` 根）

### 16.3 组件速查

| 组件 | 作用 | BSN 用法 |
|:---|:---|:---|
| `UiSvg` | UI 渲染 SVG → ImageNode | `UiSvg("icons/chat.svg")` |
| `SvgColor(Color)` | tint 着色（乘法混合） | `UiSvg("...") SvgColor(Color::WHITE)` |
| `ThemedIcon(ThemeToken)` | 令牌驱动 tint（xui 自建） | `ThemedIcon(T2)` → 系统解析为 `SvgColor` |

### 16.4 系统

| 系统 | 触发 | 作用 |
|:---|:---|:---|
| `on_insert_themed_icon` | `On<Insert, ThemedIcon>` | 新图标立即解析令牌 → 写 `SvgColor` |
| `update_icon_tints` | `theme.is_changed()` | 主题切换时全量刷新所有图标 tint |
