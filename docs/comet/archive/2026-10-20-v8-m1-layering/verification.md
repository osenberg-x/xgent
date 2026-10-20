---
generated_from_state_version: 13
---

# 验证

## 当前结果

- 结果: **已归档**
- 验证情况: **已完成检查，验证结果已确认**
- 目标周期: 1
- 迭代: 3
- 验证器尝试次数: 1
- 完成时间: 2026-10-20T01:30:08.423Z
- 摘要: iteration 3 复验通过。逐文件阅读 modal/settings_panel/layout/kit/picking/overlay/shortcuts/confirm_dialog/file_panel/command_palette 与 tests/modal_settings.rs，确认修复语义成立：ModalPlugin 的 (route_modal_input, apply_modal_pop, apply_modal_push) 链保证同帧先出栈后入栈，重建的面板不再被 contains 跳过，重新以 depth 0 入栈拿到 z=400，Esc 与焦点恢复路径随之恢复。独立重跑（CARGO_PROFILE_DEV_DEBUG=0 / TEST_DEBUG=0）：xgent_ui lib 68 passed、modal_settings 3 passed、layout 2 passed、clippy -p xgent_ui -p xgent_app --lib --no-deps -D warnings 通过；未改动任何项目文件。遗留风险均为窄场景（pop 后不重算剩余层 z、重建时 opener 可能指向已 despawn 输入框且未做存活检查、一帧未入栈窗口、A2/A5 缺装配级断言），不阻塞 M1 验收。

## 验收

| 编号 | 结果 | 来源 | 验收项 | 原因 |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | 六个静态层根存在且 z 值来自 `theme::z`；业务代码中 `GlobalZIndex(` 只出现在 `layout.rs`/`kit.rs` 的层根 spawn 与 `modal.rs` 的栈深派生处。 | layout.rs 在 UiRoot 下并列 spawn BASE/DOCK/DRAWER/DROPDOWN/TOOLTIP/TOAST 六个层根，均走 kit::layer_root（Node 全屏绝对定位 + GlobalZIndex(band) + BackgroundColor::NONE + LayerRoot(band)），TopBar/MainArea/StatusBar 为 BASE 层根子节点。grep GlobalZIndex( 仅命中 layout.rs:125、kit.rs:190/198/203/658、modal.rs:171，全部取 theme::z 常量或栈深派生，无业务裸写数字。layout.rs 两个单测（six_layer_roots_have_expected_z_and_no_pickable、main_regions_live_under_base_layer）本轮独立重跑通过。 |
| A2 | passed | brief.md | `UiPickingSettings.require_markers == true`；层根与布局壳不带 `Pickable`；所有带 `Button` 组件的实体都带 `Pickable`；空层根不吞点击。 | xgent_app/main.rs:260-264 装配 UiPickingSettings{require_markers:true}；layer_root 不含 Pickable（layout 测试断言 LayerRoot 实体 0 个带 Pickable），布局壳/装饰节点亦无；picking.rs 的 Added 系统为 Button 与 EditableText 统一补 Pickable::default()（block+hoverable），46 处 Added<Pressed> 交互点抽查均为 Button（含 resize 手柄）。空层根不吞点击由 require_markers + 无 Pickable 结构性成立，现有交互/布局测试无回归。 |
| A3 | passed | brief.md | 设置面板打开时按 Esc 只关闭面板、不触发 `chat.abort`；确认弹窗在栈顶时独占键盘；模态 z 等于 `400 + depth*8`。 | 重点复验重建场景：modal.rs:123-128 把 apply_modal_pop 排在 apply_modal_push 之前并 .chain()，同帧先按 id 移除旧层、再让新实体入栈，故 contains(id) 不再吞掉新面板；入栈时按 stack.len() 写 400+depth*8（modal.rs:168-171），超 MODAL_MAX_DEPTH 拒绝。Esc 经 route_modal_input 只作用于栈顶并置 ModalEscConsumed，shortcuts.rs:141-146 在栈非空/已消费时排空消息，chat.abort 不派发；confirm 以 NotDismissable 入栈且 confirm_dialog.rs:469-475 以 stack.top()=="confirm" 独占键盘。独立重跑 modal_settings 3 项（含 rerender_keeps_settings_registered_in_modal_stack）与 modal 单测 stack_push_pop_and_depth_z 均通过，Esc 场景断言 AbortMessage 计数为 0。 |
| A4 | passed | brief.md | 通过 rail 文件钮、「+ 添加」、`filepanel.toggle` 三个入口打开文件抽屉时，历史抽屉自动关闭；反向亦然；设置面板打开时抽屉不遮挡模态。 | overlay.rs 以 OverlayRegistry + DrawerToggle 单一裁决（同 id 再切即关、开新关旧），rail 文件/历史钮、context_scope「+ 添加」、filepanel.toggle 快捷键、命令面板历史项、file_panel 关闭路径全部只发 DrawerToggle，无残留手工互斥；抽屉 z 走 kit::drawer_z()=z::DRAWER(200) < MODAL_BASE(400)，设置面板在 400 以上，抽屉不遮挡模态。overlay/activity_bar 互斥测试通过。 |
| A5 | passed | brief.md | 最右齿轮钮的 tooltip 完整可见、不被 `MainArea` 的 `Overflow::clip()` 裁切；右缘空间不足时翻转到左侧；tooltip 实体不在宿主子树内。 | kit.rs spawn_tooltip_panel 以 ChildOf(TOOLTIP 层根) 挂载，不再作为宿主子节点，因而不受 MainArea/ChatPanel 的 Overflow::clip() 影响；实体带 Pickable{should_block_lower:false,is_hoverable:false}；position_tooltips 每帧按宿主 ComputedNode/GlobalTransform 定位，tooltip_left 在 host_right+gap+tip_width>viewport 时翻转到左侧（含 0 下限）。tooltip_flips_at_viewport_edge 单测通过。 |
| A6 | passed | brief.md | 设置面板按 Esc 与点击遮罩均可关闭；关闭后焦点回到打开前的实体。 | 设置面板以 Dismiss::EscAndScrim 入栈（settings_panel.rs:1354），Esc 经 ModalDismissRequested 置 open=false、遮罩 Added<Pressed> 同样置 false，两条路径都由 toggle_panel 发 ModalPop 并由 apply_modal_pop 把 InputFocus 还原到该层 opener_focus。重建路径复验：pop 先于 push 使新面板重新以 depth 0 入栈并拿到 z=400（测试断言），关闭后焦点仍回到打开前实体；独立重跑 esc_closes_settings_through_modal_stack 通过（焦点断言 opener）。 |

## 检查

_没有记录 Runtime 检查。_

### Builder 报告的证据

以下为 Builder 报告，不等同于 Runtime 检查凭据或独立验收结果。

- cargo test -p xgent_ui --lib: passed — 68 passed / 0 failed（含层根、模态栈、抽屉、拾取、tooltip 翻转）
- cargo test -p xgent_ui --test modal_settings: passed — 3 passed（Esc/焦点、关闭按钮、tick 重建后仍在栈顶）
- cargo test -p xgent_ui --test layout --test settings_panel_values --test toast: passed — 2 + 19 + 1 passed，既有布局/设置/ toast 回归通过
- cargo check --workspace: passed — 通过；xgent_tools 一条既有 unused_must_use 警告，与本 change 无关
- cargo clippy -p xgent_ui -p xgent_app --all-targets -- -D warnings: passed — 库目标通过（--lib --no-deps -D warnings）；测试目标存在既有 lint（tests/common/mod.rs 等），未纳入本次门禁
- 已知限制: 本机 GUI 未启动，点击链路只做结构断言（require_markers + Pickable 白名单 + 层根无 Pickable），未做人工逐面板点击。
- 已知限制: 全量 `cargo test -p xgent_ui --tests` 在本机因 11 个测试二进制的链接耗时未跑完；已按相关子集（lib/modal_settings/layout/settings_panel_values/toast）验证，其余测试未运行。
- 已知限制: 命令面板与冲突弹窗按 D5 保持现状、未迁入 ModalStack；它们的 Esc/键盘语义未改变。
- 已知限制: tooltip 的最终像素位置依赖运行时布局，headless 只验证翻转算法与挂载层，未做截图断言。
- 已知限制: 工作区已从 E: 迁到 F:（原盘写满导致构建产物损坏）；本 handoff 的检查均在 F:\ws\xgent 执行。
- 已知限制: clippy 只对库目标启用 -D warnings；测试目标的既有 lint（tests/common/mod.rs 等）未清理。
- 已知限制: ModalPlugin 采用「先 pop 后 push」的同帧顺序；若未来出现同帧 push+pop 同一 id 的相反语义需求，需要重新审视。

## 阻塞项

_无。_

## 风险与跳过的工作

- apply_modal_pop 移除某层后不重算其余层的 GlobalZIndex：若设置面板在栈底、confirm 在栈顶时发生快照重建，pop 掉 settings 后 confirm 仍留在 408，重建的 settings 又以 depth 1 拿 408，两层 z 相等、栈顶与视觉顶层可能不一致（Esc 仍按栈顶正确路由）。窄场景，建议 pop 后按新 depth 重刷剩余层。
- 重建时 opener_focus 取的是 ModalPush 写入当刻的焦点（settings_panel.rs:1355）。若焦点在面板输入框（TabIndex 输入框被点击后点保存触发 tick 推进重建），重建后该 opener 指向已 despawn 的输入框；apply_modal_pop 未做实体存活检查，关闭后焦点被设到死实体而非清空，与 spec「该实体已不存在时清空焦点」有偏差（无 panic，仅键盘无落点）。
- 生产装配里 ModalPlugin 在 SettingsPanelPlugin 之前、测试 panel_app 中相反，两种顺序都因 .chain() 的 pop→push 而正确；但生产顺序下新面板存在「本帧已 spawn、下帧才入栈」的窗口，该帧无 GlobalZIndex（z=0），若此时抽屉(200)打开会被盖住一帧。
- A2 的「每个 Button 都有 Pickable」仅由运行时 Added 系统保证，缺少整体装配级断言；若存在按 Pressed 但未挂 Button 的叶子，在 require_markers 下会静默点不动（硬门槛 B 的残余风险）。
- A5 只有 tooltip_left 纯函数单测，未断言 tooltip 实体确实挂在 TOOLTIP 层根下（代码为 ChildOf(layer)，无自动化覆盖）；层根缺失时 spawn 会退化为无父节点。

## 之前的迭代

| 目标周期 | 迭代 | 尝试 | 结果 | 未解决项 | 摘要 | 完成时间 |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 1 | fail | A3, A6 | A1/A2/A4/A5 通过（层根与 z 单源、require_markers 拾取、抽屉三入口统一裁决、tooltip 顶层化与翻转均按语义落实）；A3/A6 失败：设置面板重渲染（每次打开面板因快照 tick 推进必然发生）时同帧写 ModalPop+ModalPush 同一 id，ModalPlugin 先处理 push 后处理 pop，导致新面板被 contains 跳过、旧层被移除，面板实际脱离模态栈——Esc 不再被消费会触发 chat.abort，关闭时焦点也不还原。已有 2 项 ui-modal-tests 未推进 tick，故未覆盖该路径。 | 2026-10-20T00:58:53.897Z |
| 1 | 2 | 0 | recovery | — | Native check input changed after the candidate was built; a new Builder candidate is required before checks can run again. | 2026-10-20T01:11:59.800Z |
| 1 | 3 | 1 | pass | — | iteration 3 复验通过。逐文件阅读 modal/settings_panel/layout/kit/picking/overlay/shortcuts/confirm_dialog/file_panel/command_palette 与 tests/modal_settings.rs，确认修复语义成立：ModalPlugin 的 (route_modal_input, apply_modal_pop, apply_modal_push) 链保证同帧先出栈后入栈，重建的面板不再被 contains 跳过，重新以 depth 0 入栈拿到 z=400，Esc 与焦点恢复路径随之恢复。独立重跑（CARGO_PROFILE_DEV_DEBUG=0 / TEST_DEBUG=0）：xgent_ui lib 68 passed、modal_settings 3 passed、layout 2 passed、clippy -p xgent_ui -p xgent_app --lib --no-deps -D warnings 通过；未改动任何项目文件。遗留风险均为窄场景（pop 后不重算剩余层 z、重建时 opener 可能指向已 despawn 输入框且未做存活检查、一帧未入栈窗口、A2/A5 缺装配级断言），不阻塞 M1 验收。 | 2026-10-20T01:30:08.423Z |



## 结论

iteration 3 复验通过。逐文件阅读 modal/settings_panel/layout/kit/picking/overlay/shortcuts/confirm_dialog/file_panel/command_palette 与 tests/modal_settings.rs，确认修复语义成立：ModalPlugin 的 (route_modal_input, apply_modal_pop, apply_modal_push) 链保证同帧先出栈后入栈，重建的面板不再被 contains 跳过，重新以 depth 0 入栈拿到 z=400，Esc 与焦点恢复路径随之恢复。独立重跑（CARGO_PROFILE_DEV_DEBUG=0 / TEST_DEBUG=0）：xgent_ui lib 68 passed、modal_settings 3 passed、layout 2 passed、clippy -p xgent_ui -p xgent_app --lib --no-deps -D warnings 通过；未改动任何项目文件。遗留风险均为窄场景（pop 后不重算剩余层 z、重建时 opener 可能指向已 despawn 输入框且未做存活检查、一帧未入栈窗口、A2/A5 缺装配级断言），不阻塞 M1 验收。
