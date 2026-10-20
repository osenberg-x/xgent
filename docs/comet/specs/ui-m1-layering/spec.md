# ui-m1-layering · UI v8 分层与模态

## Purpose

为 XGent 的 2D 界面建立**唯一的层级与键盘权威**：所有浮层挂在固定的层根上，z 值只来自常量或模态栈深度；指针命中有明确的 `Pickable` 白名单；Esc 只关闭栈顶模态，不再穿透到对话中断。本能力是 M2–M7 的结构前提。

本能力不改变视觉，不新增功能。

## Requirements

### 六静态层根

系统 SHALL 在 `theme::z` 定义层级常量：

```
BASE = 0        // 顶栏 / 主区 / 状态栏
DOCK = 100      // 侧区内联浮层
DRAWER = 200    // 文件/历史抽屉（非阻塞、无遮罩）
DROPDOWN = 300  // 补全、菜单、查找条（非阻塞、无遮罩）
MODAL_BASE = 400
TOOLTIP = 500
TOAST = 600
MODAL_STRIDE = 8
MODAL_MAX_DEPTH = 8
```

`layout.rs` SHALL 在 `UiRoot` 下并列 spawn 六个全屏绝对定位层根（BASE/DOCK/DRAWER/DROPDOWN/TOOLTIP/TOAST），每个层根：

- `Node { width: 100%, height: 100%, position_type: Absolute }`
- `GlobalZIndex(band)`
- `BackgroundColor(Color::NONE)`
- `LayerRoot(band)`
- **不带** `Pickable`

`TopBar`、`MainArea`、`StatusBar` SHALL 作为 `BASE` 层根的子节点。业务模块只向对应层根挂子树；浮层不得留在 `Overflow::clip()` 的子树内。

`kit.rs` SHALL 提供 `layer_root(band) -> impl Bundle` 构造器；`layout.rs` 与 `kit.rs` 是唯二允许出现 `GlobalZIndex(` 的静态位置（另一处是 `modal.rs` 的栈深派生）。

#### Scenario: 层根与 z 单源

- **Given** 应用启动完成
- **When** 检查 UI 世界
- **Then** 六个 `LayerRoot` 实体存在，其 `GlobalZIndex` 分别等于 `z::BASE/DOCK/DRAWER/DROPDOWN/TOOLTIP/TOAST`；`TopBar`/`MainArea`/`StatusBar` 的祖先链包含 `LayerRoot(z::BASE)`；业务代码中 `GlobalZIndex(` 只出现在 `layout.rs`/`kit.rs` 的层根与 `modal.rs` 的派生处

验收：A1

### 拾取模型

`xgent_app` SHALL 装配 `UiPickingSettings { require_markers: true, ..default() }`。

- 层根、布局容器、纯装饰节点**不得**带 `Pickable`。
- 所有可交互叶子（`Button`、输入框、列表项、可点卡片、页签、rail 项）**必须**带 `Pickable { should_block_lower: true, is_hoverable: true }`。
- 模态遮罩带 `Pickable { should_block_lower: true, is_hoverable: true }`。
- tooltip、toast 等穿透装饰层带 `Pickable { should_block_lower: false, is_hoverable: false }`。

#### Scenario: 拾取白名单与空层不吞点击

- **Given** 应用以 `require_markers = true` 启动
- **When** 检查层根与可交互控件
- **Then** 六个层根均无 `Pickable`；所有带 `Button` 的实体均有 `Pickable`；空层根不阻挡其下层的点击（结构断言 + 现有交互测试不回归）

验收：A2

### 模态栈

系统 SHALL 提供 `modal.rs`：

- `ModalStack { layers: Vec<ModalLayer> }`，`ModalLayer { id, entity, dismiss, opener_focus }`。
- `Dismiss { EscOnly, EscAndScrim, NotDismissable }`。
- `ModalPush { id, entity, dismiss }` / `ModalPop { id: Option<&'static str> }` Message；`ModalTopChanged` Event。
- `derive_modal_z`：栈内第 `depth` 层写 `GlobalZIndex(MODAL_BASE + depth * MODAL_STRIDE)`；`depth >= MODAL_MAX_DEPTH` 时拒绝入栈并记警告。
- `route_modal_input`：栈非空时，Esc 只作用于栈顶且按其 `dismiss` 决定是否出栈；栈顶不是 `EscOnly`/`EscAndScrim` 时不关闭。
- 全局快捷键（含 `chat.abort`）在栈非空时 SHALL NOT 派发到底层业务。
- 出栈时把 `InputFocus` 恢复到该层的 `opener_focus`。

`confirm_dialog.rs` 与 `settings_panel.rs` SHALL 通过 `ModalPush`/`ModalPop` 入栈、出栈；它们的 z 不再由自身写死。

#### Scenario: 设置面板 Esc 不穿透

- **Given** 对话正在进行且设置面板已打开
- **When** 按下 Esc
- **Then** 设置面板关闭；不产生 `AbortMessage`；确认弹窗在栈顶时 Esc 只交给确认弹窗；模态 z 等于 `400 + depth*8`

验收：A3

### 抽屉统一裁决

系统 SHALL 用「已注册可覆盖层集合」取代 `activity_bar.rs` 的手工互斥：

- `OverlayRegistry` 记录当前打开的可覆盖层（文件抽屉 / 历史抽屉）。
- `DrawerToggle(OverlayId)` Message 是唯一入口；`activity_bar.rs`、`context_scope.rs` 的「+ 添加」、`filepanel.toggle` 快捷键都发同一条消息。
- 单一系统裁决：打开任一层时关闭其他层；再次切换同一层则关闭它。
- 抽屉层的 z 恒为 `z::DRAWER`，低于 `z::MODAL_BASE`。

#### Scenario: 三个入口的抽屉互斥

- **Given** 历史抽屉已打开
- **When** 通过 rail 文件钮、或「+ 添加」、或 `filepanel.toggle` 打开文件抽屉
- **Then** 文件抽屉打开且历史抽屉关闭；反向切换同理；设置面板打开时抽屉不遮挡模态

验收：A4

### tooltip 顶层化

`kit.rs` 的 tooltip SHALL 挂在 `TOOLTIP` 层根下，而不是宿主节点的子节点：

- 按宿主的 `ComputedNode` 尺寸与 `GlobalTransform` 计算屏幕位置，默认出现在宿主右侧、垂直居中偏上。
- 右缘空间不足（`host_right + tip_width + margin > viewport_width`）时翻转到宿主左侧。
- tooltip 实体带 `Pickable { should_block_lower: false, is_hoverable: false }`。
- 宿主被裁剪或滚动时，tooltip 不随宿主子树被裁。

#### Scenario: 右缘 tooltip 完整可见

- **Given** 窗口宽度固定，齿轮按钮位于最右侧
- **When** 悬停超过延迟阈值
- **Then** tooltip 出现在 `TOOLTIP` 层根下、完整位于视口内、不与宿主子树共享祖先；右缘不足时翻转到左侧

验收：A5

### 设置面板 dismiss 与焦点恢复

设置面板 SHALL 以 `Dismiss::EscAndScrim` 入栈：

- Esc 关闭栈顶设置面板。
- 点击遮罩关闭。
- 关闭后 `InputFocus` 回到打开前的实体；该实体已不存在时清空焦点。

#### Scenario: Esc 与遮罩都能关闭设置面板

- **Given** 设置面板已打开
- **When** 按 Esc，或点击遮罩
- **Then** 面板关闭；焦点回到打开前的实体；再次打开仍可正常操作

验收：A6

## Constraints

- 只改 `xgent_ui` 与 `xgent_app`；不改协议类型与其他 crate 的运行时行为。
- 不新增色值、字号、圆角；沿用现有 `Theme` 字段。
- 抽屉与下拉不带遮罩；只有模态带 scrim。
- z 值只来自 `theme::z` 或 `ModalStack` 派生。
- 硬门槛：`require_markers = true` 后若大面积控件点不动且短期补不齐，回退该开关并另开子账，不得带病进入 M3。
