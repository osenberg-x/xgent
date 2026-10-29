# UI v8（交互架构层重建）落地任务清单

> 依据：需求简报 [ui-v8-requirements-brief.md](../design/ui-v8-requirements-brief.md)、技术设计 [ui-v8-layout-architecture.md](../design/ui-v8-layout-architecture.md)、设计系统 [ui-v8-design-system.md](../design/ui-v8-design-system.md)、证据基线 [ui-v8-audit.md](../notes/ui-v8-audit.md)。
> 视觉蓝本：可交互原型 `docs/design/ui-prototype-v8.html`（本轮产出；若尚未落到 docs/，见文末「交付缺口」）。
> 决策记录：ADR-0015（正文见 [ADR-0015](../decisions/0015-ui-v8-交互架构层重建-保留-linear-视觉基准.md)，定稿后移入 `docs/decisions/`）。
> 状态：**M1 / M2a / M2b / M3 / M4 / M5 / M7 已交付；M6 模型层 + M6.5 接线已交付并经一轮只读复审修复**（M6 模型 `9ac4daf`；M6.5 实现 `b962f97`，启动修复 `5dde747`，复审修复 `e485d5e`（见 §10 勘误 51）；真实窗口截图仍待用户走查，须含中文输入法与 HiDPI 缩放）。**下一期 M8（Git 探针）**（进度与下一步见 [ui-v8-handoff.md](ui-v8-handoff.md)；M1/M2 段落勾选仍有滞后，以归档 change 与代码实测为准）。任务编号 `M{期}-T{序}`，完成打勾。本文是唯一进度台账；实施中发现与文档不符的事实时，改代码的同时在本文 §10 追加勘误（文档跟随代码）。

---

## 0. 执行原则

1. **严格按期推进**：M1 → M2 →（M3 ∥ M4 可并行）→ M5 → M6 → M7；期内按编号顺序（有依赖标注的除外）。
2. **每任务有验收证据**：非「编译通过」，而是可观察行为或对照物（三平台截图 / 几何断言 / grep 计数）。
3. **提交粒度**：每任务一提交（中文消息，引用任务号如 `M2-T3`）。M4-T2（终端双宽）与 M5-T1（注册表）各自留独立提交便于回滚。
4. **每期收尾**：`cargo fmt -p <改动 crate>` → `cargo clippy --workspace` → `cargo test --workspace` 全绿才进下一期。**勿用 `cargo fmt --all`**（会改写 `../bevy` 上游检出，AGENTS.md §3）。
5. **每条 P0 缺陷配一条"先红后绿"回归测试**（M7 收口清点，共 ≥ 9 条）。
6. **截图对照**：`ui-snapshot` feature F12 出图（`docs/plans/ui-v7-tasks.md:12` 已建工具）与原型逐状态人工比对。
7. **API 先核实再引用**：任何 bevy 0.20 接口用法先在 `../bevy` 源码确认，写入本任务备注。本轮已核实的清单见 §9。

## 0.1 依赖总览

```
M1（层根/拾取/模态栈）──┬──→ M3（滚动契约/region_fill）──┐
                        ├──→ M4（字体/双宽单元格）        ├──→ M6（会话内容：有序树+Markdown+输入区）──→ M7（验证层/文档勘误）
M2（令牌收敛/主题热驱动）┴──→ M5（槽位注册表/派生可见性）─┘
                                                        └──→ M8（Git 探针，证明可扩展性达标）
硬门槛 A：M4-T1 三平台 mono 资产解析不到真实等宽 → 停止铺开（与 v7 CJK spike 同级，`ui-v7-tasks.md:23`）。
硬门槛 B：M1-T2 `require_markers=true` 后若出现大面积控件点不动 → 停下补 Pickable，不得跳过。
风险标记：⚠ M4-T2（终端双宽，改动面最大且影响已验收行为）、⚠ M5-T1（注册表取代 closed 枚举，牵动 editor/terminal）。
```

M1 与 M2 相互独立，可并行分头推进。

## 0.2 缺陷 → 任务映射（闭合判定用）

| 审计 ID | 级别 | 承接任务 | 闭合判据 |
|:---|:---|:---|:---|
| A1 | P0 | M4-T1/T3 | 三平台 `mono` 实测为等宽 + 日志可核对 |
| A2 | P1 | M2-T6 | 主窗有确定性默认尺寸（DPI 感知） |
| A3 | P0 | M4-T2 | 中文 `ls -l` 三平台列对齐（Windows 已自动化断言；macOS/Linux 待办，见 §10 勘误 31） |
| B1 | P0 | M1-T1/T2/T3 | 业务代码零裸 `GlobalZIndex(` |
| B2 | P1 | M1-T4 | 抽屉互斥经消息裁决，单一路径 |
| B3 | P1 | M1-T5 | tooltip 右缘不裁、不出窗 |
| B4 | P1 | M1-T3/T6 | 设置面板 Esc + 点外部可关，不触发 `chat.abort` |
| C1 | P0 | M3-T1/T2 | `grep Overflow::clip_y()` 于 xgent_ui = 0 |
| C2 | P1 | M3-T2 | 模型列表可达末项 |
| C3 | P0 | M3-T4/T5 | 宽度不足出 `…` 且消费者 ≥ 6 |
| C4 | P1 | M3-T3 | 输入框超上限可内滚、光标行可见 |
| C5 | P1 | M3-T3 | chips 窄窗换行不裁 |
| D1 | P0 | M5-T1 | `SideViewContent` 枚举删除 |
| D2 | P1 | M3-T3/M5-T3 | 无子节点越界（几何断言） |
| D3 | P1 | M5-T4 | 单页签条所有者 |
| D4 | P1 | M5-T2 | rail 选中态派生，连点两次一致 |
| D5 | P1 | M5-T2 | 历史进入槽位 |
| D6 | P1 | M5-T5 | 抽屉可调宽、支持纵向分割 |
| E1 | P0 | M6-T2/T3 | 六类元素混合样张渲染正确 |
| E2 | P1 | M6-T1 | 流式中顺序不倒退 |
| E3 | P1 | M6-T4 | `@` 出现补全并可方向键选择 |
| E4 | P1 | M6-T5 | 纯鼠标完成一次发送与一次中断 |
| E5 | P1 | M6-T6 | 上下文 chip 可单独移除并生效 |
| E6 | P2 | M3-T5 | 50 字手工截断被 `Ellipsis` 取代 |
| F1/F2 | P0 | M7-T1 | 启动期一致性断言全绿，死键归零 |
| F3 | P1 | M7-T2 | Tab 按视觉顺序遍历 |
| F4 | P1 | M7-T2 | 焦点环可见（消费 `accent_glow`） |
| G1 | P1 | M2-T1 | 改设置即时生效、无需重启 |
| G2 | P2 | M2-T2 | 零 `Theme::dark()` 现场构造 |
| G3/G4 | P2 | M2-T3/T4 | 零魔法像素、`px()` 单源 |
| G5 | P2 | M2-T5 | 非 theme 色仅剩 ANSI 表白名单 |
| G6 | P2 | M2-T7 | hover 全量化，废逐帧轮询 |
| H1/H2/H3 | P1/P2 | M7-T3 | 全模块 i18n 扫描 + EN 无 CJK |
| I1 | P1 | M7-T4 | 零 Entity 缓存进 Resource |
| I2 | P1 | M7-T5 | 几何断言层建成并覆盖 P0 |
| I3 | P2 | M5-T3 | 单一 `derive_panel_visibility` |
| I4 | P2 | M3-T6 | `VirtualList` 消费者 ≥ 2 |

---

## M1 分层与模态（修 B1/B2/B3/B4，解 I1 的拾取根因）

> 状态：**已交付**（change `v8-m1-layering`，归档 `f5fdb87` / 实现 `e7834fe`）。下列复选框是开工前的任务草稿、未逐条回勾，**不要据此判断未做**。

### M1-T1 六个静态层根
- [ ] `theme.rs` 新增 `z` 模块（`BASE/DOCK/DRAWER/DROPDOWN/MODAL_BASE/TOOLTIP/TOAST/MODAL_STRIDE/MODAL_MAX_DEPTH`），值见架构 §2.2。
- [ ] `layout.rs` 在 `UiRoot` 下并列 spawn 六个层根（全屏 Absolute、`BackgroundColor::NONE`、**不带 `Pickable`**）。
- [ ] `layer_root(band)` bundle 构造器进 `kit.rs`。
- [ ] `TopBar/MainArea/StatusBar` 迁入 `BASE` 层。
- **验收**：`grep -rn "GlobalZIndex(" crates/xgent_ui/src/ --include=*.rs` 命中仅限 `kit.rs`/`layout.rs` 的层根 spawn 处。

### M1-T2 拾取模型切换（⚠ 硬门槛 B）
- [ ] `xgent_app` 装配处 `app.set_resource(UiPickingSettings { require_markers: true, ..default() })`（依据：✅ `bevy_ui/src/picking_backend.rs:67` 默认 false；`:197` 非文本节点无 `Pickable` 被跳过）。
- [ ] 全量扫描可交互叶子补 `Pickable`：按钮、输入、列表项、可点卡片、tab、rail 项。
- [ ] 遮罩实体 `Pickable { should_block_lower: true, is_hoverable: true }`；穿透装饰层 `Pickable { should_block_lower: false, is_hoverable: false }`。
- **验收**：手动逐面板点击全绿；空层根不吞点击（M7-T5 的 `assert_no_blocking_empty_layer` 覆盖）。
- **回滚**：若大面积点不动且短期补不齐 → 暂时回退 `require_markers=false`，另开子账逐区迁移，**不得**带病进 M3。

### M1-T3 `ModalStack` 与键盘路由
- [ ] `ModalStack` Resource + `ModalPush`/`ModalPop` Message + `ModalTopChanged` Event + `Dismiss{EscOnly,EscAndScrim,NotDismissable}`。
- [ ] `derive_modal_z`：栈深写 `GlobalZIndex = MODAL_BASE + depth*MODAL_STRIDE`。
- [ ] `route_modal_input`：栈非空时阻断全局快捷键派发到底层（修 B4 的 Esc→`chat.abort`，`shortcuts.rs:143-157`）。
- [ ] 迁 `confirm_dialog.rs:91`（原裸 `GlobalZIndex(50)`）、`settings_panel.rs:683-712`（原无 z）入栈。
- **验收**：开着设置面板按 Esc 关面板、对话不中断；确认弹窗仍独占键盘。

### M1-T4 抽屉互斥经消息裁决（修 B2）
- [ ] `FileDrawerOpen`/`SessionHistoryState` 布尔 Resource 改为「已注册可覆盖层」集合，`activity_bar.rs:274-287` 的手工互斥删除。
- [ ] `context_scope.rs:202-209` 的「+ 添加」与 rail 钮、快捷键统一发 `DrawerToggle`。
- **验收**：三个入口打开文件抽屉后，历史抽屉自动互斥关闭；打开的抽屉不遮挡设置面板（配合 M1-T3）。

### M1-T5 tooltip 脱离裁剪子树（修 B3）
- [ ] `kit.rs:235` 的「tooltip 作为宿主右側子节点」改为挂 `TOOLTIP` 层根 + 定位器（按宿主 `ComputedNode` 计算屏幕位置）。
- [ ] 视口边界翻转：右缘不足时翻到左侧。
- **验收**：最右的齿轮钮 tooltip 完整可见；`MainArea`/`ChatPanel` 的 `clip`（`layout.rs:117,149`）不再裁到 tooltip。

### M1-T6 设置面板可 dismiss
- [ ] 挂 `Dismiss::EscAndScrim`（今天只有按钮可关，`settings_panel.rs:1308-1325`）。
- **验收**：Esc 与点击遮罩均关闭；关闭后焦点回到原实体。

---

## M2 令牌收敛与主题热驱动（修 G1–G6，铺 A2）

> 状态：**已交付**（change `v8-m2-tokens` + `v8-m2b-density-hover`，归档 `7f67dc1` / `9bb29c9`，实现 `141d05c` / `6d83d18`）。M2-T1 的密度部分与 M2-T7 已在 M2b 勾选，其余条目的勾选滞后于实现。

### M2-T1 主题成为可替换权威
- [x] `ThemeHandle { theme, generation }` + `Themed { role, gen }` 组件 + `repaint_on_theme_change` 系统（架构 §8.1）。（v8-m2a-tokens）
- [x] `preferences.theme`/`density` 接现有配置热应用通路（R3-13 已闭合）。（v8-m2b-density-hover：密度经 `ThemeHandle.density` + `density.rs` 的间距基线写回；主题沿用 m2a 通路）
- **验收**：设置里切密度档位，全界面即时重排、无需重启。（`xgent_ui/tests/density.rs` 断言 rail/对话区/设置卡片三处像素）

### M2-T2 消灭现场构造与影子定义
- [ ] `Theme::dark()` 可见性降 `pub(crate)`；修 `diff_view.rs:217`。
- [ ] 删 `confirm_dialog.rs:45`、`settings_panel.rs:1251` 两个 `fn px()`，统一用 `theme.rs:300`。
- **验收**：`grep -rn "Theme::dark()" crates/xgent_ui/src/` 仅命中 `theme.rs` 内定义与测试。

### M2-T3 尺寸令牌化（约 40 处）
- [ ] 新增 `size::{ICON_BTN,RAIL_ITEM,BRAND,SCOPE_H,MSG_MAX_W,TOAST_BOTTOM,OVERLAY_PALETTE_W,OVERLAY_SETTINGS_W,OVERLAY_CONFIRM_W,MODAL_MIN_W}`（对应值见设计系统 2.3）。
- [ ] 替换散落点：`chat_panel.rs:434,545`（820 ×3）、`top_bar.rs` 34²、`activity_bar.rs:88-96` 40²、`command_palette.rs:130-208` 560/440、`settings_panel.rs:683-712` 640、`confirm_dialog.rs:77-110` 560、`conflict.rs:148-171` 360、`chat_panel.rs:364-393` 150。
- [ ] 圆角字面量归 `radius`：`chat_panel.rs:734`、`tool_panel.rs:118,179`、`welcome.rs:112,247`、`editor/mod.rs:228`。
- **验收**：`grep -c "Val::Px([0-9]" crates/xgent_ui/src/*.rs` 区域尺寸类命中为 0（纯装饰 1px 边框除外，需注释）。

### M2-T4 最小宽度单源
- [ ] `resize.rs:25-27` 改为引用 `theme.rs:230-232`，消除双定义漂移（G4）。
- **验收**：`CONTEXT_W_MIN`/`CHAT_MIN` 各只有一处定义。

### M2-T5 色板纪律
- [ ] 修 `activity_bar.rs:207,395`（`0x1C1917` 星形）、`editor/tabs.rs:429`（`Color::WHITE`）入 theme。
- [ ] `terminal/output.rs:261-306` ANSI 表加显式白名单注释（合理保留）。
- **验收**：非 theme 色字面量仅剩 ANSI 表白名单项。

### M2-T6 窗口基线（修 A2）
- [ ] `main.rs:229-238` 补 `LogicalSize` 默认尺寸（1280×800 逻辑像素）+ 记住上次尺寸。
- **验收**：首帧尺寸确定，HiDPI 下不异常。

### M2-T7 hover 全量化（修 G6）
- [x] 以 `Hovered` + `Pressed`（0.20 的 `Interaction` 已废弃）数据驱动单系统替换 `kit.rs:125-157` 的每帧轮询；松手帧用 `RemovedComponents<Pressed>` 补。（v8-m2b-density-hover）
- **验收**：无状态变化的节点不进入查询；空闲帧 `HoverTintPollStats.visits == 0`（单测），帧内该开销可测量下降。

---

## M3 滚动契约与省略（修 C1–C5、E6、D2、I4）

> 状态：**已交付**（提交 `19fff0a` M3-T1/T2、`3acbb17` M3-T3、`663d1ad` M3-T4/T5、
> `f5c4e32` M3-T6）。下方勾选为回填，细节与勘误见 §10。

### M3-T1 `ScrollFrame` 唯一入口（xui）
- [x] `xui` 暴露 `ScrollFrame::vertical()/horizontal()/stick_bottom()`，`overflow` 由构造器决定、**不接受外部覆盖**（正确形态参照 `scroll_area.rs:104-105`）。
- [x] 滚动条与视口成套产出；`scroll_to_child`（`scroll_area.rs:12`）保持公开。
- [x] 删除 `ScrollArea`（与 `ScrollFrame` 职责重复，留着就是第二个入口）。
- **验收**：单元测试断言构造产物 `overflow.y == OverflowAxis::Scroll` ✅（`xui` 单测）。

### M3-T2 迁移 9 处假滚动
- [x] `file_panel.rs:186`、`session_history.rs:161,221`、`command_palette.rs:157,201`、`confirm_dialog.rs:221`、`editor/mod.rs:252`、`settings_panel.rs:796`、`tool_panel.rs:211` 全部改用 `ScrollFrame`。
- [x] `settings_panel.rs:791-800` 模型列表（`max_height:150` 且完全无 `ScrollPosition`）挂 `ScrollFrame::vertical()`。
- **验收**：`grep -rn "Overflow::clip_y()" crates/xgent_ui/src/` = 0 ✅（新增源码级契约测试守住）；七处各塞 200 条，滚轮与滚动条均可达末条 ✅（`tests/scroll_contract.rs`）。

### M3-T3 填满契约与自适应输入（修 D2/C4/C5）
- [x] `region_fill()`（`flex_grow:1 + min_width/height:ZERO`）替换所有「有兄弟节点的列里 `height:100%`」：`editor/mod.rs:184`、`terminal/mod.rs:262`、`diff_view.rs:54`（vs `:668` 的 38px 页签条）。
- [x] `chat_panel.rs:182-219` 输入框：自适应到 200 后内部滚动 + 光标行可见。
- [x] `chat_panel.rs:301-339` chips 行改 wrap 容器。
- **验收**：700px 高窗口内终端状态条完整可见 ✅（页面改 `region_fill`）；长输入不隐藏当前行 ✅（`update_input_scroll_to_caret`）。

### M3-T4 `xui::Ellipsis` / `Clamp`（修 C3，⚠ 引擎无此能力）
- [x] 自建测量-二分-追加 `…`；measure 走 bevy 自己的排版管线（`TextPipeline::create_text_measure`，比 `update_text_layout_info` 轻量且不碰图集）。
- [x] `EllipsisCache` 按 `(source hash, 可用宽量化, 字号, 省略号, 模式)` 缓存；命中/未命中计数可观测。
- [x] 多行 clamp **未做**：`TextBounds` 官方自陈硬裁不可靠，本轮只做单行省略（见 §10 勘误 12）。
- **验收**：宽度不足出 `…`、宽度够则原样、宽度变化跟随重算 ✅（`tests/ellipsis_contract.rs`，真实 Inter 字体量测）。

### M3-T5 接入 6 处截断点（修 E6）
- [x] `file_panel.rs`（路径，中段省略保留末两段）、`editor/tabs.rs`（tab 标题）、`status_bar.rs`（provider 文本）、`command_palette.rs` 行、`tool_panel.rs`（删 50 字手工截断）、确认弹窗标题 + 路径。
- [ ] 截断时完整值进 tooltip（依赖 M1-T5）：**未做**，M1-T5 只做了 tooltip 脱离裁剪子树，尚未建立「文本 → tooltip」的通用绑定（留 M7）。
- **验收**：`Ellipsis` 消费者 ≥ 6 ✅（6 处，源码级测试守住）；宽度不足出 `…` 而非溢出/换行 ✅。

### M3-T6 启用 `VirtualList`（修 I4 / R3-6）
- [x] 文件树接入 `xui::VirtualList`（扁平数据 + 只渲染可见行 + 占位块撑出总高度）。
- [x] 会话历史列表接入 `xui::VirtualList`（**替代**台账原写的「会话消息列表」，理由见 §10 勘误 13）。
- [x] 不等高行：`VirtualList` 支持 measured 行高表（估算 + 实测回写的累积偏移表），本轮两个消费者用固定行高。
- **验收**：500 条文件树只渲染可见行 ✅（`tests/virtual_list_contract.rs`）；消费者 ≥ 2 ✅。

---

## M4 跨平台字体与终端双宽（修 A1/A3，⚠ 硬门槛 A）

> 状态：**M4 全部交付**（M4-T1 `v8-m4-font-assembly`、M4-T2 `v8-m4-terminal-cells`、M4-T3/T4 `v8-m4-mono-typography`）。
> 硬门槛 A 的 macOS / Linux 启动日志确认**仍待办**（用户 2026-10-21 决定：先做 Windows 适配，其他平台后续）。
> 台账与勘误见本文 §10。

### M4-T1 字体装配层（三平台一阶）
- [x] 内嵌 **JetBrains Mono**（OFL 1.1）到 `crates/xgent_app/assets/fonts/`，与 `Inter-Variable.ttf` 同目录（今天只有 Inter，✅ 已核实）。
- [x] `FontSpec/FontCandidate` 候选栈数据表：`mono` 末条必须为内嵌资产；`ui_cjk`/`mono_cjk` 走系统发现（`system_font_discovery` 已启用，根 `Cargo.toml:7`）。
- [x] **删除** `startup.rs:88` 的 `else { Handle::default() }` 分支（这是 A1 的直接病灶）；`fonts.rs:19-20` 注释同步。
- [x] `tracing::info!` 输出实际命中家族与来源（C-FONT-2）。
- **验收（硬门槛 A）**：三平台各启动一次，日志显示 `mono` 解析到真实等宽字体（非 Inter）；不过则停止铺开并升级处理。
  - Windows（本机）：✅ `tests/font_slots.rs::mono_resolves_to_real_monospace_on_this_machine` 断言本机解析到真等宽（家族 ≠ Inter）。
  - macOS / Linux：⏳ 需在对应平台各启动一次确认同一行日志（**M4-T2 起不阻塞**，见 §10 勘误 31）。

### M4-T2 终端双宽单元格（⚠ 最大改动面）
- [x] `MonoMetrics { latin_advance, cjk_advance, line_height, at_size }` 启动期实测；校验 `cjk_advance ≈ 2×latin_advance`，不满足记警告并调基准（C-FONT-4）。
  - 实现在 `xgent_ui::terminal::metrics`：探针节点走 bevy 文本管线（终端渲染同一条 measure 路径）反推步进与行高；校验与基准宽调整是纯函数。
- [x] `display_width(char) -> usize`（wcwidth 语义：CJK 全角 / emoji / U+FF00–FF60 → 2；组合记号 → 0）。
  - 实现在 `xgent_terminal::render`（终端领域 crate，无 Bevy 依赖），附 `display_width_str` 与 `layout_cells`（按格布局内核）。
- [x] 终端排版改为按格数 × `cell_w` **手工定位**，不再依赖 shaper 连续排版；`terminal/output.rs` 的 `font × 0.6` 估算随之删除。
  - 行渲染改为「行容器 + 绝对定位的 run 节点」（`left = 起始列 × cell_w`）；PTY 的列数改为 `floor(视口逻辑宽 / cell_w)`。
- [x] 保留旧路径在 feature flag 下做 A/B 截图对照，通过后再删。
  - cargo feature `legacy-terminal-layout`（默认不开启）保留旧的 flex 连续排版路径。
- **验收**：三平台跑 `ls -l` / `cargo test` 中文输出 / `top`，列对齐正确；`vim` 全屏 TUI 不乱行。
  - Windows（本机）：✅ 自动化断言（`tests/terminal_cell_layout.rs`：实测度量 / run 的 `left == 起始列 × cell_w` / PTY 列数 = `floor(宽 / cell_w)` / 旧估算已删）；实机视觉确认待人工过一遍。
  - macOS / Linux：⏳ 待办（不阻塞本项归档，见 §10 勘误 31）。
- 前置：硬门槛 A 的 macOS/Linux 启动日志确认（见 M4-T1 验收）——**已按用户决定降级为不阻塞**。

### M4-T3 代码与行号消费 `mono`
- [x] 行号列、diff 行、工具卡参数摘要走等宽（`mono_text` / `ellipsis_mono_text`）；禁止用 `ui_text` 冒充等宽。
- [x] 编辑器**代码正文**与高亮层 span 走 `mono` 槽位（`EditorTheme.font` 注入 + 每个 `TextSpan` 自带字体来源——bevy 逐 span 解析字体，根 Text 为空会被跳过）。
- [x] `type_scale` 新增 `CODE` 档、`line_height::CJK = 1.7`；`SMALL 12.5 → 12`（消除半像素模糊）。
- **验收**：`theme.rs` 测试更新令牌断言；行号列与代码列在 125%/150% 缩放下仍对齐。
  - 代码侧已保证「行号列与代码正文同源同字体」（等宽）→ 数字等宽 → 列对齐；非整数 DPI 的**视觉**确认仍需真实窗口（见 §10 勘误 26、28）。

### M4-T4 中文排版约束
- [x] 展示级负字距只作用拉丁段，中文不挤（设计系统 1.4）。
- [x] 换行走已落地的 CJK 分词（R3-8），禁整句单 token。
- **验收**：中英混排标题与正文无豆腐块、无异常挤靠。

---

## M5 槽位注册表与派生可见性（修 D1–D6、I3）

> 状态：**已交付**（Comet change `v8-m5-panel-registry`；实现提交 `0aeecc4` T1–T3、
> `0141343` T4、`dc22458` T5、`7eb83e9` T6）。落点：`xgent_ui::panels`（模型 / 派生可见性 /
> 槽位页签 / 分割树）、`activity_bar.rs`（rail 描述符驱动）、`session_history.rs`（注册面板）、
> `resize.rs`（遍历分割树）、`tests/panel_registry_contract.rs`（21 项契约测试）。
> M5-T3 的验收口径见 §10 勘误 34（内容态可见性不进派生系统）。

### M5-T1 `PanelRegistry` 取代 closed 枚举（⚠）
- [x] `Slot{Rail,Center,SideDock,BottomDock,SettingsSection,StatusSegment,Palette,Overlay}` + `PanelDescriptor`（`title_key` 必填 i18n key）+ `PanelRuntime`。
- [x] `PanelRegister/PanelActivate/PanelDeactivate` 消息（遵守 ECS 契约，禁直接方法调用）。
- [x] `SideViewContent`（`editor/mod.rs:69-81`）加**临时 shim**（枚举 → 描述符一次性映射），使后续可分小批合入且每批可编译。
- **验收**：新增一个 `SideDock` 面板无需修改 `layout.rs`/`editor/mod.rs` 的枚举与页签条代码。✅ 源码级断言（`layout.rs` / `editor/mod.rs` 不含 `SideViewContent`、不含按面板 id 的分派）。

### M5-T2 rail 与历史进槽位（修 D4/D5）
- [x] 删除 `ActiveActivity`，选中态从 `registry ∩ slots.is_visible()` 派生。
- [x] `ActivityKind{Chat,Files,History,Terminal}`（`activity_bar.rs:20-30,251-306`）改为描述符驱动；`session_history.rs:136-251` 从浮层改为可注册面板。
- **验收**：连点 Files 两次关掉抽屉后选中态回落正确；历史面板与文件抽屉不再争层级。✅ 单测（`active_rail` 回落）+ 集成测试（Overlay 单例槽位）。

### M5-T3 派生可见性单一系统（修 I3/D2）
- [x] `derive_panel_visibility` 成为唯一写「面板 / 区域」`Node.display` 的系统；原 7 处动态写手（`layout.rs` 分屏与手柄、`file_panel.rs` 遮罩与面板、`activity_bar.rs` 展开钮、`editor/mod.rs` 编辑器视图、`terminal/mod.rs` 终端视图）全部并入；3 处 spawn 期默认值改为构造期字面量。
- **验收**：`grep -rn "\.display = " crates/xgent_ui/src/` 的命中仅限**派生系统 + 构造期字面量 + 内容态白名单**（白名单口径见 §10 勘误 34，由源码级测试固化）。✅

### M5-T4 单页签条所有者（修 D3）
- [x] 页签条由槽位布局拥有并渲染（`xgent_ui::panels::tabs`）；编辑器内部 tab 条降级为「文档列表」语义（与槽位页签不是一回事，不合并）。
- [x] 合并两套 `EditorBackButtonMarker`（`editor/mod.rs:220-239` 与 `:706-731`）。
- **验收**：侧区顶部只有一条页签条、只有一个返回入口。✅ 源码级断言（`EditorBackButtonMarker` 全仓唯一定义）。

### M5-T5 分割模型（修 D6）
- [x] `SplitTree/Split/LeafSize` + `resize.rs` 从单字段（`resize.rs:47-50,205`）改为遍历生成分割条。
- [x] `DRAWER_W` 从定值 320（`theme.rs:236`）改为 `Min(240, Preferred(320))`、上限 480。
- **验收**：抽屉可拖宽；可注册纵向分割而无需改 `resize.rs`。✅ 单测（手柄遍历 / 宽度钳制）+ 源码级断言（`resize.rs` 不含按**分割拓扑**的特判：手柄遍历与轴向取自分割树，尺寸上下界按叶子 id 取尺寸策略）。

### M5-T6 移除 `SideViewContent` 枚举
- [x] shim 拆除，所有既有面板（editor/preview/diff/terminal）为原生注册项。
- [x] `i18n`：页签标题取 `title_key`，删除 `editor/mod.rs:659-664` 的 `let _ = loc;`。
- **验收**：`grep -rn "enum SideViewContent" crates/` 无命中；英文语料下页签为英文。✅ 源码级断言 + 语料断言（en-US 无 CJK）。

---

## M6 会话内容与输入（修 E1–E5）

> 状态：**部分交付 = 模型层**（提交 `9ac4daf`；M6-T5 接线在 `81d3060`）。
> M6 归档（`247d1bd`）的验证记录自述四条 Known limitation（Transcript 未集成、
> ContextSet 未替换、依赖评审未落、@ 补全数据源未接），且 `No Runtime checks were recorded`。
> 因此 T1/T2/T3/T4/T6 只勾「模型」，接线一律移入 §M6.5。见 §10 勘误 44/45。

### M6-T1 `Transcript` 有序权威（修 E2）
- [x] 模型：`Transcript` + `BlockRef` 块序列与顺序键 `(turn_index, block_index)`（`9ac4daf`）。
- [x] 渲染按顺序 spawn/despawn/update；删除命令式挪子节点（`chat_panel.rs`、`tool_panel.rs`、`undo.rs`）→ **M6.5-T1**。
- **验收**：边流式边插工具卡，顺序稳定不倒退（自动化断言序列单调）。

### M6-T2 Markdown 块模型与依赖评审
- [x] 解析器 `pulldown-cmark 0.12` + `MarkdownBlock` 模型与单测（`9ac4daf`）。
- [x] 依赖评审记录落 `docs/notes/markdown-dependency-review.md` → **M6.5-T2**。
- **验收**：评审记录落 `docs/notes/`；解析器单测覆盖六类元素。

### M6-T3 富文本渲染（修 E1 / R3-5）
- [x] `spawn_markdown` 渲染辅助（单节点多 `TextSection`、代码块样式、复制按钮）（`9ac4daf`）。
- [x] 接入 chat_panel 的终稿消息与流式增量路径；语法色接 `xui` 高亮链 → **M6.5-T2**。
- **验收**：六类元素混合样张视觉正确；对照原型截图。

### M6-T4 `@` 内联补全（修 E3）
- [x] `CompletionState` / `spawn_completion_popup` / `detect_at_trigger` 模型与单测（`9ac4daf`）。
- [x] 接入输入框与真实候选来源（文件树 / ProjectRoot；符号与 selection 后置）→ **M6.5-T3**。
- **验收**：输入 `@` 出现补全并可键盘选择；窄窗不被裁。

### M6-T5 发送/停止动作条（修 E4）
- [x] 显式发送/停止按钮 + 按 `ConversationStatus` 派生可见性（`81d3060`）。
- [x] 删除「发送按钮区」空注释。
- **验收**：不碰键盘可完成一次完整发送与一次中断。

### M6-T6 `ContextSet` 建模（修 E5）
- [x] 模型：独立 Resource（增删/清空/token 合计）（`9ac4daf`）。
- [x] 替换 `context_scope.rs` 的只读镜像、token 占比显示、可持久化 → **M6.5-T4**（会话级 sidecar，见 §10 勘误 48）。
- **验收**：移除某 chip 后其不再进入下一轮请求。

---

## M6.5 会话内容接线（M6 降级后的集成期）

> 状态：**已交付（自动化证据齐；真实窗口截图待走查）**。目标不是新增抽象，而是让
> `9ac4daf` 的模型进入产品路径，并把 M6 六项验收补成可复跑证据。实现提交 `b962f97`，
> 启动修复 `5dde747`；依赖已交付的 M3 滚动契约与 M5 槽位注册表。

| 任务 | 范围 | 验收（可机器判定） |
|:---|:---|:---|
| M6.5-T1 Transcript 渲染 | 把用户消息、助手流式增量、工具调用/结果、压缩通知写入 `Transcript`；渲染系统按 `(turn_index, block_index)` 差分 spawn/update/despawn；删除 chat_panel/tool_panel/undo 的命令式挪节点；会话消息列表虚拟化（承接 §10 勘误 13） | ✅ 流式中插工具卡顺序单调；全仓无「add_child 挪序」；长会话只渲染可见行；段间空档与 HiDPI 物理/逻辑换算有契约（`transcript_render_contract` 10 项）；无当前轮时消息排空不重放、steering 也落块、denied/progress 都渲染 |
| M6.5-T2 Markdown 接线 | 终稿助手消息走 `parse_markdown` + `spawn_markdown`；流式期纯文本、完成后替换，避免每帧重排；代码块复制按钮接线；补 `docs/notes/` 依赖评审 | ✅ 六类元素单测 + 内联组（段落内粗体/链接同一行；标题/列表项内并入文本；嵌套列表不丢外层）；终稿档位与流式一致（`mono_typography` 扩到 `markdown_render.rs`）；窄窗不溢出；复制按钮可用；评审记录在库；真实窗口样张待走查 |
| M6.5-T3 `@` 补全接线 | 输入框触发 `detect_at_trigger`；候选来自文件树 / ProjectRoot；浮层挂 `DROPDOWN`；方向键 / Enter / Esc；接受后写回 `EditableText` | ✅ 浮层挂 DROPDOWN；键盘路由与 Enter 消费（带修饰键不吞发送）；主动关闭即销毁浮层；IME 组合期不接管且索引有上界（不再 panic）；窄窗钳制 + 锚点逻辑像素；候选去抖 |
| M6.5-T4 ContextSet 接线 | 替换 `context_scope.rs` 只读镜像；chips 与 `ContextSet` 双向同步；发送请求时内容进入 `AgentCommand::StartLoop`；token 占比显示；持久化（会话内先做，跨会话按 ADR-0008 评估） | ✅ 移除 chip 后下一轮请求不含该内容；token 占比；会话级 sidecar 恢复（磁盘往返 + 旧数组格式 + 损坏回退有测试，失败/损坏告警）；pinned 路径与检索同形、显式项优先且受 `retrieval_max_tokens` 约束 |
| M6.5-T5 视觉与端到端证据 | 真实窗口走查 + 截图（Markdown 样张、工具卡插入顺序、`@` 浮层、chips 删除）；把 M6 A1–A6 从「用户接受降级」补成可复跑证据 | 🟡 自动化证据已落测试；真实窗口截图待用户走查 |
| M6.5-T6 文档与门槛 | M6 档案口径订正；handoff / AGENTS / dev-tutorial 同步；mac/Linux 硬门槛 A 排期（用户 2026-10-28：Windows 收尾后立刻排） | ✅ 文档同步；门槛 A 定为 M6.5 后第一个里程碑首项 |

> ⚠ M6.5 的 T1 会同时触碰流式路径、工具卡与 undo 通知三处命令式渲染；
> 必须按 M6-T1 的顺序键先建数据流，再替换渲染，禁止先删旧路径。

---

## M7 闭环：快捷键、焦点、i18n、验证层

> 状态：**已交付**（实现提交 `81d3060`）。A1 的强度按实际实现订正：`handler_name`
> 是字符串而非函数指针，断言只比较 id 集合——见 §10 勘误 46。

### M7-T1 快捷键单一来源（修 F1/F2）
- [x] `HotkeyAction` 清单（`id/label_key/default/handler_name`），注册从清单派生。
- [x] 修 `palette.open_files` 与 `input.focus` 空实现；12 个 action 均有分发表分支。
- [x] 启动期一致性断言：注册表 id 集 == 清单 id 集。
- **验收**：一致性断言全绿；死键数为 0；命令面板显示的键位即真实绑定。**未达成**：分发表缺分支无编译期/启动期保障。

### M7-T2 焦点遍历与模态隔离（修 F3/F4）
- [x] 装配 `TabNavigationPlugin` + `DirectionalNavigationPlugin`（`xgent_app/main.rs`）。
- [x] `TabIndex` 由布局顺序派生（间隔 10）。
- [x] 模态层根挂 `TabGroup::modal()` → Tab 不跳出模态。
- [x] 焦点环消费 `accent_interactive`，配合 `InputFocusVisible` 只在键盘导航时显形。
- **验收**：设置面板内 Tab 按视觉顺序遍历且可见焦点环；模态内 Tab 不外逃。

### M7-T3 i18n 全覆盖（修 H1/H2/H3）
- [x] 硬编码点迁移（tooltip / 编辑器读取取消等）；新增 `i18n_full_corpus` 守卫。
- [ ] `xui` 的 `StringSource` 真正被消费（R3-7/D5）——**未做**，移交后续。
- [x] 语料守卫：EN 无 CJK、zh 无缺失 key、键集一致。**注意**：守卫只读 `.ftl` 语料，不是全模块源码扫描（见 §10 勘误 47）。
- **验收**：英文语料下全界面无可读中文残留（白名单显式）；中文界面文案正确。

### M7-T4 状态健壮性（修 I1）
- [x] 删 `ChatPanelEntities`，改 marker 查询。
- [x] `single_mut()` 静默早退改 `single()` + `tracing::warn!`（`debug_assert!` 按 §10.1 移除）。
- **验收**：布局重建后无静默失效；数量异常有日志。

### M7-T5 布局几何断言层（修 I2）
- [x] `crates/xgent_ui/tests/layout_geometry.rs`：6 个断言函数 + 11 项回归。
- [x] 覆盖 chat / palette / confirm / status / topbar / resize / 滚动 / 重叠。
- [ ] 每条 P0 缺陷的先红后绿证据已并入测试，但尚无独立复跑记录 → **M6.5-T5 一并补**。
- **验收**：`cargo test --workspace` 全绿——**只对 `xgent_ui` 子集成立**；全量仍有既有红（见交接 §2）。

### M7-T6 文档同步与勘误
- [x] `docs/dev-tutorial.md` 同步（M6 口径按 §10 勘误 44 订正）。
- [x] `docs/plans/README.md` 的 v7 状态勘误。
- [x] `docs/design/ui-design.md` 标注被 v8 取代；`ui-bsn-refactor-*` 顶部加放弃说明。
- [x] ADR-0015 已归位（状态：草案，待确认转「接受」）。
- [x] 本文 §10 追加勘误条目 40–47。

---

## M8 Git 探针（验收北极星指标，非功能开发）

### M8-T1 源代码控制面板最小注册
- [ ] 仅**新增** 1 个 `PanelDescriptor`（`SideDock`，`title_key`、icon、order）+ 1 个视图模块，展示 F-10 插件已有的 status 数据。
- [ ] 不修改 `layout.rs`、`editor/mod.rs` 的枚举/页签条/显隐代码。
- **验收（北极星）**：改动文件数 = 新增 2 个（视图模块 + 注册项）、既有布局文件改动 = 0；面板可切换、可键盘到达、可滚动、可截断、i18n 完整。
- **意义**：这是 U-05/U-12 的达标证明，也是「下一个界面不用再改布局」的可检验承诺。

---

## 8. 里程碑验收清单

| 里程碑 | 关键产出 | 闭合缺陷 | 门槛 |
|:---|:---|:---|:---|
| M1 层根 + 模态栈 | 六层根、`ModalStack`、键盘路由 | B1 B2 B3 B4 | ⚠ 硬门槛 B |
| M2 令牌与主题热驱动 | 语义令牌、`Themed`、尺寸令牌化 | G1–G6 A2 | — |
| M3 滚动与省略 | `ScrollFrame`、`Ellipsis/Clamp`、`VirtualList` | C1–C5 E6 I4 D2 | — |
| M4 字体与双宽 | 三平台字体装配、`MonoMetrics`、按格定位 | A1 A3 | ⚠ 硬门槛 A |
| M5 槽位注册表 | `PanelRegistry`、派生可见性、`SplitTree` | D1–D6 I3 | ⚠ 最大改动面 |
| M6 会话内容（模型层） | `Transcript`/Markdown/补全/ContextSet 模型 | E1–E5（仅 T5 接线） | **部分交付**（`9ac4daf`） |
| M6.5 会话内容接线 | 渲染/输入/上下文接入产品 + 视觉证据 | E1–E5 补齐 | 已交付（`b962f97`/`5dde747`；真实窗口截图待走查） |
| M7 闭环与验证 | 快捷键单源、焦点栈、i18n 扩面、几何断言 | F1–F4 H1–H3 I1 I2 | 已交付（`81d3060`；A1 强度见 §10.46） |
| M8 Git 探针 | 1 注册 + 1 视图模块落地 | 验证 U-05/U-12 | 未开工 |

每期收尾命令：`cargo fmt -p xui -p xgent_ui -p xgent_app` → `cargo clippy --workspace` → `cargo test --workspace`。

---

## 9. 本设计已核实的引擎事实（实现时可直接引用）

均为本会话在 `../bevy`（v0.20.0）与 `E:/ws/xgent` 内 grep/sed 确认，非推断：

1. ✅ `UiPickingSettings::default().require_markers == false` — `bevy_ui/src/picking_backend.rs:67`；置 `true` 后非文本节点无 `Pickable` 即被后端跳过 — `:197`。
2. ✅ 无 `Pickable` 组件的实体默认**阻挡**下层 — `bevy_picking/src/lib.rs:212`。`Pickable{should_block_lower, is_hoverable}` 两字段可表达"穿透"与"只挡不吃事件"。
3. ✅ bevy 0.20 **无 `TextTruncate`**；全仓 `truncat` 仅命中 `bevy_text/src/bounds.rs` 注释与 `stack.rs`。`TextBounds` 存在但官方自陈硬裁不可靠 — `bounds.rs:9-12`。
4. ✅ `TextLayoutInfo`（含 `size`）由 `TextPipeline::update_text_layout_info` 在实体有 `TextLayout`+`ComputedTextBlock` 时生成 — `bevy_text/src/pipeline.rs:485`。文本按 section 命中 — `bevy_ui/src/picking_backend.rs:219`。
5. ✅ `TabNavigationPlugin` 需手动 add（`bevy_input_focus/src/tab_navigation.rs:18`）；`TabGroup{order,modal}` 与 `TabGroup::modal()` 提供模态内焦点循环 — `:73,86,94`；`InputFocusVisible(bool)` — `lib.rs:182`；`InputFocusPlugin` 已在 DefaultPlugins — `bevy_internal/src/default_plugins.rs:16`。
6. ✅ `GlobalZIndex(pub i32)` — `bevy_ui/src/ui_node.rs:2886`；`FocusPolicy{Block,Pass}`（默认 `Pass`）— `bevy_ui/src/focus.rs:121-129`，且项目实测 picking 忽略它（`docs/dev-tutorial.md:565`）。
7. ✅ 项目侧现状：字体资产只有 `crates/xgent_app/assets/fonts/Inter-Variable.ttf`，无等宽资产；`startup.rs:88` 非 macOS 走 `Handle::default()`；CJK 依赖 `system_font_discovery`（根 `Cargo.toml:7`，见 `docs/plans/ui-v7-migration.md:280`）。

## 10. 实施勘误（随实施追加）

1. **`ThemeHandle` 只有一个 `generation`**（v8-m2b-density-hover）：密度与主题都是显示偏好，共用同一个计数；切密度时 `repaint_on_theme_change` 会多跑一遍，但写入的是同一份主题值，视觉无差异。架构 §8.1 里「`ThemeHandle { theme, generation }`」的字段清单据此补了 `density`。
2. **`preferences.density` 需要 daemon 键白名单**（v8-m2b-density-hover）：`ConfigCoordinator::write` 对未知 key 直接报错，只改 UI 侧会让密度永远存不下来。已在 `config_store.rs` 的读/写各加一条（写只接受 `comfortable`/`compact`），除此之外 daemon 逻辑未动。
3. **`M2-T7` 的触发源订正**：任务书原文写「`Interaction` + 焦点态」，实际 0.20 的 `Interaction` 已废弃，实现用 `Hovered` + `Pressed`（松手是「移除」，须用 `RemovedComponents<Pressed>` 单独接）。
4. **`scroll_to_child` 从未实现**（M3-T1）：台账与 `scroll_area.rs:12` 的文档注释都提到它，但代码里没有该函数。已补齐（消息驱动 + 纯函数 `compute_scroll_to_child`）。
5. **`Overflow::clip_y()` 是「假滚动」的信号**（M3-T2）：bevy 的 `ScrollPosition` 只在 `OverflowAxis::Scroll` 轴生效（`bevy_ui/src/layout/mod.rs:476-491`），`clip_y` 轴上的偏移被布局系统直接丢弃 → 「看起来能滚、实际滚不动」。真正需要硬裁的地方要用 `Overflow::clip()`（两轴）。
6. **文本挂在滚动容器自身时不随内容滚动**（M3-T2）：`Text` 渲染在节点自身的内容盒原点（`bevy_ui_render` 用节点的 `content_box().min`），滚动偏移只施加给**子节点**。因此工具卡结果区的文本必须放进滚动容器的子节点。
7. **可编辑文本的高度由 `visible_lines` 决定，不由文本内容决定**（M3-T3）：bevy 0.20 的 `EditableText::visible_lines`（默认 `Some(1.0)`）经 `update_editable_text_content_size` 写进节点的 `ContentSize`；要「随内容长高」必须自己回写 `visible_lines`。
8. **`Text` 与 `EditableText` 会争 `ContentSize`**（M3-T3）：同一节点上两者都存在时，`measure_text_system`（按 `Text` 量）与 `update_editable_text_content_size`（按 `visible_lines` 量）各写一次，结果与执行顺序有关（高度时对时错）。可编辑文本节点因此**不挂** `Text`；但 `Text` 缺失时 `require_markers` 下节点必须自带 `Pickable` 才收得到点击。
9. **bevy 自带的 `TextViewport::reveal_caret` 只在焦点变化时触发**（M3-T3）：`bevy_ui/src/widget/text_input_layout.rs:543` 的条件是 `focus_changed`，继续打字不会跟随光标 → 长输入会把当前行藏起来。对话输入区自行按 `TextLayoutInfo.cursor` 的光标矩形算滚动目标。
10. **xui 的滚动条有两个缺陷**（M3-T1）：① 轨道是容器的子节点，而 bevy 把父容器的滚动偏移施加给**所有**子节点（含绝对定位的），轨道会随内容滚走 → 需 `IgnoreScroll`；② `spawn_scrollbar_nodes` 每帧都给容器追加一条新轨道（过滤条件 `Without<ScrollbarTrack>` 落在容器上，而轨道在**子**节点上）→ 子节点无限增殖。两处均已修。
11. **headless 装配缺 `OverlayPlugin` 会让文件面板整体报错**（M3-T6 顺带修复）：`DrawerToggle` 消息由 `xgent_ui::overlay::OverlayPlugin` 注册，缺它时 `handle_drawer_overlay_click` 直接报「Message not initialized」，文件树从未重建 → `file_preview`(7) 与 `file_tree_refresh`(5) 长期失败。补插件 + 补齐 headless 装配（窗口/UI/picking 插件、相机、TextureAtlasLayout 资产）后 12 个测试转绿。
12. **多行 clamp 未做**（M3-T4）：`TextBounds` 官方自陈「only characters that are completely out of the bounds will be truncated, so this is not a reliable limit」，用它做硬裁不可靠；本轮只交付单行省略。多行 clamp 留待 M6（与 Markdown 渲染一起做，届时有真实的多行文本场景可测）。
13. **M3-T6 的第二个消费者改为「会话历史列表」**（原写「会话消息列表」）：会话消息列表目前仍是命令式 spawn（消息、工具卡、压缩通知由不同系统直接挂节点），要虚拟化必须先有 M6-T1 的 `Transcript` 有序数据模型；本轮改接会话历史列表（结构简单、天然一维），会话消息列表的虚拟化随 M6-T1 一起做。
14. **虚拟列表的数据换代需要「代次」**（M3-T6）：数据重建后，落在可见区间内的旧行内容会过期（同一文件出现两行）。行上记数据代次（`FileTreeRow.generation`），换代即作废旧行。
15. **虚拟列表需要视口高度才有内容**（M3-T6）：headless 装配没有渲染目标时视口高 0 → 可见行数为 0 → 列表看起来是空的。文件树因此加最小高度 240px（`FILE_TREE_MIN_H`）。
16. **省略号的「中段」模式不能用「head + suffix」当完整文本**（M3-T4）：中段省略的完整文本是**原文**；`head + suffix` 是省略后的形态（少了中间段且还没有省略号），拿它判断「是否装得下」会漏判。
17. **bevy 0.20 的「系统字体发现」要自己查集合**（M4-T1）：`parley/system` 特性让 `fontique::Collection` 默认加载系统字体（`CollectionOptions::default().system_fonts == true`），但**没有**「按家族名查存在性」的便捷 API——用 `collection.family_names()` 取快照或 `collection.family_id(name)`（两者都要 `&mut`，所以要先收集快照再闭包查询）。另：`Font::from_bytes` 的 `alias` 是**空串**（内嵌资产只能按 `FontSource::Handle` 引用，不能按家族名进 fallback 链）。
18. **`ui` 槽位不能带字体句柄**（M4-T1）：`xgent_ui::fonts::ui_text` 不接收 `UiFonts`（约 50 处调用点），所以 `ui` 槽位只能继续用**默认字体资产**（`Handle::default()` → 内嵌 Inter Variable），CJK 靠 parley 的系统回退承担。设计系统的 `ui_cjk` 槽位本轮只做**解析与日志**（保证 CJK 可用性可核对），专用 CJK 表面留给 M4-T4。
19. **`UiFonts.mono` 的类型从 `Handle<Font>` 变成 `FontSource`**（M4-T1）：候选栈的解析结果不是单一资产而是「家族名或内嵌句柄」，只能用 `FontSource` 表达；`mono_text` / `mono_text_weighted` 的调用点不变（`FontSource` 直接进 `TextFont.font`）。headless 装配用 `UiFonts::fallback()`（等宽与 CJK 走系统通用族）。
20. **硬门槛 A 的跨平台部分无法在本机验证**（M4-T1）：本机（Windows）可用 `tests/font_slots.rs::mono_resolves_to_real_monospace_on_this_machine` 断言解析到真等宽；macOS / Linux 的同一行启动日志需在对应平台各启动一次确认（台账 M4-T1 验收已标注为待确认）。
21. **`type_scale::MONO` 只改了字号、没改字体**（M4-T3）：编辑器代码正文、行号列、diff 行、工具卡参数摘要此前**全都用比例字体渲染**（等宽字号 + Inter 字体）→ 行号数字宽度不等 → 行号列与代码列错位。M4-T3 已让这四处走 `mono` 槽位（`EditorTheme.font` 注入 + `mono_text`/`ellipsis_mono_text` 构造器）。
22. **`EditorTheme` 需要携带字体来源**（M4-T3）：它此前只有颜色与字号，代码正文因此落回默认字体。新增 `EditorTheme.font: FontSource`，由 `sync_editor_theme` 注入 `UiFonts.mono`；`sync_editor_theme` 由此多依赖一个 `UiFonts` 资源（headless 测试需 `UiFonts::fallback()`，否则系统报缺资源）。
23. **代码档用改名而非新增**（M4-T3）：台账写「新增 `CODE` 档」，实现为把 `MONO` **改名**为 `CODE`——原 `MONO` 的三个使用点（确认弹窗 diff 行、diff 视图行、终端输出）全是代码场景，留两个同值常量只会让人随便挑一个用。
24. **中文判定用「含任一 CJK 码位」**（M4-T4）：负字距一旦作用在 CJK 上就会挤成一团；用占比阈值会漏掉「一句中文带一个英文词」的混排串。行高档按**文本内容**判定（不按 UI 语言设置）——同一会话中英混排，按语言选档会出现两种行高。
25. **`CODE` 档的 Compact −1 未做**（M4-T3）：密度系统按 M2b 纪律**只缩间距**、不缩字号；字号缩放要另立机制（否则密度档与字号档两套系数互相干扰）。
26. **bevy 0.20 逐 span 解析字体来源**（M4-T3 修复轮）：只给**根 Text** 实体设 `TextFont.font` 对字形**无效**——根 Text 为空会被排版管线跳过，真正承载字形的 `TextSpan` 各自解析自己的 `TextFont`。因此代码正文与高亮层的**每个 span** 都必须自带字体来源（`virtual_render.rs` / `render.rs` 的 span spawn 处）。这是「行号列换了等宽、代码列仍是比例字体 → 两列不同源」的根因。
27. **消息正文有三条渲染路径**（M4-T4 修复轮）：流式助手正文、**固化副本**（`finalize_on_done`）、**会话历史回看**。只改流式路径会出现「流式 1.7 → 完成 1.6」的行高跳变，且历史回看又是 1.6 → 三条路径都要走 `line_height::for_text`。
28. **非整数 DPI 下的视觉对齐仍需真实窗口**（M4-T3）：代码侧只能保证行号列与代码正文同源同字体（等宽 → 数字等宽）；「125%/150% 缩放下行号列与代码列仍对齐」这一**视觉**判据需真实窗口 + 非整数 DPI 才能确认。
29. **等宽度量不能读 `TextLayoutInfo.size`**（M4-T2）：`TextLayoutInfo.size` 是 `ceil` 过的物理像素（12px × 1.6 行高会被抬成 **20.0** 而不是 19.2；32 个 6.6px 字符的 211.2 会变成 212）。误差会**直接**进到单元格基准宽与行高上。正确取法是 `ComputedTextBlock::buffer()` 的 parley layout 原始值（`full_width()` / `height()`），再除以 `layout.scale()` 还原逻辑像素。
30. **CJK 回退字体几乎不可能满足 2:1**（M4-T2）：微软雅黑 / 苹方 / 思源黑体的**汉字**步进是 1em，而等宽拉丁是 0.6em——`cjk_advance ≈ 2 × latin_advance` 在三平台**都**不成立（实测本机通用等宽：latin 6.60px / cjk 12.0px，2×latin = 13.2px，偏差 9%）。因此 C-FONT-4 的校验结果是**平台相关**的：本机（偏差 9% < 10% 容差）走「不告警、cell_w = latin_advance」分支；偏差更大的平台（如 Linux 的 DejaVu Sans Mono 0.602em → 2×latin 与 cjk 差 17%）才走「告警 + cell_w = max(latin_advance, cjk_advance / 2)」分支。两种分支下**网格都正确**（宽字符起点严格落在格边界），差别只在宽字符的字形填不填满那 2 格（本机约差 1.2px/字）。⚠ 推论：**`mono_cjk` 槽位（Sarasa / Noto Sans Mono CJK）才是真正 2:1 的等宽 CJK**，本轮未接入（见 §M4-T2 非目标）。
31. **硬门槛 A 的 macOS / Linux 部分降级为不阻塞**（M4-T2，用户 2026-10-21 决定）：M4-T2 的按格定位依赖「`mono` 是真等宽」这一前提，但该前提只在 Windows 上确认过。用户决定「不要阻塞于 macOS/Linux 下的运行结果，可以优先做 Windows 的适配，后面再适配其他平台」→ macOS / Linux 的 `mono` 启动日志确认与实机验证**记为待办**，不阻塞 M4-T2 归档。后续在其他平台适配时须回头确认该项。
32. **headless 集成测试里 UI 视口恒为 0**（M4-T2）：没有渲染管线时相机的 `computed.target_info` 为空 → `physical_viewport_size()` 返回 0 → UI 根节点尺寸 0 → 所有子节点尺寸 0（与 §10 勘误 15 同一个坑，但更隐蔽：不是「视口高 0」而是**宽高全 0**）。解法：测试 App 建好后手动给相机塞一个 `RenderTargetInfo { physical_size: UVec2::new(1280, 720), scale_factor: 1.0 }`，UI 才有非零视口，终端的列数/行数才可断言。
33. **绝对定位的子节点按 max-content 定宽，不按「容器宽 − left 偏移」收缩**（M4-T2 验证轮）：曾担心靠右的 run 会因可用宽不足而在 run 内折行、行高超出固定 `line_h` 压到下一行。实测（`tests/terminal_cell_layout.rs::long_run_does_not_wrap_inside_the_run`，200 字符 ≈1320px 放在 ≈700px 宽的输出容器里）**不成立**——run 节点实测尺寸就是 1320×20（max-content），没有折行。run 仍显式挂 `TextLayout { linebreak: NoWrap }` 作**硬化**（不依赖这条 taffy 行为）。

34. **M5-T3 的 `.display =` 验收口径与实测不符**（M5，用户 2026-10-21 裁决）：台账原文写「命中仅限该系统与层根构造」，但 `crates/xgent_ui/src/` 实测共 **15 处** `.display =` 写手，分三类——① **7 处动态面板/区域可见性**（`layout.rs` 分屏与右手柄、`file_panel.rs` 抽屉遮罩与面板、`activity_bar.rs` rail 展开钮、`editor/mod.rs` 编辑器视图、`terminal/mod.rs` 终端视图）；② **3 处 spawn 期构造默认值**（`editor/mod.rs` 186、`editor/diff_view.rs` 59、`terminal/mod.rs` 275，视图容器初始隐藏）；③ **5 处内容态可见性**（`chat_panel.rs` 1054 回到底部按钮、`activity_bar.rs` 234 陪伴激活环、`welcome.rs` 190 欢迎空态、`welcome.rs` 223 最近会话标题、`editor/mod.rs` 398 文档页签）。裁决取**收窄**方案：只归一 ① ②（面板 / 区域可见性），③ 不动——它们描述的是**内容状态**（会话是否为空、列表是否为空、滚动位置、当前文档页签、陪伴开关）而非面板 / 区域，塞进面板注册表是类别错误，且会把本期扩到内容层（越出 M5 边界、逼近 M6/M7）。口径以**显式白名单 + 源码级测试**固化（`tests/panel_registry_contract.rs::only_derive_system_and_content_whitelist_write_display`），台账 M5-T3 的验收句据此改写。
35. **`Slot` 集合补 `Center`**（M5）：台账 M5-T1 的列表（`Slot{Rail,SideDock,BottomDock,SettingsSection,StatusSegment,Palette,Overlay}`）早于架构 §3.1 的 2026-10-19 增补，**缺 `Center`**；按架构（`Center` 与 `SideDock`/`Overlay` 同为单例互斥槽位）与提示词「本轮只把 `Center` 槽位建模好、不接消费者」实现，首个消费者留给 F-16 的 3D 空间视图。
36. **Bevy 0.20 不允许重复添加同一插件**（M5）：`App::add_plugins` 对已添加的插件直接 panic（`plugin was already added in application`），**不是**幂等。多个业务插件共享 `PanelsPlugin` / `VisibilityPlugin` 时必须先 `app.is_plugin_added::<T>()` 判空再添加。另：`PanelsPlugin` 用 `init_resource` 兜底 `Localizer` / `Theme` / `SplitTree`（`init_resource` **不覆盖**宿主注入的真值，故 `xgent_app` 注入的真实 `Localizer` 仍然生效）。
37. **派生视图不可当权威状态改**（M5）：`FileDrawerOpen` / `SessionHistoryState` 现在是**派生视图**（由注册表算出），headless 测试直接 `insert_resource(FileDrawerOpen(true))` 会被派生系统覆盖 → 抽屉保持隐藏、文件树（依赖抽屉宽度有值）不渲染行。测试要开抽屉必须发 `PanelActivate { id: ids::FILE_DRAWER }`。同理打开代码文件后面板可见性比旧枚举**晚 1～2 帧**（多一跳 `OpenFileRequest → PanelActivate → 注册表`），断言要从「逐帧等于 Editor」改成「一旦可见不得再消失」（`tests/file_preview.rs`）。
38. **提交粒度与台账 §0.3「每任务一提交」的偏差**（M5）：M5-T1～T3 共享同一批类型与消息（注册表模型 → rail / 历史迁移 → 派生可见性），拆成三条独立可编译提交需重建三个中间态，收益低于成本；实际落为 **4 条实现提交**：`0aeecc4`（T1–T3）、`0141343`（T4）、`dc22458`（T5）、`7eb83e9`（T6），每条均 `cargo test -p xgent_ui` 全绿。T4/T5/T6 三条严格按任务边界。

39. **交接文档 §4.1 / §4.2 的口径订正**（M5，Verifier 复验轮）：M5 交付前写的 `ui-v8-handoff.md` §4.1「起点事实」与 §4.2「下一期的任务范围（台账 §M5）」在 M5 交付后**过时且会误导下一会话**——§4.2 的 M5-T5 写的是「拖拽排序 + 面板宽度持久化」（与台账 §M5-T5「分割模型」不是一回事，见勘误 2 的判定），且 §4.1/§4.2 被插在 §5 之后造成节序错乱。已改写为：**§4.0 = M5 落点速查**（新增）、**§4.1 = M6 的起点事实**、**§4.2 = 下一期的任务范围（台账 §M6）**（六行 M6 任务表）、**§5 = 下一步 M6**（原 §4.3 顺带记 M3 的 12 个长期失败测试 → §5.1），节序恢复为 §4 → §5 → §6 → §7 → §8 → §9。

40. **M7-T4 的 `debug_assert!` 移除**（M7）：原文要求 `single()` + `debug_assert` + `tracing::warn!`。headless 测试下部分实体不存在（如 `update_input_scroll_to_caret` 查询 `ChatInputMarker`），`debug_assert!` 会在 debug/test profile panic，故只保留 `tracing::warn!`。影响 `chat_panel.rs`（20 处）与 `top_bar.rs`（2 处）。

41. **headless 下 UI 视口为零**（M7-T5）：`layout_geometry.rs` 的 `ComputedNode.size()` 在无真实窗口时返回零向量，因此涉及绝对尺寸的断言改为结构性质断言（`Node.flex_grow` / `Node.height` 的 `Val`），而非像素值。

42. **`AbortMessage` 必须注册**（M7-T4）：删除 `ChatPanelEntities` 后 `handle_abort` 仍写 `MessageWriter<AbortMessage>`；用 `ChatPanelPlugin` 的测试 App 未注册该消息会 "Message not initialized" panic。

43. **v7 状态勘误**（M7-T6）：`docs/plans/README.md` 原标 v7 为「待实施」，实际 v7 已完成并被 v8 交互架构层重建取代，已改为「已完成（已被 v8 取代）」。

44. **M6 归档的「6/6 通过」只覆盖模型层**（2026-10-28 复核）：M6 实现提交是 `9ac4daf`（模型层）+ `81d3060` 里的 M6-T5。代码实测：`Transcript` 只有 `chat_panel.rs:118` 的 `init_resource`；`spawn_markdown`、`completion`、`context_set` 全仓零消费者；`context_scope.rs` 未动。归档验证记录自身写明 `No Runtime checks were recorded` 与四条 Known limitation（`docs/comet/archive/2026-10-26-v8-m6-transcript-markdown/verification.md:30-41`）。台账据此把 M6 改判为「模型层交付」。

45. **M6.5 立项**（2026-10-28，用户裁决）：M6 降级后新增接线期，任务见 §M6.5；同日决定把 mac/Linux 硬门槛 A 排在 Windows 收尾之后，不再无限期挂着。

46. **M7-A1 不是编译期保障**（2026-10-28 复核）：`HotkeyAction.handler_name` 是 `&'static str`（`shortcuts.rs:35`），`assert_hotkey_consistency` 只比较注册表与清单的 id 集合（`shortcuts.rs:157-171`）；分发表缺分支不会在编译期或启动期失败。验证记录里「注册即必填函数指针」的措辞与代码不符。

47. **文档计数与 i18n 守卫口径订正**（2026-10-28 复核）：实测 19 个 crate、`crates/**/*.rs` 共 68322 行（AGENTS.md 原写 13 crate / 约 5 万行，dev-tutorial 原写约 22k 行）；`i18n_full_corpus.rs` 只校验 `.ftl` 语料的键集与 CJK，不是「全模块源码扫描」。

48. **M6.5 交付口径**（2026-10-29）：`b962f97` 把 M6 模型接入产品——Transcript 成为顺序权威（用户/助手分段/工具卡/通知全量渲染）、Markdown 终稿、@ 补全、ContextSet+sidecar、消息列表虚拟窗口。自动化证据：`transcript_write` 7 项、`transcript_render_contract` 5 项（顺序/Markdown/工具卡/通知/200 条消息只渲染窗口）、pinned context 合并、i18n 语料 3 项；`xgent_ui` 全套 + `xui` + `xgent_terminal` 全绿。**未闭环**：真实窗口截图/走查待用户完成（A9 的可视部分）；虚拟窗口在「可见区与当前轮不相邻」时会塌掉中间间隙（已知限制）；@ 补全只做文件候选。

49. **实机启动暴露的三个既有缺陷**（2026-10-29，`5dde747`）：① `XgentUiPlugin` 中 `LayoutPlugin`/`OverlayPlugin`/`FilePanelPlugin` 会先自加 `PanelsPlugin`，随后元组再次无条件添加 → `plugin was already added` panic；改为在 `XgentUiPlugin` 开头判空预加 Panels/Visibility/Tabs。② `TabsPlugin` 把 `handle_slot_tab_click` 等同时挂进 `PanelSet::Requests` 与 `PanelSet::Visibility`，而两个集合有 before/after 链 → schedule 初始化报循环；改为只把纯视图系统挂 Visibility。③ `context_scope` 的 `ContextSetRemove` 未 `add_message` → 运行时 `Message not initialized`。三者都不是 M6.5 引入，而是之前只在 headless 测试中未暴露。

50. **代码量更新**（2026-10-29）：`crates/**/*.rs` 实测 70002 行（M6.5 接线后）。

51. **M6.5 只读复审修复 20 条**（2026-10-29，`/comet-review` 后，提交 `e485d5e`）：1 CRITICAL + 7 IMPORTANT + 8 WARNING + 4 SUGGESTION 全部处理。要点：① 中文输入法组合期会 panic——`EditableText::text()` 排除 IME preedit 而 `raw_selection()` 索引含 preedit，`detect_at_trigger` 无上界检查直接 `bytes[cursor-1]`；已加上界 + `clamp_index`（落字符边界）+ 组合期不接管。② `@` 浮层在 Esc/点击/接受后不销毁（`suppress_text` 分支只 deactivate 不清签名）；带修饰键的 Enter 被误判为接受候选，导致 Ctrl+Enter 发送被吞。③ `record_*` 在无当前轮时提前 return 不排空 reader，流式中「新建会话」后迟到的 Delta/Done 会重放到下一轮（Done 还会提前终稿）。④ 运行中引导（steering）文本不再渲染（旧的 `spawn_user_message` 无状态守卫）；未知 `tool_call_id` 建孤立结果块（Spec 要求 warn 并忽略）。⑤ denied 被折叠成 failed、progress 入模型不渲染、空助手分段渲染成空气泡。⑥ 虚拟窗口两处几何缺陷：可见区与当前轮不相邻时中间间隙塌陷（现按连续段补占位）；`ComputedNode::size`/`ScrollPosition` 是物理像素却与逻辑偏移表直接比较（HiDPI 下顶部可见行漏渲染，补全浮层锚点同理）。⑦ Markdown：段落内粗体/链接断行（现收进 `Inline` 组同行渲染）、列表项/标题内联元素丢内容、嵌套列表丢外层项、终稿段落用固定 `type_scale::BODY` 与流式档位不一致（Done 瞬间跳字号/行高，`mono_typography` 守护已扩到 `markdown_render.rs`）。⑧ pinned 上下文用绝对路径而检索片段是相对项目根 → 去重永不命中（同文件注入两遍）且与 `format.rs` 注释的「用户显式项优先」相反；显式项还绕过 `retrieval_max_tokens`（打开大文件即撑爆请求）。现同形 + 显式项优先 + 剩余预算内截断留痕。⑨ sidecar 写入失败与损坏文件静默；已告警并补磁盘往返 + 旧数组格式 + 损坏回退测试。**勘误 48 与本文件 §7 风险 10 里「中间间隙塌陷」的已知限制已由本条闭合**。验证：`xgent_ui` lib 135 + 24 个测试 target 全绿；`xgent_agent` 77 passed（唯一失败是 §2 台账基线里的既有并发 flake）；`cargo check --workspace` 绿；真实进程 12s 零 panic（`target/app-run10.log`）。未闭环不变：真实窗口截图/走查（含中文输入法与 125%/150% 缩放的实测）、@ 只做文件候选。代码量随之为 `crates/**/*.rs` 实测 71276 行（订正勘误 50）。

52. **M1 遗留的「全局点击无响应」缺陷**（2026-10-29 用户实机走查发现）：`require_markers = true` 之后，bevy 的 UI 拾取后端在 `ui_picking` 的相机过滤里要求**相机自带 `UiPickingCamera`**（`bevy_ui/src/picking_backend.rs:122`：`(!settings.require_markers || *cam_can_pick) && ...`），而 `layout.rs` 只 spawn 了 `Camera2d`，全仓没有任何地方加过这个标记 → 相机被跳过 → **每个指针的命中集合恒空** → 所有按钮/输入框点不动、hover 反馈也没有。M1 门槛 B 只盯「节点缺 `Pickable`」这一半，节点侧确实由 `attach_pickable_to_interactive` 补齐（实测 35 个 `Pickable`），所以白名单测试与 headless 契约测试全绿，缺陷一直潜伏到第一次真机点击。诊断证据（临时插桩，已清理）：注入点击时 `CursorMoved`/`MouseButtonInput` 均送达（`press_events` 随点击递增、`ButtonInput<MouseButton>` 可见按下）、`fps=60`、`pickable_entities=35`，但 `Pressed`/`Hovered` 实体数恒为 0 → 问题在拾取计算，不在输入或渲染。修复：`layout.rs` 的相机改为 `spawn((Camera2d, UiPickingCamera))`；回归测试 `layout::tests::layout_camera_carries_ui_picking_camera` 锁组件契约（去掉标记即红，已做变异反证）。端到端复验（真实窗口注入点击 + 截图像素差，基线噪声 0.04%）：齿轮 99.94%、第二次 99.93%（开→关）、文件图标 91.30%/91.45%、qa chip 0.65%。**方法论（下次别重走）**：① 截图判定依赖显示器未休眠——显示器断电时 `CopyFromScreen` 全域报「参数无效」，会被误读成「UI 无响应」；② `SendInput` 注入鼠标按键在本机不可靠（返回 accepted 但应用侧 `press_events=0`），改用 legacy `mouse_event(LEFTDOWN/LEFTUP)` + `SetCursorPos` 才送达——红信号必须先验证「注入是否送达」再判定应用有无响应，否则测的是注入器不是产品。**新发现待排**：tooltip 渲染在窗口左上角而非目标旁（点击修好后 hover 才可见，故此前无从发现）。

53. **`xgent_agent` 测试与异步 bridge 赛跑的缺陷族**（2026-10-29）：`bridge_tests` 用「跑固定帧数再断言」等异步任务推进，而一帧只有微秒级——机器负载稍高，tokio 上的 bridge 任务还没被调度，帧预算就烧完了。实测复现率：`max_parallel_tools_written_to_handle_bounds_next_turn` **6/6 稳定红**（§2 基线记的「并发时序偶发」不准确，真因是 `drive` 的 300 帧 ≈1.2s 小于「串行 4 个探针 × 400ms 等待」的 1.6s，第 4 个结果还没到）；`abort_interrupts_confirming_state`、`error_message_propagates`、`compaction_syncs_conv_messages`、`approved_read_tools_actually_overlap` 各约 1/6 flake。修复：① 新增 `drive_within`/`drive_until`（有界轮询到条件成立，预算用尽仍由原断言判红，不掩盖真实缺陷）；② `drive` 的预算从帧数改墙钟 15s；③ `ProbeState::overlapping()` 给「断言必须真重叠」的四个用例 2.5s 等待预算（串行/钳制型用例保持 400ms，它们要等的就是超时）。复验：单跑 8/8 全绿（78 passed），全 workspace 并发负载下 `bridge_tests` 亦全绿；套件耗时 1.9s→5.1s。**剩余基线红**（截至本次）：`xgent_plugin_host` 6（git 插件 WASM 缺 `preview-diff` 导出）、`xgent_daemon::config_store::global_write_lands_inside_the_isolated_dir` 1（并行测试共享同一临时配置文件）、`xgent_settings_core::paths::daemon_socket_env_override` 1（Windows 下期望 named pipe 却断言 `/tmp` 路径）。

54. **`cargo test --workspace` 从 15 红到 0 红**（2026-10-29，用户要求「分析其他存在的 bug 并修复」）：
    ① **随包 git 插件 wasm 与 WIT 契约脱节**——本机 `crates/xgent_app/assets/plugins/git/extension.wasm` 还是 8 月 5 日的产物，缺 `preview-diff` 导出（该导出后加进 `plugin.wit:67`），导致 `xgent_plugin_host` 6 个测试红 + 应用每次启动打一条 `加载插件失败 plugin=git` WARN。`./build_plugins.sh release` 重建即修（15s，178KB）。注意该 wasm **被 `.gitignore` 排除**（只入库 `plugin.toml`），所以过期发生在每台机器/每次打包上——这正是 ② 更要紧的原因。
    ② **真正的产品缺陷是内建插件永不更新**：`load_builtin_plugins` 判的是 `installed.exists()` → 只装一次，之后随包版本再新也不刷新；用户在 ABI 变更后永久停在旧 wasm 上，且没有任何自愈路径（删目录才行）。现改为**以随包内容为准**：`builtin_needs_refresh` 先比长度再比字节，不一致就清理重拷；dev 模式的 symlink 目录不参与（源码说话）。单测 `builtin_refresh_detects_stale_asset` 覆盖「未装/长度不同/等长异字节/一致」四态；实测第一次启动打「刷新内建插件」且加载失败归零，第二次启动不再重拷（无 5MB churn）。
    ③ **daemon 配置测试并行互踩**：全局配置路径进程级唯一，15 个用例并行写同一份 `config.toml`，`global_write_lands_inside_the_isolated_dir` 会读到别的测试的磁盘状态（`left: None`）。`isolate_agent_dir()` 现返回 `(&Path, MutexGuard)`，把写盘串行到用例结束（每用例只调一次——std Mutex 不可重入）。
    ④ **`paths::daemon_socket_env_override` 的断言没分平台**：Windows 走命名管道且刻意不读 `XGENT_AGENT_DIR`（管道名必须固定），测试却无条件断言 `/tmp/.../daemon.sock`。改为 `cfg` 分叉断言。
    ⑤ **§2 基线表里的 `xgent_tools::run_command` 6 项已不复现**（本轮三次全量跑均 ok），该行是过期记录，一并订正。
    教训：**长期容忍「基线红」会让真缺陷混在里面**——`git_plugin` 那 6 个红本身就是随包资产过期的报警器，被记成基线后没人再看；而 `preview-diff` 的 WIT 变更其实早该被这三个测试拦住。**改 WIT/插件 API 后必须重跑 `./build_plugins.sh`**——资产不入库，打包与 CI 都得先跑它，否则每台旧机器都会停在过期 wasm 上（dev-tutorial §10 已补）。验证：`cargo test --workspace --no-fail-fast` = 63 targets ok / 0 红；`xgent_plugin` 18 单测绿；应用两次启动零 panic、零插件加载失败。

## 附：文档树变更（2026-10-16，已完成）

原 `doc/` 与 Comet 专用的 `docs/` 已合并为单一 `docs/` 树：`docs/` 现含 `comet/`、`design/`、`decisions/`、`notes/`、`plans/`、`tasks/`、`README.md`、`dev-tutorial.md`、`conversation-flow-fixes-*.md`，`doc/` 不再存在。

- 已跟踪文件用 `git mv` 保留历史；`doc/design` 目录级 rename 在 Windows 被句柄占用拒绝，改为逐项移动 27 个子项，无残留。
- 约 318 处 `doc/` 路径引用改写为 `docs/`，覆盖 113 个文件：`AGENTS.md`、`Cargo.toml`、`.agents/skills/design-md/SKILL.md`、35 个 `crates/**` 源文件的文档注释、`docs/**` 内部引用。校验：`crates/**` 中**非注释改动行数 = 0**（无运行时字符串被误改）。
- `docs/comet/archive/**`（22 个历史验收文件）**已回退**——其中记录的是当时真实执行过的命令与产物路径（如 `git status --porcelain doc/`），属不可篡改留痕；`docs/comet/specs/**`（3 个活文档）保留更新，因其引用目标确已在新位置。`.workbuddy/memory/*` 同理未触碰。
- v8 文档按 AGENTS.md §2 归位：审计留 `docs/notes/`（评估报告类），需求简报/设计系统/布局架构入 `docs/design/`，决策记录入 `docs/decisions/0015-…`，任务台账在 `docs/plans/`。
- 全量链接校验：`docs/**/*.md` 断链数 **0**（排除 archive 历史快照）。

原型已用浏览器逐条验证（非目测）：模态 z=400 高于抽屉 z=100；终端底部状态条溢出 0px（`region_fill` 契约生效）；文件树 5733px 内容在 655px 视口内可滚；长名 `scrollWidth 378 > clientWidth 221` 且 `text-overflow:ellipsis` 实际裁切；注册表探针 4→5 页签并带「注册表」标记；死键在 v8 模式标注「M7 补齐 handler」、在复现模式划线；控制台零消息。

## 附：原型复审（2026-10-16，已完成）

> 范围：`docs/design/ui-prototype-v8.html`（1125 行静态+grep 复核，非目测）。上一节逐条验证验证的是「v8 演示行为是否符合设计」；本轮复审补的是「原型脚本与复现演示自身的缺陷」，两者互斥不重叠。逐条验证记录的演示行为（抽屉可关、对比层、样式检查、浏览器控制台零报错）仍然是有效的证据——本轮全部 10 组缺陷都位于一次性演示路径（旧字形代码、`[完成]` ANSI 残留、`bad2` 类、treeData 死代码、双参 `$`/`$$` 尾逗号调用）与 v8 文案承诺的键位导航里，不影响该轮已验证的 v8 正确性展示面。

### 复审发现并修复的 10 组缺陷

行号约定：**原** = 缺陷在原 1125 行底稿中的位置（历史存档，行号不再随修复漂移）；**现** = 修复后定型文件（1151 行，2026-10-16 复测基准）中的位置。

1. **`const DESC` 重赋值**（原 :697 声明 / 原 :714 页签 ✕ 处理器内 `DESC=DESC.filter(...)`）→ 点击任一可关页签抛 `TypeError: Assignment to constant variable`，页签与注册探针的可关性全面失效。现改为 `let DESC=[...]` 声明（现 :698），✕ 处理器重赋值合法化（现 :716）并在关闭时同步清理 `S.registered`（现 :716）、隐藏 rail 对应项（现 :717）、`S.active` 回落到 order 最小的剩余 side 页签（现 :718），页签与页面区同步重渲染（现 :719）。
2. **停止按钮失效**（原停止按钮与流式完成分支调 `clearInterval(window.__iv)`，而 `startStream` 句柄是局部 `const iv` 从未暴露）→ 流式无法中断。现 `stopStream()` 统一收口（现 :957，句柄存入 `S.streamIv` 现 :980），发送按钮停止分支与 document Esc 流式分支都改走它，完成 toast 只在 interval 回调内触发、停止后不再出现。
3. **@ 补全误发送**（原 keydown 发送分支先于补全分支）→ 补全菜单打开时按 Enter 直接发送。现 `#input.onkeydown` 将补全分支排到发送分支之前并在命中后 return（现 :1052-1059），Enter 由 `acceptComp()` 采纳补全项（现 :1062：替换 `@query` 片段、更新字符计数、不发送），Esc 关菜单并 stopPropagation 阻断 document 层（现 :1056）；菜单关闭后 Enter 仍走原发送路径。
4. **命令面板无键盘导航**：现 `#pq` 增加 ↑/↓/Enter 处理（现 :1041-1050：`.pitem` 间移动 `.sel`、`scrollIntoView(block:'nearest')`、Enter 触发选中项 click），与键盘优先定位目标对齐；Esc 关面板继续走既有模态栈入口。
5. **C2 复现接而不通**：CSS `.mlist.bad2{...}`（现 :293-294）与 `::after` 警告条早已存在但无任何 JS 赋类。现 `syncToggles()` 里 `$('#mlist').classList.toggle('bad2',S.bad)`（现 :874），模态打开与开关切换两条路径均生效，bad=on 列表滚不动 + 警告条出现，off 双向恢复。
6. **C3 复现选择器失配**：CSS 原 `.tree li.bad .lb` 期望类挂 `li`，实际生成 `<span class="lb ${S.bad?'bad':''}">`（现 :825 普通行 / 现 :827 mono 行）。现选择器 `.tree .lb.bad{overflow:visible;text-overflow:clip}`（现 :212），中段省略仅 `if(!S.bad)` 时执行（现 :831）—— bad 模式真实溢出无省略，good 模式尾部 `…` 与 mono 中段截断保留。
7. **D2 仅存在于文案**（原 :1006 toast 与 :646 note 说「终端底部被裁 38px」但无任何 DOM 呈现）。现补 `.v8.bad .term{flex:0 0 auto;height:100%}`（现 :226）真实复现底条被裁（`.term-foot` 现 :239 被 flex 契约推出底帧、祖先 `overflow:hidden` 裁掉），并新增 pinned 警告条呈现：`PAGE.terminal` 包裹容器加 `termwrap` 类（现 :779），`.v8.bad .termwrap{position:relative}`（现 :225）+ `.v8.bad .termwrap::after`（现 :227-228）与 C1 警告条（现 :221-222）同构 —— 警告条固定衬在页容器底缘、随开关出现/消失，不再藏在滚动内容尾部。同轮文案对齐：tBad toast（现 :1006）与「缺陷复现」note（现 :647）逐项点名实际可复现缺陷 ID（C1/C2/C3/D2；E2 由流式演示完成分支承担，现 :977）。
8. **git 徽标样式失配**：原 `.git.*` 选择器被 scoped 在 `.tree` 内，实体徽标在 `.srcline`（PAGE.git 的 `#gitList`，现 :784-794）里 → 全部无样式。现 `.git/.git.M/.git.A,.git.U/.git.D` 提升为全局类选择器（现 :214-217），M/A/U/D 徽标黄/绿/绿/红四色可辨；类名与徽标字符未动。
9. **终端 ESC 只清首处**（原 `l.replace('\x1b','')` 只清首个控制符；现 :930 的 TERM 行数据含两处 `\x1b`）→ `[完成]` 残留 ANSI 残片。现 `/\x1b/g` 全量替换（现 :933-934 两条分支）。
10. **死代码/死 CSS 清零**（均为原文件引用，已删除）：`treeData()`（原 :815，从未调用）、`S.estCell`（原 :936，未定义未使用）、`window.__iv`、`#sp` 追加 `.slice(0,0)`（原 :976）、`t.style.color=''` 无操作（原 :975）、双参 `$`/`$$` 调用（`('#sp',R)`/`$('#stTool',R)`/`$$('.lb.mono',)` 尾逗号）、死 CSS `.tree .lb.mid`、`.broken{}/.broken .hint{}/.v8.bad .broken .hint{}`、`.term .cw`；本轮 D2 警告条固定化后（`.v8.bad .term::after` 为旧选择器，一并归零）再次 grep 复核零命中。

### 复测口径

- 静态判定（PowerShell 模式断言）：**22 组必现模式**命中（`let DESC=[`、`.tree .lb.bad{`、pinned D2 三规则 `.v8.bad .termwrap*/.v8.bad .term{`、`.git.*` 四色、`function stopStream()`/`S.streamIv=iv`、`function acceptComp()`/`c.dataset.query`、`#pq onkeydown`/`scrollIntoView`、`toggle('bad2',S.bad)`、`replace(/\x1b/g`、toast 与 note 点名 `E2`、附段识别）+ **15 组死代码/失配模式**（含旧 `.v8.bad .term::after` 选择器）零命中 → A1-A9 的代码侧均可重现；A10 即本附段本身。
- 静态脚本 `node --check`：原型内联 `<script>` 块语法检查。
- 行为项（关页签回落、停止中断流式、补全采纳、面板导航、D2/C2/C3 视觉、bad2 警告条开关、E2 流式顺序对照）注明浏览器自查边界：v8 / bad 双模式整页走查 + 控制台零报错；D2 警告条为固定位置呈现，无需滚到内容尾部即可见。

## 附：模型对话框重设计（2026-10-16，已完成）

> 范围：`docs/design/ui-prototype-v8.html`（重设计后 1282 行）。本段为独立变更 `ui-v8-model-dialog-redesign`（Comet Native）的交付记录，与上方两段的缺陷修复不重叠：所有既有页签/复现开关/模态栈行为原样保留。

### 动机与参照

用户需求：对话时「选择模型 + 思考等级」设置收进对话输入框（composer）右下角，重新设计模型对话框，参照当前流行 agent（Claude composer 的「模型 pill + 思考等级」组合、Zed composer 右侧内联下拉、Cursor 的模型下拉）的信息架构，但保留 XGent 自身特色（后续 3D 陪伴特性）。参考仅取交互模式，不复制视觉：全部组件沿用 v7-Linear Tokens（ADR-0014 的色/字号/圆角/间距 CSS 变量），未新增任何色值。

### 结构（交付后定型）

- **入口 pill（`.mpl`，composer `.foot` 行最右）**：常驻显示「当前模型名 · 分隔线 · 思考 lv 徽标 · ▾」，`.inbox` 为 `position:relative` 既有容器、pill 为流内最右元素，视觉落在 composer 右下角；`:hover` 与打开态 `.on`（`--accent-int` 描边 + `--accent-glow` 光晕）。字符计数/发送按钮/`@` 补全/停止流式等既有行为不受影响。
- **popover（`.mpop`，`.inbox` 内 absolute，`right:0;bottom:calc(100% + 6px)` 向上展开）**：与既有 `#comp` 补全菜单同构（BASE 层内浮层、`DROPDOWN 300` 语境），不进模态栈、不显示 scrim；打开时**若模态栈非空则拒绝打开**（toast 提示），保证与 settings/palette/drawer 的 z 分层（`MODAL 400 + depth×8`）永不冲突。
  - 模型列表（`.mdrop`）：12 个典型模型的最小演示集（`MPICK`），搜索即过滤（`#mpq` → `renderMpopList`），「常用 / 其他」分组头（`.mgrp`），当前模型 `✓` 标记（`.ck`），键盘 ↑/↓/Enter（`#mpq` 与 `#input` 在 popover 打开时两条路径都接 `mpMove`，含 `scrollIntoView(block:'nearest')`）与点击采纳（`mpSelect` → 选择后自动收合）。
  - 思考等级（`.mlv > .mseg` segmented control）：off / low / medium / high 四档（OpenAI 系主流档位语言，D-1）；选中即改、pill 徽标与段内 `.on` 同步渲染（`renderMpill`）；Esc/点外关掉后重开保持选择（页内状态）。
  - XGent 3D 陪伴身份区（`.m3d`）：radial-gradient 陪伴形象占位球（既有 accent 变量 + 高光 ::after）+ 联动文案「当前模型与思考等级将同步作用于 3D 陪伴演示（P1 预留）」——仅身份呈现，无新功能面（D-3）。
- **关闭路径全覆盖**：Esc（`document` 段 mpop 分支优先于补全/抽屉/流式，且仅在模态栈为空时可达）、点击 popover/pill 外区域（`document` click 的 `#mpop/#mpill` closest 判断，与 `#comp` 外点关闭同款）、点选模型后自动收合。
- **数据源一致性**：`MPICK`/`MODEL_BASE` 常量为单一数据源，设置面板 200 条 `#mlist` 演示列表改由 `MODEL_BASE[i%MODEL_BASE.length]` 轮转生成（A8：两处模型数据同源）；provider 状态栏与设置面板入口（`provPill/gearBtn` 打开 settings、C2 复现）原样保留。

### 复测口径

- 静态断言（Runtime check `c1`）：**18 组原型必现模式**命中（`.mpl{`/`.mpop{`/`.mseg button.on{`/`.mpop .m3d{` CSS、`id="mpill"`/`id="mpop"`、`function renderMpopList/mpOpen(/mpClose(/mpMove(/mpSelect(`、`const MODEL_BASE=`、pill 锚定 `bottom:calc(100% + 6px)`、`$('#mpill').onclick=mpToggle`、`const MPICK_LV=`、pill 模板 `思考 ${S.pick.lv}`、`XGent 3D 陪伴`、`MODEL_BASE[i%MODEL_BASE.length]`）+ **18 组失配模式零命中**（上一轮全部 15 组禁项 + `gpt-4o-mini-2024-07-18` 旧写死串 + 死字段 `S.mpSel`/`mpItems`）+ **5 组台账识别**（附段标题 / 变更名 / 日期 / `MPICK` / `mseg`）。
- 静态脚本 `node --check`：内联 `<script>` 语法检查（本轮 1282 行实测 exit 0）。
- 行为项（pill 开合、键盘导航、思考等级切换联动、模态互斥、与 settings 并存、C2 复现照旧）为浏览器走查边界，由 Verifier 对照 brief A1-A8 逐项复核。

## 附：原型优化第二轮（2026-10-19，已完成）

> 范围：`docs/design/ui-prototype-v8.html`（优化后 1701 行）。本段为独立变更 `optimize-ui-v8-prototype-r2`（Comet Native）的交付记录，与上方三段不重叠：既有演示（缺陷复现 / 图层检查器 / 密度 / 模态栈 / 模型对话框）与全部既有锚点原样保留。

### 动机与参照

用户需求：继续优化 v8 原型、参照当前成熟 agent 的 UI 与功能设计、并保留 XGent 特色（3D 陪伴）。参照对象（**仅取信息架构，不复制视觉**）：Claude Code 权限模式（default / acceptEdits / plan / dontAsk / bypassPermissions / auto 与 `/permissions` 预批准规则、受保护目录）、opencode 的 allow/ask/deny 规则集、检查点恢复的业界实践、项目内 `docs/notes/oh-my-pi-borrowing-analysis.md`（ApprovalMode 矩阵 P1）。全部组件沿用 v7-Linear Tokens（ADR-0014），未新增色值（`rgba(255,178,36)`=4、`#FFB224`=2 与基线一致）。

### 结构（交付后定型）

- **陪伴（补齐产品既有两处占位并升级）**：rail 底部星标按钮（暖色身份，对齐 `activity_bar.rs` 既有位置；激活环升级为**状态环**——暖色动画语义：空闲静止 / 思考呼吸 / 流式持续 / 工具执行虚线旋转 / 等待确认叠加琥珀点徽标 / 错误红环）；点击开**陪伴浮层**（琥珀形象球 + 当前状态 + 模型·思考等级联动 + 窗口预览入口 + 显示开关）；状态栏补齐「陪伴 · 状态」段（对齐 `status_bar.rs` 口径）；独立陪伴窗口概念预览卡（显式标注 F-15 / K-04 / 概念预览，非交付承诺）；模型 popover `.m3d` 区琥珀化升级（保留 `XGent 3D 陪伴` 锚点）。
- **权限**：顶栏权限模式 pill（询问 / 自动 / 全自动，含与 §5.4 的关系说明、受保护路径说明、「管理预批准规则」入口）；全自动警示（pill 描边 + 状态栏 warn 段）；确认弹窗三档放行（仅此一次 / 本会话允许 / 始终允许·写入配置）+ 拒绝并说明（理由随拒绝传递）+ 受保护路径提示；设置「工具与安全」实表（10 行工具策略可循环切换 + 4 条预批准/预拒绝规则 + 受保护目录），snab 分区切换。
- **运行态与恢复**：good 模式流式路径并入 3 个只读工具**并行**（状态栏「运行中 N 操作」指示）；中断恢复条（重新生成 / 编辑后再发）；回合尾部**撤销入口**（回退本回合文件检查点，含覆盖边界如实说明与已撤销态）；压缩事件卡；上下文用量条。
- **空态与历史**：欢迎空态（品牌 / 问候 / 最近会话；结构对齐 `welcome.rs`——快捷入口保持输入框上方单处，不重复摆放）；历史面板（搜索过滤 + 日期分组 + 当前会话高亮 + T-6 空态文案）。
- **对齐**：rail 补「展开面板」按钮（对齐产品结构）；设计先行项（权限模式会话级覆盖、受保护路径、并行指示、历史检索）均在文案显式标注。

### 复测口径

- 静态断言（Runtime check `c1`）：**48 组原型必现模式**命中（陪伴：`.rbtn.cmp{`/`id="cmpBtn"`/`id="cmpRing"`/`.cmp-badge{`/`id="cmpSeg"`/`id="cmpSegTxt"`/`.cpop{`/`id="cpop"`/`id="cpopToggle"`/`id="cpopWin2"`/`id="cwmockOrb"`/`概念预览`/`F-15`/`K-04`；权限：`id="permPill"`/`id="permPop"`/`data-perm="full"`/`id="permWarnSeg"`/`受保护路径`/`id="freason"`/`data-allow="always"`/`data-deny`/`id="pane-tools"`/`id="toolRows"`/`data-snab="tools"`/`bash: rm -rf*`；运行态：`id="parSlot"`/`id="opsSeg"`/`.recoverbar`/`重新生成`/`id="failCard"`/`回退检查点`/`id="undoRow"`/`撤销此回合`/`覆盖边界`/`R3-11`/`id="ctxbar"`/`id="compactCard"`/`上下文已压缩`；空态：`id="welcome"`/`welcome.rs`/`id="welcomeRecent"`/`id="histSearch"`/`hgrp`/`无匹配会话`；对齐与标记：`id="expBtn"`/`设计先行`/`XGent 3D 陪伴`）+ **21 组既有锚点保全**（round 1 必现组 + 复现开关锚点不回归）+ **10 组禁项零命中**（旧写死串、死字段、死代码、旧选择器 + 孤儿 `.confirm .cf` 规则与无消费者 `data-lvl` 属性）+ 色值计数不增（`rgba(255,178,36` 前缀=4、`#FFB224`=2）；台账识别（本附段标题 / 变更名 / 日期）。
- 静态脚本 `node --check`：内联 `<script>` 语法检查（本轮 1701 行、内联脚本 44736 字符实测 exit 0）。
- 行为项（陪伴六态联动、浮层开关路径、权限切换与全自动警示、弹窗三档放行、设置分区切换、并行 / 恢复 / 撤销 / 压缩、欢迎空态、历史搜索；以及既有演示回归）为浏览器走查边界，由 Verifier 对照 brief A1-A14 逐项复核。

**修复轮记录**：首轮独立验证发现 A1 的「错误红环」被 `.rbtn.cmp.on .ring`（0-4-0）特异性压过（`.v8 .cmp-error .ring` 为 0-3-0，红环不可见）；已把状态环选择器提升为 `.rbtn.cmp.on.cmp-{state} .ring`（0-5-0，错误红环/等待确认点可见），并随修孤儿 `.confirm .cf` 规则与无消费者 `data-lvl` 属性；其余实现未改动，重新走完整验证。

## 附：3D 视口设计探针（2026-10-19，设计先行）

> 范围：新增 `docs/design/ui-v8-3d-viewport.md`；`ui-v8-design-system.md` 增补 §9（3D 令牌边界）与 §10（动效令牌）；`ui-v8-layout-architecture.md` §3.1 增补 `Slot::Center`；`ui-prototype-v8.html` 新增「空间视图」聚焦态。本段不改变 M1–M8 的进度口径：**v8 主体仍未开工**。

### 结论（回答「3D 能否启动」）

| 事项 | 判定 |
|:---|:---|
| 空间语法 / 交互契约 / 视觉规范 | ✅ 已完成（本文档 + 原型） |
| 数据契约 + 确定性布局内核（S0） | ✅ 可立即开工：不依赖 Bevy、不碰 UI、可 headless 测 |
| 视口渲染（S1） | ⏸ 等 M1（层根/拾取）+ M2（主题热驱动）+ G7 性能 spike |
| 交互接入（S2） | ⏸ 等 M5（`Slot::Center` + 同槽单活跃） |
| 时空回溯（S3） | ⏸ 等 S2 + D-3D-07（时间轴数据源） |

已核实前提：bevy 0.20 的 `3d` 是默认特性，`bevy_pbr`/`bevy_core_pipeline` 已在 `Cargo.lock` → **3D 不新增依赖**；全仓 `PanelRegistry`/`ModalStack`/`ScrollFrame`/`MonoMetrics` grep = 0 → 闸门按未通过计。

### 原型结构（交付后定型）

- **入口**：rail 底部组上方的星图按钮（无 `data-act`，不进入页签枚举）；点击进入空间视图，`Esc` 逐级回退（L2→L1→L0→退出）。
- **层**：`#layerSpace`（z 120，位于 DOCK 100 之上、DRAWER 200 之下），内部 `.spwrap` 从顶栏下沿铺到状态栏上沿；聚焦态以 `.v8.space-mode .main{visibility:hidden}` 隐藏主区，**不新建层根**。
- **空间语法**：13 个 crate 星团（半径 ∝ 代码量）+ 85 个文件星体（确定性伪随机分布）；crate 依赖边（亮度渐变表方向）、L1 星团内 `use` 细线；改动文件白环、运行中靛紫脉冲、选中 `accent` 描边；地面仅 L0 三圈参照环。
- **LOD**：`dist > 760` → L0 星系；`> 420` → L1 星团（只保留聚焦星团文件，其余星团降权 45%）；否则 L2 星体。双击星团飞行穿越（320ms cubic），双击星体交棒 2D 编辑器。
- **HUD**：面包屑 + 节点统计；右上 LOD/帧预算/历史视图 chips；选中详情卡（LOC/依赖边/状态 + 打开编辑器/定位对话）；底部时间轴（T-4…T-0，拖动只回放状态、相机不自动移动）；左下「回到对话」；中下操作提示。
- **键盘**：`Tab/Shift+Tab` 遍历当前层节点、`/` 搜索并飞行、`Enter` 默认动作、`+/-` 缩放、`Esc` 逐级回退；画布 `tabindex=0` 接管焦点（否则打开后按 Enter 会重新触发 rail 按钮）。
- **标签**：独立一遍近→远绘制，矩形避让 + 同屏上限 24，全部走 2D 叠加（不建 3D 文本）。

### 同轮修复的既有缺陷

- **抽屉关闭残留 39px**：`.drawer{left:var(--rail-w); transform:translateX(-104%)}` 中 `-104%` 只按自身宽 320 计算（−332.8px），右缘停在 `52−332.8+320 = 39.2px`，关闭态在 base 层左缘露出一条文件树。改为 `translateX(calc(-100% - var(--rail-w)))`，右缘归零。该缺陷在 3D 聚焦态下最显眼（露出一列「2 KB/3 KB…」），普通态下被 rail 掩盖。

### 复测口径

- 静态：内联脚本 `node --check` 通过（本轮 2200+ 行、脚本 62KB+）。
- 行为（Playwright headless Chromium，`target/smoke/smoke.js`，不随仓库提交）：进入空间视图 → canvas 有绘制（非零像素）→ `Tab` 选中星团并出卡 → `Enter` 飞行至 LOD 1 → 时间轴 T-2 出现历史视图徽标与「改动 3 文件」→ 拖拽旋转 → `/` 搜索 `theme.rs` 命中 → `Esc` 逐级退出 → 退出后设置面板仍可打开；**控制台零 error / 零 pageerror**。
- 截图：`target/smoke/space-app.png`（L0）、`space-l1.png`（L1 + 详情卡）。
- 边界：原型用 Canvas 2D 伪 3D 表达契约，**不演示**真实索引、tree-sitter 耦合边、2k 节点性能与离屏纹理；这些属于 S0/S1 验收，不得据原型判定性能达标。
