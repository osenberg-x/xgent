# UI BSN 重构方案 — 第四轮深度 Review（架构设计）

> 前三轮聚焦 API 可编译性。本轮聚焦**架构设计合理性**与**可行性**，并补漏 r3 未应用的修复。
>
> 对照 Bevy 0.19 源码（bevy_feathers 完整实现 + bevy_ui_widgets + BSN 宏 + propagate 系统）逐条验证。

---

## 一、架构设计问题（A 系列）

### A-1: 主题系统重复实现 vs 直接依赖 bevy_feathers

**问题**：方案在 `xui` 内完整复制了 bevy_feathers 的主题系统（ThemeToken / XuiTheme / ThemedBg / ThemedBorder / InheritableThemedText / ThemedTextColor / ThemedText + 观察者 + update 系统），约 200 行代码。

**验证发现**：
- bevy_feathers 的主题类型**全部公开导出**（`pub mod theme`），可通过 `bevy` feature `"bevy_feathers"` 直接使用
- bevy_feathers 是 Bevy monorepo 的一部分（`bevy/crates/bevy_feathers/`），不是外部 crate
- AGENTS.md 约束「xui 不依赖非必要外部 crate」，但 bevy_feathers 已在 `../bevy` 源码树中

**方案自建的理由**：bevy_feathers 是「示例 crate，非稳定 API」。

**反论**：项目已使用 BSN（同为 Bevy 0.19 新引入、仍在演进的 API），「不稳定」顾虑与使用 BSN 不一致。且自建代码完全照搬 bevy_feathers 模式，API 演进时同样需要跟进。

**建议**：两种可行路径——
- **路径 A（推荐）**：在 `bevy` features 中启用 `"bevy_feathers"`，xui 直接 `use bevy::feathers::theme::*`。省 200 行重复代码，主题 bug 修复自动跟进。
- **路径 B（当前方案）**：保留自建，但在文档中明确记录「模式 1:1 对齐 bevy_feathers::theme，升级 bevy 时需同步检查 theme.rs 变更」。

> **决策点**：如选路径 A，xui 的 Cargo.toml 需在 bevy features 中加 `"bevy_feathers"`。如选路径 B，当前方案可用但需加维护注释。

---

### A-2: 动态内容插入模式未文档化 — 架构缺口

**问题**：方案展示了 BSN 根布局场景（`root_layout()`），用 marker 组件（`TopBarMarker`、`RailMarker` 等）标识容器实体，但**从未解释动态内容如何插入这些容器**。

**验证发现**（BSN 动态内容模式）：
- BSN `Children [...]` 在 spawn 时一次性创建子层级
- `Box<dyn SceneList>` Props 用于在 spawn 时传入动态子场景
- **spawn 后插入子实体**：使用标准 ECS 层级 API — `commands.entity(parent).add_child(child)` 或 `.with_children(|spawner| { ... })`
- BSN 支持 `#name` 语法在 spawn 时捕获实体引用

**影响**：对话消息流、文件树节点、终端输出、工具卡等都是高度动态的内容。如果不明确插入模式，开发者会困惑如何将动态内容挂到 BSN 布局的容器中。

**建议**：在方案中增加「动态内容插入模式」章节：

```rust
// 1. Spawn 根布局（一次性）
commands.spawn_scene(root_layout());

// 2. 查询容器实体
fn spawn_initial_content(
    q_rail: Query<Entity, With<RailMarker>>,
    q_main: Query<Entity, With<MainMarker>>,
    mut commands: Commands,
) {
    let rail = q_rail.single();
    // 3. 用 with_children 挂载动态内容
    commands.entity(rail).with_children(|parent| {
        parent.spawn(bsn! {
            Node { width: px(52.0), height: px(52.0) }
            XIconButton(XIconButtonProps { icon: icons::CHAT, ..default() })
        });
    });
}

// 4. 运行时追加消息（chat panel）
fn append_message(
    mut commands: Commands,
    q_conversation: Query<Entity, With<ConversationMarker>>,
) {
    let conv = q_conversation.single();
    commands.entity(conv).with_children(|parent| {
        parent.spawn(bsn! { /* message scene */ });
    });
}
```

---

### A-3: Z-Index 分层管理策略缺失

**问题**：方案有 5 类浮层（抽屉、下拉菜单、tooltip、toast、命令面板/确认弹窗），全部使用 `PositionType::Absolute`，但**未定义 z-index 分层策略**。

**验证发现**：
- `GlobalZIndex(pub i32)` 存在于 bevy_ui 0.19，可让 UI 节点脱离布局树层级全局排序
- 无 `GlobalZIndex` 的节点默认 0，有 `GlobalZIndex` 的节点按值排序
- 同值时按 `ZIndex`（兄弟级）和添加顺序排序

**影响**：无分层策略时，浮层可能被后续渲染的普通 UI 覆盖，或浮层之间互相遮挡。

**建议**：定义分层常量：

```rust
// xui/src/constants.rs
pub mod z_index {
    pub const BASE: i32 = 0;        // 普通布局
    pub const DRAWER: i32 = 100;    // 滑出抽屉 + 遮罩
    pub const DROPDOWN: i32 = 200;  // 下拉菜单
    pub const TOOLTIP: i32 = 300;   // 悬浮提示
    pub const TOAST: i32 = 400;     // Toast 通知
    pub const MODAL: i32 = 500;     // 命令面板/确认弹窗
}
```

每个浮层 BSN 场景加 `GlobalZIndex(z_index::DRAWER)` 等。

---

### A-4: 按钮 hover/press/disabled 状态架构不完整

**问题**：方案的 XButton 使用 `Button`（来自 `bevy_ui::prelude`，即 `bevy_ui::widget::Button`），但未实现 hover/press/disabled 状态的样式变化。

**验证发现**：
- `bevy_ui::widget::Button`：简单标记组件，`#[require(Node, FocusPolicy::Block, Interaction)]`，无行为逻辑
- `bevy_ui_widgets::Button`：headless 控件，`#[require(AccessibilityNode(Role::Button))]`，注册 6 个观察者管理 `Pressed` 状态 + 发射 `Activate` 事件
- **bevy_feathers 使用 `bevy_ui_widgets::Button`**（button.rs 第 20 行：`use bevy_ui_widgets::Button;`）
- bevy_feathers 的 `update_button_styles` 系统在 `PreUpdate` / `PickingSystems::Last` 中运行，查询 `(ButtonVariant, Has<InteractionDisabled>, Has<Pressed>, &Hovered, &ThemeBackgroundColor)`，按 `(variant, disabled, pressed, hovered)` 四维 match 计算颜色令牌

**影响**：
- 用 `bevy_ui::widget::Button` 不会有 `Pressed` 状态管理，hover/press 视觉反馈需自行实现
- 无 `Activate` 事件，点击只能通过 `Interaction` 组件轮询（非事件驱动）
- 无障碍标注缺失（无 `Role::Button`）

**建议**：
1. XButton / XIconButton 使用 `bevy_ui_widgets::Button`（需 `use bevy_ui_widgets::Button;`）
2. 在 XuiPlugin 中注册 `bevy_ui_widgets::ButtonPlugin`（或通过 bevy features 确保）
3. 实现 `update_xbutton_styles` 系统：

```rust
fn update_xbutton_styles(
    q_buttons: Query<
        (Entity, &ButtonVariant, Has<InteractionDisabled>, Has<Pressed>, &Hovered, &ThemedBg),
        Or<(Changed<Hovered>, Changed<ButtonVariant>, Changed<Pressed>, Changed<InteractionDisabled>)>,
    >,
    mut commands: Commands,
    theme: Res<XuiTheme>,
) {
    for (entity, variant, disabled, pressed, hovered, _bg) in &q_buttons {
        let (bg_token, text_token) = match (variant, disabled, pressed, hovered) {
            (ButtonVariant::Primary, true, _, _)    => (BG_ELEVATED, T3),
            (ButtonVariant::Primary, false, true, _) => (ACCENT_HOVER, T0),
            (ButtonVariant::Primary, false, false, true) => (ACCENT_HOVER, T0),
            (ButtonVariant::Primary, false, false, false) => (ACCENT, T0),
            // ... 其他变体
        };
        commands.entity(entity)
            .insert(ThemedBg(bg_token))
            .insert(InheritableThemedText(text_token));
    }
}
```

---

### A-5: 迁移期双主题系统共存 — 架构风险

**问题**：方案 §14.2 说「新旧代码可共存（旧用 `Theme` Resource，新用 `XuiTheme` Resource）」，Phase 9.7 统一清理。

**影响**：
- 两套颜色系统并行运行，同一界面中新旧组件颜色来源不同
- 主题切换时只有 `XuiTheme` 刷新，旧 `Theme` 组件不跟随 → 视觉不一致
- 维护负担：两个 Theme 结构、两套颜色常量

**建议**：改为**一次性迁移**策略——
1. Phase 0 完成 xui 主题系统后，立即在 Phase 2.1 删除旧 `xgent_ui::theme::Theme`
2. 所有现有面板迁移时一次性切换到 `XuiTheme` 令牌
3. 不允许新旧 Theme 在同一编译中共存
4. 如果迁移工作量大，可按面板分 PR，但每个 PR 内必须完全切换

---

### A-6: 消息 Markdown 重建 — 性能与体验

**问题**：方案 §8.4 说「Done 后整体解析 markdown → 重建 MsgBody 子层级（despawn 旧 → spawn 新，单帧完成避免闪烁）」。

**影响**：
- 长消息（多代码块 + 列表）的 despawn + spawn 在单帧内完成，可能导致帧卡顿
- 如果 despawn 和 spawn 不在同一帧，会闪烁
- 消息流中多条消息同时 Done 时，批量重建开销大

**建议**：改为**增量渲染**——
1. 流式期间：只追加纯文本段落（append-only，不重建）
2. Done 后：不 despawn 整个 MsgBody，而是逐 chunk 替换：
   - 遍历 markdown chunks，对每个 chunk 检查是否已有对应 UI 节点
   - 新增 chunk → spawn 新节点追加到 MsgBody
   - 变化 chunk → 替换该节点
   - 保持不变 chunk → 跳过
3. 这样大多数情况只新增不替换，避免闪烁

---

### A-7: bevy_resvg 在 xui 中破坏「纯依赖」约束

**问题**：xui 的定位是「纯依赖 bevy + xui_i18n，可独立发布」。方案将 `bevy_resvg` 作为 xui 的可选依赖（`default = ["icons"]`），但默认启用。

**影响**：
- 默认构建 xui 时引入 bevy_resvg + resvg + usvg + tiny-skia（C 依赖）
- 其他 Bevy 项目想用 xui 组件库但不需要 SVG 图标时，被迫引入这些依赖
- 违反 AGENTS.md「xui 不依赖非必要外部 crate」约束

**建议**：两种方案——
- **方案 A**：`icons` 改为**非默认 feature**（`default = []`），xgent_ui 在自己的 Cargo.toml 中启用 `xui/icons`
- **方案 B**：将图标系统（ThemedIcon + icon() + tint 系统）移到 `xgent_ui`，xui 只提供令牌/主题/基础组件（不含 SVG 依赖）

---

### A-8: 状态→渲染管道未明确

**问题**：方案有零散的 Resource（`DrawerState`、`ContextPanelCollapsed`、`CurrentTheme`）和组件（`PillState`），但没有统一的「UI 状态 → 渲染」管道设计。

**影响**：开发者不清楚 agent 状态变化如何驱动 AgentPill 颜色变化、文件树数据如何驱动 FileTreeDrawer 渲染。

**建议**：在方案中增加「状态驱动渲染」模式说明——

```rust
// 模式：Event/Message → Resource/Component 更新 → 系统查询变化 → 更新 UI

// 1. Agent 状态变化通过 ECS Event 传播
// xgent_agent 发出 AgentStateChanged { state: Thinking }

// 2. UI 系统订阅事件，更新 PillState 组件
fn update_agent_pill_state(
    mut events: EventReader<AgentStateChanged>,
    mut q_pill: Query<&mut PillState, With<AgentPillMarker>>,
) {
    for event in events.read() {
        if let Ok(mut state) = q_pill.single_mut() {
            *state = match event.state {
                AgentState::Idle => PillState::Ready,
                AgentState::Thinking => PillState::Thinking,
                // ...
            };
        }
    }
}

// 3. PillState 变化触发样式更新（Change Detection）
fn update_pill_styles(
    q_pill: Query<(Entity, &PillState), Changed<PillState>>,
    mut commands: Commands,
) {
    for (entity, state) in &q_pill {
        let (bg, text) = match state { /* ... */ };
        commands.entity(entity)
            .insert(ThemedBg(bg))
            .insert(InheritableThemedText(text));
    }
}
```

---

### A-9: SceneComponent 单元结构体无法运行时查询变体

**问题**：XButton / XIconButton / XPill 都是单元结构体（`pub struct XButton;`），所有数据在 Props 中。Props 不是组件，spawn 后不可查询。

**验证发现**：bevy_feathers 同样使用单元结构体，但通过 `template_value(props.variant)` 将 `ButtonVariant` 作为独立组件插入。hover/press 样式系统查询 `&ButtonVariant` 组件（而非 Props）。

**影响**：方案的 XButton BSN 场景中 `template_value(props.variant)` 会将 `ButtonVariant` 插入为组件，可以查询。但 XPill 的 `PillState` 同理——需通过 `template_value(props.state)` 插入为组件，样式系统查询 `&PillState`。

**建议**：确认所有需要运行时查询的状态（variant、state、disabled 等）都通过 `template_value()` 作为独立组件插入，而非仅存在 Props 中。

---

### A-10: build.rs 路径可移植性

**问题**：`../../doc/design/icons` 是相对于 `crates/xui/` 的路径。如果 xui 被其他项目作为依赖使用，此路径不存在。

**影响**：xui 无法真正独立发布——其他项目编译 xui 时 build.rs 会找不到图标源目录。

**建议**：
- 将 SVG 源文件直接放在 `xui/assets/icons/` 中（作为 xui 的一部分），不再从 `doc/design/icons` 拷贝
- 或在 build.rs 中检测路径存在性，不存在时跳过（仅 XGent workspace 内构建时预处理）
- 推荐前者：`doc/design/icons` 保留为设计参考，`xui/assets/icons/` 为实际使用的副本

---

## 二、编译问题（C 系列）— r3 未应用的修复

> r3 Review 发现了 P0-7 ~ P0-12 + P1-7 ~ P1-10 共 10 个问题，但因模型错误中断，**修复从未应用到方案文档**。以下确认当前方案中这些问题仍然存在。

| # | 问题 | 当前状态 | 影响 |
|:--|:-----|:---------|:-----|
| C-1 | `ThemedText(text)` 在 XButton（第 884 行）和 XPill（第 1002 行） | **未修复** | 编译失败（ThemedText 是单元结构体，无参数） |
| C-2 | `UiRoot` 在根布局（第 1065 行） | **未修复** | 编译失败（bevy_ui 无 UiRoot 组件） |
| C-3 | `Box::new(())` 在 XButtonProps（第 857 行）和 XPillProps（第 979 行） | **未修复** | 编译失败（() 不实现 SceneList） |
| C-4 | `props.state` 裸标识符在 XPill BSN（第 1003 行） | **未修复** | BSN 语法错误 |
| C-5 | `Changed<>` 过滤器在所有观察者 Query 中 | **未修复** | 观察者可能匹配失败 |
| C-6 | `smol_str = "1"` 在 Cargo.toml（第 726、732 行） | **未修复** | 版本不匹配 bevy 0.19（需要 0.2） |
| C-7 | `on_insert_themed_border` 实现缺失（第 302 行） | **未修复** | 编译失败（函数不存在但已注册） |
| C-8 | Button 应使用 `bevy_ui_widgets::Button` | **未修复** | 功能缺失（无 Activate 事件/Pressed 状态/无障碍） |
| C-9 | `update_theme_colors` 应在 PostUpdate 而非 Update | **未修复** | 传播时序问题（可能 1 帧延迟） |
| C-10 | XButton/XPill 子 Text 需加 ThemedText 标记 | **未修复** | 文字色不生效 |

---

## 三、新发现的编译问题

### C-11: `ThemedTextColor` 缺少 `PropagateOver::<TextColor>` — 可能被传播覆盖

**位置**：方案第 246-250 行

**问题**：方案的 `ThemedTextColor` 使用 `#[require(TextColor)]`，不要求 `ThemedText` 也不要求 `PropagateOver::<TextColor>`。

**分析**：
- `HierarchyPropagatePlugin::<TextColor, With<ThemedText>>` 的过滤器是 `With<ThemedText>`
- `ThemedTextColor` 不 require `ThemedText`，所以传播系统**不会**向此实体传播 TextColor
- 因此 `ThemedTextColor` 的直接设色不会被覆盖 ✅

**结论**：方案的设计实际上是**正确的** — 通过不加 `ThemedText` 标记来排除传播。这与 bevy_feathers 的做法不同（后者加 `ThemedText` + `PropagateOver` 来阻止传播），但两种方式都有效。方案的更简洁。**无需修改**，但应文档化此设计选择。

### C-12: `bevy_ui_widgets` 无 prelude 模块

**问题**：方案修复 C-8 后需 `use bevy_ui_widgets::Button;`，但 `bevy_ui_widgets` 没有 `prelude` 模块（文件不存在）。需直接从 crate 根导入。

**修正**：
```rust
use bevy_ui_widgets::Button;  // ✅ 直接导入
// 而非
use bevy_ui_widgets::prelude::*;  // ❌ prelude 不存在
```

### C-13: `bevy_ui_widgets::ButtonPlugin` 需注册

**问题**：`bevy_ui_widgets::Button` 的 6 个观察者（pointer down/up/click/drag-end/cancel + key event）需要 `ButtonPlugin` 注册才能工作。

**验证**：`bevy_ui_widgets::ButtonPlugin` 存在（button.rs 第 140 行），需在 App 中 `add_plugins(ButtonPlugin)`。

**修正**：在 XuiPlugin 中注册：
```rust
.add_plugins(bevy_ui_widgets::ButtonPlugin)
```

或确认 bevy 的 `UiPlugin` 是否已自动注册（需检查）。

---

## 四、技术选型再评估

| 选型 | 评估 | 结论 |
|:---|:---|:---|
| BSN 声明式 UI | bevy_feathers 全量验证，`{expr}` 语法 + `#name` 实体引用 + `template_value` + `bsn_list!` 均确认可用 | ✅ 选型正确 |
| bevy_resvg 图标 | `UiSvg` + `SvgColor` + 同实体 `ImageNode` 确认可用 | ✅ 选型正确 |
| 主题系统自建 | 代码正确但维护成本高；bevy_feathers 可直接用 | ⚠️ 建议评估路径 A |
| SceneComponent 单元结构体 | bevy_feathers 标准模式，数据在 Props | ✅ 模式正确 |
| Propagate 文字色传播 | `PropagateOver` 阻止父实体接收 + `ThemedText` 标记子实体接收，机制确认正确 | ✅ 设计正确 |
| pulldown-cmark Markdown | 成熟 crate，零依赖 | ✅ 选型正确 |
| 布局 flexbox 映射 | CSS Grid → 嵌套 flexbox 是标准做法 | ✅ 合理 |

---

## 五、总结

### 架构层面
- **A-1**（主题重复）是最大的架构决策点——建议评估直接用 bevy_feathers
- **A-2**（动态内容插入）是最大的文档缺口——必须补充
- **A-4**（按钮状态）是最大的功能缺口——必须实现 hover/press/disabled
- **A-3**（z-index）是必须补的基建——浮层正确渲染的前提
- **A-5**（双主题共存）是必须改的策略——一次性迁移更安全

### 编译层面
- **10 个 r3 修复未应用**（C-1 ~ C-10）——必须全部应用
- **3 个新发现问题**（C-11 无需改、C-12/C-13 需修正）
