---
generated_from_state_version: 8
---

# 验证

## 当前结果

- 结果: **已归档**
- 验证情况: **已完成检查，验证结果已确认**
- 目标周期: 1
- 迭代: 1
- 验证器尝试次数: 1
- 完成时间: 2026-10-20T03:25:33.497Z
- 摘要: 9/9 通过。A1 偏好类型与老配置兼容、A4 daemon 键白名单由单测直接覆盖；A2 的三处数值断言确实落在真实插件节点（ActivityBarMarker / MessageListMarker / 设置卡片 ScrollArea）上并与 brief 文本逐项一致；A3 的幂等来自「基线为唯一写回源」的结构而非一次性算术；A5/A6 的 visits/releases 语义与「空闲帧零扫描」「松手回 hover 色」对应；A7 的 76 项 lib 测试 = 既有 69 + 新增 7，六个集成目标全绿且未见弱化断言；A8 的 fmt 由 Verifier 独立补验无 diff；A9 文档两处齐全且「M2b 待做」计数为 0。未发现实现与 brief/spec 的实质不符，仅留三条非验收项风险（同帧后段 spawn 的一帧舒适尺寸、主题色值变更时的 hover 色陈旧、三个未纳入计划的集成测试未重跑）。

## 验收

| 编号 | 结果 | 来源 | 验收项 | 原因 |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | A1 偏好缺省与兼容：`Preferences::default().density == Comfortable`；`density = "compact"` 可反序列化为 `Compact`；不含 `density` 键的老 TOML 反序列化后仍为 `Comfortable`（`xgent_settings_core` 单测）。 | crates/xgent_settings_core/src/global.rs:277-279 对 density 字段加 #[serde(default)]，Density 派生 Default 且 #[default] 在 Comfortable（:296-304）；单测 global::tests::density_defaults_and_parses（:370-396）断言 Preferences::default()==Comfortable、density="compact" 反序列化为 Compact、仅有 [providers.*] 的老 TOML 仍为 Comfortable、往返保持；density-units 日志：xgent_settings_core lib 14 passed / 0 failed。 |
| A2 | passed | brief.md | A2 切档即时重排：切到 `Compact` 后同一帧或下一帧，rail（8→6 / 4→3）、对话区（24→18、row_gap 20→15）、设置面板卡片（16→12、row_gap 8→6）的 `Node.padding` / `row_gap` 按 0.75 收紧（集成测试断言这三处具体数值）。 | 断言落在真实插件产出的节点上：tests/density.rs:52-92 取 ActivityBarMarker（layout.rs:174-193，padding vertical=space::SM=8、row_gap=XS=4）与 TopBarMarker，切 Compact 后 8→6、4→3、16→12、12→9 且 rail width 仍 52；:96-109 取 chat_panel::MessageListMarker（chat_panel.rs:173-177，padding XXL=24、row_gap XL=20）断言 24→18、20→15；:180-195 取设置面板卡片（settings_panel.rs:758-762，width=size::OVERLAY_SETTINGS_W=640、padding LG=16、row_gap SM=8，全仓仅此一处使用该宽度常量）断言 16→12、8→6。数值与 brief/spec 文本逐项一致；ui-density 日志 4 passed / 0 failed。生效时机由 density.rs:175-187 的单测（set 后仅 app.update() 一次即 18/15）证到「同一帧或下一帧」。 |
| A3 | passed | brief.md | A3 幂等不累乘：连续 5 帧不切档数值不变；重复应用 `Compact` 不再继续缩小；切回 `Comfortable` 回到基线（单测）。 | 不累乘由结构保证而非一次性算术：DensityBase 仅在 Without<DensityBase> 时捕获一次基线（density.rs:102-113），写回始终是 scale(base, density)（:47-79），从不以当前值再乘。单测 density_is_idempotent_and_reversible（:191-213）连续 5 帧断言仍为 12.0、切回 Comfortable 回到 16.0/8.0；集成测试 tests/density.rs:78-91 再跑 5 帧仍为 6.0 并逐字还原 8.0/12.0。 |
| A4 | passed | brief.md | A4 持久化：daemon 配置协调器写入 `preferences.density = "compact"` 成功（不再报「未知配置 key」）并可读回 `Compact`（`xgent_daemon` 单测）。 | crates/xgent_daemon/src/config_store.rs:87 读白名单返回 as_str()，:128-136 写白名单新增 preferences.density 且对非 comfortable/compact 直接报 Config 错误（不静默回落）；单测 config_store::tests::density_key_is_writable_and_readable（:471-491）验证默认读 comfortable、写 compact 后 config().preferences.density==Compact 且可读回 "compact"、非法值 "tight" 被拒且权威副本不变；density-units 日志：xgent_daemon 1 passed / 0 failed。 |
| A5 | passed | brief.md | A5 空闲帧零扫描：无 hover 变化的帧里 `HoverTintPollStats.visits == 0`（单测）。 | kit.rs:133-154 查询过滤改为 Or<(Changed<Hovered>, Changed<Pressed>)>，无变化的节点不进入迭代，每帧只重置并累加统计；单测 idle_frames_do_not_visit_any_node（kit.rs:824-840）在 3 个 HoverTint 节点稳定后连跑 3 帧断言 visits==0 且 releases==0（首帧 3 次仅是组件新增带来的 Added/Changed，符合语义）；ui-lib 日志含该用例并通过。 |
| A6 | passed | brief.md | A6 悬停/按下/松手色序：`Hovered(true)` → hover 色；`Pressed` 插入 → 按下色；`Pressed` 移除 → 回到 hover 色（不停在按下色）；对应帧 `visits > 0`（单测）。 | 松手单独由 hover_tint_release_system（kit.rs:159-179）读 RemovedComponents<Pressed>，按 hovered/pressed 重新 apply_tint，因此移除 Pressed 后回到 hover 色而非停在 active。单测 hover_press_release_sequence_restores_hover_color（:844-875）断言背景依次 subtle→theme.hover→theme.active→theme.hover→subtle，松手帧 releases==1 且 visits>0，前两段各 visits>0。 |
| A7 | passed | brief.md | A7 不回归：`xgent_ui` 既有 69 项 lib 测试与 `layout` / `settings_panel_values` / `settings_panel_i18n` / `toast` / `modal_settings` / `interaction_model` 集成测试全绿。 | ui-lib 日志 76 passed / 0 failed（= brief 的既有 69 项 + 新增 7 项：density 5 + kit 2）；ui-integration 日志六个目标全绿：interaction_model 7、layout 2、modal_settings 3、settings_panel_i18n 3、settings_panel_values 19、toast 1；核对 git diff，tests/modal_settings.rs 的改动仅为 rustfmt 换行，interaction_model.rs 无改动，未见为通过而弱化断言。已知基线失败（file_preview 7、file_tree_refresh 5、paths::daemon_socket_env_override、config_store::tests::global_write_lands_inside_the_isolated_dir）不在本 brief 的 A7 范围内，且与本次改动无因果关系。 |
| A8 | passed | brief.md | A8 静态门禁：`cargo check --workspace` 通过；`cargo clippy -p xgent_ui --no-deps`（lib）无新增告警；`cargo fmt -p ...` 后无 diff。 | workspace-check 日志以 `Finished dev profile` 结束、exit_code=0；clippy-ui 日志在 `Checking xgent_ui v0.1.0` 之后无任何告警（日志中的告警全部来自 xgent_tools/xgent_context 等既有 crate 的既有 lint），且以 exit_code=0 结束；fmt 由我独立执行 `cargo fmt -p xgent_ui -p xgent_settings_core -p xgent_app -p xgent_daemon --check`，无 diff 输出。 |
| A9 | passed | brief.md | A9 文档同步：`docs/dev-tutorial.md` 有 §5.19 且 `grep -c "M2b 待做"` 为 0；`docs/plans/ui-v8-tasks.md` 勾选 M2-T1（密度）/M2-T7 并追加勘误。 | docs/dev-tutorial.md:741 把 §5.18 末尾改为「M2b 已闭合」，:743 新增「### 5.19 密度档位与 hover 量化（v8 M2b）」并写明权威链、基线写回、只缩间距、入口与启动读取、hover 变更驱动与松手补帧；`grep -c "M2b 待做" docs/dev-tutorial.md` = 0；docs/plans/ui-v8-tasks.md:121 勾选 M2-T1 的密度部分、:149 勾选 M2-T7，§10（:366-370）追加三条勘误，含 ThemeHandle 单 generation 共用。 |

## 检查

| 检查 | 命令 | 工作目录 | 状态 | 退出码 | 耗时 |
| --- | --- | --- | --- | ---: | ---: |
| xgent_ui lib（含新增 7 项） | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo test -p xgent_ui --lib | . | passed | 0 | 2703 ms |
| xgent_ui 密度集成测试 | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo test -p xgent_ui --test density | . | passed | 0 | 3044 ms |
| xgent_ui 既有集成测试不回归 | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo test -p xgent_ui --test layout --test modal_settings --test toast --test settings_panel_values --test settings_panel_i18n --test interaction_model | . | passed | 0 | 55216 ms |
| 偏好与 daemon 密度单测 | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo test -p xgent_settings_core --lib global:: && cargo test -p xgent_daemon --bins config_store::tests::density | . | passed | 0 | 2914 ms |
| 全量编译检查 | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo check --workspace | . | passed | 0 | 13213 ms |
| clippy（lib，无依赖 lint） | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo clippy -p xgent_ui --no-deps --lib | . | passed | 0 | 11862 ms |

### Builder 报告的证据

以下为 Builder 报告，不等同于 Runtime 检查凭据或独立验收结果。

- xgent_ui lib 测试: passed — 76 passed（基线 69 + 新增 7）
- xgent_ui tests/density.rs: passed — 4 passed
- xgent_ui 既有集成测试（layout/modal_settings/toast/settings_panel_values/settings_panel_i18n/interaction_model）: passed — 全部通过
- xgent_settings_core 偏好单测: passed — global:: 14 passed
- xgent_daemon config_store 密度单测: passed — 25 passed（过滤 config_store）
- cargo check --workspace: passed — Finished
- cargo clippy -p xgent_ui --no-deps --lib: passed — xgent_ui 无告警
- cargo fmt --check: passed — FMT CLEAN
- 基线既有失败复核（stash 撤销本次改动后复跑）: passed — file_preview / file_tree_refresh / daemon 隔离测试在基线上同样失败
- 已知限制: M2b 只做两档（comfortable/compact）且只缩放间距；字号、图标、rail 宽 52、ICON_BTN 40、页签高度均不随密度变化（决策 D-2）。
- 已知限制: ThemeHandle 共用一个 generation：切密度时颜色重绘会多跑一遍（写入同一份主题值，视觉无差异），已在 ui-v8-tasks.md §10 记为勘误。
- 已知限制: 密度重排依赖 DensityBase 的首次捕获：若有系统在 spawn 之后另行改写 Node 的 padding/margin/gap，基线会过期（当前 crates/xgent_ui、crates/xui 中无此类后置写入，已 grep 确认）。
- 已知限制: xgent_daemon 的 global_write_lands_inside_the_isolated_dir 是基线既有的并行写冲突（stash 复跑同样失败），本次未修，避免扩大范围。
- 已知限制: file_preview（7 项）与 file_tree_refresh（5 项）在基线上即失败（Message not initialized），本次未修。

## 阻塞项

_无。_

## 风险与跳过的工作

- capture_density_bases 在 Update 中没有相对 spawn 系统的显式排序：若某系统在同一帧晚于它 spawn 节点，该节点当帧 PostUpdate 的布局仍按舒适间距跑一次，下一帧才收紧（一帧闪烁）。现有单测只覆盖「帧前已存在」与「切档」两种情形；本仓 layout/chat/settings 的节点均在启动或状态变更时生成，未观察到可见问题。
- hover 换色改为变更驱动后，唯一不再被覆盖的路径是「主题色值变化而节点状态不变」：正悬停的按钮不会在主题切换那一帧刷新 hover 色，要等下一次 Hovered/Pressed 变化。当前只有 dark 单一主题（spec 明确不做亮色主题），因此不可观测。
- A7 只按 brief 列出的六个集成测试目标取证；tests/settings_input_focus.rs、editor_theme_sync.rs、conflict_save_guard.rs 未在本次 Runtime 计划内重跑（静态核对：三者只使用 Theme，未构造 ThemeHandle 字面量，不含 density 相关断言，编译与行为风险低）。

## 之前的迭代

| 目标周期 | 迭代 | 尝试 | 结果 | 未解决项 | 摘要 | 完成时间 |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 1 | pass | — | 9/9 通过。A1 偏好类型与老配置兼容、A4 daemon 键白名单由单测直接覆盖；A2 的三处数值断言确实落在真实插件节点（ActivityBarMarker / MessageListMarker / 设置卡片 ScrollArea）上并与 brief 文本逐项一致；A3 的幂等来自「基线为唯一写回源」的结构而非一次性算术；A5/A6 的 visits/releases 语义与「空闲帧零扫描」「松手回 hover 色」对应；A7 的 76 项 lib 测试 = 既有 69 + 新增 7，六个集成目标全绿且未见弱化断言；A8 的 fmt 由 Verifier 独立补验无 diff；A9 文档两处齐全且「M2b 待做」计数为 0。未发现实现与 brief/spec 的实质不符，仅留三条非验收项风险（同帧后段 spawn 的一帧舒适尺寸、主题色值变更时的 hover 色陈旧、三个未纳入计划的集成测试未重跑）。 | 2026-10-20T03:25:33.497Z |



## 结论

9/9 通过。A1 偏好类型与老配置兼容、A4 daemon 键白名单由单测直接覆盖；A2 的三处数值断言确实落在真实插件节点（ActivityBarMarker / MessageListMarker / 设置卡片 ScrollArea）上并与 brief 文本逐项一致；A3 的幂等来自「基线为唯一写回源」的结构而非一次性算术；A5/A6 的 visits/releases 语义与「空闲帧零扫描」「松手回 hover 色」对应；A7 的 76 项 lib 测试 = 既有 69 + 新增 7，六个集成目标全绿且未见弱化断言；A8 的 fmt 由 Verifier 独立补验无 diff；A9 文档两处齐全且「M2b 待做」计数为 0。未发现实现与 brief/spec 的实质不符，仅留三条非验收项风险（同帧后段 spawn 的一帧舒适尺寸、主题色值变更时的 hover 色陈旧、三个未纳入计划的集成测试未重跑）。
