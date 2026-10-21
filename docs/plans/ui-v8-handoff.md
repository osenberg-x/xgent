# UI v8 交接：进度、基线与下一步

> 用途：跨会话交接。**进度台账以 [ui-v8-tasks.md](ui-v8-tasks.md) 为准**（任务级勾选），本文记录「到哪儿了、基线是什么、下一步怎么开工、有哪些坑」。
> 更新时间：2026-10-20（M3 交付后）。

---

## 0. 一句话现状

UI v8 的 **M1（分层/拾取/模态）、M2a（主题权威/令牌收敛）、M2b（密度档位/hover 量化）、M3（滚动契约/填满契约/省略原语/虚拟列表）已交付**；`cargo check --workspace` 绿，`xgent_ui` lib 76 项 + `xui` 103 项测试全绿。下一期按台账顺序是 **M4 跨平台字体与终端双宽**（⚠ 硬门槛 A：三平台 `mono` 解析不到真实等宽就停止铺开）。

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
| M4-T3/T4 排版消费 | Comet change `v8-m4-mono-typography` | 实现 `de2bb68`（归档待补） | 代码类文本走 `mono` 槽位（`EditorTheme.font` 注入 + **span 自带字体** + 行号列同源 + diff 行 `mono_text` + 工具卡摘要 `ellipsis_mono_text`）；`SMALL 12.5 → 12`、`MONO` → `CODE`、新增 `line_height::CJK = 1.7`；中文约束（`display_spacing` 负字距不作用中文、`line_height::for_text` 正文按内容选档） |

### 1.1 M4-T1 的硬门槛 A 状态

- ✅ **Windows（本机）**：`tests/font_slots.rs::mono_resolves_to_real_monospace_on_this_machine` 断言本机解析到真实等宽（家族 ≠ Inter）。
- ⏳ **macOS / Linux**：需在对应平台各启动一次 `xgent_app`，确认启动日志里 `mono` 那行是 system/embedded 来源的真实等宽家族。这是**唯一还没闭环的验收项**，闭环前不要开始 M4-T2（终端双宽按格定位的前提是 mono 真的是等宽）。


**提交习惯**：M1/M2 走 Comet change（归档提交只含 `docs/comet/**` 正式工件，实现文件另起一条 `feat(ui): ...`）；**M3 起直接以提交落地**（台账 §10 记录勘误）。`.workbuddy/memory` 单独 `chore(memory)`。不提交 `.agents/.claude/.codex/...` 的换行噪声文件。

---

## 2. 当前基线（下次开工前直接用，别重跑）

- `cargo check --workspace`：通过。
- `xgent_ui` lib：**76 passed**（与 M2b 同）。
- `xui` lib：**103 passed**（M2b 基线 + M3 新增 34）。
- 集成测试（`xgent_ui`，全绿）：`layout`(2)、`modal_settings`(3)、`toast`(1)、`settings_panel_values`(19)、`settings_panel_i18n`(3)、`interaction_model`(7)、`editor_theme_sync`(2)、`conflict_save_guard`(4)、`settings_input_focus`(2)、`density`(4)、`scroll_contract`(8)、`ellipsis_contract`(6)、`virtual_list_contract`(3)、**`file_preview`(7)**、**`file_tree_refresh`(5)**。
  - 标粗的两组在 M2b 基线是**长期失败**（见 §4.3），M3-T6 顺带修好了。

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
   - 缺 `xgent_ui::overlay::OverlayPlugin` → `DrawerToggle` 消息未注册 → **文件面板整体报错**（这条曾让 12 个测试长期红）。
   排查手段：临时给 bevy 加 `debug` feature，系统名会直接印在报错里（`bevy` 的 "Enable the debug feature to see the name"）。
5. **提交信息要写文件再 `git commit -F`**：PowerShell 管道按 GBK 代码页转码，中文提交信息会被搞坏（`git log` 里能看到乱码）。
6. **验收人独立性**：平台无法自证 Verifier 独立性，`confirm-skill-coordinated-pass` 必须由用户点头，不要代签。

---

## 4. 下一步：M4 跨平台字体与终端双宽（⚠ 硬门槛 A）

### 4.1 起点事实（2026-10-20 实测）

- 字体资产：`crates/xgent_app/assets/fonts/` 现有 **Inter-Variable.ttf**（879KB）+ **JetBrainsMono-Regular.ttf**（264KB，M4-T1 新增）+ `OFL.txt` + `README.md`（来源与许可说明）。
- `mono` 槽位已按候选栈装配（`xgent_ui::fonts::font_specs()`），**不再有** `cfg!(target_os)` 特权路径，也不再有 `Handle::default()` 兜底。
- `MonoMetrics`、双宽单元格、`display_width()` 全部**不存在**（grep = 0）→ M4-T2 未开工。
- 终端输出仍按 shaper 连续排版（`terminal/output.rs` 的 `font × 0.6` 估算仍在）。
- `type_scale` 仍无 `CODE` 档、`SMALL` 仍是 12.5、`line_height` 无 `CJK` → M4-T3/T4 未开工。

### 4.2 建议拆成 3 个 change（按风险从低到高）

| 顺序 | 范围 | 验收（可机器判定） |
|:---|:---|:---|
| 1 | **M4-T1 字体装配层** ✅ 已交付 | 见 §1.1（Windows 已闭环；macOS/Linux 待确认） |
| 2 | **M4-T3 代码与行号消费 `mono`** + **M4-T4 中文排版约束** ✅ 已交付 | 见 §1.1 |
| 3 | **M4-T2 终端双宽单元格**（⚠ 改动面最大）：`MonoMetrics` 启动期实测 + `display_width(char)`（wcwidth 语义）+ 终端按格数 × `cell_w` 手工定位（保留旧路径 feature flag 做 A/B 对照） | 三平台 `ls -l` / 中文输出 / `top` 列对齐正确；`vim` 全屏 TUI 不乱行 |

> 顺序理由：T1 是硬门槛（不满足就没必要做 T2 的按格定位）；T3/T4 只换字体与排版约束，风险低；T2 最重，独立提交便于回滚。**T2 开工前先确认 §1.1 的 macOS/Linux 日志**。

### 4.3 M3 顺带修好的 12 个长期失败测试（别再当成"基线失败"）

`file_preview`(7) + `file_tree_refresh`(5) 的根因**不是**产品缺陷，而是 headless 装配缺
`xgent_ui::overlay::OverlayPlugin`（`DrawerToggle` 消息未注册 → 文件面板的系统整体报错，
文件树从未重建）。M3-T6 补上插件并补齐装配缺项后全绿。
改动落在 `crates/xgent_ui/tests/file_preview.rs` 与 `tests/file_tree_refresh.rs`：
加插件/窗口/UI/picking/相机/资产；抽屉开关改走 `DrawerToggle` 消息（M1-T4 之后
`OverlayRegistry` 是唯一权威，直接改 Resource 会与注册表失配）。

---

## 5. M3 之后的可选方向（按台账 §8 里程碑表）

- **M5 槽位注册表**（⚠ 最大改动面）：`PanelRegistry` 取代 `SideViewContent` closed 枚举、派生可见性单一系统、`SplitTree` 分割模型。
- **M6 会话内容**：`Transcript` 有序树 + Markdown + 输入区（依赖 M3 的滚动/省略/填满契约；M3-T6 的会话消息列表虚拟化随 M6-T1 一起做，见台账 §10 勘误 13）。
- **M7 闭环**：快捷键单一来源、焦点栈、i18n 扩面、几何断言层（台账 §0.2 的 P0 缺陷回归测试多数在 M7 收口）。
- **M8 Git 探针**（北极星：新增 1 个 `PanelDescriptor` + 1 个视图模块，既有布局文件改动 = 0）。

---

## 6. 本轮遗留风险（Verifier 口径，不修也不违约）

1. **对话输入区的可视行数是 1 帧滞后**：`update_input_visible_lines` 在 PostUpdate 写 `visible_lines`，下一帧的 Content 阶段才生效——打字时输入框高度有极轻微的滞后感。
2. **光标行跟随也是 1 帧滞后**（同上，滚动位置写入 PostUpdate → 下一帧布局生效）。
3. **文件树最小高度 240px**：窗口极矮时树会超出抽屉（抽屉 `overflow: clip` 会裁掉底部）——极端小窗下的取舍，非典型场景。
4. **虚拟列表只覆盖固定行高场景**：不等高（会话消息列表）待 M6-T1。
5. **省略号测量每帧最多触发一次二分**（宽度不变时命中缓存）；拖窗口改变宽度时会重新二分（log2(n) 次排版），超大文本时可能有轻微开销。

---

## 7. 下次开工的前 5 步

1. `git log --oneline -8` 确认 M3 四个提交在 HEAD。
2. 读本文 §4 + `docs/plans/ui-v8-tasks.md` §M4。
3. 复核 §4.1 的三条事实（字体资产 / `startup.rs:88` / `MonoMetrics` 是否存在）。
4. 准备 JetBrains Mono 资产（OFL 1.1，subset 到 ASCII + 常用符号）并放进 `crates/xgent_app/assets/fonts/`。
5. 先做「硬门槛 A」的最小验证：三平台启动一次，日志打出 `mono` 命中的家族与来源。

---

## 8. 维护约定

- 每次里程碑交付后更新本文 **§1（交付表）** 与 **§2（基线数字/既有失败）**；§4 的「起点事实」在开工时复核一次。
- 任务级勾选仍记在 [ui-v8-tasks.md](ui-v8-tasks.md)；两边冲突时以「提交 + 代码实测」为准，并在台账 §10 记勘误。
