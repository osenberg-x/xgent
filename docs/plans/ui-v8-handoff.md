# UI v8 交接：进度、基线与下一步

> 用途：跨会话交接。**进度台账以 [ui-v8-tasks.md](ui-v8-tasks.md) 为准**（任务级勾选），本文记录「到哪儿了、基线是什么、下一步怎么开工、有哪些坑」。
> 更新时间：2026-10-29（M6.5 会话内容接线交付后）。

---

## 0. 一句话现状

UI v8 的 **M1–M5、M7 已交付；M6 模型层 + M6.5 接线已交付并经一轮只读复审修复**（M6 模型 `9ac4daf`、M6.5 实现 `b962f97`、启动修复 `5dde747`、复审修复 `e485d5e`）。Transcript 已成顺序权威，Markdown 终稿、`@` 补全、ContextSet+sidecar 与消息虚拟窗口均接入产品；`cargo check --workspace` 绿，`xgent_ui` lib **135 passed**（2026-10-29 实测），渲染契约 10 项、`xui` 103 项、`xgent_terminal` 33 项全绿。**下一期是 M8（Git 探针）**；真实窗口截图/走查仍待用户完成（须含中文输入法与 HiDPI 缩放两项）。

> ⚠ 未闭环项：**硬门槛 A 的 macOS / Linux 的 `mono` 启动日志确认**（用户 2026-10-21 决定降级为不阻塞；2026-10-28 决定 Windows 收尾后立刻排）。另：M6.5 的真实窗口截图/走查待用户完成（自动化证据已齐）。

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
| M5 槽位注册表与派生可见性 | Comet change `v8-m5-panel-registry` | 实现 `0aeecc4` / `0141343` / `dc22458` / `7eb83e9` | `xgent_ui::panels`：`Slot`（含 `Center`，只建模不接消费者）/ `PanelDescriptor`（`title_key` 必填）/ `PanelRegistry` / `SlotLayout` + `PanelRegister`/`PanelActivate`/`PanelDeactivate` 消息；`derive_panel_visibility` 唯一写面板可见性；rail 选中态派生（`ActiveActivity` 删除）；会话历史改 `Slot::Overlay` 注册面板；槽位页签条（`panels::tabs`）+ `EditorBackButtonMarker` 合一；`SplitTree`/`Split`/`LeafSize` 分割模型 + 抽屉可拖宽（240–480）；`SideViewContent` 枚举与 shim 全量拆除；`tests/panel_registry_contract.rs` 21 项契约测试 |
| M6 会话内容（模型层） | Comet change `v8-m6-transcript-markdown` | 归档 `247d1bd` / 模型提交 `9ac4daf` / T5 在 `81d3060` | `transcript.rs`（`Transcript`/`BlockRef`/`ContentBlock`）、`markdown.rs`（`pulldown-cmark` 解析）、`markdown_render.rs`（`spawn_markdown`）、`completion.rs`（`CompletionState`/`spawn_completion_popup`/`detect_at_trigger`）、`context_set.rs`（`ContextSet`）；**除 send/stop 按钮外均无消费者**；归档验证无 Runtime checks，四条 Known limitation |
| M7 闭环与验证 | Comet Native `v8-m7-closing-loop` | 归档 `6f10081` / 实现 `81d3060` | `shortcuts.rs` 清单 + 启动期 id 集断言；`TabNavigationPlugin`/`DirectionalNavigationPlugin` + `TabIndex` 派生 + `TabGroup::modal()`；焦点环只在键盘导航显形；`i18n_full_corpus` 3 项 + tooltip/错误文案迁移；删除 `ChatPanelEntities`；`layout_geometry` 11 项。**A1 强度打折**：`handler_name` 是字符串，分发表缺分支无编译期/启动期保障 |
| M6.5 会话内容接线 | Comet Native `v8-m65-content-wiring` | 实现 `b962f97` / 启动修复 `5dde747` / 影子写入 `d516bdb` | Transcript 写入+渲染器（块键差分、命令式挪节点全删）；Markdown 终稿（`parse_markdown`+`spawn_markdown`）；`@` 补全（文件候选 + DROPDOWN + Enter 消费）；ContextSet chips/请求注入/token 占比/会话级 sidecar；消息列表 `VirtualList` 虚拟窗口；`transcript_render_contract` 5 项 + `transcript_write` 7 项 |
| M4-T2 终端双宽单元格 | Comet change `v8-m4-terminal-cells` | 实现 `ff9c203`+`5e1d4cb`+`e06f77d`+`d3a276b`+`0fd632b`+`fe96a82` | `xgent_terminal::render`：`display_width`（wcwidth）+ `display_width_str` + `CellRun` + `layout_cells`（按格布局内核）；`xgent_ui::terminal::metrics`：`MonoMetrics`（启动期**实测**）+ 双宽校验 + `CellGrid`；`terminal/output.rs`：行渲染改「行容器 + 绝对定位 run 节点」（`left = 起始列 × cell_w`）、PTY 列数改 `floor(视口逻辑宽 / cell_w)`、**删除** `font × 0.6` 估算；cargo feature `legacy-terminal-layout` 保留旧路径做 A/B |

### 1.1 硬门槛 A 状态

- ✅ **Windows（本机）**：`tests/font_slots.rs::mono_resolves_to_real_monospace_on_this_machine` 断言本机解析到真实等宽（家族 ≠ Inter）；M4-T2 的等宽度量在本机实测成功（通用等宽族：latin 6.60px / CJK 12.0px / 行高 19.2px @ 12px）。
- ⏳ **macOS / Linux**：需在对应平台各启动一次 `xgent_app`，确认启动日志里 `mono` 那行是 system/embedded 来源的真实等宽家族。**用户 2026-10-21 决定：该项降级为不阻塞**（先做 Windows 适配，其他平台后续适配）→ 后续在其他平台适配时须回头闭环。


**提交习惯**：M1/M2 走 Comet change（归档提交只含 `docs/comet/**` 正式工件，实现文件另起一条 `feat(ui): ...`）；**M3 起直接以提交落地**（台账 §10 记录勘误）。`.workbuddy/memory` 单独 `chore(memory)`。不提交 `.agents/.claude/.codex/...` 的换行噪声文件。

---

## 2. 当前基线（下次开工前直接用，别重跑）

- `cargo check --workspace`：通过。
- `xgent_ui` lib：**114 passed**（2026-10-29 实测；M6.5 新增渲染契约等）。
- `xui` lib：**103 passed**（M2b 基线 + M3 新增 34）。
- `xgent_terminal` lib：**33 passed**（M4-T2 新增 12：display_width 边界 / layout_cells）。
- **验证范围提醒**：2026-10-29 用 `-j 2` 跑完 `xgent_ui` 全套 + `xui` 103 + `xgent_terminal` 33/6 全绿；`cargo test --workspace` 仍有下表既有红（高并发编译会 OOM，见 §3）。
- **实机启动**：`target/debug/xgent_app.exe` 运行 12s 零 panic；daemon 连接、窗口创建、字体槽位正常（2026-10-29）。
- 集成测试（`xgent_ui`，全绿）：`layout`(2)、`modal_settings`(3)、`toast`(1)、`settings_panel_values`(19)、`settings_panel_i18n`(3)、`interaction_model`(7)、`editor_theme_sync`(2)、`conflict_save_guard`(4)、`settings_input_focus`(2)、`density`(4)、`scroll_contract`(8)、`ellipsis_contract`(6)、`virtual_list_contract`(3)、**`file_preview`(7)**、**`file_tree_refresh`(5)**、`mono_typography`(9)、`font_slots`(6)、**`terminal_cell_layout`(6，M4-T2 新增)**、**`panel_registry_contract`(21，M5 新增)**、**`transcript_render_contract`(5，M6.5 新增：顺序/Markdown/工具卡/通知/虚拟窗口)**、`i18n_full_corpus`(3)。
  - 标粗的两组在 M2b 基线是**长期失败**（见 §4.3），M3-T6 顺带修好了。
  - `terminal_cell_layout` 需**给相机一个非零渲染目标**（headless 无渲染管线 → UI 视口宽高全 0，见台账 §10 勘误 32）；其中 2 项按格定位专属用例在 `legacy-terminal-layout` feature 下自动跳过（旧路径不 spawn `OutputCellRun`）。

**基线既有失败（不是回归，别当自己弄坏的）**——`cargo test --workspace` 里仍有：

| 目标 | 数量 | 现象 |
|:---|:---|:---|
| `xgent_tools` `builtins::run_command::tests::*` | 6 | 依赖 POSIX shell，Windows 上失败 |
| `xgent_plugin_host` `git_plugin` / `ecs_registration` | 6 | git 插件 WASM 未构建（`preview-diff` 导出缺失） |
| `xgent_daemon::config_store::global_write_lands_inside_the_isolated_dir` | 1 | 多个测试并行写同一个临时配置文件 |
| `xgent_settings_core::paths::daemon_socket_env_override` | 1 | Windows 下期望 `/tmp/.../daemon.sock`，实际 named pipe |
| `xgent_agent::bridge_tests::max_parallel_tools_written_to_handle_bounds_next_turn` | 1 | 并发时序偶发；2026-10-29 连跑 3 次均 3/4（未修） |

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

## 4. 历史：M5 槽位注册表与派生可见性（已交付）

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

### 4.2 M6 的历史任务范围（已过时，2026-10-28 订正）

> 本节是 M6 开工前写的范围，**不再是下一期任务**。M6 实际只交付模型层，
> 下一期以 [ui-v8-tasks.md](ui-v8-tasks.md) §M6.5 为准；下表保留作历史对照。

| 任务 | 范围 | 验收（可机器判定） |
|:---|:---|:---|
| M6-T1 | `Transcript` 有序树（`Transcript` / `BlockRef` 块序列），渲染按顺序 spawn/despawn/update，顺序键 `(turn_index, block_index)`；删除命令式挪子节点（`chat_panel.rs:489`、`tool_panel.rs:85`、`chat_panel.rs:724`、`undo.rs:286`） | 边流式边插工具卡，顺序稳定不倒退（自动化断言序列单调） |
| M6-T2 | Markdown 块模型与依赖评审（优先 `pulldown-cmark`，走一次依赖评审；退化方案为仅代码块 + 列表 + 粗体） | 评审记录落 `docs/notes/`；解析器单测覆盖六类元素 |
| M6-T3 | 富文本渲染（单节点多 `TextSection`；代码块 `surface_code` 底 + `mono` + 语言标签 + 复制按钮；语法色接 `xui` 高亮链） | 六类元素混合样张视觉正确；对照原型截图 |
| M6-T4 | `@` 内联补全（浮层挂 `DROPDOWN` 层；方向键导航 + `Enter` 接受；候选来自文件/符号/`@selection`） | 输入 `@` 出现补全并可键盘选择；窄窗不被裁 |
| M6-T5 | 发送/停止动作条（显式按钮；禁用态由 `AgentStatus` 派生；删除「发送按钮区」空注释） | 不碰键盘可完成一次完整发送与一次中断 |
| M6-T6 | `ContextSet` 建模（独立 `Resource`，可增删、可持久化、显示 token 占比；替换 `context_scope.rs` 的只读镜像） | 移除某 chip 后其不再进入下一轮请求 |

> ⚠ M6.5 依赖同上，且顺序不可颠倒：**先建 Transcript 数据流，再替换命令式渲染**；
> 直接删旧路径会让流式、工具卡、undo 通知三处同时失控。

## 5. M6.5 回顾与下一步：M8 Git 探针

### 5.0 M6.5 交付回顾（详见台账 §M6.5）

1. **T1**：把用户消息、助手流式增量、工具调用/结果、压缩通知写入 `Transcript`；渲染按
   `(turn_index, block_index)` 差分 spawn/update/despawn；删除命令式挪节点；消息列表虚拟化。
2. **T2**：终稿消息走 `parse_markdown` + `spawn_markdown`；流式期纯文本，完成后替换；
   代码块复制按钮接线；补 `docs/notes/` 依赖评审。
3. **T3**：`@` 补全接入输入框与文件树候选，浮层挂 `DROPDOWN`。
4. **T4**：`ContextSet` 替换 `context_scope.rs` 只读镜像，chips 双向同步，发送时进入请求，
   token 占比 + 会话内持久化。
5. **T5**：真实窗口走查 + 截图，把 M6 A1–A6 补成可复跑证据。
6. **T6**：文档口径订正 + mac/Linux 硬门槛 A 排期。

### 5.0.1 交付结果（2026-10-29）

- 实现：`b962f97`（Transcript 渲染器 / Markdown 终稿 / `@` 补全 / ContextSet+sidecar / VirtualList 窗口），
  影子写入 `d516bdb`，启动修复 `5dde747`。
- 测试：`transcript_render_contract`(10) + `transcript_write`(5) + pinned context 合并与预算；
  `xgent_ui` lib **135** + `xui` 103 + `xgent_terminal` 33 全绿，`xgent_agent` 77 passed（唯一失败是 §2 基线里的既有并发 flake）。
- 复审修复（同日 `e485d5e`，`/comet-review` 20 条发现全部处理）：中文输入法组合期 panic、`@` 浮层不关闭与
  Ctrl+Enter 被吞、无当前轮时消息不排空导致跨轮重放、steering 文本不再渲染、denied/progress 丢失、
  空助手气泡、虚拟窗口间隙塌陷与 HiDPI 物理/逻辑像素混用、Markdown 内联丢内容与终稿档位跳变、
  pinned 路径不同形导致去重失效且不受检索预算约束、sidecar 静默失败。详见台账 §10 勘误 51 与
  设计文档 §12。
- 未闭环：真实窗口截图/走查（A9 可视部分，**须含中文输入法输入与 125%/150% 缩放**）；
  `@` 补全仅文件候选；设计文档「实现前先提交」的仓库级证据无法满足（见设计文档 §11）。

### 5.0.2 下一步 M8（Git 探针）

北极星：新增 1 个 `PanelDescriptor`（SideDock）+ 1 个视图模块，既有布局文件改动 = 0；
面板可切换、可键盘到达、可滚动、可截断、i18n 完整。数据来自 `xgent_plugin_git` 的 status。

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
- **M6 会话内容**（**模型层 + M6.5 接线已交付**）：`Transcript` / Markdown / 补全 / `ContextSet` 已接产品；见 §5.0。
- **M7 闭环**（**已交付**，`81d3060`）：快捷键单一来源、焦点栈、i18n 扩面、几何断言层。A1 的「函数指针 / 编译期」表述不成立，见台账 §10 勘误 46。
- **M8 Git 探针**（**下一期**，北极星：新增 1 个 `PanelDescriptor` + 1 个视图模块，既有布局文件改动 = 0）。
- **M4 尾巴**：硬门槛 A 的 macOS / Linux 确认 + 终端按格定位的实机视觉确认（`ls -l` / 中文输出 / `top` / `vim`）。用户 2026-10-28 决定：**Windows 收尾后立刻排**。

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
9. **M6.5 已接线，但真实窗口证据未闭环**（2026-10-29）：自动化测试覆盖顺序/Markdown/工具卡/通知/虚拟窗口；真实窗口截图与走查仍需用户完成（A9 可视部分）。
10. **虚拟窗口的中间间隙**（2026-10-29，**已闭合**）：渲染窗口 = 可见区间 ∪ 当前轮；原实现只在两端放占位，两段不相邻时中间空档没有高度 → 流式中上翻历史会把当前轮贴到可见区正下方。现按**连续段**布局并逐段补占位（`transcript_render.rs` 的 `build_runs`/`push_gap`，段集合变化才重建），契约测试 `gap_between_visible_window_and_active_turn_is_spaced` 锁住。同类问题一并修：`ComputedNode::size`/`ScrollPosition` 是物理像素，必须乘 `inverse_scale_factor` 再与逻辑偏移表比较（`hidpi_window_matches_the_same_logical_geometry`）。
11. **M7-A1 的保障强度**（2026-10-28）：`handler_name` 是字符串，启动期只比 id 集合；新增 action 时若只加清单不加分发表分支，不会失败，不要把它当已闭合的契约。
12. **未闭环门槛**：mac/Linux 的 `mono` 启动日志与终端实机仍未确认（NF-01 是需求硬项）；`cargo test --workspace` 仍有既有红，M6.5 的「全绿」只对 `xgent_ui` + `xui` + `xgent_terminal` 成立。

---

## 8. 下次开工的前 5 步

1. `git log --oneline -8` 确认 `b962f97`（M6.5）/ `5dde747`（启动修复）在 HEAD。
2. 读本文 §5.0.2 + `docs/plans/ui-v8-tasks.md` §M8 与 §10 勘误 48–50。
3. 复核 `xgent_plugin_git` 的 status 数据与 `PanelDescriptor` 注册路径（M8 只允许新增 1 注册 + 1 视图模块）。
4. 读 `docs/design/ui-v8-layout-architecture.md` §3.1（注册表模型）与 §6（Transcript 有序树）。
5. 开工前先跑基线：`cargo test -p xgent_ui -p xgent_terminal -p xui -j 2`（全绿 = 基线没被本轮弄坏）。

---

## 9. 维护约定

- 每次里程碑交付后更新本文 **§1（交付表）** 与 **§2（基线数字/既有失败）**；§4 的「起点事实」在开工时复核一次。
- 任务级勾选仍记在 [ui-v8-tasks.md](ui-v8-tasks.md)；两边冲突时以「提交 + 代码实测」为准，并在台账 §10 记勘误。
