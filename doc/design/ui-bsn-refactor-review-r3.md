# UI BSN 重构方案 — 第三轮深度 Review

> 对照 Bevy 0.19 源码 + bevy_resvg 2.5 文档 + bevy_feathers 实际实现，逐条验证方案代码可编译性。
>
> 第二轮已修复 6 个 P0 + 4 个 P1。本轮发现 **6 个新的 P0 + 4 个 P1 + 4 个 P2**。

---

## P0 — 编译错误

### P0-7: `ThemedText(text)` 在 XButton 和 XPill 中 — 编译失败

**位置**：`ui-bsn-refactor-plan.md` 第 884 行（XButton）、第 1002 行（XPill）

**问题**：第二轮 P0-6 修正将 `ThemedText` 改为**单元标记组件**（`pub struct ThemedText;`，无字段）。但 XButton 和 XPill 的 BSN 场景代码仍使用 `ThemedText(text)`（带参数构造），这会编译失败——单元结构体不接受参数。

**bevy_feathers 对照**：`FeathersButton` 使用 `InheritableThemeTextColor(text_token)` 作为父组件，子 `Text` 实体加 `ThemedText` 标记。

**修正**：
```rust
// XButton — 修正前
ThemedText(text)  // ← 编译错误

// XButton — 修正后
InheritableThemedText(text)  // ← 父实体令牌 + Propagate 传播
// 调用方在 children 中的 Text 实体需加 ThemedText 标记才能继承

// XPill — 同理修正
InheritableThemedText(text)  // 替换 ThemedText(text)
```

### P0-8: `UiRoot` 组件不存在 — 编译失败

**位置**：`ui-bsn-refactor-plan.md` 第 1065 行（根布局 BSN）

**问题**：方案在根布局 BSN 中使用 `UiRoot` 组件标记，但 bevy_ui 0.19 **没有 `UiRoot` 组件**。验证结果确认 bevy_ui 中不存在此类型。根 UI 节点在 bevy 中通过 `With<Node>, Without<ChildOf>` 查询识别。

**修正**：移除 `UiRoot`，或定义项目自己的标记组件：
```rust
// 方案 A: 直接移除 UiRoot（根节点靠 Node + 无 ChildOf 自动识别）
bsn! {
    Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
    }
    // 无需 UiRoot
    Children [ ... ]
}

// 方案 B: 自定义标记（如需 Query 定位根节点）
#[derive(Component, Default, Reflect)]
#[reflect(Component)]
pub struct XgentUiRoot;
```

### P0-9: `Box::new(())` 作为 SceneList 默认值 — 编译失败

**位置**：`ui-bsn-refactor-plan.md` 第 858 行（XButtonProps）、第 979 行（XPillProps）

**问题**：`Box<dyn SceneList>` 的默认值用了 `Box::new(())`，但 `()` **不实现 `SceneList` 或 `SceneListBox`**。bevy_feathers 使用 `Box::new(bsn_list!())` 创建空场景列表。

**bevy_feathers 对照**（button.rs 第 77 行）：
```rust
caption: Box::new(bsn_list!()),
```

**修正**：
```rust
use bevy_scene::prelude::*;  // 包含 bsn_list

impl Default for XButtonProps {
    fn default() -> Self {
        Self {
            variant: ButtonVariant::Normal,
            children: Box::new(bsn_list!()),  // ← 修正
            corners: 8.0,
        }
    }
}
```

### P0-10: `props.state` 裸标识符在 BSN 中 — 语法错误

**位置**：`ui-bsn-refactor-plan.md` 第 1003 行（XPill BSN 场景）

**问题**：XPill 场景中直接写 `props.state` 作为 BSN body 元素。BSN 宏将裸标识符解释为**组件类型名**（如 `Button`、`Hovered`），而非变量引用。`props.state` 不是类型名，会解析失败。

**bevy_feathers 对照**（button.rs 第 95 行）：
```rust
template_value(props.variant)  // ← template_value() 函数接受任意 Template 值
```

`template_value<T: Template>(value: T)` 是 `bevy_scene` 提供的函数，接受任何实现了 `Template` 的值。由于 `Template` 有 `T: Clone + Default + Unpin` 的 blanket impl，`PillState`（derive 了 `Clone + Default`）自动满足约束。

**修正**：
```rust
// 修正前
props.state   // ← BSN 语法错误

// 修正后
template_value(props.state)  // ← 正确：通过 template_value 注入组件值
```

需在文件顶部导入：
```rust
use bevy_scene::prelude::*;  // 包含 template_value
```

### P0-11: 观察者使用 `Changed<>` 过滤器 — 可能导致匹配失败

**位置**：`ui-bsn-refactor-plan.md` 第 293-300 行（`on_insert_themed_bg`）、第 318-326 行（`on_insert_themed_text_color`）、第 441-449 行（`on_insert_themed_icon`）

**问题**：所有 `On<Insert, T>` 观察者函数的 Query 使用了 `Changed<T>` 过滤器。`On<Insert>` 在组件插入时触发，但同帧内的 `Changed<>` 检测可能因为 change tick 未更新而不匹配，导致观察者拿不到数据。

**bevy_feathers 对照**（theme.rs 第 153-203 行）：观察者函数**不使用 `Changed<>`**：
```rust
fn on_changed_background(
    insert: On<Insert, ThemeBackgroundColor>,
    mut q: Query<(&mut BackgroundColor, &ThemeBackgroundColor)>,  // ← 无 Changed
    theme: Res<UiTheme>,
) {
```

**修正**：移除所有观察者 Query 中的 `Changed<>`：
```rust
// 修正前
pub fn on_insert_themed_bg(
    insert: On<Insert, ThemedBg>,
    mut q: Query<(&mut BackgroundColor, &ThemedBg), Changed<ThemedBg>>,  // ← Changed
    theme: Res<XuiTheme>,
) {

// 修正后
pub fn on_insert_themed_bg(
    insert: On<Insert, ThemedBg>,
    mut q: Query<(&mut BackgroundColor, &ThemedBg)>,  // ← 移除 Changed
    theme: Res<XuiTheme>,
) {
```

同理修正 `on_insert_themed_text_color` 和 `on_insert_themed_icon`。

### P0-12: `smol_str = "1"` 版本错误 — 编译失败

**位置**：`ui-bsn-refactor-plan.md` 第 726 行、第 732 行（Cargo.toml）

**问题**：方案指定 `smol_str = "1"`，但 Bevy 0.19 依赖的是 `smol_str = "0.2"`。版本不匹配会导致 `ThemeToken` 的 `SmolStr::new_static` 找不到对应实现（该方法在 0.2 版本中存在），或类型不兼容。

**验证**：`bevy_text/Cargo.toml` 第 41 行：`smol_str = { version = "0.2", default-features = false }`。bevy_feathers 使用 `SmolStr::new_static`（theme.rs 第 33 行），确认 0.2 版本有此方法。

**修正**：
```toml
# xui/Cargo.toml
smol_str = "0.2"  # 与 bevy 0.19 一致

# Cargo.toml [workspace.dependencies]
smol_str = "0.2"
```

---

## P1 — 语义/时序问题

### P1-7: `update_theme_colors` 应在 PostUpdate 中

**位置**：`ui-bsn-refactor-plan.md` 第 767 行

**问题**：方案将 `update_theme_colors` 和 `update_icon_tints` 注册在 `Update` schedule 中。但 bevy_feathers 将 `update_theme` 注册在 **`PostUpdate`** 中（lib.rs 第 105 行）。这与 `HierarchyPropagatePlugin::<TextColor>::new(PostUpdate)` 的 schedule 一致——主题刷新和文字色传播在同一 schedule 中，确保正确的执行顺序。

如果 `update_theme_colors` 在 `Update` 而 `HierarchyPropagatePlugin` 在 `PostUpdate`，主题切换时 `Propagate(TextColor)` 的插入会晚于主题刷新系统，可能导致传播延迟 1 帧。

**修正**：
```rust
// 修正前
.add_systems(Update, (update_theme_colors, update_icon_tints))

// 修正后
.add_systems(PostUpdate, (update_theme_colors, update_icon_tints))
```

### P1-8: `Button` 应使用 `bevy_ui_widgets::Button`

**位置**：`ui-bsn-refactor-plan.md` 第 881 行（XButton）、第 936 行（XIconButton）

**问题**：方案从 `bevy_ui::prelude` 导入 `Button`，获得的是 `bevy_ui::Button`（简单标记组件，仅 require `Node + FocusPolicy + Interaction`）。但 bevy_feathers 使用的是 **`bevy_ui_widgets::Button`**（额外 require `AccessibilityNode(Role::Button)`，支持无障碍 + 发射 `Activate` 事件）。

两者行为不同：`bevy_ui_widgets::Button` 有 accessibility 标注，可被屏幕阅读器识别；有 `Activate` 事件，可被 Action 系统触发。

**修正**：
```rust
// 修正前
use bevy_ui::prelude::*;  // → bevy_ui::Button

// 修正后
use bevy_ui_widgets::Button;  // → 带无障碍 + Activate 事件
```

> 注意：需确认 `bevy_ui_widgets` 是否在 bevy 0.19 的默认 features 中。若不在，需在 `Cargo.toml` 中启用。

### P1-9: XButton/XPill 子 Text 需加 `ThemedText` 标记 — 未文档化

**位置**：XButton 和 XPill 组件使用说明

**问题**：使用 `InheritableThemedText(token)` 后，**只有带 `ThemedText` 标记的子 `Text` 实体才能继承传播的颜色**。调用方传入的 children 中的 `Text` 节点必须显式加 `ThemedText` 标记，否则文字颜色不生效。

**bevy_feathers 对照**：FeathersButton 的用法示例中，调用方在 `bsn_list!` 中显式加 `ThemedText`：
```rust
@FeathersButton(FeathersButtonProps {
    caption: bsn_list!(Text("Click"), ThemedText),  // ← ThemedText 标记
    ..
})
```

**修正**：在组件文档注释中标注此要求：
```rust
/// XButton — 通用按钮。
/// 【注意】children 中的 Text 实体需加 `ThemedText` 标记才能继承文字色令牌。
```

### P1-10: `on_insert_themed_border` 观察者实现缺失

**位置**：`ui-bsn-refactor-plan.md` 第 302 行

**问题**：方案只写了 "on_insert_themed_border 同理..." 注释，未给出实际实现。且 XuiPlugin 中注册了该观察者（第 774 行 `add_observer(on_insert_themed_border)`），但函数体不存在 → 编译失败。

**修正**：补全实现：
```rust
/// ThemedBorder 插入时 → 解析令牌 → 设置 BorderColor
pub fn on_insert_themed_border(
    insert: On<Insert, ThemedBorder>,
    mut q: Query<(&mut BorderColor, &ThemedBorder)>,
    theme: Res<XuiTheme>,
) {
    if let Ok((mut border, themed)) = q.get_mut(insert.entity) {
        border.set_all(theme.color(&themed.0));
    }
}
```

---

## P2 — 文档/准确性

### P2-3: bevy_resvg `UiSvg("path")` 机制说明不准确

**位置**：§4.2 第 556 行

**问题**：方案说 "UiSvg 在 bsn!{} 中直接接受字符串路径，无需预加载 Handle"。实际机制是：`UiSvg` derive 了 `FromTemplate`，生成 `UiSvgTemplate` 包装 `HandleTemplate<SvgFile>`。`HandleTemplate<T>` 实现 `From<AssetPath<'static>>`，而 `AssetPath` 实现 `From<&'static str>`。BSN 宏自动插入 `.into()` 转换链。

**影响**：不影响编译（结论正确），但原理描述应更准确。

### P2-4: `SvgColor::default()` = `Color::WHITE` — 应文档化

**位置**：§9 图标着色方案

**问题**：`#[require(SvgColor)]` 自动插入 `SvgColor(Color::WHITE)`。白色 tint 是 no-op（白色 × 任意色 = 任意色），意味着图标在主题系统更新 `SvgColor` 之前会以**白色描边**显示（白色底图 × 白色 tint = 白色）。这在浅色背景上不可见。

**影响**：启动首帧可能短暂白图标闪烁（<1 帧，不可感知）。应文档化此行为，避免开发时困惑。

### P2-5: 缺少 `PropagateSet` 配置

**位置**：§4.9 XuiPlugin 注册

**问题**：bevy_feathers 在 `PostUpdate` 中配置 `PropagateSet`（lib.rs 第 96-99 行）以确保传播系统的执行顺序。方案未做此配置。

**影响**：不阻塞编译，但可能导致传播时序不确定。

### P2-6: 缺少 `template_value` 和 `bsn_list` 导入说明

**位置**：§5 BSN 组件库

**问题**：修正 P0-10 和 P0-9 后，组件代码需要导入 `template_value` 和 `bsn_list`（来自 `bevy_scene::prelude`）。方案未提及这两个导入。

---

## 验证通过的 API（无问题）

以下 API 经三个验证 agent 确认**正确**，无需修改：

| API | 验证结果 | 来源 |
|:---|:---|:---|
| `#[derive(SceneComponent)]` 自动实现 `Component` | ✅ 不需额外 derive Component | bevy_scene/macros/src/scene_component.rs |
| `On<Insert, T>` 观察者参数类型 | ✅ 正确（非 `Trigger`） | bevy_ecs/src/observer/system_param.rs |
| `insert.entity` 字段访问（非方法） | ✅ 通过 Deref 到 `Insert` 事件 | bevy_ecs/src/lifecycle.rs |
| `app.add_observer(fn)` 注册 | ✅ 正确 | bevy_app/src/app.rs |
| `Propagate(TextColor(color))` 构造 | ✅ 元组结构体，pub 字段 | bevy_app/src/propagate.rs |
| `#[require(PropagateOver::<TextColor>)]` | ✅ turbofish 语法，bevy_feathers 使用 | bevy_feathers/src/theme.rs |
| `HierarchyPropagatePlugin::<C, F>::new(PostUpdate)` | ✅ 三泛型参数，F 默认 `()`，R 默认 `ChildOf` | bevy_app/src/propagate.rs |
| `#[component(immutable)]` | ✅ 存在，bevy_feathers 使用四处 | bevy_ecs/macro_logic/src/component.rs |
| `insert_resource` + `is_changed()` 主题切换 | ✅ 正确工作 | bevy_ecs/src/world/mod.rs |
| `BorderColor::set_all()` | ✅ 存在（BorderColor 有 per-side 字段） | bevy_ui/src/ui_node.rs |
| `Overflow::clip()` | ✅ const fn，非 enum variant | bevy_ui/src/ui_node.rs |
| `Val::ZERO` | ✅ `Val::Px(0.0)` | bevy_ui/src/geometry.rs |
| `warn_once!` | ✅ bevy_log 导出 | bevy_log/src/once.rs |
| `SmolStr::new_static` | ✅ smol_str 0.2 有此方法 | bevy_feathers/src/theme.rs |
| `SceneList` trait + `Box<dyn SceneList>` | ✅ 存在，bevy_feathers 使用 | bevy_scene/src/scene_list.rs |
| `px()` 函数 | ✅ geometry 模块，prelude 导出 | bevy_ui/src/geometry.rs |
| `PositionType::Absolute` + Node 字段 | ✅ 单元变体，Node 有 position_type/left/right/top/bottom | bevy_ui/src/ui_node.rs |
| `Text("string")` 在 BSN 中 | ✅ `Text(pub String)` 元组结构体 | bevy_ui/src/widget/text.rs |
| `Hovered` 在 BSN 中 | ✅ derive Default，BSN 用 Default::default() | bevy_picking/src/hover.rs |
| `TabIndex(0)` | ✅ i32 类型（非 usize），0 字面量推断为 i32 | bevy_input_focus/src/tab_navigation.rs |
| `UiSvg("path")` 在 BSN 中 | ✅ 通过 FromTemplate 机制 | bevy_resvg 源码 |
| `SvgColor` 是 Component + `.0: Color` | ✅ derive Component, pub field | bevy_resvg/src/effects/components.rs |
| `SvgColor` 实现 Default（= White） | ✅ Color::default() = Color::WHITE | bevy_color/src/color.rs |
| bevy_resvg 2.5 支持 Bevy 0.19 | ✅ Cargo.toml 确认 | bevy_resvg GitHub |
| `UiSvg` 在同一实体插入 `ImageNode` | ✅ 不创建子实体 | bevy_resvg 源码 |
| `SvgFileLoaderSettings` 存在 | ✅ prelude 导出 | bevy_resvg/src/settings.rs |

---

## 技术选型再评估

第二轮已确认 BSN、bevy_resvg、主题系统自建选型正确。本轮补充：

1. **`bevy_ui_widgets::Button` vs `bevy_ui::Button`**：选 `bevy_ui_widgets::Button` 更好——有无障碍标注 + `Activate` 事件。但需确认 `bevy_ui_widgets` 是否在 bevy 默认 features 中，或需手动启用。
2. **`template_value` vs match 分支**：对于 `PillState` 这种枚举组件，`template_value(props.state)` 是正确做法（bevy_feathers 对 `ButtonVariant` 同样如此）。
3. **`bsn_list!()` 空列表**：是 BSN 场景列表的标准空值，不可用 `()` 替代。
4. **`smol_str` 版本**：必须与 bevy 一致（0.2），不能自行指定其他版本——否则 `SmolStr` 类型不兼容。

**结论**：技术选型全部正确，问题仍在实现细节层面。
