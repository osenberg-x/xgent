# ui-m5-panel-registry — 槽位注册表与派生可见性

## Purpose

把 XGent 的界面区域从「closed enum + 各模块自己写显隐」改成「**描述符注册 + 状态派生**」：

- 任何界面元素经**描述符**（`PanelDescriptor`）进入宿主，宿主只认描述符，不再认枚举（闭合枚举是 P-2 的反面）。
- 可见性、rail 选中态、页签条、分割尺寸全部**从状态派生**（P-1：状态是权威，呈现是派生）。
- 子系统之间只经 ECS Events / Messages 通信（P-5，AGENTS.md §5.2 硬约束）。

这是 UI v8 交互架构层重建的**第 5 步**（布局架构 §12 迁移策略），闭合缺陷 **D1–D6** 与 **I3**（台账 `docs/plans/ui-v8-tasks.md` §0.2、§M5）。

交付后的直接效果：**新增一个 `SideDock` 面板 = 一个描述符 + 一个视图模块 + 一条注册消息**，既有布局文件（`layout.rs` / `editor/mod.rs`）零改动——这是 M8「Git 探针」北极星指标的前置条件。

## Requirements

### 槽位与描述符模型

- `xgent_ui` 提供面板注册模型：
  - `Slot { Rail, Center, SideDock, BottomDock, SettingsSection, StatusSegment, Palette, Overlay }`。`Center` 本期只建模、不接消费者（3D 空间视图留给 F-16）；`Center` 与 `SideDock` 的差别是**互斥**而非并列。
  - `PanelDescriptor`：`id: &'static str`、`slot: Slot`、`title_key: &'static str`（**必填**的 i18n fluent key，禁止裸字符串）、`icon`、`order: i16`（同槽内排序，允许稀疏）、`closeability`（`Closable | Sticky`）、`min_size`、`default_size`。
  - `PanelRuntime`：单个面板的运行时状态（视图实体、是否可见、是否聚焦）。
  - `PanelRegistry`（`Resource`）：持有「描述符 + 运行时状态」，支持按 id 查询、按槽位查询（按 `order` 升序）。
  - `SlotLayout`（`Resource`）：槽位布局状态（各槽位当前可见的面板、几何尺寸），是派生可见性与选中态的输入。
- 面板视图实体挂 `PanelRuntimeMarker` 组件，派生系统据此定位要写 `Node.display` 的节点。

#### Scenario: 槽位集合与描述符必填字段

- **Given** `xgent_ui` 的面板注册模型
- **When** 检查 `Slot` 的变体集合与 `PanelDescriptor` 的字段
- **Then** `Slot` 覆盖 `Rail` / `Center` / `SideDock` / `BottomDock` / `SettingsSection` / `StatusSegment` / `Palette` / `Overlay`；`PanelDescriptor` 含必填的 `title_key`（i18n key）与 `slot`；`PanelRegistry` 可按 id 与按槽位查询，槽位内结果按 `order` 升序

验收：A1

### 注册与激活（ECS 消息）

- 注册与激活一律经 ECS `Message`（AGENTS.md §5.2；布局架构 P-5），**业务代码不直接调用注册表方法**：
  - `PanelRegister { desc: PanelDescriptor, view: Entity }`
  - `PanelActivate { id: &'static str }`
  - `PanelDeactivate { id: &'static str }`
- `PanelRegister` 的处理系统把描述符与视图实体写入 `PanelRegistry`，并把该实体标记为 `PanelRuntimeMarker`。
- `PanelActivate` / `PanelDeactivate` 改变对应面板的运行时可见状态；实际 `Node.display` 由派生系统统一写（见下）。

#### Scenario: 注册与激活只经消息

- **Given** 装配了面板模型的 App
- **When** 写入一条 `PanelRegister` 消息并跑若干帧
- **Then** `PanelRegistry` 中出现该描述符与视图实体；`PanelActivate` / `PanelDeactivate` 之后该面板的运行时可见状态随之改变；注册表对外不提供业务侧可直接调用的「注册 / 激活」方法（源码级断言：调用点均为 `MessageWriter` 写入）

验收：A1

### 迁移 shim（本期内部，跨任务存在）

- 在 M5-T1…M5-T5 期间，`SideViewContent`（`editor/mod.rs`）作为**临时 shim** 保留，提供「枚举变体 → 描述符 id」的**一次性映射**（纯函数），使各批改动都能独立编译。
- shim 的每个变体都对应注册表中的一个描述符 id；映射是纯函数，不持有状态。
- M5-T6 拆除 shim 与 `SideViewContent` 类型本身；此后「一致性」由注册表自身承担（每个既有面板在注册表中有唯一描述符）。

#### Scenario: shim 映射是纯函数且与注册表一致

- **Given** shim 存在期间的注册表
- **When** 遍历 shim 的每个枚举变体并求其映射的描述符 id，再在注册表中按该 id 查询
- **Then** 每个变体都命中注册表中的唯一描述符；映射函数对同一输入恒返回同一 id（纯函数，无状态依赖）

验收：A10

### 派生可见性（唯一写手）

- `derive_panel_visibility` 是**唯一**写「面板 / 区域」`Node.display` 的系统：输入 `PanelRegistry` + `SlotLayout`，按槽位规则计算每个面板的期望 `display`，**仅在期望值与当前值不同时**写。
- 原有各模块的动态面板可见性写手全部并入该系统，语义保持不变：
  - 右侧分屏容器与其右手柄（`layout.rs`）随分屏折叠态显隐
  - 文件抽屉的遮罩与面板（`file_panel.rs`）随抽屉开关态显隐
  - rail 的「展开面板」钮（`activity_bar.rs`）随分屏折叠态显隐
  - 编辑器视图（`editor/mod.rs`）与终端视图（`terminal/mod.rs`）随分屏内容切换显隐
- 三个视图容器的「初始隐藏」由**构造期字面量**承担（`Node { display: Display::None, .. }`），不再是运行时写手。
- 内容态可见性**不**属于面板可见性，本期**不归一**（用户 2026-10-21 裁决）：欢迎空态（会话是否为空）、最近会话标题（列表是否为空）、回到底部按钮（滚动位置）、编辑器文档页签（当前文档）、陪伴激活环（开关状态）——它们描述**内容状态**而非**面板 / 区域**，塞进槽位注册表是类别错误。它们以**显式白名单**形式留在原处，白名单由源码级测试固化，且每条注明属内容态而非面板态。

#### Scenario: 派生可见性是唯一的面板写手

- **Given** 装配了完整 UI 的 App（抽屉、分屏、终端、编辑器均已就位）
- **When** 切换抽屉开关、分屏折叠、面板激活/失活，并检索 `crates/xgent_ui/src/` 中所有 `.display =` 写点
- **Then** 写「面板 / 区域」`Node.display` 的写点只存在于派生系统与其构造期字面量中；其余写点逐条命中内容态白名单（源码级断言：命中集合 = 派生系统 ∪ 构造期字面量 ∪ 白名单，白名单外无其它写点）；开关抽屉 / 折叠分屏时对应节点的 `display` 仍正确切换

验收：A5

### rail 选中态派生

- `ActiveActivity` 资源被删除；rail 不再独立存储选中态。
- rail 项由注册表中 `Slot::Rail` 的面板描述符生成（图标与 tooltip 取自描述符）。
- 选中态派生为「注册表中该槽位的面板 ∩ 槽位当前可见面板」：即 `registry.iter_rail().find(|p| slots.is_visible(p.id))`；没有任何 rail 面板可见时无选中项。
- 连点同一 rail 钮两次（开→关）后，选中态自然回落，不残留。

#### Scenario: rail 选中态从注册表派生

- **Given** rail 面板已注册、文件抽屉可开关
- **When** 点 Files 打开抽屉（选中 Files），再点 Files 关闭抽屉
- **Then** rail 上不再有选中项（选中态由「可见面板」派生，而非独立存储的 `ActiveActivity`）；`ActiveActivity` 类型在仓库中不存在

验收：A3

### 历史面板进槽位

- `session_history` 由「自建浮层 + 遮罩 + 手写 `SessionHistoryState.open`」改为**可注册面板**，槽位为 `Slot::Overlay`。
- 文件抽屉与历史抽屉同属 `Overlay` 单例槽位 → 同一时刻至多一个可见，二者不再争层级。
- 抽屉的打开 / 关闭仍经既有 `DrawerToggle` 消息入口（快捷键、rail 钮、遮罩点击、关闭按钮），不新增直连调用。

#### Scenario: 历史与文件抽屉同属单例槽位

- **Given** 文件抽屉与历史面板均已注册到 `Slot::Overlay`
- **When** 打开文件抽屉，再打开历史面板
- **Then** 同一时刻至多一个可见（打开其一自动关闭另一）；历史面板不再以独立浮层实体形式存在

验收：A4

### 单页签条所有者

- 侧区顶部的页签条由**槽位布局**拥有并渲染；页签项来自注册表中 `Slot::SideDock`（及同列其它槽位）面板的描述符，标题取 `title_key`。
- 编辑器内部的 tab 条**保留**，但语义降级为「文档列表」（打开的代码文件列表）；它与槽位页签**不合并**、不共用同一数据源。
- `EditorBackButtonMarker` 全仓**只定义一次**（原 `editor/mod.rs` 的两套标记合一）；侧区顶部只有一个返回入口。

#### Scenario: 侧区只有一条页签条且只有一个返回入口

- **Given** 装配了完整 UI 的 App
- **When** 检索 `crates/xgent_ui/src/editor/` 中的页签条与 `EditorBackButtonMarker` 定义
- **Then** 槽位页签条只有一处（由槽位布局拥有），编辑器内部 tab 条语义为文档列表；`EditorBackButtonMarker` 的定义点数量为 1

验收：A6

### 分割模型

- 建模 `SplitTree / Split / LeafSize`：
  - `Split::Leaf { id, size: LeafSize }`
  - `Split::Splitter { axis, children: Vec<Split>, handles: Vec<HandleState> }`
  - `LeafSize { Fixed(f32), Min(f32, Preferred(f32)), Grow(f32) }`
- `resize.rs` 从「单字段 `PanelWidths { side_view }` + 只认 `ResizeEdge::Right`」改为**遍历分割树生成分割条**：每个分割条持 `HandleState { target }`，拖动写回相邻 leaf 的尺寸比例。
- 注册一个新的纵向分割**不需要**修改 `resize.rs`（分割树的遍历与手柄生成是通用逻辑）。
- 抽屉宽度从定值 `DRAWER_W = 320.0` 改为 `Min(240, Preferred(320))`，**上限 480**；抽屉可拖宽，且宽度被钳制在 `[240, 480]`。

#### Scenario: 分割树产出分割条且抽屉可拖宽

- **Given** 分割树中注册了一个抽屉 leaf（`Min(240, Preferred(320))`）与一个纵向分割
- **When** 遍历分割树生成手柄，并把抽屉宽度从 240 拖到 600
- **Then** 生成的分割条数量与分割树的分割节点数一致；抽屉宽度被钳制为 480（上限），最小 240；注册该纵向分割的过程未修改 `resize.rs` 的通用遍历逻辑（源码级断言：分割项的注册处不新增 `resize.rs` 的特判分支）

验收：A7

### 移除 closed 枚举

- M5-T6 拆除 shim 与 `SideViewContent` 类型；`crates/` 内 `SideViewContent` **零引用**（含 `enum SideViewContent` 的定义）。
- editor / preview / diff / terminal 四个既有面板全部为**原生注册项**（各自的描述符 + 注册消息）。

#### Scenario: 仓库内 SideViewContent 零引用

- **Given** M5-T6 完成后的仓库
- **When** 检索 `crates/` 下的 `SideViewContent`
- **Then** 命中数为 0；editor / preview / diff / terminal 四个面板在注册表中各有唯一描述符

验收：A8

### i18n 页签标题

- 页签与面板标题取自描述符的 `title_key`（fluent 语料），不硬编码。
- 删除 `editor/mod.rs` 中「取到 `Localizer` 却丢弃」（`let _ = loc;`）的写法。
- `xgent_settings/locales/{zh-CN,en-US}/main.ftl` 两种语料均补齐相应 key；**英文语料不含 CJK 码位**。

#### Scenario: 英文语料下页签标题为英文

- **Given** 英文（`en-US`）语料
- **When** 渲染侧区页签并取其标题
- **Then** 标题来自描述符 `title_key` 对应的本地化串，且不含任何 CJK 码位；`editor/mod.rs` 中不再存在 `let _ = loc;` 一类的丢弃写法

验收：A9

### 扩展性（北极星前置）

- 新增一个 `SideDock` 面板的**全部**改动是：新增一个 `PanelDescriptor`（含 `title_key`）、新增一个视图模块、发一条 `PanelRegister` 消息。
- `crates/xgent_ui/src/layout.rs` 与 `crates/xgent_ui/src/editor/mod.rs` 中不含 `SideViewContent`，也不含按面板 id 分派的 `match` / `if` 分支（面板集合的变化不落在布局文件里）。

#### Scenario: 新增面板不需要改既有布局文件

- **Given** 装配了面板注册表的 App
- **When** 运行时注册一个新的 `SideDock` 面板
- **Then** 注册表按槽位查询能返回该面板（且 `order` 正确）；`layout.rs` 与 `editor/mod.rs` 中不出现 `SideViewContent`、也不出现按面板 id 分派的分支（源码级断言）

验收：A2

### 不回归与交付纪律

- `cargo check --workspace` 通过；`cargo fmt -p xgent_ui` 后无 diff；`cargo clippy -p xgent_ui --all-targets` 在改动文件零新增警告。
- `cargo test --workspace` 的失败集合**不超出**既有 15 项基线（`xgent_tools` 6 / `xgent_plugin_host` 6 / `xgent_daemon` 1 / `xgent_settings_core` 1 / `xgent_agent` 1）。
- 提交粒度：M5-T1…M5-T6 各一条 `feat(ui): …` 提交 + 一条文档同步提交 + 一条 `chore(native): archive v8-m5-panel-registry` 归档提交。

#### Scenario: 既有基线不回归

- **Given** 实现完成的仓库
- **When** 跑 `cargo check --workspace`、`cargo clippy -p xgent_ui --all-targets` 与 `cargo test --workspace`
- **Then** 编译与 clippy 通过；测试失败集合不超出既有 15 项基线

验收：A11

### 文档同步

- `docs/plans/ui-v8-tasks.md`：§M5 六个任务逐条勾选 + §10 追加本轮勘误（含交接 §4.2 的过时判据订正）。
- `docs/plans/ui-v8-handoff.md`：交付表、基线数字与既有失败、下一步。
- `docs/dev-tutorial.md`：新增 M5 小节（注册表模型、派生可见性、分割模型、坑）。

#### Scenario: 文档同步与提交粒度

- **Given** 实现完成的仓库
- **When** 检索 `docs/plans/ui-v8-tasks.md`、`docs/plans/ui-v8-handoff.md`、`docs/dev-tutorial.md`，并查看提交历史
- **Then** 三处都记录了 M5 的落地内容、模型要点与取舍；提交历史含 M5-T1…M5-T6 各自的 `feat(ui): …` 提交、一条文档提交与一条 `chore(native): archive v8-m5-panel-registry`

验收：A12
