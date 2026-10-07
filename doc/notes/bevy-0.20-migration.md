# Bevy 0.20 适配与优化策略（0.19 → 0.20.0-rc.2）

> 状态：适配已落地（本文 §3），优化路线见 §4。
> 参考：bevy 0.20.0-rc.2 源码 `_release-content/migration-guides/`（74 篇）。

---

## 1. 0.20 与 xgent 相关的核心变化

### 1.1 UI 交互模型重构（影响最大）

| 0.19 | 0.20 | xgent 影响 |
|:---|:---|:---|
| `bevy::ui::Interaction`（三态，`ui_focus_system` 全局维护） | **弃用**。官方推荐 `picking::hover::Hovered`（opt-in 组件）+ `ui::Pressed`（控件自维护） | xgent_ui 约 145 处引用 |
| `bevy::ui::Button`（unit struct） | **变为弃用 type alias**，不能再作值使用；正主是 `bevy::ui_widgets::Button` | 48 个编译错误 |
| `FocusPolicy`（bevy_ui 焦点系统配套） | UI picking 后端**完全忽略**；改用 `bevy_picking::Pickable { should_block_lower, is_hoverable }` | 2 处显式 Block，删除即可 |
| `ui_focus_system` 按 FocusPolicy 走树的悬停语义 | `build_hover_map` 按 `Pickable` 自顶向下阻断，默认行为等价（最上层阻断实体获得悬停） | 语义基本一致，无行为回归 |

**0.20 交互组件的正确认知**：
- `picking::hover::Hovered(pub bool)`：**opt-in**。插入后由 picking 系统按 `HoverMap` 同步（CSS `:hover` 语义，含子节点），`#[component(immutable)]`，变更检测友好。
- `ui::Pressed`：**控件自维护**（feathers/ui_widgets 的控件经 observer 插拔）；`ButtonPlugin`（已含于 DefaultPlugins 的 `UiWidgetsPlugins`）会为 `ui_widgets::Button` 实体自动维护。
- `picking::hover::PickingInteraction`（三态枚举 Pressed/Hovered/None）：**自动插入并维护**的聚合组件，`set_if_neq` 保留变更检测语义——是旧 `Interaction` 轮询模式的 1:1 对应物。
- 指针事件扁平化：`Pointer<Press>` → `PointerPress`（`Pointer` 不再泛型，字段直接在事件上）。

### 1.2 BSN（Bevy Scene Notation）

- 0.19 引入、0.20 语法改良（PR #25318）：场景引用一律 `@` 前缀；`template_value` 全部可去；enum 必须写全字段（`VariantDefaults`/`FromTemplate` 移除）；子实体列表用 `--` 分隔；推荐 `bsn_list! {}`（rustfmt 不吞）。
- 宏分布在 `bevy_scene`/`bevy_ecs`/`bevy_asset` 等 crate，示例见 `examples/scene/bsn.rs`。
- **xgent 未使用 BSN**，本轮无迁移成本；是否采纳属优化决策（§4.3）。

### 1.3 bevy_feathers（0.20 重点强化）

- 结构重组：`widgets/` → `controls/`（button/checkbox/dialog/listview/menu/select/slider/text_input/toggle_switch/radio/number_input/color_* 等）+ `containers/`（group/pane/subpane/flex_spacer）。
- **Contextual theming**（PR #24969）：`ThemeProps` 体系改为**语义 token**（`tokens.rs` 中 `ThemeToken` 常量，如 `feathers.button.bg`）。自定义主题 = 把语义 token 映射到自己的调色板——这让 xgent 自有 Theme 接入 feathers 成为可行路径。
- `cursor` 模块（EntityCursor/DefaultCursor/OverrideCursor/CursorIconPlugin）移至 `bevy_picking::cursor`。
- **xgent 未使用 feathers**（UI 全部手写：Theme + kit + HoverTint），本轮无迁移成本。

### 1.4 文本/输入

- `EditableText` 职责拆分：0.19 中它既是状态载体又是完整输入控件；0.20 起是纯状态载体（内部改为持有 `parley::PlainEditor`），**完整输入控件需同时挂 `TextInput`**（`bevy_ui_widgets`，`#[require(EditableText)]`，`TextInputPlugin` 已含于 DefaultPlugins）。`EditableText::new/allow_newlines/queue_edit` 等构造与编辑 API 兼容。
- `EditableText::viewport`（`TextViewport`）替代 `TextScroll`（xgent 未用 TextScroll）。
- `TextFont::default()` 字号变为 `FontSize::Rem(1.)`（随 `RemSize` 缩放）；xgent 全部显式设 px 字号，不受影响。
- `Val` 新增 `Em`/`Rem` 变体；`Val::resolve` 等签名新增 em/rem 参数（xgent 未直接调用 resolve）；`Node` 新增 `EmSize` 字段（有默认值，无需设置）。
- 字体：`Font::from_bytes` 去掉 family 参数；`FontSource` 泛型家族改 `GenericFontFamily` 枚举 + 构造函数；`resolve_font_source` 移除（xgent 未用）。

### 1.5 布局/渲染杂项

- `BorderRadius` 字段变为 `CornerRadius2d`（支持椭圆角）；构造函数参数改为 `impl Into<CornerRadius>`——`BorderRadius::all(px(..))`、`BorderRadius::MAX` 用法**自动兼容**。
- `CalculatedClip` 变枚举（Rects/FullyClipped）（xgent 未直接消费）。
- macOS 激活策略：启动/建窗时按 `Window::focused` 决定是否抢激活——**多开场景**可用 `focused: false` 避免多窗口互抢前台。
- ECS：exclusive 系统统一（`&mut World` 可任意位置）；`On<E, B>` 观察者泛型并入事件类型；`NextState::set_if_neq` → `set_if_different`（有兼容包装）；`TypeIdMap` 弃用别名化；`iter_many` 系列迭代 `Result`。**xgent 全部未触碰**。
- xgent 用到的其余 API（`ScrollPosition`/`UiStack`/`RelativeCursorPosition`/`HoverMap`/`ChildSpawnerCommands`/`EntityEvent` 派生/`FocusedInput`/`AutoFocus`/window 级 `CursorIcon`）在 0.20 均存续；`HoverMap` 形状变化（外层键改 PointerId）对 `mouse_wheel_scroll.rs` 的遍历写法透明。

---

## 2. 影响面盘点（cargo check 实测）

- **仅 `xgent_ui` 失败**：48 个错误（全部为 `Button` 弃用别名作值）+ 145 条弃用警告（全部为 `Interaction`）。
- 其余 9 个 bevy 相关 crate（xui/xgent_app/xgent_agent/xgent_settings/xgent_context/xgent_tools/xgent_plugin_host 等）**零错误零警告**。
- 结论：0.19→0.20 的破坏面 100% 集中在「UI 交互模型」，且 xui 的隔离层设计（不触碰 feathers/widgets）在本轮得到验证。

## 3. 本轮适配方案（已实施，经三轮自审修订）

### 3.1 决策：`Interaction` → 官方 `Hovered` + `Pressed` 双组件

四个候选（第 1、2 项在自审中被否决）：
1. **保留弃用 `Interaction`**：能编译但死路一条，且满屏警告。否决。
2. **`picking::hover::PickingInteraction`（初版方案，自审后否决）**：API 形状与旧 `Interaction` 同构（三态、自动维护、`Changed<>` 轮询），迁移即改类型名——但**语义不等价**：它只写 hover map 的**最顶层命中实体**，无后代回溯。xgent 交互根（按钮/列表行/tab 项）几乎都带文字/图标子节点，且子节点渲染在根之上、被指针先命中——悬停子节点时根的 `PickingInteraction` 不更新，hover tint / 点击 / tooltip 全部失效。旧 `ui_focus_system` 之所以没这个问题，是靠 `Node` required 的 `FocusPolicy` 默认 **Pass** 穿透非交互子节点。教训：**API 形状相同 ≠ 语义相同**，迁移前必须核对上游数据流的维护算法。
3. **`Hovered` + `Pressed`（最终方案，官方推荐）**：
   - `picking::hover::Hovered`：opt-in 组件（spawn 时插 `Hovered::default()`），由 picking 的 `update_is_hovered` 按 **CSS `:hover` 后代语义**维护——算法是「命中实体 + 全部祖先」集合，悬停按钮内文本子节点时根节点为 true，与旧模型行为一致。
   - `ui::Pressed`：由 `ButtonPlugin`（在 DefaultPlugins）的全局 observer 监听 `PointerPress/Release`（`#[entity_event(auto_propagate)]` 冒泡事件）自动插拔；handler 内部 `propagate(false)`——嵌套按钮（tab 项里的 ×）在最内层 Button 消费事件，父按钮不会误触发，等价旧 FocusPolicy::Block 语义。
   - 查询侧：点击类 `Query<..., Added<Pressed>>`（按下帧即触发，等价旧 `Changed<Interaction> + == Pressed` 边沿语义）；悬停类 `&Hovered`（`.get()`）+ `Has<Pressed>` 轮询（`hover_tint_system` 等对齐官方 widgets 示例的每帧刷新 + 写前比较模式，避免无谓变更检测）。
4. **给交互根的子节点批量加 `Pickable{should_block_lower:false}`**：可 restore 旧穿透语义，但 spawn 点分散、未来新增交互点易漏标，且与引擎默认方向相反。否决。

### 3.2 变更清单

| 变更 | 位置 | 说明 |
|:---|:---|:---|
| `Button` → `bevy::ui_widgets::Button` | xgent_ui 20 个文件 | 每文件加一行显式 `use`（压过 prelude 的弃用导出）；spawn 点与 `With<Button>` 查询同步切换。附带 a11y `Role::Button` |
| `Interaction` → `Hovered` + `Pressed` | xgent_ui 20 文件，48 个 spawn 点 + 约 30 个查询系统 | spawn 点补挂 `Hovered::default()`；点击查询 `Added<Pressed>`；悬停查询 `&Hovered`/`Has<Pressed>`；见 §3.1 |
| 面板压遮罩防护 | `kit.rs::block_press_bubbling` + session_history / file_panel 两 drawer 面板 | **功能性 review 发现**：0.20 指针事件从非 Button 子实体冒泡到 Button 祖先，点面板会误触遮罩关闭；面板挂 `observe()` 实体作用域 observer 消费 `PointerPress`（等价旧 FocusPolicy::Block）。**注意须用 `ui_widgets::observe` 助手**——裸 `Observer` 组件是全局观察者，会消费全世界的同类事件 |
| resize 拖拽启动加 hovered 门控 | `resize.rs::handle_resize_drag` | **功能性 review 发现**：手柄极窄，按下后移出再释放时 `Pressed` 不被移除（release 目标非手柄、小位移无 drag_end），残留 `Pressed` 会让下一次任意位置按下从错误边缘拖拽；启动条件加 `hovered.get()` 防御 |
| `handle_confirm_keyboard` 修 B0002 | confirm_dialog.rs | **冒烟测试发现（既存缺陷，0.20 起在参数初始化时即 panic 拒启）**：同系统 `Res<InputFocus>` + `ResMut<InputFocus>` 重复访问；合并为单一 `ResMut` |
| 交互契约测试 | `tests/interaction_model.rs`（新增，6 测试） | headless 全链路（真实 First/PreUpdate/Update 调度 + 喂 `WindowEvent`）验证后代悬停、按下冒泡、嵌套消费、面板压遮罩 4 大契约 + 同帧 press+release 边界观测。**0.20 交互模型的回归金丝雀** |
| `EditableText` spawn 点补挂 `TextInput` | 各文本输入创建处 | 0.20 起输入行为挂在 `TextInput`（`TextInputPlugin`）；`EditableText::new(...)` 初始文本写法保持（require 只填缺省） |
| 模拟点击改用新模型 | `tests/file_preview.rs` | spawn `bevy::ui::Pressed`（`Added<Pressed>` 过滤在插入帧触发）+ `Hovered::default()` |
| `Cargo.toml`/`AGENTS.md` 版本注释 | workspace 根 | 0.19.0 → 0.20.0-rc.2 |

验证方式（三轮功能性 review）：
1. **headless 交互契约测试**（`tests/interaction_model.rs`，6/6 通过）——四个承重语义全部端到端实证；
2. **逐系统行为审计**（30+ 查询系统 × 边沿语义/实体归属/自愈路径）——Esc/Enter 键盘守卫读全局 `KeyboardInput` 消息，不受 0.20 `FocusedInput` Esc 语义变化影响；确认 `Added<Pressed>` 与旧"按下即触发"边沿等价；
3. **运行时冒烟**（`XGENT_SHOT` 截图）——app 在 0.20.0-rc.2 上启动、建窗、渲染、UI 完整（顶栏/图标轨/欢迎页/分屏/状态栏全部正常），并借此抓出 B0002 既存缺陷。

已知边界（记录于契约测试 `same_frame_press_release_behavior`）：同帧 press+release（长帧期间的快速点击）`Added<Pressed>` 观测不到（插拔同帧完成）——官方 widgets 以 `PointerClick`/`Activate` 为准不受影响；若实际使用中可感，把点击系统迁 `Activate` observer（§4.2）。
另：git 插件加载告警（`does not have export summarize`）为 wasm 产物与清单不同步的既存问题（wasmtime 29 未变），重跑 `./build_plugins.sh` 即可，与本轮迁移无关。

## 4. 优化策略（后续迭代）

### 4.1 短期（可随小版本做）

- **全局 UI 缩放（rem 化）**：0.20 的 `RemSize` + `Val::Rem` + 默认字号 rem 化，为「界面字号缩放/无障碍」铺路。xgent 的 `type_scale` 目前是 px 常量表，可加一层「px → rem」换算或直接迁 rem，一行 `RemSize` 资源即可全局缩放。建议在设置面板加字号档位时一并做。
- **多开焦点策略**：利用 macOS 激活新语义，非前台窗口建窗时 `focused: false`，减少轻量多开时窗口互抢前台（契合 NF 的轻量多开支柱）。
- **`EditableText` viewport**：终端/聊天输入的自动滚动可改用 `EditableText::viewport.offset`（替代手写 TextScroll 补丁，如有）。

### 4.2 中期：交互组件的正统化（已随本轮完成）

`Hovered` + `Pressed` 双组件已在本轮落地（§3.1），与 feathers 控件（全部基于 `Hovered`/`Pressed`/`InteractionDisabled`）天然同构，§4.4 的 feathers 接入不再有交互语义迁移成本。后续可做的收尾：
- 把纯轮询的点击系统逐步改为 `On<Activate>` observer（`ButtonPlugin` 已发 `Activate` 事件；`ActivateOnPress` 标记可改"按下即触发"），减少每帧轮询；
- 需要"按住拖拽"的场景（resize 手柄）保持 `Has<Pressed>` 轮询即可，不必改 observer。

### 4.3 中期：BSN 采纳评估

- **收益**：声明式 UI 结构、模板组合（`@scene_fn()`、`#Name` 实体命名）、未来热重载/格式化工具链；对 xgent 的「数据驱动」支柱契合。
- **成本/风险**：API 仍在 rc 期动语法（0.20 刚改良过一轮）；rustfmt 尚不支持（官方计划中）；现有 kit.rs 生成器 + 各 panel 是成熟命令式代码，整体重写收益低。
- **建议路径**：**新界面试点**（如宠物窗、设置子页、diff 预览等新面板用 BSN 写），存量面板不动；BSN 桥接代码放 `xgent_ui`（业务层），`xui` 保持纯 bevy + i18n 不引 BSN。等 0.20 正式版 + 格式化工具落地后再评估存量迁移。

### 4.4 中期：feathers 控件接入

- **前提已成熟**：0.20 的 contextual theming（语义 token）+ `ThemeProps` 自定义主题，可把 xgent `theme.rs` 的调色板映射进 feathers 语义 token（先 1:1 映射再合并同色 token）。
- **候选替换**（官方已覆盖 → 按 AGENTS.md §5.5 应优先官方）：`confirm_dialog` → feathers dialog；菜单/下拉 → menu/select；复选/开关 → checkbox/toggle_switch；`number_input`（终端字号等数字输入）。
- **不替换**：虚拟列表、命令面板、文本编辑器（xui 自建核心竞争力 + F-11 深度定制），i18n 桥接保持 xui_i18n trait 注入。
- **接入位置**：feathers 依赖放 `xgent_ui`；若 `xui` 需要包一层，仅做「token 映射 + 样式适配」，不改变「xui 不依赖 feathers」的既有隔离约定（或届时重新评估该约定）。

### 4.5 风险与跟进

- 本地依赖是 **0.20.0-rc.2**：正式版发布前仍可能有 API 微调（尤其 BSN/feathers），升级正式版时重跑一轮 `cargo check` 即可，预计破坏面远小于本轮。
- 观察上游：`Interaction`/`Button` 弃用别名计划在 0.20 发布后移除；`FilteredResources` 弃用（xgent 未用）；`UiPickingSettings` 可调 picking 行为（如 `pickable_events`）。
- bevy_dev_tools 依赖（xgent_ui dev）中 `RenderDebugOverlay` 有变更，本轮已兼容，后续 dev-tools 变更关注 release notes。

---

## 附：本轮实测数据（含三轮自审）

- 迁移前：`cargo check --workspace` → xgent_ui 48E / 153W，其余全绿。
- 迁移后：`cargo check`/`test`/`clippy --workspace --all-targets` 全绿。
- 自审修订：初版 `PickingInteraction` 方案经三轮自审发现「最顶层命中、无后代回溯」回归（§3.1 候选 2），改定为官方 `Hovered`+`Pressed`；文档层级（dev-tutorial §5.12.4）与若干过时注释一并修正。
- 变更量：xgent_ui 20 源文件 + 1 测试文件 + workspace 配置 + 3 处文档。
