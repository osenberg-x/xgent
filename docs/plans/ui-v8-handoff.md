# UI v8 交接：进度、基线与下一步

> 用途：跨会话交接。**进度台账以 [ui-v8-tasks.md](ui-v8-tasks.md) 为准**（任务级勾选），本文记录「到哪儿了、基线是什么、下一步怎么开工、有哪些坑」。
> 更新时间：2026-10-21（M4-T2 交付后）。

---

## 0. 一句话现状

UI v8 的 **M1（分层/拾取/模态）、M2a（主题权威/令牌收敛）、M2b（密度档位/hover 量化）、M3（滚动契约/填满契约/省略原语/虚拟列表）、M4-T1（字体候选栈与等宽内嵌兜底）、M4-T2（终端双宽单元格与按格定位）、M4-T3/T4（代码类文本消费 `mono` + 中文排版约束）已交付**；`cargo check --workspace` 绿，`xgent_ui` lib 84 项 + `xui` 103 项测试全绿。**M4 已收口**，下一期是 **M5（槽位注册表与派生可见性，⚠ 最大改动面）**。

> ⚠ 唯一未闭环项：**硬门槛 A 的 macOS / Linux 的 `mono` 启动日志确认**（用户 2026-10-21 决定降级为不阻塞，先做 Windows 适配，其他平台后续适配）。详见 §1.1。

---

## 1. 已交付里程碑

| 期 | 交付形式 | 提交 | 关键落点 |
|:---|:---|:---|:---|
| M1 分层与模态 | Comet change `v8-m1-layering` | 归档 `f5fdb87` / 实现 `e7834fe` | `theme::z` 六静态层根 + `layout.rs` 层根 + `kit::layer_root`；`picking.rs` 与 `UiPickingSettings{require_markers:true}`；`modal.rs` 的 `ModalStack/Dismiss/ModalPush/Pop` 与栈深 z；`overlay.rs` 的 `OverlayRegistry` + `DrawerToggle`；tooltip 挂 TOOLTIP 层 |
| M2a 主题与令牌 | Comet change `v8-m2-tokens` | 归档 `7f67dc1` / 实现 `141d05c` | `ThemeHandle{theme,generation}` 唯一权威 + `Themed` 重绘；`Theme::dark()` 降 `pub(crate)`；`px()` 唯一入口；`theme::size` 尺寸令牌；星形/`Color::WHITE` 入 theme；窗口 1280×800 |
| M2b 密度与 hover | Comet change `v8-m2b-density-hover` | 归档 `9bb29c9` / 实现 `6d83d18` | `density.rs`（`DensityBase` 基线写回，幂等）；`Preferences.density` + `ThemeHandle.density`；设置面板密度分段按钮 + `SaveDensityMessage` + daemon 白名单；`kit.rs` hover 改为 `Changed<Hovered>/Changed<Pressed>` + `RemovedComponents<Pressed>` 松手帧 + `HoverTintPollStats` |
| M3-T1/T2 滚动契约 | 直接提交 | `19fff0a` | `xui::scroll_frame`（`ScrollFrame` 唯一入口 + overflow 运行时回写 + 视口/滚动条成套 + `scroll_to_child`）；删 `ScrollArea`；修 xui 滚动条两个缺陷（轨道随内容滚走 / 每帧增殖轨道）；`xgent_ui` 9 处 `Overflow::clip_y()` 归零；工具卡结果区重构（文本挪进子节点 + 按 `tool_call_id` 定位） |
| M3-T3 填满契约 | 直接提交 | `3acbb17` | `xui::layout::region_fill()`；分屏三页改 `region_fill`（修 D2 的 38px 顶出）；对话输入区改「外框 + 内层滚动 + 文本节点」三段式 + `visible_lines` 自适应 + 光标行跟随；chips 行改 wrap |
| M3-T4/T5 省略原语 | 直接提交 | `663d1ad` | `xui::ellipsis`（`Ellipsis`/`EllipsisMode::{End,Mid}` + `compute_ellipsis` 纯函数 + `EllipsisCache` + 中段省略）；6 处截断点接入（文件树/页签/状态栏/命令面板/工具卡/确认弹窗） |
| M3-T6 虚拟列表 | 直接提交 | `f5c4e32` | `xui::VirtualList` 支持不等高（measured 行高表 + 累积偏移表 + 实测回写）；文件树改扁平虚拟列表（数据层 `flatten_tree` + 只渲染可见行 + 占位块撑总高 + 数据代次）；会话历史列表虚拟化；**顺带修好 12 个长期失败的既有测试**（见 §4.3） |
| M4-T1 字体装配层 | Comet change `v8-m4-font-assembly` | 归档 `bd18ddd` / 实现 `d314256` | `xgent_ui::fonts`：四槽位模型（`FontSlot`/`FontSpec`/`FontCandidate`）+ 纯函数 `resolve_slot`（系统命中 → 内嵌兜底 → 失败记警告）；`UiFonts` 扩为 `mono/ui_cjk/mono_cjk: FontSource`；内嵌 **JetBrains Mono**（OFL 1.1，264KB）；`startup::load_fonts` 解析四槽位并打「槽位→家族名→来源」日志；**删除** `else { Handle::default() }` 分支 |
| M4-T3/T4 排版消费 | Comet change `v8-m4-mono-typography` | 归档 `7c76366` / 实现 `de2bb68`+`5fa84e6` | 代码类文本走 `mono` 槽位（`EditorTheme.font` 注入 + **span 自带字体** + 行号列同源 + diff 行 `mono_text` + 工具卡摘要 `ellipsis_mono_text`）；`SMALL 12.5 → 12`、`MONO` → `CODE`、新增 `line_height::CJK = 1.7`；中文约束（`display_spacing` 负字距不作用中文、`line_height::for_text` 正文按内容选档） |
| M5 槽位注册表与派生可见性 | Comet change `v8-m5-panel-registry` | 实现 `0aeecc4` / `0141343` / `dc22458` / `7eb83e9` | `xgent_ui::panels`：`Slot`（含 `Center`，只建模不接消费者）/ `PanelDescriptor`（`title_key` 必填）/ `PanelRegistry` / `SlotLayout` + `PanelRegister`/`PanelActivate`/`PanelDeactivate` 消息；`derive_panel_visibility` 唯一写面板可见性；rail 选中态派生（`ActiveActivity` 删除）；会话历史改 `Slot::Overlay` 注册面板；槽位页签条（`panels::tabs`）+ `EditorBackButtonMarker` 合一；`SplitTree`/`Split`/`LeafSize` 分割模型 + 抽屉可拖宽（240–480）；`SideViewContent` 枚举与 shim 全量拆除；`tests/panel_registry_contract.rs` 18 项契约测试 |
| M4-T2 终端双宽单元格 | Comet change `v8-m4-terminal-cells` | 实现 `ff9c203`+`5e1d4cb`+`e06f77d`+`d3a276b`+`0fd632b`+`fe96a82` | `xgent_terminal::render`：`display_width`（wcwidth）+ `display_width_str` + `CellRun` + `layout_cells`（按格布局内核）；`xgent_ui::terminal::metrics`：`MonoMetrics`（启动期**实测**）+ 双宽校验 + `CellGrid`；`terminal/output.rs`：行渲染改「行容器 + 绝对定位 run 节点」（`left = 起始列 × cell_w`）、PTY 列数改 `floor(视口逻辑宽 / cell_w)`、**删除** `font × 0.6` 估算；cargo feature `legacy-terminal-layout` 保留旧路径做 A/B |

### 1.1 硬门槛 A 状态

- ✅ **Windows（本机）**：`tests/font_slots.rs::mono_resolves_to_real_monospace_on_this_machine` 断言本机解析到真实等宽（家族 ≠ Inter）；M4-T2 的等宽度量在本机实测成功（通用等宽族：latin 6.60px / CJK 12.0px / 行高 19.2px @ 12px）。
- ⏳ **macOS / Linux**：需在对应平台各启动一次 `xgent_app`，确认启动日志里 `mono` 那行是 system/embedded 来源的真实等宽家族。**用户 2026-10-21 决定：该项降级为不阻塞**（先做 Windows 适配，其他平台后续适配）→ 后续在其他平台适配时须回头闭环。


**提交习惯**：M1/M2 走 Comet change（归档提交只含 `docs/comet/**` 正式工件，实现文件另起一条 `feat(ui): ...`）；**M3 起直接以提交落地**（台账 §10 记录勘误）。`.workbuddy/memory` 单独 `chore(memory)`。不提交 `.agents/.claude/.codex/...` 的换行噪声文件。

---

## 2. 当前基线（下次开工前直接用，别重跑）

- `cargo check --workspace`：通过。
- `xgent_ui` lib：**84 passed**（M2b 基线 76 + M4-T2 新增 8）。
- `xui` lib：**103 passed**（M2b 基线 + M3 新增 34）。
- `xgent_terminal` lib：**33 passed**（M4-T2 新增 12：display_width 边界 / layout_cells）。
- 集成测试（`xgent_ui`，全绿）：`layout`(2)、`modal_settings`(3)、`toast`(1)、`settings_panel_values`(19)、`settings_panel_i18n`(3)、`interaction_model`(7)、`editor_theme_sync`(2)、`conflict_save_guard`(4)、`settings_input_focus`(2)、`density`(4)、`scroll_contract`(8)、`ellipsis_contract`(6)、`virtual_list_contract`(3)、**`file_preview`(7)**、**`file_tree_refresh`(5)**、`mono_typography`(9)、`font_slots`(6)、**`terminal_cell_layout`(6，M4-T2 新增)**、**`panel_registry_contract`(18，M5 新增)**。
  - 标粗的两组在 M2b 基线是**长期失败**（见 §4.3），M3-T6 顺带修好了。
  - `terminal_cell_layout` 需**给相机一个非零渲染目标**（headless 无渲染管线 → UI 视口宽高全 0，见台账 §10 勘误 32）；其中 2 项按格定位专属用例在 `legacy-terminal-layout` feature 下自动跳过（旧路径不 spawn `OutputCellRun`）。

**基线既有失败（不是回归，别当自己弄坏的）**——`cargo test --workspace` 里仍有：

| 目标 | 数量 | 现象 |
|:---|:---|:---|
| `xgent_tools` `builtins::run_command::tests::*` | 6 | 依赖 POSIX shell，Windows 上失败 |
| `xgent_plugin_host` `git_plugin` / `ecs_registration` | 6 | git 插件 WASM 未构建（`preview-diff` 导出缺失） |
| `xgent_daemon::config_store::global_write_lands_inside_the_isolated_dir` | 1 | 多个测试并行写同一个临时配置文件 |
| `xgent_settings_core::paths::daemon_socket_env_override` | 1 | Windows 下期望 `/tmp/.../daemon.sock`，实际 named pipe |
| `xgent_agent::bridge_tests::max_parallel_tools_written_to_handle_bounds_next_turn` | 1 | 并发时序偶发（复跑通常过） |

---

## 3. 环境与工具坑（省时间，逐条都踩过）

1. **必须带**：`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`（本机全量 debug info 链接极慢甚至卡死）。
2. **`cargo` 报 `invalid metadata files for crate ...`**：target 被写坏了（多为并发/杀软占用）。`cargo clean -p <crate>` 后复跑；这一步偶尔被安全软件打断返回 SIGTERM，重试即可。
3. **`cargo fmt`** 只能用 `-p <crate>`，`--all` 会改写 `../bevy` 检出。
4. **headless 装配的坑（这轮踩得最多）**：UI 布局需要**渲染目标**才有尺寸——
   - 缺 `bevy::ui::UiPlugin` → 没有 `ComputedNode`（尺寸恒 0）；
   - 缺 `bevy::window::WindowPlugin` → 相机没有视口；
   - 缺 `bevy::picking::PickingPlugin` → `bevy_ui::widget::viewport` 报缺 `HoverMap`；
   - 缺 `bevy::image::ImagePlugin` / `init_asset::<TextureAtlasLayout>()` → 字体图集相关系统报错；
   - 缺 `xgent_ui::overlay::OverlayPlugin` → `DrawerToggle` 消息未注册 → **文件面板整体报错**（这条曾让 12 个测试长期红）；
   - 缺 `xgent_ui::editor::EditorPlugin` → `SideViewContent` 资源不存在 → 终端系统直接 panic；
   - **装了 `WindowPlugin` 也不够**：没有渲染管线时相机 `computed.target_info` 为空 → `physical_viewport_size()` = 0 → **UI 视口宽高全 0**。解法：测试 App 建好后手动给相机塞 `RenderTargetInfo { physical_size: UVec2::new(1280, 720), scale_factor: 1.0 }`（M4-T2 新踩，台账 §10 勘误 32）。
   排查手段：临时给 bevy 加 `debug` feature，系统名会直接印在报错里（`bevy` 的 "Enable the debug feature to see the name"）。
5. **提交信息要写文件再 `git commit -F`**：PowerShell 管道按 GBK 代码页转码，中文提交信息会被搞坏（`git log` 里能看到乱码）。
6. **验收人独立性**：平台无法自证 Verifier 独立性，`confirm-skill-coordinated-pass` 必须由用户点头，不要代签。
7. **Bevy 系统元组上限 16**：`add_systems(Update, (a, b, c, …))` 的链式元组超过 16 个会编译失败（`spawn_mono_probes` 因此单独 `add_systems`）。

---

## 4. 上期：M5 槽位注册表与派生可见性（已交付）

### 4.0 M5 落点速查

| 任务 | 落点 | 关键决策 |
|:---|:---|:---|
| M5-T1 注册表 | `crates/xgent_ui/src/panels.rs` | `Slot`（含 `Center`，只建模不接消费者）/ `PanelDescriptor`（`title_key` 必填）/ `PanelRegistry` / `SlotLayout`；注册与激活一律 ECS 消息；`SideViewContent` 期间为迁移 shim（M5-T6 已拆） |
| M5-T2 rail + 历史 | `activity_bar.rs` / `session_history.rs` / `overlay.rs` | 删 `ActiveActivity`，选中态从「注册表 ∩ 槽位可见性」派生（后激活者优先）；历史面板改 `Slot::Overlay` 注册面板，与文件抽屉同属单例槽位；`FileDrawerOpen` / `SessionHistoryState` 变派生视图 |
| M5-T3 派生可见性 | `panels/visibility.rs` | `derive_panel_visibility` 唯一写「面板 / 区域」`Node.display`（原 7 处动态写手归一，3 处 spawn 期默认值改构造期字面量）；**内容态可见性 5 处按用户裁决不动**（白名单 + 源码级测试固化，见台账 §10 勘误 34） |
| M5-T4 单页签条 | `panels/tabs.rs` | 页签条由槽位布局拥有，标题取 `title_key`（i18n）；编辑器 tab 条降级为「文档列表」；`EditorBackButtonMarker` 全仓唯一定义 |
| M5-T5 分割模型 | `panels/split.rs` / `resize.rs` | `SplitTree`/`Split`/`LeafSize`；`resize.rs` 遍历分割树生成分割条（注册纵向分割不改 `resize.rs`）；抽屉宽度 `Min(240, Preferred(320))` 上限 480 |
| M5-T6 拆枚举 | `editor/mod.rs` | `SideViewContent` 与 shim 全量拆除，全仓零引用 |

> ⚠ **Bevy 0.20 的 `add_plugins` 不幂等**（重复添加会 panic）——共享插件必须先
> `app.is_plugin_added::<T>()` 判空；`PanelsPlugin` 用 `init_resource` 兜底
> `Localizer` / `Theme` / `SplitTree`（不覆盖宿主注入的真值）。
> ⚠ **`FileDrawerOpen` / `SessionHistoryState` 是派生视图**——测试要开抽屉必须发
> `PanelActivate { id: ids::FILE_DRAWER }`，直接改 Resource 会被派生系统覆盖。

### 4.1 M6 的起点事实（2026-10-21 实测）

- 字体资产：`crates/xgent_app/assets/fonts/` 现有 **Inter-Variable.ttf**（879KB）+ **JetBrainsMono-Regular.ttf**（264KB）+ `OFL.txt` + `README.md`。
- `mono` 槽位已按候选栈装配（`xgent_ui::fonts::font_specs()`），**不再有** `cfg!(target_os)` 特权路径，也不再有 `Handle::default()` 兜底。
- **M4 已全部落地**：`MonoMetrics`（`xgent_ui::terminal::metrics`，启动期实测）、`display_width` / `layout_cells`（`xgent_terminal::render`）、终端按格定位渲染与实测列宽（`terminal/output.rs`）。
- `type_scale` 已有 `CODE` 档、`SMALL` 已 12.0、`line_height::CJK` 已 1.7；代码类文本已走 `mono` 槽位。
- 仍**未闭环**：硬门槛 A 的 macOS / Linux `mono` 启动日志确认（不阻塞，见 §1.1）。

> ⚠ **本文的 §4.1 / §4.2 原文写的是 M5 的起点事实与任务范围**（M5 交付前写的），
> 已在 M5 交付时改写为 **M6** 的口径：M5 的落点见 §4.0，台账 §M5 已勾选。

### 4.2 下一期的任务范围（台账 §M6）

| 任务 | 范围 | 验收（可机器判定） |
|:---|:---|:---|
| M6-T1 | `Transcript` 有序树（`Transcript` / `BlockRef` 块序列），渲染按顺序 spawn/despawn/update，顺序键 `(turn_index, block_index)`；删除命令式挪子节点（`chat_panel.rs:489`、`tool_panel.rs:85`、`chat_panel.rs:724`、`undo.rs:286`） | 边流式边插工具卡，顺序稳定不倒退（自动化断言序列单调） |
| M6-T2 | Markdown 块模型与依赖评审（优先 `pulldown-cmark`，走一次依赖评审；退化方案为仅代码块 + 列表 + 粗体） | 评审记录落 `docs/notes/`；解析器单测覆盖六类元素 |
| M6-T3 | 富文本渲染（单节点多 `TextSection`；代码块 `surface_code` 底 + `mono` + 语言标签 + 复制按钮；语法色接 `xui` 高亮链） | 六类元素混合样张视觉正确；对照原型截图 |
| M6-T4 | `@` 内联补全（浮层挂 `DROPDOWN` 层；方向键导航 + `Enter` 接受；候选来自文件/符号/`@selection`） | 输入 `@` 出现补全并可键盘选择；窄窗不被裁 |
| M6-T5 | 发送/停止动作条（显式按钮；禁用态由 `AgentStatus` 派生；删除「发送按钮区」空注释） | 不碰键盘可完成一次完整发送与一次中断 |
| M6-T6 | `ContextSet` 建模（独立 `Resource`，可增删、可持久化、显示 token 占比；替换 `context_scope.rs` 的只读镜像） | 移除某 chip 后其不再进入下一轮请求 |

> ⚠ M6 依赖 M3 的滚动/省略/填满契约与 M5 的槽位注册表（面板 = 注册项）——两者都已交付，
> **M6 可以直接开工**。

## 5. 下一步：M6 会话内容（Transcript 有序树 + Markdown + 输入区）

### 5.1 M3 顺带修好的 12 个长期失败测试（别再当成"基线失败"）

`file_preview`(7) + `file_tree_refresh`(5) 的根因**不是**产品缺陷，而是 headless 装配缺
`xgent_ui::overlay::OverlayPlugin`（`DrawerToggle` 消息未注册 → 文件面板的系统整体报错，
文件树从未重建）。M3-T6 补上插件并补齐装配缺项后全绿。
改动落在 `crates/xgent_ui/tests/file_preview.rs` 与 `tests/file_tree_refresh.rs`：
加插件/窗口/UI/picking/相机/资产；抽屉开关改走 `DrawerToggle` 消息（M1-T4 之后
`OverlayRegistry` 是唯一权威，直接改 Resource 会与注册表失配）。

---

## 6. 之后的可选方向（按台账 §8 里程碑表）

- **M5 槽位注册表**（⚠ 最大改动面，**已交付**）：`PanelRegistry` 取代 `SideViewContent` closed 枚举、派生可见性单一系统、`SplitTree` 分割模型。详见 §4.0。
- **M6 会话内容**（**下一期**）：`Transcript` 有序树 + Markdown + 输入区（依赖 M3 的滚动/省略/填满契约与 M5 的槽位注册表；M3-T6 的会话消息列表虚拟化随 M6-T1 一起做，见台账 §10 勘误 13）。
- **M7 闭环**：快捷键单一来源、焦点栈、i18n 扩面、几何断言层（台账 §0.2 的 P0 缺陷回归测试多数在 M7 收口）。
- **M8 Git 探针**（北极星：新增 1 个 `PanelDescriptor` + 1 个视图模块，既有布局文件改动 = 0）。
- **M4 尾巴**：硬门槛 A 的 macOS / Linux 确认 + 终端按格定位的实机视觉确认（`ls -l` / 中文输出 / `top` / `vim`）。

---

## 7. 本轮遗留风险（Verifier 口径，不修也不违约）

1. **对话输入区的可视行数是 1 帧滞后**：`update_input_visible_lines` 在 PostUpdate 写 `visible_lines`，下一帧的 Content 阶段才生效——打字时输入框高度有极轻微的滞后感。
2. **光标行跟随也是 1 帧滞后**（同上，滚动位置写入 PostUpdate → 下一帧布局生效）。
3. **文件树最小高度 240px**：窗口极矮时树会超出抽屉（抽屉 `overflow: clip` 会裁掉底部）——极端小窗下的取舍，非典型场景。
4. **虚拟列表只覆盖固定行高场景**：不等高（会话消息列表）待 M6-T1。
5. **省略号测量每帧最多触发一次二分**（宽度不变时命中缓存）；拖窗口改变宽度时会重新二分（log2(n) 次排版），超大文本时可能有轻微开销。
6. **终端宽字符的字形填不满那 2 格**（M4-T2）：CJK 回退字体（雅黑/苹方/思源）的汉字步进是 1em，不是等宽拉丁的 2 倍 → 宽字符虽**严格占 2 格**（起点落在格边界，网格正确），但字形本身比 2 格窄（本机约差 1.2px/字），视觉上略有间隙。**根治办法是接 `mono_cjk` 槽位**（Sarasa / Noto Sans Mono CJK，真 2:1），本轮未做（见台账 §10 勘误 30）。
7. **终端 UI 侧不再二次换行**（M4-T2）：超长行（PTY 未折行）会被视口裁掉而非 UI 再折一次——真实终端也是这个行为，但与旧路径不同（旧的会折行）。
8. **等宽度量在首帧后才有**（M4-T2）：实测需要 UI 布局管线跑起来 → 度量就绪前终端不渲染行（游标不推进，下一帧补齐）；PTY 也暂不发 resize（保持默认 80×24）。

---

## 8. 下次开工的前 5 步

1. `git log --oneline -8` 确认 M5 的实现提交（`0aeecc4` / `0141343` / `dc22458` / `7eb83e9`）在 HEAD。
2. 读本文 §4.0 + `docs/plans/ui-v8-tasks.md` §M5 与 §10 勘误 34–38。
3. 复核 §4.0 的落点（`xgent_ui::panels` 的 `Slot` / `PanelRegistry` / `SlotLayout` / `derive_panel_visibility` / `SplitTree`）。
4. 读 `docs/design/ui-v8-layout-architecture.md` §6（Transcript 有序树）与 §3.1（注册表模型）。
5. 开工前先跑一遍基线：`cargo test -p xgent_ui -p xgent_terminal -p xui`（全绿 = 基线没被本轮弄坏）。

---

## 9. 维护约定

- 每次里程碑交付后更新本文 **§1（交付表）** 与 **§2（基线数字/既有失败）**；§4 的「起点事实」在开工时复核一次。
- 任务级勾选仍记在 [ui-v8-tasks.md](ui-v8-tasks.md)；两边冲突时以「提交 + 代码实测」为准，并在台账 §10 记勘误。
