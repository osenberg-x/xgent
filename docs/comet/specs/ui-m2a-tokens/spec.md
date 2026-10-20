# ui-m2a-tokens · 主题权威与令牌收敛

## Purpose

把主题与尺寸从「散落在各模块的字面量与影子定义」收敛为**单一权威**：主题可替换、节点可按 generation 重绘；尺寸、圆角、最小宽度、颜色都只从 `theme` 取值；窗口首帧有确定尺寸。为 M2b（密度/hover）与 M3 提供稳定的令牌基座。

本能力不改变现有视觉像素值。

## Requirements

### 主题权威

- `ThemeHandle { theme: Theme, generation: u64 }` 是主题的唯一权威 Resource。
- `ThemeRole { Canvas, Surface, Elevated, Border, Text, TextDim }` 描述语义色角色。
- `Themed { role: ThemeRole, gen: u64 }` 组件挂在需要随主题重绘的节点上。
- `repaint_on_theme_change`：当 `ThemeHandle.generation` 变化时，
  - 按 `role` 写 `BackgroundColor`/`BorderColor`/`TextColor`；
  - 把 `ThemeHandle.theme` 同步到旧 `Theme` 资源快照；
  - 把 `Themed.gen` 更新为当前 generation。
- `preferences.theme` 变化时，`settings_io`/config bridge 更新 `ThemeHandle.theme` 并 `generation += 1`；未知主题值回落 `dark`。
- 现有 `Res<Theme>` 调用点不改签名，继续读快照。

#### Scenario: 主题替换即时重绘

- **Given** 界面已启动，BASE 的六个表面节点挂有 `Themed`
- **When** 修改 `ThemeHandle.theme`（例如把 `surface` 换成另一个颜色）并 `generation += 1`
- **Then** 下一帧这些节点的 `BackgroundColor`/`BorderColor` 变为新主题值，`Theme` 资源快照同步，无需重启

验收：A1

### 消灭影子定义

- `Theme::dark()` 可见性降为 `pub(crate)`；生产代码不得再现场构造主题。
- 删除 `confirm_dialog.rs` 与 `settings_panel.rs` 的本地 `fn px()`，统一用 `theme::px`。

#### Scenario: 单一构造入口

- **Given** 全仓 `crates/xgent_ui/src`
- **When** 搜索 `Theme::dark()` 与 `fn px(`
- **Then** `Theme::dark()` 只命中 `theme.rs` 的定义与测试；`fn px(` 只命中 `theme.rs`

验收：A2

### 尺寸令牌

`theme::size` 新增并以常量形式被引用：

```
ICON_BTN = 40
RAIL_ITEM = 40
BRAND = 34
SCOPE_H = 28
MSG_MAX_W = 820
TOAST_BOTTOM = 50
OVERLAY_PALETTE_W = 560
OVERLAY_SETTINGS_W = 640
OVERLAY_CONFIRM_W = 560
MODAL_MIN_W = 360
HANDLE_W = 6
```

替换清单以 `ui-v8-tasks.md` M2-T3 为准：消息宽度、品牌块、rail 图标钮、命令面板/设置/确认/冲突卡片、welcome 圆角、tool_panel 圆角等。区域尺寸不得再出现裸 `Val::Px(<数字>)`；1px 边框等装饰需行内注释说明。

#### Scenario: 区域尺寸无裸数字

- **Given** `crates/xgent_ui/src/*.rs`
- **When** 搜索 `Val::Px([0-9]`
- **Then** 命中的仅为带注释的装饰性 1px/2px 值；清单内的区域尺寸全部改为 `px(size::*)`

验收：A3

### 最小宽度单源

`theme::size` 是 `CONTEXT_W_MIN`（380）、`CHAT_MIN`（520）、`HANDLE_W`（6）的唯一来源；`resize.rs` 不再本地定义，改为引用。

#### Scenario: 最小宽度只有一处定义

- **Given** 全仓
- **When** 搜索 `380.0` / `520.0` 作为最小宽度
- **Then** 只在 `theme::size` 出现；`resize.rs` 使用 `size::CONTEXT_W_MIN`/`size::CHAT_MIN`

验收：A4

### 色板纪律

- 活动栏星形色（`0x1C1917` 系）与 `editor/tabs.rs` 的 `Color::WHITE` 收进 `Theme`（`star` / `on_accent`）。
- `terminal/output.rs` 的 ANSI 色表保留，但加显式白名单注释，说明这是终端协议色、不参与主题。

#### Scenario: 非主题色只剩 ANSI 白名单

- **Given** `crates/xgent_ui/src`
- **When** 搜索 `Color::WHITE`、`Color::srgb(`、`Color::hex(`
- **Then** 仅剩 ANSI 表与带注释的透明/占位色；星形与白色文字改用 `theme.star`/`theme.on_accent`

验收：A5

### 窗口基线

`xgent_app` 的主窗口 SHALL 设置 `resolution: WindowResolution::new(1280, 800)`（逻辑像素），保留现有最小尺寸约束；不新增「记住上次尺寸」。

#### Scenario: 首帧尺寸确定

- **Given** 无窗口尺寸持久化
- **When** 应用启动
- **Then** 主窗口逻辑尺寸为 1280×800，HiDPI 缩放下布局不溢出

验收：A6

## Constraints

- 令牌替换必须保持像素值不变，不改视觉。
- 只改 `xgent_ui`/`xgent_app`（必要时 `xgent_settings_core` 的偏好字段）与文档。
- 层级继续走 M1 的 `theme::z`。
