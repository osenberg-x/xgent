# Verification Report: mvp2-3d-home

- 日期：2026-10-30（复核 2026-11-02）
- 模式：full
- 基线：`5b68211`
- 实现提交：`f1a552c`（主体）、`42d5cde`（复审修复）、`5aec438`（终端布局断言）、`9bb6ed3`（刷新相机断言）
- 验证命令：`cargo build`；`cargo test -p xgent_spatial -p xgent_ui -p xgent_app -j 2`（`CARGO_PROFILE_TEST_DEBUG=0`）
- Runtime 证据：build `docs/openspec/changes/mvp2-3d-home/.comet/checks/dfe28686-8b6f-4077-a77b-f9673d0fe9a8.log`；verify `docs/openspec/changes/mvp2-3d-home/.comet/checks/c508e102-87fc-46bd-925d-6adc5619bef2.log`（346 passed / 0 failed / 1 ignored）
- 状态：通过（无 CRITICAL / IMPORTANT 未决项）

## Summary

| Dimension | Status |
|:---|:---|
| Completeness | 27/27 tasks；3 个 delta spec；15 条 requirement、29 个 scenario |
| Correctness | 15/15 requirement 有实现映射；29 个 scenario 有代码/测试/走查证据 |
| Coherence | 遵循 design.md D1–D10 与 Implementation Notes；无 delta/design 冲突 |

## 1. Completeness

- [x] tasks.md 27/27 全部完成（`comet state tasks` 记录 27/27）。
- [x] 3 个 delta spec 均已落地：`3d-home-view`、`spatial-code-graph`、`workspace-layout`。
- [x] 变更文件与任务范围一致：`crates/xgent_spatial`（L1/忽略/上限/布局）、`crates/xgent_ui`（space/layout/panels/resize/shortcuts/settings）、`crates/xgent_app`（settings_io/G7 spike）、`crates/xgent_daemon`（preferences 白名单）、`crates/xgent_settings*`（偏置/FTL）、文档与 OpenSpec 产物。
- [x] 计划映射 `unmappedCount=0`。

## 2. Correctness：Requirement → Evidence

### 3d-home-view

| Requirement | Evidence |
|:---|:---|
| 3D 首页默认可见 | `layout_mode::universe_layout_is_the_default`；真实应用截图 `target/walkthrough/shots/universe3.png` |
| 轨道相机、数据刷新不漂移 | `space::view::orbit_transform_is_stable_and_responds_to_yaw`；`space_panel::graph_refresh_keeps_camera`；`sync_space_graph` 的 `framed` 只首帧取景 |
| 悬停/选中/打开、Esc 返回 | `space_panel` 投影/拾取测试、`escape_returns_from_editor_to_space_and_keeps_camera`、`enter_on_module_does_not_open_editor`；`pick_space_stars` 仅 `File` 打开 |
| 首期键盘可达 | `bracket_keys_select_and_enter_opens_file`（`[`/`]` 选中、Enter 打开）、`text_focus_blocks_enter_open`（输入焦点让位）、方向键/`+`/`-` 相机、Esc 返回 |
| G7 性能门与降级 | `docs/notes/g7-3d-perf.md`：2k@60.0fps、真实仓库 767 节点 59.9fps；`degrade_ladder_thresholds`；release 复跑在 30fps 环境正确触发 Full→800→2D |
| 3D 不渲染源码 | 视口只渲染星体/边；文件经 `OpenFileRequest` 交棒 2D 编辑器（`space_panel` 断言） |

### spatial-code-graph

| Requirement | Evidence |
|:---|:---|
| 任意项目 L1 | `l1_nodes_and_contains_edges`（Rust）、非 Rust fixture、`malformed_manifest_still_yields_directory_graph` |
| 扫描忽略规则 | `ignore_rules_skip_dependency_dirs_and_gitignore`；`scan_with_options` 对缺失/解析失败 `.gitignore` 只 `warn` 一次 |
| 节点上限与聚合 | `node_cap_aggregates_and_reports_omitted`、`ten_thousand_files_are_capped`；HUD 角标显示「已省略 N」（`space_panel` 断言） |
| 确定性布局与增量锚定 | `layout::full_layout_is_bitwise_deterministic`、`incremental_layout_keeps_existing_positions`、`l1_scan_is_deterministic` |
| 只读投影 | 扫描/布局/视口代码无写文件路径；仅 `FileChangedEvent` 触发重扫 |

### workspace-layout

| Requirement | Evidence |
|:---|:---|
| 3D 主区与对话侧栏 | `layout_mode::universe_layout_is_the_default`、`classic_layout_restores_chat_as_center` |
| 打开文件/终端接管主区、对话仍可见 | `layout_mode::terminal_handoff_keeps_chat_in_both_modes`；编辑器由 `space_panel`/`editor` 路径覆盖 |
| 主区单活跃、Esc 返回 | `panels` 单例槽位测试、`space_panel::escape_returns_from_editor_to_space_and_keeps_camera`、`layout_mode` 切换断言 |
| 2D fallback 功能完整 | `layout_mode` 三态测试；`docs/notes/mvp2-layout-walkthrough.md`；`cargo test -p xgent_ui` 全部既有测试绿 |
| 侧栏可调宽、重启保留 | `chat_side_respects_range_and_ratio`、`only_universe_side_dock_drag_is_persisted`、`chat_side_preference_applies_only_in_universe`；`preferences.chat_side_width` 读写测试 |

## 3. Coherence / Design Adherence

- D1 G7 先行：G7 报告与真实仓库复测均已记录；release 自动退化已接。
- D2 离屏纹理、D3 CPU 拾取、D4 L1 无 Bevy、D5 `ignore` crate、D6 目录聚合/2k、D7 Center 注册面板、D8 400px 侧栏、D9 `three_d_home` 偏好、D10 只读数据流均按设计落地。
- Design Doc 的 Implementation Notes 已记录 S1 键盘切片、自动退化、刷新、L1 健壮性与节点语义，delta spec 与 design 无未解释冲突。

## 4. Independent Review

- 第 1 轮独立复审：2 个 CRITICAL（键盘输入泄漏、自动退化缺失）、多个 IMPORTANT（目录节点被当文件打开、缩放夹取、SideDock 常驻、省略数、忽略警告、刷新、键盘选中）。
- 第 2 轮复审：确认修复，另发现点击视口外误清焦点（CRITICAL）与 Static 跨模式不复位（IMPORTANT），并建议 Reduced 关边、共享 `SKIP_DIRS`、切换恢复 720。
- 第 3 轮复审：确认焦点门控、Static 复位、Reduced 关边、720 恢复、遮挡门控正确；仅剩刷新路径过滤的 IMPORTANT（已改为按项目根相对路径）与低风险建议。
- 所有 CRITICAL/IMPORTANT 已修复并有针对性测试；无未决阻塞项。

## 5. Issues

### CRITICAL

无。

### IMPORTANT

无。

### WARNING / SUGGESTION（不阻塞归档）

- 自动退化按显示刷新率调整预算；本次复核会话被 DWM/远程限到 30fps，产品与同机 G7 spike 均为 30fps，因此自动退化按设计触发并持久化 2D。60fps 结论采用早先同机 59.9fps 实测。
- Windows 上 winit 0.30 不发 `WindowOccluded`，遮挡限帧无法被该事件识别；当前以焦点 + 显示器刷新率预算缓解。隐藏窗口仍可能触发自动退化，属平台限制。
- 编辑器挡住 3D 时（Universe 且 space 不可见），文件变更仍会防抖重扫；这是后台数据保鲜的取舍，未做可见性门控。
- `space.show` 在 Classic 下是静默 no-op（命令入口未隐藏），属体验细节。
- 遮挡门控/刷新防抖没有专门的 headless 测试；刷新与相机不漂移、Reduced 关边已有测试。

## 6. Final Assessment

无 CRITICAL / IMPORTANT 未决项；WARNING/SUGGESTION 已记录且不影响验收。Verification 通过，可进入 Archive（归档仍需用户确认交付方式）。
