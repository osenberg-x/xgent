# UI v8 交接：进度、基线与下一步

> 用途：跨会话交接。**进度台账以 [ui-v8-tasks.md](ui-v8-tasks.md) 为准**（任务级勾选），本文记录「到哪儿了、基线是什么、下一步怎么开工、有哪些坑」。
> 更新时间：2026-10-20（M2b 归档后）。

---

## 0. 一句话现状

UI v8 的 **M1（分层/拾取/模态）、M2a（主题权威/令牌收敛）、M2b（密度档位/hover 量化）已交付并归档**；`cargo check --workspace` 绿，`xgent_ui` lib 76 项测试全绿。下一期按台账顺序是 **M3 滚动契约与省略**（备选：S1 3D 视口，需先补 G7 性能 spike）。

---

## 1. 已交付里程碑

| 期 | Comet change | 归档提交 | 实现提交 | 关键落点 |
|:---|:---|:---|:---|:---|
| M1 分层与模态 | `v8-m1-layering` | `f5fdb87` | `e7834fe` | `theme::z` 六静态层根 + `layout.rs` 层根 + `kit::layer_root`；`picking.rs` 与 `UiPickingSettings{require_markers:true}`；`modal.rs` 的 `ModalStack/Dismiss/ModalPush/Pop` 与栈深 z；`overlay.rs` 的 `OverlayRegistry` + `DrawerToggle`；tooltip 挂 TOOLTIP 层 |
| M2a 主题与令牌 | `v8-m2-tokens` | `7f67dc1` | `141d05c` | `ThemeHandle{theme,generation}` 唯一权威 + `Themed` 重绘；`Theme::dark()` 降 `pub(crate)`；`px()` 唯一入口；`theme::size` 尺寸令牌；星形/`Color::WHITE` 入 theme；窗口 1280×800 |
| M2b 密度与 hover | `v8-m2b-density-hover` | `9bb29c9` | `6d83d18` | `density.rs`（`DensityBase` 基线写回，幂等）；`Preferences.density` + `ThemeHandle.density`；设置面板密度分段按钮 + `SaveDensityMessage` + daemon 白名单；`kit.rs` hover 改为 `Changed<Hovered>/Changed<Pressed>` + `RemovedComponents<Pressed>` 松手帧 + `HoverTintPollStats` |

**提交习惯**：归档提交只含 `docs/comet/**` 正式工件；实现文件另起一条 `feat(ui): ...`（提交信息注明归档号）；`.workbuddy/memory` 单独 `chore(memory)`。不提交 `.agents/.claude/.codex/...` 的换行噪声文件。

---

## 2. 当前基线（下次开工前直接用，别重跑）

- `cargo check --workspace`：通过。
- `xgent_ui` lib：**76 passed**（基线 69 + M2b 新增 7）。
- 集成测试（全绿）：`layout`(2)、`modal_settings`(3)、`toast`(1)、`settings_panel_values`(19)、`settings_panel_i18n`(3)、`interaction_model`(7)、`editor_theme_sync`(2)、`conflict_save_guard`(4)、`settings_input_focus`(2)、`density`(4)。
- `xgent_settings`：14 passed。

**基线既有失败（不是回归，别当自己弄坏的）**——已用 `git stash` 撤改动复跑确认：

| 目标 | 数量 | 现象 |
|:---|:---|:---|
| `tests/file_preview.rs` | 7 | `Parameter ::messages failed validation: Message not initialized` |
| `tests/file_tree_refresh.rs` | 5 | 同上 |
| `settings_core::paths::daemon_socket_env_override` | 1 | Windows 下期望 `/tmp/.../daemon.sock`，实际 named pipe |
| `daemon::config_store::global_write_lands_inside_the_isolated_dir` | 1 | 多个测试并行写同一个临时配置文件，基线两次复跑都失败 |

复跑基线的办法：`git stash push -- <你有改动的源文件>` → 跑目标测试 → `git stash pop`。

---

## 3. 环境与工具坑（省时间，逐条都踩过）

1. **必须带**：`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`（本机全量 debug info 链接极慢甚至卡死）。Comet 检查计划里用 `cmd /c "set X=0&& set Y=0&& cargo ..."` 包装。
2. **Comet 检查尾部被标 `interrupted`**：先执行 continuation 的 `retry-checks`（`{"kind":"retry-checks","check_ids":[...]}`），Runtime 只重跑这些项并保留已通过项；**不要**一上来就 `doctor --repair`（会清空检查记录，得再用空 checks 分派、让 Verifier 读 `.comet/runtime/native/changes/<change>/logs/checks/*`）。
3. **`cargo` 报 `invalid metadata files for crate ...`**：target 被写坏了（多为并发/杀软占用）。`cargo clean -p <crate>` 后复跑；这一步偶尔被安全软件打断返回 SIGTERM，重试即可。
4. **`cargo fmt`** 只能用 `-p <crate>`，`--all` 会改写 `../bevy` 检出。
5. **headless 测试装配**：`ChatPanelPlugin` 需要 `UiFonts`、`MeshPlugin`、`Clipboard` 与一批 agent 消息（`DeltaMessage`/`BoundedMessage`/`ToastMessage`/`ChatInputSubmitted` 等），缺了只报 `Resource does not exist` 且看不到系统名（bevy `debug` feature 未开）。完整清单见项目知识「ChatPanelPlugin 的 headless 测试装配」，实现样例见 `crates/xgent_ui/tests/density.rs`。
6. **验收人独立性**：平台无法自证 Verifier 独立性，`confirm-skill-coordinated-pass` 必须由用户点头，不要代签。

---

## 4. 下一步 A：M3 滚动契约与省略（推荐）

### 4.1 起点事实（2026-10-20 实测，不是文档里的旧数字）

- `grep -rn "Overflow::clip_y()" crates/xgent_ui/src` = **9 处 / 7 个文件**：`file_panel.rs`、`session_history.rs`(×2)、`command_palette.rs`(×2)、`confirm_dialog.rs`、`editor/mod.rs`、`settings_panel.rs`、`tool_panel.rs`。
- `xui` **没有** `ScrollFrame`；`xgent_ui`/`xui` **没有** `region_fill()`；**没有** `Ellipsis`；`xui::VirtualList` **0 消费者**。
- 正确形态的参照实现：`crates/xui/src/scroll_area.rs`（`x: Hidden, y: Scroll` + `ScrollPosition` + 视口 + 滚动条），`scroll_to_child` 已公开。

### 4.2 建议拆成 3 个 change（按风险从低到高，每个都能独立回滚）

| 顺序 | 建议 change 名 | 范围 | 验收（可机器判定） |
|:---|:---|:---|:---|
| 1 | `v8-m3-scroll-frame` | M3-T1 + M3-T2：`xui::ScrollFrame::{vertical,horizontal,stick_bottom}`（overflow 由构造器定，不接受外部覆盖）+ 迁移 9 处；`settings_panel` 模型列表（`max_height:150` 且无 `ScrollPosition`）一并接入 | `grep -c "Overflow::clip_y()" crates/xgent_ui/src` = 0；7 处各塞 200 条滚轮可达末项（集成测试） |
| 2 | `v8-m3-fill-and-input` | M3-T3：`region_fill()` 替换「有兄弟节点的列里 `height:100%`」（`editor/mod.rs`、`terminal/mod.rs`、`diff_view.rs`）；`chat_panel` 输入框自适应到 200 后内部滚动 + 光标行可见；chips 行改 wrap | 700px 高窗口内终端状态条完整可见；长输入不隐藏当前行 |
| 3 | `v8-m3-ellipsis` | M3-T4 + M3-T5：自建 `Ellipsis`/`Clamp`（`TextPipeline::update_text_layout_info` → `TextLayoutInfo.size` 测量，`(源哈希, 量化宽, 字号)` 缓存 + 2px 迟滞）+ 接 6 处截断点（路径用 `MidEllipsis`），完整值进 tooltip | 6 处宽度不足出 `…` 而非溢出；拖窗口不逐帧重排（缓存命中可观测） |
| 4（可选，最重） | `v8-m3-virtuallist` | M3-T6：会话消息列表 + 文件树接 `VirtualList`，> 50 条才启用，不等高行用「估算 + 首帧实测回写」 | 500 条滚动不明显劣化；消费者 ≥ 2 |

> 顺序理由：T1/T2 是「唯一入口 + 迁移」，改完立刻让 `clip_y` 归零、后续几项才有正确基座；T4 是引擎缺口（bevy 0.20 无 `TextTruncate`），自建测量风险最高，单独 change 便于回滚。

### 4.3 开工前读什么

- `docs/plans/ui-v8-tasks.md` §M3（任务原文与行号）
- `docs/design/ui-v8-layout-architecture.md` §4（滚动契约）、§5（省略与测量）
- `crates/xui/src/scroll_area.rs`、`crates/xgent_ui/src/chat_panel.rs:174-175`（现有正确用法样例）

---

## 5. 下一步 B（备选）：S1 3D 视口

- 闸门：M1 已满足 **G1（层根/拾取）**、M2a 已满足 **G2（主题热驱动）**；**G7 性能 spike 仍缺**（2k 节点、实例化/合批、降级阶梯）。
- S1 范围：`xgent_spatial` 图 → 离屏纹理（`RenderTarget::Image`）→ UI `ImageNode` 呈现 + 只读相机（轨道/缩放/拾取）。**不做**：写回代码、L3 源码渲染、时间轴（S3）；L1/L2（文件/符号）数据也不在 S1。
- `Slot::Center` 是 **S2** 的前提（G3），S1 不需要。
- 建议先单开一个 `v8-s1-perf-spike` change 交 G7 结论，再决定是否进 S1；不要和 M3 并行（都要动 `xgent_ui` 主区结构）。

---

## 6. 本轮遗留风险（M2b，Verifier 已记录；不修也不违约）

1. **同帧后段 spawn 的节点有一帧舒适间距**：`capture_density_bases` 在 `Update` 里没有相对各 spawn 系统的排序约束。若想彻底消除，把它挪到 `PostUpdate` 且 `.before(UiSystem::Layout)`；改动很小但要重跑验收。
2. **主题换色时正悬停节点不会即时刷新 hover 色**：改成变更驱动后的固有代价，等下一次指针状态变化；当前只有 dark 单主题，不可观测。

---

## 7. 下次开工的前 5 步

1. `comet native status --json` 确认无活动 change（当前 14 条全是 Archive）。
2. 读本文 §4 + `docs/plans/ui-v8-tasks.md` §M3。
3. 复核 §4.1 的三条 grep（数字会随 M3 推进变化）。
4. `comet native new v8-m3-scroll-frame --isolation current --language zh-CN --json`，Shape 只圈 T1+T2。
5. Shape 里的验收写死成：`clip_y` = 0 + 7 处 200 条可达末项 + 既有 lib/集成测试不回归。

---

## 8. 维护约定

- 每次 change 归档后更新本文 **§1（交付表）** 与 **§2（基线数字/既有失败）**；§4 的「起点事实」在开工时复核一次。
- 任务级勾选仍记在 [ui-v8-tasks.md](ui-v8-tasks.md)；两边冲突时以「归档 change + 代码实测」为准，并在台账 §10 记勘误。
