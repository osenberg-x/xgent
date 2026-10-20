# UI v8（交互架构层重建）落地任务清单

> 依据：需求简报 [ui-v8-requirements-brief.md](../design/ui-v8-requirements-brief.md)、技术设计 [ui-v8-layout-architecture.md](../design/ui-v8-layout-architecture.md)、设计系统 [ui-v8-design-system.md](../design/ui-v8-design-system.md)、证据基线 [ui-v8-audit.md](../notes/ui-v8-audit.md)。
> 视觉蓝本：可交互原型 `docs/design/ui-prototype-v8.html`（本轮产出；若尚未落到 docs/，见文末「交付缺口」）。
> 决策记录：ADR-0015（正文见 [ADR-0015](../decisions/0015-ui-v8-交互架构层重建-保留-linear-视觉基准.md)，定稿后移入 `docs/decisions/`）。
> 状态：**M1 / M2a / M2b 已交付并归档，M3 起未开工**（进度与下一步见 [ui-v8-handoff.md](ui-v8-handoff.md)；下表的勾选在 M1/M2 段落有滞后，以归档 change 与代码实测为准）。任务编号 `M{期}-T{序}`，完成打勾。本文是唯一进度台账；实施中发现与文档不符的事实时，改代码的同时在本文 §10 追加勘误（文档跟随代码）。

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
| A3 | P0 | M4-T2 | 中文 `ls -l` 三平台列对齐 |
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

### M3-T1 `ScrollFrame` 唯一入口（xui）
- [ ] `xui` 暴露 `ScrollFrame::vertical()/horizontal()/stick_bottom()`，`overflow` 由构造器决定、**不接受外部覆盖**（正确形态参照 `scroll_area.rs:104-105`）。
- [ ] 滚动条与视口成套产出；`scroll_to_child`（`scroll_area.rs:12`）保持公开。
- **验收**：单元测试断言构造产物 `overflow.y == OverflowAxis::Scroll`。

### M3-T2 迁移 9 处假滚动
- [ ] `file_panel.rs:186`、`session_history.rs:161,221`、`command_palette.rs:157,201`、`confirm_dialog.rs:221`、`editor/mod.rs:252`、`settings_panel.rs:796`、`tool_panel.rs:211` 全部改用 `ScrollFrame`。
- [ ] `settings_panel.rs:791-800` 模型列表（`max_height:150` 且完全无 `ScrollPosition`）挂 `ScrollFrame::vertical()`。
- **验收**：`grep -rn "Overflow::clip_y()" crates/xgent_ui/src/` = 0；七处各塞 200 条，滚轮与滚动条均可达末条。

### M3-T3 填满契约与自适应输入（修 D2/C4/C5）
- [ ] `region_fill()`（`flex_grow:1 + min_width/height:ZERO`）替换所有「有兄弟节点的列里 `height:100%`」：`editor/mod.rs:184`、`terminal/mod.rs:262`、`diff_view.rs:54`（vs `:668` 的 38px 页签条）。
- [ ] `chat_panel.rs:182-219` 输入框：自适应到 200 后内部滚动 + 光标行可见。
- [ ] `chat_panel.rs:301-339` chips 行改 wrap 容器。
- **验收**：700px 高窗口内终端状态条完整可见（当前永久掉出视口 38px）；长输入不隐藏当前行。

### M3-T4 `xui::Ellipsis` / `Clamp`（修 C3，⚠ 引擎无此能力）
- [ ] 自建测量-二分-追加 `…`；measure 走 `TextPipeline::update_text_layout_info` → `TextLayoutInfo.size`（✅ `bevy_text/src/pipeline.rs:485`）。
- [ ] `EllipsisCache` 按 `(source hash, 可用宽量化 1px, font_size)` 缓存；迟滞 < 2px 不重算。
- [ ] `TextBounds` 仅用于多行 clamp（✅ `bounds.rs:13-27` 官方自陈硬裁不可靠，故不作为单行省略方案）。
- **验收**：100 条不同长度路径改变窗口宽度时不逐帧重排（缓存命中率可观测）；无闪烁。

### M3-T5 接入 6 处截断点（修 E6）
- [ ] `file_panel.rs:349`（路径，用 `MidEllipsis` 保留末两段）、`editor/tabs.rs:509`（tab 标题）、`status_bar.rs:208`（provider 文本）、`command_palette.rs` 行、`tool_panel.rs:380-387`（删 50 字手工截断）、确认弹窗标题。
- [ ] 截断时完整值进 tooltip（依赖 M1-T5）。
- **验收**：`Ellipsis` 消费者 ≥ 6；宽度不足出 `…` 而非溢出/换行；hover 见全值。

### M3-T6 启用 `VirtualList`（修 I4 / R3-6）
- [ ] 会话消息列表与文件树接入 `xui::VirtualList`（140 行，今天**零消费者**）。
- [ ] 不等高行：估算高 + 首帧实测回写的累积偏移表；阈值 > 50 条才启用。
- **验收**：500 条会话滚动帧时间不明显劣化；消费者 ≥ 2。

---

## M4 跨平台字体与终端双宽（修 A1/A3，⚠ 硬门槛 A）

### M4-T1 字体装配层（三平台一阶）
- [ ] 内嵌 **JetBrains Mono**（OFL 1.1）到 `crates/xgent_app/assets/fonts/`，与 `Inter-Variable.ttf` 同目录（今天只有 Inter，✅ 已核实）。
- [ ] `FontSpec/FontCandidate` 候选栈数据表：`mono` 末条必须为内嵌资产；`ui_cjk`/`mono_cjk` 走系统发现（`system_font_discovery` 已启用，根 `Cargo.toml:7`）。
- [ ] **删除** `startup.rs:88` 的 `else { Handle::default() }` 分支（这是 A1 的直接病灶）；`fonts.rs:19-20` 注释同步。
- [ ] `tracing::info!` 输出实际命中家族与来源（C-FONT-2）。
- **验收（硬门槛 A）**：三平台各启动一次，日志显示 `mono` 解析到真实等宽字体（非 Inter）；不过则停止铺开并升级处理。

### M4-T2 终端双宽单元格（⚠ 最大改动面）
- [ ] `MonoMetrics { latin_advance, cjk_advance, line_height, at_size }` 启动期实测；校验 `cjk_advance ≈ 2×latin_advance`，不满足记警告并调基准（C-FONT-4）。
- [ ] `display_width(char) -> usize`（wcwidth 语义：CJK 全角 / emoji / U+FF00–FF60 → 2）。
- [ ] 终端排版改为按格数 × `cell_w` **手工定位**，不再依赖 shaper 连续排版；`terminal/output.rs:380` 的 `font × 0.6` 估算随之删除。
- [ ] 保留旧路径在 feature flag 下做 A/B 截图对照，通过后再删。
- **验收**：三平台跑 `ls -l` / `cargo test` 中文输出 / `top`，列对齐正确；`vim` 全屏 TUI 不乱行。

### M4-T3 代码与行号消费 `mono`
- [ ] 行号、diff 行、代码块、chips 中代码全部走 `mono_text`；禁止用 `ui_text` 冒充等宽。
- [ ] `type_scale` 新增 `CODE` 档、`line_height::CJK = 1.7`；`SMALL 12.5 → 12`（消除半像素模糊）。
- **验收**：`theme.rs` 测试更新令牌断言；行号列与代码列在 125%/150% 缩放下仍对齐。

### M4-T4 中文排版约束
- [ ] 展示级负字距只作用拉丁段，中文不挤（设计系统 1.4）。
- [ ] 换行走已落地的 CJK 分词（R3-8），禁整句单 token。
- **验收**：中英混排标题与正文无豆腐块、无异常挤靠。

---

## M5 槽位注册表与派生可见性（修 D1–D6、I3）

### M5-T1 `PanelRegistry` 取代 closed 枚举（⚠）
- [ ] `Slot{Rail,SideDock,BottomDock,SettingsSection,StatusSegment,Palette,Overlay}` + `PanelDescriptor`（`title_key` 必填 i18n key）+ `PanelRuntime`。
- [ ] `PanelRegister/PanelActivate/PanelDeactivate` 消息（遵守 ECS 契约，禁直接方法调用）。
- [ ] `SideViewContent`（`editor/mod.rs:69-81`）加**临时 shim**（枚举 → 描述符一次性映射），使后续可分小批合入且每批可编译。
- **验收**：新增一个 `SideDock` 面板无需修改 `layout.rs`/`editor/mod.rs` 的枚举与页签条代码。

### M5-T2 rail 与历史进槽位（修 D4/D5）
- [ ] 删除 `ActiveActivity`，选中态从 `registry ∩ slots.is_visible()` 派生。
- [ ] `ActivityKind{Chat,Files,History,Terminal}`（`activity_bar.rs:20-30,251-306`）改为描述符驱动；`session_history.rs:136-251` 从浮层改为可注册面板。
- **验收**：连点 Files 两次关掉抽屉后选中态回落正确；历史面板与文件抽屉不再争层级。

### M5-T3 派生可见性单一系统（修 I3/D2）
- [ ] `derive_panel_visibility` 成为唯一写 `Node.display` 的系统；删除 `layout.rs:199`、`file_panel.rs:199`、`editor/mod.rs:342`、`terminal/mod.rs:475`、`welcome.rs:178` 五处各自写手。
- **验收**：`grep -rn "\.display = " crates/xgent_ui/src/` 命中仅限该系统与层根构造。

### M5-T4 单页签条所有者（修 D3）
- [ ] 页签条由 `SlotLayout` 拥有并渲染；编辑器内部 tab 条降级为「文档列表」语义（与槽位页签不是一回事，不合并）。
- [ ] 合并两套 `EditorBackButtonMarker`（`editor/mod.rs:220-239` 与 `:706-731`）。
- **验收**：侧区顶部只有一条页签条、只有一个返回入口。

### M5-T5 分割模型（修 D6）
- [ ] `SplitTree/Split/LeafSize` + `resize.rs` 从单字段（`resize.rs:47-50,205`）改为遍历生成分割条。
- [ ] `DRAWER_W` 从定值 320（`theme.rs:236`）改为 `Min(240, Preferred(320))`、上限 480。
- **验收**：抽屉可拖宽；可注册纵向分割而无需改 `resize.rs`。

### M5-T6 移除 `SideViewContent` 枚举
- [ ] shim 拆除，所有既有面板（editor/preview/diff/terminal）为原生注册项。
- [ ] `i18n`：页签标题取 `title_key`，删除 `editor/mod.rs:659-664` 的 `let _ = loc;`。
- **验收**：`grep -rn "enum SideViewContent" crates/` 无命中；英文语料下页签为英文。

---

## M6 会话内容与输入（修 E1–E5）

### M6-T1 `Transcript` 有序权威（修 E2）
- [ ] `Transcript(Vec<TurnRef>)` + `BlockRef` 块序列；渲染按顺序 spawn/despawn/update，顺序键 `(turn_index, block_index)`。
- [ ] 删除命令式挪子节点：`chat_panel.rs:489`、`tool_panel.rs:85`、`chat_panel.rs:724`、`undo.rs:286`。
- **验收**：边流式边插工具卡，顺序稳定不倒退（自动化断言序列单调）。

### M6-T2 Markdown 块模型与依赖评审
- [ ] 决定解析器：优先 **pulldown-cmark**（纯 Rust、无 Bevy 依赖），走一次依赖评审；退化方案为仅代码块 + 列表 + 粗体。
- [ ] `MdBlock/Inline` 模型定义。
- **验收**：评审记录落 `docs/notes/`；解析器单测覆盖六类元素。

### M6-T3 富文本渲染（修 E1 / R3-5）
- [ ] 单节点多 `TextSection` 承载行内样式（✅ `bevy_text/src/picking_backend.rs:219` 的 section 级文本存在）。
- [ ] 代码块：`surface_code` 底 + `mono` + 语言标签 + 复制按钮（复制取定稿全文）；语法色接已落地的 `xui` 高亮链。
- [ ] 增量渲染（依赖 M6-T1），不整段重建。
- **验收**：六类元素混合样张视觉正确；对照原型截图。

### M6-T4 `@` 内联补全（修 E3）
- [ ] 浮层挂 `DROPDOWN` 层（**不能**留输入框子树，否则被 `ChatPanel` 的 `clip` 裁）。
- [ ] 方向键导航 + Enter 接受；候选来自文件/符号/`@selection`。
- **验收**：输入 `@` 出现补全并可键盘选择；窄窗不被裁。

### M6-T5 发送/停止动作条（修 E4）
- [ ] 显式按钮：发送（`accent`）、停止（流式中，`st_running` + pulse）；禁用态由 `AgentStatus` 派生。
- [ ] 删除「发送按钮区」空注释（`chat_panel.rs:221`）。
- **验收**：不碰键盘可完成一次完整发送与一次中断。

### M6-T6 `ContextSet` 建模（修 E5）
- [ ] 独立 Resource，可增删、可持久化、显示 token 占比；替换 `context_scope.rs:1-7` 的「打开 tab 只读镜像」。
- **验收**：移除某 chip 后其不再进入下一轮请求。

---

## M7 闭环：快捷键、焦点、i18n、验证层

### M7-T1 快捷键单一来源（修 F1/F2）
- [ ] `HotkeyAction`（`id/label_key/default/when/handler`），`handler` 为函数指针 → 注册即必填，语言层面消灭空实现。
- [ ] 修 `palette.open_files`（`shortcuts.rs:44` 注册、全仓零消费者）与 `input.focus => {}`（`:196` 空实现）。
- [ ] 启动期一致性断言：注册 action 必有 handler、分发表 id 集等于注册表 id 集。
- **验收**：一致性断言对现有 action 全绿；死键数为 0；命令面板显示的键位即真实绑定。

### M7-T2 焦点遍历与模态隔离（修 F3/F4）
- [ ] 装配 `TabNavigationPlugin` + `DirectionalNavigationPlugin` + `InputDispatchPlugin`（✅ `tab_navigation.rs:18` 要求手动 add；`InputFocusPlugin` 已在 `default_plugins.rs:16`）。
- [ ] `TabIndex` 由布局顺序派生（间隔 10），废除全 0；修 `tests/settings_input_focus.rs:154-167` 把错误当不变量的断言。
- [ ] 模态层根挂 `TabGroup::modal()`（✅ `tab_navigation.rs:94`）→ Tab 不跳出模态。
- [ ] 焦点环消费 `theme.accent_interactive` + `accent_glow`（`theme.rs:62-63` 今天零消费），配合 `InputFocusVisible`（✅ `bevy_input_focus/src/lib.rs:182`）只在键盘导航时显形。
- **验收**：设置面板内 Tab 按视觉顺序遍历且可见焦点环；模态内 Tab 不外逃。

### M7-T3 i18n 全覆盖（修 H1/H2/H3）
- [ ] 硬编码点迁移：`activity_bar.rs:130-167`、`top_bar.rs:222,225`、`editor/mod.rs:659-664`、`status_bar.rs:140`（`"$0.00"`）、`chat_panel.rs:385`（`"▼"`）。
- [ ] `xui` 的 `StringSource` 真正被消费（R3-7/D5）：命令面板、虚拟列表、滚动条 aria 与空态文本。
- [ ] 扫描从「只扫 settings」扩为全模块 + EN 无 CJK 码位 + zh 无缺失 key，纳入 CI。
- **验收**：英文语料下全界面无可读中文残留（白名单显式）；中文界面文案正确。

### M7-T4 状态健壮性（修 I1）
- [ ] 删 `ChatPanelEntities` 这类把 `Entity` 缓存进 Resource 的做法（`chat_panel.rs:85-92`），改 marker 查询。
- [ ] `q.single_mut()` 静默早退（`chat_panel.rs:796,889`、`top_bar.rs:293`）改 `single()` + `debug_assert` + `tracing::warn!`。
- **验收**：布局重建后无静默失效；数量异常有日志。

### M7-T5 布局几何断言层（修 I2）
- [ ] 新建 `crates/xgent_ui/tests/layout_geometry.rs`：`assert_fills_region` / `assert_no_overflow` / `assert_scrollable` / `assert_truncated_in_width` / `assert_pickable_leaf` / `assert_no_blocking_empty_layer`（架构 §9）。
- [ ] 覆盖当前完全无测模块：chat、palette、confirm、status、topbar、resize、滚动、重叠。
- [ ] 每条 P0 缺陷配一条"先红后绿"回归（≥ 9 条）。
- **验收**：`cargo test --workspace` 全绿；故意引入 `clip_y` 假滚动会被测试抓住。

### M7-T6 文档同步与勘误
- [ ] `docs/dev-tutorial.md`：已实现功能总览、crate 拓扑、ADR 落地点、开发注意点（AGENTS.md §6.8 硬要求）。
- [ ] `docs/plans/README.md:85` 把 v7 从「待实施」勘误为已完成。
- [ ] `docs/design/ui-design.md` 标注被 v8 取代的章节；`ui-bsn-refactor-*` 顶部加「已放弃，见 ui-v8-audit.md 第七节」。
- [x] ADR-0015 已归位 `docs/decisions/0015-ui-v8-交互架构层重建-保留-linear-视觉基准.md`（状态：草案，待确认转「接受」）。
- [ ] 本文追加勘误条目（§10）记录与文档不符的事实。

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
| M6 会话内容 | `Transcript`、Markdown、输入区 | E1–E5 | 依赖 M3/M4/M5 |
| M7 闭环与验证 | 快捷键单源、焦点栈、i18n 扩面、几何断言 | F1–F4 H1–H3 I1 I2 | — |
| M8 Git 探针 | 1 注册 + 1 视图模块落地 | 验证 U-05/U-12 | 北极星 |

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
