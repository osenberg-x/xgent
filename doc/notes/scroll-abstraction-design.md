# 滚动能力通用封装设计

## 1. 背景：为什么要封装

项目内已多次独立实现"可滚动区域"，每次都要重新调试同一组问题：

| 实现 | 位置 | 踩过的坑 |
|:---|:---|:---|
| 消息列表（对话预览） | `xgent_ui/src/chat_panel.rs` | flex 撑破、贴底误判、单位混淆、时序错位 |
| 文件编辑器 | `xgent_ui/src/editor/tabs.rs` + `xui/src/text_editor/virtual_render.rs` | flex 撑破（缺 `min_height:0`，滚轮 `max_offset≈0`） |
| 通用滚轮桥接 | `xui/src/mouse_wheel_scroll.rs` | 已封装，作用于任意 `OverflowAxis::Scroll` 节点 |

文件编辑器这次重踩了 chat_panel 早就总结过的"flex 防撑破"坑。根因是**滚动契约散落在各业务模块**，没有一处集中表达"怎样构造一个能正确滚动的 bevy UI 容器"。

## 2. bevy 滚动的本质契约

bevy 0.19 的滚动由三层组成，缺一不可：

1. **节点布局契约**（`bevy_ui::layout`）：容器需 `Overflow::scroll_y()`（`y: OverflowAxis::Scroll`）。`ScrollPosition` 只对 `Scroll` 轴生效，`Clip` 仅裁剪渲染不影响布局、`Hidden` 影响布局但 `ScrollPosition` 不响应。
2. **flex 防撑破契约**：`flex_grow:1.0` 默认 `flex-basis: auto`，主轴纵向时若子内容高于容器，basis 跟着撑大，容器反被内容撑到内容高度 → `size.y ≈ content_size.y` → `max_offset = content_size - size ≈ 0`，滚轮无意义。必须配 `min_height: Val::ZERO`（必要时 `min_width:0`、`flex_basis:0`、`flex_shrink:1.0`）让容器收缩到父容器给定的视口高度。
3. **滚轮→ScrollPosition 桥接**：bevy 不自动映射 `MouseWheel`，需读 `HoverMap` 沿悬停路径触发实体事件，handler 累加 `ScrollPosition`。已由 `MouseWheelScrollPlugin` 通用封装。

**关键时序**：`ScrollPosition` 组件值在 `Update` 阶段被业务系统读写；`ComputedNode.content_size` / `size` 在 `PostUpdate` 的 `UiSystems::Layout`（`ui_layout_system`）才更新。读 `max_offset` 做跟随/钳位必须在 `PostLayout` 之后，否则用过时值。

**clamp 责任**：bevy 布局系统自身在 `layout/mod.rs:365` 用当帧最新 `content_size` 对 `ScrollPosition` 做 `[0, max]` 钳位（写回 `ComputedNode.scroll_position`，不回写组件）。业务代码不应再用上一帧 `content_size` 手动 clamp `ScrollPosition` 组件，否则会把它锁死在过时的 0。

## 3. zed 的抽象（参考，非照搬）

zed（gpui）的滚动核心在 `crates/gpui/src/elements/div.rs` 的 `ScrollHandle`：

- **句柄模式**：`ScrollHandle(Rc<RefCell<ScrollHandleState>>)`，挂在 div 上，业务层通过句柄调用，状态集中。
- **API**：`offset()` / `set_offset()` / `bounds()` / `bounds_for_item(ix)` / `scroll_to_item(ix)`（最小滚动让子项可见）/ `top_item()` / `logical_scroll_top()` / `set_logical_scroll_top(ix, px)`（子项索引+像素偏移，虚拟列表友好）。
- **editor 层**（`editor/src/scroll.rs` + `scroll/autoscroll.rs`）：`Autoscroll` 策略枚举 — `Fit` / `Newest` / `Center` / `Focused` / `TopRelative(n)` / `Bottom`。`Autoscroll::bottom()` 即我们的"贴底跟随"。

差异：gpui 是立即模式+句柄，bevy 是保留模式+组件。bevy 下"句柄"对应"组件 + 系统"，状态直接存 ECS。zed 的 `scroll_to_item` / `logical_scroll_top` 思路可移植为 bevy 系统。

## 4. 封装设计

在 `xui` 新增 `scroll_area` 模块，提供三件套：

### 4.1 `ScrollArea` —— 节点契约 Bundle

集中表达"能正确滚动的容器"所需组件与样式，消除每次手搭 flex 防撑破契约的重复。

```rust
/// 可滚动容器契约：保证 ScrollPosition 生效 + flex 不被内容撑破。
///
/// 调用方 spawn 此 bundle 后往里挂子节点即可滚动。
/// 横向滚动同理（axis 配置，MVP 仅纵向）。
#[derive(Bundle)]
pub struct ScrollArea {
    pub node: Node,           // 预置 min_height:0 / flex_grow:1 / Overflow{Hidden,Scroll}
    pub scroll_position: ScrollPosition,
    pub marker: ScrollAreaMarker,
}
```

提供 `ScrollArea::vertical()` / `ScrollArea::horizontal()` 构造器，内部填好防撑破样式。业务方仍可在外层 `Node { .. }` 覆盖 `padding` / `gap` 等。

### 4.2 `StickToBottom` —— 贴底跟随组件

把 chat_panel 的 `MessageListStickBottom` + `maintain_stick_bottom` + `auto_scroll_to_bottom` 泛化。

```rust
/// 挂在 ScrollArea 上：内容增长时自动滚到底部，用户上滚后停止跟随，
/// 重新滚回底部附近恢复跟随。
#[derive(Component, Default)]
pub struct StickToBottom {
    pub stick: bool,
    /// 判定贴底的阈值（逻辑像素）
    pub threshold: f32,
}
```

系统 `maintain_stick_to_bottom`（仅本帧有滚轮时刷新 stick）+ `auto_scroll_to_bottom`（stick 为真时推到底），均在 `PostUpdate` 的 `PostLayout` 后跑。`MouseWheelScrolled` 已是 `MouseWheelScrollPlugin` 的公共 resource，直接复用。

### 4.3 `scroll_to_child` —— 滚动到子项可见

对应 zed `scroll_to_item`。MVP 提供一个系统/工具函数：给定滚动容器实体 + 目标子实体，读两者 `ComputedNode` 的 `content_size`/`size`/`scroll_position`，最小调整 `ScrollPosition` 让子项进入视口。用于"跳转到某行/某消息"。

虚拟化场景（编辑器）调用方传"目标行对应的占位偏移"即可，ScrollArea 不耦合虚拟化实现。

### 4.4 `ScrollAreaPlugin`

注册上述系统，配置 `PostLayout` 后的时序。挂到 `XuiPlugin`。

## 5. 迁移计划

1. 新增 `xui/src/scroll_area.rs`（Bundle + 组件 + 系统 + 插件 + 单测）。
2. 迁移 `chat_panel.rs`：消息列表改用 `ScrollArea::vertical()` + `StickToBottom`，删除本地 `maintain_stick_bottom`/`auto_scroll_to_bottom`/`MessageListStickBottom`。
3. 迁移 `editor/tabs.rs`：buffer 实体改用 `ScrollArea::vertical()`（虚拟化占位仍由 `virtual_render` 管，ScrollArea 不阻碍）。
4. `mouse_wheel_scroll.rs` 不变（已通用，是 ScrollArea 的依赖）。
5. 验证：对话预览贴底跟随 + 文件编辑器滚轮 + 新增一个简单滚动区零调试。

## 6. 不做的事

- **不封装虚拟化**：虚拟化是编辑器/列表的渲染策略，与滚动容器正交，留在 `virtual_render` / `virtual_list`。
- **不做完整 Autoscroll 策略族**：MVP 仅 `Bottom`（贴底）。Fit/Center/TopRelative 留扩展点（`StickToBottom` 可演进为 `AutoscrollTarget` 枚举）。
- **不做自定义滚动条**：bevy 0.19 已内置 `scrollbar_width`，必要时直接用官方。
- **不引入句柄对象**：bevy 保留模式下组件+系统即句柄，无需 zed 的 `Rc<RefCell>`。
