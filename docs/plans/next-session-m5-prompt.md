【任务】继续推进 UI v8 交互架构层重建的下一期：**M5 槽位注册表与派生可见性**（台账 `docs/plans/ui-v8-tasks.md` §M5，修 D1–D6、I3）。这是迁移策略的**第 5 步**（布局架构 §12），台账标注「⚠ 最大改动面」。

【工作流要求】本项目走 Comet Native：
1. 先执行 `comet workflow resolve . --activate --json`，再按返回的 skill 走（应为 comet-native）。
2. 用 `comet native new v8-m5-panel-registry --isolation current --language zh-CN --json` 建变更，Shape → 确认 → Build → 独立 Verifier → 我确认 → Archive。
3. 提交粒度：**每任务一提交**（M5-T1…M5-T6 各自一条），中文信息（`feat(ui): ...`），文档同步一条，归档提交沿用 `chore(native): archive <change>`。

【必读（按顺序）】
- `docs/plans/ui-v8-tasks.md` —— 唯一进度台账：§M5 六个任务原文、§0.2 缺口表 D1–D6/I3、§10 勘误 1~33（已踩过的坑，别重犯）。
- `docs/plans/ui-v8-handoff.md` —— §1 交付表、§2 基线与既有失败、§3 环境坑、§4.2（M5 范围与验收判据）、§7 开工前 5 步。
- `docs/design/ui-v8-layout-architecture.md` —— §3.1 描述符取代 closed enum（`Slot`/`PanelDescriptor`/`PanelRegistry`/`PanelRuntime`）、§3.2 派生可见性、§3.3 尺寸契约、§12 迁移策略第 5 步与风险预案（含 `SideViewContent` 临时 shim 的分批合入要求）。
- `crates/xgent_ui/src/editor/mod.rs` —— `SideViewContent` 枚举与它的可见性写手、页签条、两套 `EditorBackButtonMarker`。
- `crates/xgent_ui/src/activity_bar.rs` —— `ActivityKind` / `ActiveActivity` / rail 选中态。
- `crates/xgent_ui/src/layout.rs`（层根与 `toggle_panel_visibility`）、`file_panel.rs`（抽屉）、`terminal/mod.rs`（终端视图）、`session_history.rs`（当前是 overlay）、`overlay.rs`（`OverlayRegistry`）。
- `crates/xgent_ui/src/resize.rs` —— `PanelWidths` 单字段与拖拽手柄。
- `crates/xui/src/scroll_frame.rs` + `crates/xui/src/layout.rs` —— M3 已落地的 `ScrollFrame` / `region_fill` 契约（M5 依赖它们，别重复发明）。

【起点事实（2026-10-21 实测，可直接采信）】
- M1 / M2a / M2b / M3 / M4-T1 / **M4-T2（终端双宽单元格，本轮刚归档，提交 `0f0b7b5`）** / M4-T3 / M4-T4 全部交付并归档；`cargo check --workspace` 通过。
- `PanelRegistry` / `PanelDescriptor` / `Slot` / `SlotLayout` / `SplitTree` / `derive_panel_visibility` **全部不存在**（grep = 0）→ M5 未开工。
- `SideViewContent` 仍是 closed enum（`editor/mod.rs:69-81`，5 变体 `None`/`Editor`/`Preview`/`Diff`/`Terminal`）。
- `ActivityKind{Chat,Files,History,Terminal}`（`activity_bar.rs:20-30`）+ `ActiveActivity`（`:40`）仍在；rail 选中态是**独立存储**的。
- **`.display = ` 写手：`xgent_ui` 共 15 处 / 10 个文件**（`xui` 与 `xgent_app` 各 0 处）。台账 M5-T3 列的 5 个行号**已漂移**（实际在 `layout.rs:287/291`、`file_panel.rs:265/268`、`editor/mod.rs:186/275/352/398/507`、`terminal/mod.rs:275`、`activity_bar.rs:234/369`、`chat_panel.rs:1054`、`diff_view.rs:59`、`welcome.rs:190/223`）。⚠ 其中只有一部分是**动态面板可见性**写手，其余是内容态可见性（欢迎空态、对话输入区、rail 项）与 spawn 期默认值——**M5-T3 的验收口径需要你判断**（见下）。
- `EditorBackButtonMarker` **两套**：`editor/mod.rs:295` 与 `:732`（M5-T4 要合一）。
- `PanelWidths` 是**单字段** `side_view`（`resize.rs:47-55`，默认 `CONTEXT_W_DEFAULT = 720`）；`theme::size::DRAWER_W = 320.0` 是**定值**（`theme.rs:245`），不可拖宽。
- `session_history.rs` 仍是 overlay（`SessionHistoryOverlayMarker`），与文件抽屉争层级。
- **基线**：`xgent_ui` lib **84 passed**、`xui` lib **103 passed**、`xgent_terminal` lib **33 passed**；`xgent_ui` 集成测试 18 个文件全绿（含 `terminal_cell_layout` 6 项）；`cargo test --workspace` 的失败集合 = **15 项既有基线**（xgent_tools 6 / xgent_plugin_host 6 / xgent_daemon 1 / xgent_settings_core 1 / xgent_agent 1），**不是回归**。

【范围（台账 §M5 原文，逐条实现）】
1. **M5-T1 `PanelRegistry` 取代 closed 枚举（⚠）**：`Slot{Rail,SideDock,BottomDock,SettingsSection,StatusSegment,Palette,Overlay}` + `PanelDescriptor`（`title_key` 必填 i18n key）+ `PanelRuntime`；`PanelRegister/PanelActivate/PanelDeactivate` **消息**（遵守 ECS 契约，禁直接方法调用）；`SideViewContent` 加**临时 shim**（枚举 → 描述符的一次性映射），使后续可分小批合入且每批可编译。
   - 验收：新增一个 `SideDock` 面板**无需修改** `layout.rs` / `editor/mod.rs` 的枚举与页签条代码。
2. **M5-T2 rail 与历史进槽位（修 D4/D5）**：删除 `ActiveActivity`，选中态从 `registry ∩ slots.is_visible()` 派生；`ActivityKind` 改描述符驱动；`session_history.rs` 从浮层改为可注册面板。
   - 验收：连点 Files 两次关掉抽屉后选中态回落正确；历史面板与文件抽屉不再争层级。
3. **M5-T3 派生可见性单一系统（修 I3/D2）**：`derive_panel_visibility` 成为唯一写 `Node.display` 的系统，删掉各自的写手。
   - 验收：`grep -rn "\.display = " crates/xgent_ui/src/` 命中仅限该系统与层根构造。**⚠ 见下方「需你判断的一点」。**
4. **M5-T4 单页签条所有者（修 D3）**：页签条由 `SlotLayout` 拥有并渲染；编辑器内部 tab 条降级为**文档列表**语义（与槽位页签不是一回事，**不合并**）；两套 `EditorBackButtonMarker` 合一。
   - 验收：侧区顶部只有一条页签条、只有一个返回入口。
5. **M5-T5 分割模型（修 D6）**：`SplitTree/Split/LeafSize`；`resize.rs` 从单字段改为遍历生成分割条；`DRAWER_W` 从定值 320 改为 `Min(240, Preferred(320))`、上限 480。
   - 验收：抽屉可拖宽；可注册纵向分割而无需改 `resize.rs`。
6. **M5-T6 移除 `SideViewContent` 枚举**：shim 拆除，editor/preview/diff/terminal 全部为原生注册项；页签标题取 `title_key`（删 `editor/mod.rs` 里那处 `let _ = loc;` 丢弃 i18n 的写法）。
   - 验收：`grep -rn "enum SideViewContent" crates/` 无命中；英文语料下页签为英文。

【⚠ 需你（Shape 阶段）判断的一点】
台账 M5-T3 的验收口径是「`.display = ` 命中仅限该系统与层根构造」，但实际有 **15 处**写手，其中相当一部分**不是面板可见性**（`welcome.rs:190/223` 欢迎空态、`chat_panel.rs:1054` 对话输入区、`activity_bar.rs:234` rail 项、若干 spawn 期默认值）。请在 Shape 里明确：**(a)** 收窄为「**动态面板/区域可见性**写手」归一（内容态可见性不动），还是 **(b)** 字面把 15 处全清（需要把欢迎空态、对话输入区这类内容态可见性也改成派生——改动面会外扩）。选 (b) 时台账的 M5-T3 任务书也要同步改写（会触发 Shape 复确认）。

【环境坑（照做，别重蹈）】
- cargo 必须带 `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`，否则全量 debug info 链接 OOM（报 "can't find crate" 是次生错误，看着像代码坏了）。
- Comet 的 Runtime 检查计划要用 cmd 包装，否则同样 OOM：`executable="cmd"`、`argv=["/c","set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo test ..."]`。
- **headless 集成测试的 App 装配清单**（缺一项就报缺资源 / 尺寸恒 0）：`bevy::ui::UiPlugin`、`bevy::window::WindowPlugin` + 相机、`bevy::picking::PickingPlugin`、`bevy::image::ImagePlugin` + `init_asset::<TextureAtlasLayout>()`、`bevy::text::TextPlugin`、`xgent_ui::overlay::OverlayPlugin`（`DrawerToggle` 由它注册）、`xgent_ui::editor::EditorPlugin`（否则 `SideViewContent` 资源不存在 → 终端/编辑器系统 panic）、`xgent_ui::fonts::UiFonts::fallback()`。
- **headless 里 UI 视口宽高全 0**（不只是「视口高 0」）：没有渲染管线时相机 `computed.target_info` 为空 → `physical_viewport_size()` = 0 → UI 根节点 0×0。解法：测试 App 建好并跑一帧后手动给相机塞 `bevy::camera::RenderTargetInfo { physical_size: UVec2::new(1280, 720), scale_factor: 1.0 }`。M5 的「抽屉可拖宽」「分割条」类断言**依赖真实视口**，务必照做。
- **Bevy 系统元组上限 16**：`add_systems(Update, (a…z))` 链式元组超过 16 个会编译失败（M4-T2 踩过）。
- **提交信息写文件再 `git commit -F <file>`**：PowerShell 管道按 GBK 转码会把中文提交信息搞坏。
- **别用 PowerShell 的 `Set-Content -Encoding UTF8` 回写源文件**（PS 5.1 会写 UTF-8 BOM；rustc 接受 BOM 故全绿，但会污染「剥注释行」类静态断言）。要精确控制字节用 `[System.IO.File]::WriteAllText($p, $text, (New-Object System.Text.UTF8Encoding($false)))`。
- `cargo fmt` 只能 `-p <crate>`，`--all` 会改写 `../bevy` 检出。
- **Comet Native 的绑定纪律**（M4-T2 踩过）：改动已确认的 `brief.md` / `specs/**/spec.md` 会让 Shape 绑定失效 → 要求**复确认 Shape**。措辞订正尽量在 Shape 阶段一次做完；每轮 Verifier 后的修复要走 `--revise-implementation` 回到 Build、重交候选、**新起一轮独立 Verifier**（不要沿用上一轮结论）。
- **既有失败不是回归**：xgent_tools 6（依赖 POSIX shell）、xgent_plugin_host 6（git 插件 WASM 未构建）、xgent_daemon 1、xgent_settings_core 1、xgent_agent 1（时序 flake）。
- 提交前跑：`cargo fmt -p <改动crate>` → `cargo clippy -p <改动crate> --all-targets`（改动文件零新增警告）→ `cargo test -p <改动crate> --no-fail-fast` → 同步台账/交接/开发指南。

【非目标】
- 不做 M6（Transcript/Markdown/输入区）、M7（快捷键单源/焦点栈/i18n 扩面/几何断言层）、M8（Git 探针）。
- 不做 3D 空间视图（`Slot::Center` 的首个消费者留给 F-16，本轮只把 `Center` 槽位建模好、不接消费者）。
- 不做自由 docking、不做亮色主题、不改 M1–M4 已交付行为（`ScrollFrame`/`region_fill`/`Ellipsis`/`VirtualList`/等宽单元格都按已交付契约用）。
- 不改 `xui` 的依赖边界（NU-4）；`xui` 不依赖任何 `xgent_*`。
- 不做几何断言层（M7-I2）——M5 只在必要处补可判定的自动化断言。

【交付物】
实现 + 测试（纯函数级：描述符→槽位映射、shim 一致性、派生可见性、rail 选中态派生、分割树遍历）+ 台账（§M5 勾选 + §10 勘误）/交接（交付表、基线、下一步）/开发指南（新增 M5 小节）三处同步 + 一次独立 Verifier 复验（Comet 流程内）+ 归档。
