# XGent UI v8 · 布局与交互架构（技术设计）

> 状态：**提案**　时间：2026-10-16　基线 commit `d61e54b`
> 需求：[ui-v8-requirements-brief.md](ui-v8-requirements-brief.md)　证据：[ui-v8-audit.md](../notes/ui-v8-audit.md)　视觉：[ui-v8-design-system.md](ui-v8-design-system.md)
> 目标：修掉审计 P0 九条，并让路线图上的 Git/MCP/插件/成本界面**注册即落地**，不再返工布局。
> 范围：`xui`（通用原语）+ `xgent_ui`（业务布局）+ `xgent_app`（装配）。不改协议、不改 agent loop。

---

## 1. 架构原则

| # | 原则 | 反面（今天的违规） |
|:---|:---|:---|
| P-1 | **状态是权威，呈现是派生**。可见性/层级/选中态一律由单一系统从状态计算，业务代码不直接写 `Node.display`/`Node.width` | 5 个系统各自写显隐（`layout.rs:199`、`file_panel.rs:199`、`editor/mod.rs:342`、`terminal/mod.rs:475`、`welcome.rs:178`） |
| P-2 | **区域是注册表，不是枚举**。任何界面元素经描述符进入，宿主只认描述符 | `SideViewContent` closed enum（`editor/mod.rs:69-81`） |
| P-3 | **契约靠类型与原语保证，不靠 review**。滚动、截断、分层、字号只有唯一入口，违规编译不过或测试红 | `clip_y()`+`ScrollPosition` 并存 9 处；裸 z 值 8 处；40 处魔法像素 |
| P-4 | **引擎能力先核实再 prescribe**。所有 bevy API 引用须来自 `../bevy` 0.20 源码 | v7 期 BSN 评审 r4 的 10 条修复「只改 prose 未改代码」 |
| P-5 | **子系统间只经 ECS Events/Messages**，禁直接方法调用 | AGENTS.md §5.2 硬约束，本文全部接口遵守 |

本文所有 bevy 0.20 API 引用均在本会话核对过（标注 ✅）。

---

## 2. 分层与拾取（修 B1–B4、I1）

### 2.1 关键机制发现：`require_markers`

✅ `bevy_ui/src/picking_backend.rs:67` — `UiPickingSettings::default()` 是 `require_markers: false`。含义：

- 默认所有 UI 节点参与命中测试；
- ✅ `bevy_picking/src/lib.rs:212` — 「Entities without the `Pickable` component will **block** by default」。

**这就是今天浮层吞点击的机制性来源**：一个铺满全屏的浮层根节点即使视觉透明，也照样挡在下面板之上吃掉点击。反过来，✅ `picking_backend.rs:197` 显示当 `require_markers = true` 时，非文本节点若没有 `Pickable` 组件会被后端**直接跳过**。

因此 v8 的拾取模型是一条开关加一条纪律：

```rust
// xgent_app 装配阶段
app.set_resource(UiPickingSettings { require_markers: true, ..default() });
```

- 容器/层根/布局壳**不加** `Pickable` → 对拾取完全惰性（既不吞点击也不被 hover）。
- 可交互叶子（按钮、输入、列表项、可点卡片）**必须**带 `Pickable`，否则点不动 —— 这条由 §9 的几何断言层强制。
- 遮罩（modal scrim）带 `Pickable { should_block_lower: true, is_hoverable: true }`，即"吃掉"其下所有输入。
- 需要穿透的装饰浮层（tooltip、动画层）带 `Pickable { should_block_lower: false, is_hoverable: false }`。

### 2.2 六个静态层根

`UiRoot` 下并列六个全屏绝对定位层根，业务模块只往对应层挂子树。**这同时解决裁剪**：浮层不再位于被 `Overflow::clip()` 的 `MainArea` 子树内，tooltip 被裁的问题（B3）从结构上消失。

```rust
pub mod z {
    pub const BASE: i32 = 0;        // 主布局（顶栏/四列/状态栏）
    pub const DOCK: i32 = 100;      // 侧区内联浮层（页签内下拉）
    pub const DRAWER: i32 = 200;    // 抽屉（文件、历史）——非阻塞、无遮罩
    pub const DROPDOWN: i32 = 300;  // 补全、菜单、查找条——非阻塞、无遮罩
    pub const MODAL_BASE: i32 = 400;// 模态（设置、确认、冲突、命令面板）
    pub const TOOLTIP: i32 = 500;   // tooltip
    pub const TOAST: i32 = 600;     // toast / 通知
    pub const MODAL_STRIDE: i32 = 8;// 每个模态层占 8，容纳其内部下拉
    pub const MODAL_MAX_DEPTH: usize = 8;
}

#[derive(Component)] pub struct LayerRoot(pub i32);   // 携 GlobalZIndex + 全屏 Absolute + 无 Pickable
```

层根 bundle：

```rust
pub fn layer_root(band: i32) -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            ..default()
        },
        GlobalZIndex(band),
        BackgroundColor(Color::NONE),
        LayerRoot(band),
        // 故意不加 Pickable：require_markers=true 下对拾取惰性
    )
}
```

**约束**：`layout.rs:117,149` 的 `MainArea`/`ChatPanel` 保留 `Overflow::clip()`（区域内容确实需要裁），但任何"要浮在外面"的东西必须迁到层根，不允许留在裁剪子树里。用 lint/断言保证：带 `Dropdown`/`Modal`/`Tooltip` 标记的实体其祖先链上不得出现 `Overflow::clip`。

### 2.3 模态栈（唯一权威）

```rust
/// 模态层入栈即独占键盘；栈顶决定 z，也决定 Esc 关谁。
#[derive(Resource, Default)]
pub struct ModalStack { layers: Vec<ModalLayer> }

pub struct ModalLayer {
    pub id: &'static str,           // "settings" | "confirm" | "palette" | ...
    pub entity: Entity,             // 该层根
    pub dismiss: Dismiss,           // 键盘/点击外部策略
    pub tab_group_order: i32,       // 焦点隔离（见 §7）
}

pub enum Dismiss { EscOnly, EscAndScrim, NotDismissable }

/// ECS 消息（遵守 P-5：调用方发消息，不直接调方法）
#[derive(Message)] pub struct ModalPush { pub id: &'static str, pub dismiss: Dismiss }
#[derive(Message)] pub struct ModalPop  { pub id: Option<&'static str> }  // None = 栈顶
#[derive(Event)]  pub struct ModalTopChanged { pub top: Option<&'static str> }
```

`ModalStack` 是权威；一个 `derive_modal_z` 系统把 `GlobalZIndex` 写成 `MODAL_BASE + depth * MODAL_STRIDE`，一个 `route_modal_input` 系统保证：

- 栈非空时**所有**全局快捷键（含 `chat.abort`）不派发到底层 → 修 B4（设置面板开着按 Esc 却中断对话）。
- Esc 按栈顶的 `Dismiss` 策略生效 → 修 B4（设置面板无 Esc、无点击外部）。
- 遮罩实体带 `Pickable{block:true}`，点遮罩发 `ModalPop`。

同时修 **B2**：抽屉互斥不再由 `activity_bar.rs:274-287` 手工维持——抽屉改用「已注册的可覆盖层」集合，`context_scope.rs:202-209` 的「+ 添加」与 rail 钮、快捷键走同一条 `DrawerToggle` 消息，由单一系统裁决。

---

## 3. 槽位注册表与区域模型（修 D1–D6、I3）

### 3.1 描述符取代 closed enum

```rust
/// 可注册目标。与插件系统 ui_panel 扩展点对齐（plugin-system-design.md:954-955）
pub enum Slot { Rail, Center, SideDock, BottomDock, SettingsSection, StatusSegment, Palette, Overlay }

#[derive(Clone)]
pub struct PanelDescriptor {
    pub id: &'static str,          // "editor" | "terminal" | "diff" | "git" | "mcp" | ...
    pub slot: Slot,
    pub title_key: &'static str,   // i18n fluent key，禁止裸字符串（修 H1）
    pub icon: IconId,
    pub order: i16,                // 同槽内排序，允许稀疏
    pub closeability: Closeability, // Closable | Sticky（editor 可钉住）
    pub min_size: Vec2,
    pub default_size: Option<f32>,
    pub when_visible: fn(&PanelQuery<'_>) -> bool,  // 可见性谓词，不持状态
}

#[derive(Resource, Default)]
pub struct PanelRegistry { entries: Vec<(PanelDescriptor, PanelRuntime)> }

pub struct PanelRuntime { entity: Option<Entity>, visible: bool, focused: bool }
```

**`Center`（2026-10-19 增补）**：中央列同一时刻**只有一个**活跃面板，默认是「对话」。`Center` 与 `SideDock` 的差别是**互斥**而非并列——同槽任意时刻至多一个 `visible`，切换由 `PanelActivate` 表达，旧面板自动失活，业务侧不写 `display`。首个消费者是 3D 空间视图（[ui-v8-3d-viewport.md](ui-v8-3d-viewport.md) §1.2）：它证明「换主视图」不需要新增 closed enum，也把「对话 / 空间 / 未来的任务看板」统一到同一条规则上。

注册经 ECS 消息（P-5）：

```rust
#[derive(Message)] pub struct PanelRegister { pub desc: PanelDescriptor, pub view: Entity }
#[derive(Message)] pub struct PanelActivate { pub id: &'static str }
#[derive(Message)] pub struct PanelDeactivate { pub id: &'static str }
```

### 3.2 派生可见性（取代五处写手）

```rust
/// 唯一写 Display 的系统。其余模块只发 PanelActivate/Deactivate。
fn derive_panel_visibility(
    registry: Res<PanelRegistry>,
    slots: Res<SlotLayout>,
    mut q: Query<&mut Node, With<PanelRuntimeMarker>>,
) { /* 依 registry 计算期望 display，仅变化时写 */ }
```

修 **D4**（rail 陈旧激活）：rail 的选中态不再独立存储，`ActiveActivity` 删除，改为

```rust
let active = registry.iter_rail().find(|p| slots.is_visible(p.id));
```

连点两次关掉了抽屉，选中态自然回到派生结果，不会停留。

修 **D3**（双 tab 条 / 双返回按钮）：页签条由 `SlotLayout` 拥有并渲染，`editor/mod.rs:220-239` 的编辑器内部 tab 条降级为**文档列表**（不是槽位页签），两套 `EditorBackButtonMarker`（`:220-239` 与 `:706-731`）合一。

### 3.3 尺寸契约（修 D2 的 38px 永久裁切）

今天 `SideView` 内每页签都是 `height:100%`，而同列还有 38px 页签条（`editor/mod.rs:184,262,54` vs `:668`）→ 底部 38px 掉出视口。规则化：

```rust
/// 区域填满只允许这一种写法
pub fn region_fill() -> Node {
    Node { flex_grow: 1.0, min_width: Val::ZERO, min_height: Val::ZERO, ..default() }
}
```

并加静态纪律：**有兄弟节点的列内禁止 `height: Val::Percent(100.0)`**。由 §9 的几何断言在测试期兜住（断言每个面板根 `ComputedNode.size().y ≈ 分配高` 且子节点 `Rect` 在视口内）。

### 3.4 分割模型（修 D6）

```rust
#[derive(Resource, Default)]
pub struct SplitTree { root: Split }

pub enum Split { Leaf { id: &'static str, size: LeafSize }, Splitter { axis: Axis, children: Vec<Split>, handles: Vec<HandleState> } }
pub enum LeafSize { Fixed(f32), Min(f32, Preferred(f32)), Grow(f32) }
```

`resize.rs` 从「只认 `ResizeEdge::Right` 且 `PanelWidths` 单字段」（`resize.rs:47-50,205`）改为遍历 `SplitTree` 生成分割条，每个分割条持 `HandleState { target: usize }`，拖动写回相邻 leaf 比例。抽屉宽度从定值 `DRAWER_W=320`（`theme.rs:236`）变成 `Min(240, Preferred(320))`，上限 480。

**不做**自由 docking / 拖拽重排（需求简报第五节范围外）。`SplitTree` 只支持声明式注册的结构，用户只能改尺寸不能改拓扑。

---

## 4. 滚动契约（修 C1、C2、D2）

### 4.1 机制性根因

✅ `docs/notes/scroll-abstraction-design.md:19` 记录：`ScrollPosition` 只在 `OverflowAxis::Scroll` 轴生效。而全仓 9 处用 `Overflow::clip_y()`（`file_panel.rs:186`、`session_history.rs:161,221`、`command_palette.rs:157,201`、`confirm_dialog.rs:221`、`editor/mod.rs:252`、`settings_panel.rs:796`、`tool_panel.rs:211`）配 `ScrollPosition` → `ScrollPosition` 被写但引擎不消费。

### 4.2 唯一入口

`xui` 已有正确实现（`scroll_area.rs:104-105` 正是 `x: Hidden, y: Scroll`），问题是业务绕过它手搓节点。v8 把构造器收窄成**唯一可用形态**：

```rust
/// 唯一的可滚动区域入口。返回 bundle，调用方不再自己拼 Node。
pub struct ScrollFrame;
impl ScrollFrame {
    pub fn vertical() -> ScrollFrameParts;   // x:Hidden y:Scroll + ScrollPosition + 视口 + 滚动条
    pub fn horizontal(self) -> ScrollFrameParts;
    pub fn stick_bottom(self) -> ScrollFrameParts;  // 会话区
}

// ScrollFrameParts 内含：
//   ScrollAreaMarker, Node{overflow 由构造器定，不接受外部覆盖},
//   ScrollPosition, Pickable（列表项各自带）, 及滚动条子树
```

纪律与验证：

- 迁移后 grep `Overflow::clip_y()` 在 `xgent_ui` 应为 **0**（除真正需要硬裁的装饰层，需注释声明）。
- 模型列表这类"只给了 max_height 没给滚动"的（`settings_panel.rs:791-800`）改挂 `ScrollFrame::vertical()`。
- 输入框（`chat_panel.rs:182-219`，min 38/max 200 无滚动）改为自适应高度 + 到上限后 `ScrollFrame::vertical()`，并保证光标行可见（复用 `xui::scroll_to_child`，`scroll_area.rs:12`）。
- chips 行（`chat_panel.rs:301-339`）改 wrap 容器。

### 4.3 虚拟化（修 I4 / R3-6）

`xui::VirtualList`（`virtual_list.rs`，140 行，**零消费者**）接两处：会话消息列表、文件树。测量策略：等高行用常量（消息行不等高 → 用「估算高 + 首帧实测回写」的累积偏移表，`VirtualList` 需新增 `measure` 回调）。阈值：条目 > 50 才启用，避免短列表抖动。

---

## 5. 文本省略与测量（修 C3、E6）

### 5.1 引擎缺口（已核实）

✅ bevy 0.20 **没有** `TextTruncate`：全仓 grep `truncat` 仅命中 `bevy_text/src/bounds.rs` 的文档注释与 `stack.rs`。可用的是 `TextBounds`（`bounds.rs:13-27`），其行为是"按字符硬裁"，且官方注释自陈：

> "only characters that are completely out of the bounds will be truncated, so this is not a reliable limit"

所以省略号**必须自建**，不是"没接线"。

### 5.2 `xui::Ellipsis` 实现

测量 API 可用：✅ `bevy_text/src/pipeline.rs:485` `TextLayoutInfo`，由 `TextPipeline::update_text_layout_info` 在实体具备 `TextLayout` + `ComputedTextBlock` 时生成，含 `size: Vec2`（物理像素）。

```rust
#[derive(Component)]
pub struct Ellipsis { pub source: String, pub tail: char /* '…' */, pub min_keep: usize }

/// 两帧流水线：第一帧让文本自然布局并测量，随后按可用宽二分前缀、追加省略号。
fn ellipsis_system(
    pipeline: Res<TextPipeline>,
    mut q: Query<(Entity, &mut Text, &Ellipsis, &Node, &ComputedNode, &TextFont, &mut TextLayout)>,
    mut cache: ResMut<EllipsisCache>,
) { /* 命中 (source, avail_w, font_size) 直接复用；未命中才 update_text_layout_info */ }
```

要点：

- **缓存**按 `(source hash, 可用逻辑宽量化到 1px, font_size)` 键，避免逐帧重排（会话区一屏十几个截断点，逐帧 measure 不可接受）。
- 结果**不抖**：可用宽量化 + 迟滞（变化 < 2px 不重算），拖窗口时不闪烁。
- 完整值进 tooltip（走 §2.2 的 `TOOLTIP` 层，故不受祖先裁剪）。
- 多行 clamp 用 `TextBounds::new(w, h_max)` + 行数上限；「展开」是状态切换而非另一套布局。
- 路径类额外用 `MidEllipsis`（保留末两段，设计系统 T-2）。

替代方案（更省但更糙）：仅给 `TextBounds` 不做省略号。选**自建省略号**，因为无 `…` 时用户无法区分"被裁"与"就这么长"，在文件树/页签上是可用性问题。

---

## 6. 有序消息树与富文本管线（修 E1、E2）

### 6.1 顺序权威化

今天顺序靠命令式挪子节点：`chat_panel.rs:489` `add_child(current)`、`tool_panel.rs:85` append、`chat_panel.rs:724` 压缩通知、`undo.rs:286` → 流式文本排到自身工具卡之上。改为状态权威：

```rust
/// 会话呈现的唯一权威：有序块序列
#[derive(Resource, Default)]
pub struct Transcript(Vec<TurnRef>);

pub struct TurnRef { pub id: MessageId, pub role: Role, pub blocks: Vec<BlockRef>, pub seq: u64 }
pub enum BlockRef { Text(BlockId), Code(BlockId), ToolCall(BlockId), Notice(BlockId), Compaction(BlockId) }
```

渲染系统只做「按 `Transcript` 顺序，为缺失的块 spawn、为多余的节点 despawn、为变化的块更新」，顺序键 = `(turn_index, block_index)`。**彻底禁止 `add_child` 后靠调用顺序决定视觉顺序**。增量而非整段重建 —— 这正面回应 BSN 评审 r4 的 A-6（markdown 重建性能）。

### 6.2 Markdown 块模型

不引重型渲染 crate 的先例说明：`ui-gap-plan.md` 阶段 D 至今未做，且 `bevy-ui-mode-assessment.md:101-105` 把「富文本/代码高亮偏弱」列为唯一 P1 风险。取中间路线：

```rust
pub enum MdBlock { Heading{level,inline}, Paragraph(Vec<Inline>), CodeBlock{lang, text},
                  List{ordered, items}, Quote(Vec<MdBlock>), Table{..}, ThematicBreak }
pub enum Inline { Text(String), Code(String), Bold(Vec<Inline>), Italic(Vec<Inline>), Link{..}, Strike(Vec<Inline>) }
```

- 解析：轻量自研或引 pulldown-cmark（纯 Rust、无 Bevy 依赖，符合依赖边界）。**倾向 pulldown-cmark**，自研解析器是新的维护负担；需在 M5 前做一次依赖评审。
- 排版：每块 = 一个 `Text` 节点 + 多个 `TextSection`（bevy 0.20 支持单节点多 section，✅ `picking_backend.rs:219` 的 `pick_ui_text_section` 即证明 section 级文本存在）→ 行内加粗/行内码用不同 section 的 `TextColor`/`TextFont`，不拆成多节点。
- 代码块：`surface_code` 底 + `mono` + 语言标签 + 复制按钮；语法色复用已落地的 `xgent_ui::theme` 语法色与 `xui` 高亮链（`editor/../highlight.rs`）。
- 流式：增量 token 追加到当前 `Text` 块；块边界（如遇到 ``` fence）才新建节点。复制按钮的文本取"定稿全文"而非"当前屏可见"。
- `Ellipsis`/`Clamp` 与本节共用 §5.2 的测量缓存。

### 6.3 输入区（修 E3、E4）

- 显式动作条：发送（`accent`）/ 停止（流式中，`st_running` + pulse），禁用态由 `AgentStatus` 派生；Enter 发送保留。
- `@` 内联补全：浮层挂 `DROPDOWN` 层（**不能**挂在输入框子树里，否则被 `ChatPanel` 的 `clip` 裁），方向键导航 + `Enter` 接受，复用 §7 的 `DirectionalNavigation`。今天 `@file:`/`@cursor` 只在提交时静默解析（`chat_panel.rs:776`、`at_syntax.rs:10`）。
- 上下文集合模型化（修 E5）：`ContextSet` 独立 Resource，可增删可持久化并显示 token 占比；今天 `context_scope.rs:1-7` 只是打开 tab 的只读镜像。

---

## 7. 焦点、遍历与快捷键闭环（修 F1–F4、B4）

### 7.1 用引擎现成能力，不自建

✅ bevy 0.20 `bevy_input_focus` 提供：

- `tab_navigation::{TabNavigationPlugin, TabIndex, TabGroup}`（`tab_navigation.rs:64/73/405`），且 `TabGroup { order, modal }` + **`TabGroup::modal()`**（`:94`）—— 模态组内 Tab 只在该组循环（`:88-92` 注释明确）。这正是模态焦点隔离需要的。
- `InputFocusVisible(bool)`（`lib.rs:182`）—— 区分键盘焦点与指针焦点，焦点环只在键盘导航时出现（符合桌面工具惯例）。
- `directional_navigation`（`lib.rs:16`）+ `bevy_ui` 的 `auto_directional_navigation.rs` —— 列表/命令面板方向键导航。

`InputFocusPlugin` 在 `DefaultPlugins` 内（✅ `default_plugins.rs:16`），但 `TabNavigationPlugin` **不在默认插件里**（✅ `tab_navigation.rs:18` 文档要求手动 add）。v8 装配：`TabNavigationPlugin` + `DirectionalNavigationPlugin` + `InputDispatchPlugin`。

### 7.2 纪律

- **`TabIndex` 由布局顺序派生**，不再全 0（今天 `chat_panel.rs:219` 起每个输入都 `TabIndex(0)`，且 `tests/settings_input_focus.rs:154-167` 把这个错误当不变量断言）。同一模态内按视觉顺序递增，间隔 10 便于插入。
- 每个模态层根挂 `TabGroup::modal()`，`tab_group_order` 取栈深 → Tab 永不跳出模态。
- 焦点环消费 `theme.accent_interactive` + `accent_glow`（`theme.rs:62-63` 有令牌但**零消费**，即 F4）。
- `AutoFocus` 在层入栈时打在首个可聚焦元素上；出栈时归还原焦点。

### 7.3 快捷键「注册即生效」

死键根因是注册表与分发表两处手写、彼此漂移（`palette.open_files` 在 `shortcuts.rs:44` 注册、全仓零消费者；`input.focus => {}` 空实现 `:196`）。改为单一来源：

```rust
pub struct HotkeyAction {
    pub id: &'static str,
    pub label_key: &'static str,
    pub default: Vec<Hotkey>,
    pub when: Context,                 // InDock(Slot) | WhenVisible(&str) | Always | NotModal
    pub handler: fn(&mut MessageWriter, &mut Commands, &HotkeyCtx),  // 必填，无空实现
}
```

- `HotkeyAction::handler` 是**函数指针而非 match 分支** → 注册时必须提供，语言层面消灭空实现。
- 启动期一致性断言：`registered_ids == actions_ids`，且每个 `action.when` 可解析。测试红即为回归（今天这类漂移完全无测，审计 I2）。
- 键位标签与显示同源 → 命令面板里显示的快捷键永远是真实绑定。
- `when` 与模态栈联动：模态开着时 `Always` 之外的动作不派发（修 B4 的 Esc 穿透）。

---

## 8. 主题热驱动与字体装配（修 A1、A3、G1、G2）

### 8.1 主题成为可替换权威

今天 `Theme` 在 Startup 烘焙一次（`layout.rs:60` 的 `init_resource` + spawn 时取值），`preferences.theme` 无读取方（R3-14），且 `diff_view.rs:217` 现场 new 一个 `Theme::dark()`。改为：

```rust
#[derive(Resource)] pub struct ThemeHandle { pub theme: Theme, pub generation: u32 }
#[derive(Event)]  pub struct ThemeChanged { pub generation: u32 }

/// 每个可重涂节点带来源标记，重涂系统据此更新而非重建
#[derive(Component)] pub struct Themed { pub role: ThemeRole, pub gen: u32 }

fn repaint_on_theme_change(gen: Res<ThemeHandle>, mut q: Query<(&Themed, &mut BackgroundColor, &mut TextColor, &mut BorderColor), With<Themed>>, ev: EventWriter<ThemeChanged>) { /* gen 落后才写 */ }
```

`Themed` 携带 `role` 而非具体色值 → 换肤/换密度是**属性写回**，不是节点重建（呼应 P-1、避免 BSN 评审 A-6 那类重建开销）。配置热应用已有通路（R3-13 已闭合），v8 只需把 `preferences.theme`/`density` 接到这里。

同时：`Theme::dark()` 可见性降为 `pub(crate)`，`theme.rs` 成为唯一色源；语义令牌层见设计系统第六节。

### 8.2 字体装配层

```rust
pub struct FontSpec { pub slot: FontSlot, pub candidates: &'static [FontCandidate] }
pub struct FontCandidate { pub family: &'static str, pub path: Option<&'static str>, pub embedded: Option<&'static [u8]> }

#[derive(Resource)] pub struct UiFonts { pub ui: Handle<Font>, pub mono: Handle<Font>, pub metrics: MonoMetrics }
#[derive(Resource)] pub struct MonoMetrics { pub latin_advance: f32, pub cjk_advance: f32, pub line_height: f32, pub at_size: f32 }
```

- 候选栈按设计系统 1.2 表；`mono` 的最后一条**必须是内嵌 JetBrains Mono**（OFL 1.1，随包分发，与 `Inter-Variable.ttf` 同目录 `crates/xgent_app/assets/fonts/`）。今天非 macOS 走 `Handle::default()` 落回 Inter（`startup.rs:88`）→ **删除该分支**，替换为内嵌资产。
- 解析结果 `tracing::info!` 输出实际家族名（C-FONT-2）。
- `MonoMetrics` 启动期实测：渲染基准串，经 §5.2 的同一 measure 路径取 `latin_advance`/`cjk_advance`，替代 `terminal/output.rs:380` 的 `font × 0.6` 估算。
- 三平台一阶：平台差异只体现在候选栈**顺序**这一份数据上，代码零 `cfg!` 分叉（C-FONT-3）。

### 8.3 终端双宽单元格（修 A3）

```rust
pub struct CellGrid { pub cols: usize, pub rows: usize, pub cell_w: f32, pub line_h: f32 }
pub fn display_width(ch: char) -> usize;   // wcwidth 语义：CJK 全角/emoji/U+FF00-FF60 → 2，其余 → 1
```

列数 = `floor(视口逻辑宽 / metrics.latin_advance)`；一行内每格按 `display_width` 占 1 或 2 格；**定位按格数 × cell_w 手工摆位，不依赖文本 shaper 连续排版**。理由见设计系统 1.3：拉丁走 JetBrains Mono、中文走 Sarasa/Noto 时，两个字体没有共享的连续排版度量，按格定位是三平台唯一成立的做法（与 alacritty/kitty 同构）。

今天中文输出按 1 格排版（`output.rs:379-380,400-402` 无任何 CJK 分支）→ 中文表格与进度条必然错位。这是三平台一阶前提下的 P0。

---

## 9. 验证层：布局几何断言（修 I2）

现有 UI 测试只断言 marker 存在与 `Display` 状态（`tests/layout.rs:28-33`、`interaction_model.rs:160-283` 用合成节点），所以裁剪/重叠/滚不动/吞点击**不可能被发现**。v8 新增 `xgent_ui/tests/layout_geometry.rs`，提供可复用断言：

```rust
fn assert_fills_region(world: &mut World, panel: Entity);        // size().y ≈ 分配高，无越界
fn assert_no_overflow(world: &mut World, root: Entity);          // 子 Rect ⊂ 父 Rect（滚动内容除外）
fn assert_scrollable(world: &mut World, list: Entity, items: usize); // 内容高 > 视口时末项可达
fn assert_truncated_in_width(world: &mut World, node: Entity);   // 文本宽 ≤ 父可用宽
fn assert_pickable_leaf(world: &mut World, btn: Entity);         // 交互叶子带 Pickable
fn assert_no_blocking_empty_layer(world: &mut World);            // 空层根不吞点击（拾取穿透）
```

前提：`ComputedNode`/`UiRect`/`GlobalTransform` 在测试 app 内已计算，需要 `run_app` 若干帧让布局与文本测量落地（与 `settings_input_focus.rs:154-167` 的既有做法一致）。

配套：`ui-snapshot` feature（v7 M2-T6 已建，`ui-v7-tasks.md:12`）出图与 §11 原型逐状态人工对照。**每个 P0 缺陷一条"先红后绿"回归**，共 ≥ 9 条。

---

## 10. i18n 与状态健壮性（修 H1–H3、I1）

- **H1**：现存硬编码全走 fluent —— `activity_bar.rs:130-167`、`top_bar.rs:222,225`、`editor/mod.rs:659-664`（`let _ = loc;` 故意丢弃 Localizer）、`status_bar.rs:140`、`chat_panel.rs:385`。页签/分区标题从描述符的 `title_key` 取，天然单源。
- **H2**：`"$0.00"` 占位改「未启用/暂无数据」语义 key；`"▼"` 字形改用图标资产。
- **H3**：`xui` 的 `StringSource` 桥接真正被消费（R3-7/D5）—— `command_palette`/`virtual_list`/`scrollbar` 的 aria 标签与空态文本走 trait。
- **i18n 校验扩面**：`settings_panel_i18n.rs:55,72` 只扫 settings 是字面量存活的原因（审计 H 段脚注）。改为全模块扫描 + EN 语料无 CJK 码位 + zh 语料无缺失 key，纳入 CI。
- **I1**：删 `ChatPanelEntities` 这类把 `Entity` 缓存进 Resource 的做法（`chat_panel.rs:85-92`），改 marker 查询；`q.single_mut()` 的静默早退（`chat_panel.rs:796,889`、`top_bar.rs:293`）改 `single()` + `debug_assert` + `tracing::warn!`，布局重建后不再无诊断失效。

---

## 11. 与插件扩展点的接合

| 扩展点 | 现状 | v8 接法 |
|:---|:---|:---|
| `command` → `xui::CommandRegistry` | 已落地，含前缀路由 | 描述符化：palette 条目由 registry 生成，支持 `unregister(plugin_id)`（`plugin-system-design.md:415`） |
| `ui_panel` → 宿主渲染 | **声明未实现**（D-P3，`:948-956`） | `PanelDescriptor{slot,order,title_key,icon}` 即其宿主侧落点；插件面板 = `SideDock` 槽的一个注册项 |
| 工具卡 | 已渲染，无真实 diff（R3-16） | `BlockRef::ToolCall` 进有序树（§6.1）；diff 依赖 R3-12 另账 |
| `on_update` 流式 | **未接通**（D-P8，执行器恒传 `None`，`:992-996`） | 渲染层只认异步状态：无进度时显示「执行中」，**不得**假设本地执行、不得空白 |
| 插件字符串 i18n | 未决（D-P6） | `title_key` 允许字面量兜底，插件未提供语料时降级显示 id 并 warn |

守护进程升级（瘦→中→胖，`architecture.md:370-386`）对本架构的影响只在 §6.1 的顺序权威与 §11 最后一行：**渲染自异步状态**这条立住，索引进度、跨 IPC 取消、批量确认队列都只是新的块类型与新的状态字段。

---

## 12. 迁移策略（strangler，非重写）

v8 不是把 `xgent_ui` 推倒重来（BSN 正是死于"重写"）。按 P0 依赖顺序逐区替换，每步可编译可测：

```
第 1 步  层根 + require_markers + ModalStack     → 立刻消灭吞点击（B1/B2/B4）
第 2 步  字体装配 + MonoMetrics + 双宽单元格      → 消灭 A1/A3（风险最高，独立提交便于回滚）
第 3 步  ScrollFrame 迁移 9 处 + region_fill     → 消灭 C1/C2/D2
第 4 步  Ellipsis/Clamp 原语 + 接 6 处           → 消灭 C3/E6
第 5 步  PanelRegistry 取代 SideViewContent      → 消灭 D1–D6，改派生可见性（I3）
第 6 步  Transcript 有序树 + Markdown            → 消灭 E1/E2
第 7 步  HotkeyAction 单一来源 + TabGroup/焦点    → 消灭 F1–F4
第 8 步  主题热驱动 + 令牌收敛                    → 消灭 G1–G5
第 9 步  i18n 扩面 + 几何断言层 + 陈旧文档勘误     → 消灭 H/I2
```

第 5 步面最大，给 `SideViewContent` 留**临时 shim**（枚举 → 描述符的一次性映射），使 M5 的改动可分小批合入，每批都能编译。第 1、2 步彼此独立，可并行。

**风险与预案**：

| 风险 | 缓解 |
|:---|:---|
| `require_markers=true` 后某控件点不动 | 该情况是**暴露既有缺失**而非新缺陷；几何断言 `assert_pickable_leaf` 在迁移期全量扫，逐条补 `Pickable` |
| 双宽单元格重写影响终端已验收行为 | 保留旧路径在 feature flag 下，A/B 截图对照后再删 |
| Markdown 引入新依赖 | M6 前走一次依赖评审；退化方案是仅支持代码块 + 列表 + 粗体（覆盖 90% 输出） |
| `PanelRegistry` 改造牵动 editor/terminal 内部 tab 语义 | 文档列表与槽位页签分层（§3.2），二者不合并 |
| 字体资产体积（JetBrains Mono 约 400KB） | subset 到 ASCII + 常用符号；CJK 走系统发现不入库 |

---

## 13. 本轮不做（防范围膨胀）

BSN 声明式框架与 resvg 运行时图标（审计第七节逐条否决）；`SceneComponent` 可查询层；亮色主题色板；全局 rem 缩放；自研动画框架；自由 docking；`xui` 依赖边界变更（NU-4）。
