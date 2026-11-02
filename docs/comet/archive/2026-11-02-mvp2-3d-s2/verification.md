---
generated_from_state_version: 11
---

# 验证

## 当前结果

- 结果: **已归档**
- 验证情况: **已完成检查，验证结果已确认**
- 目标周期: 1
- 迭代: 2
- 验证器尝试次数: 1
- 完成时间: 2026-11-02T03:33:43.204Z
- 摘要: pass：上一轮 CRITICAL（搜索 Enter 被 ChatInput 提交观察者先 clear 查询、result 重填全量并选中首项）已修复。搜索框不再挂 ChatInput（s2.rs:475-499），bevy TextInput 在 allow_newlines=false 时忽略 Enter；sync 仍先读当前查询再处理 Enter，无匹配 Enter 保持 inert，二次 Enter 经 suppress_enter/pick_space_stars 打开同一路径。cargo test -p xgent_ui -j 2 EXIT=0（153 lib + 28 集成全绿），space_s2 11、space_panel 10、layout_mode 4 全过。A2/A3/A11/A12/A14 由代码路径 + 结构断言判定通过；real-stack 用例缺 PrimaryWindow 未真正派发 FocusedInput，已列入风险。

## 验收

| 编号 | 结果 | 来源 | 验收项 | 原因 |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | A1 **Tab 遍历不转相机**：宇宙可见且文本输入框无焦点时，连续按 `Tab` 能遍历全部可见节点并回绕；每次仅选中态与相机 target 改变，yaw/pitch/dist 保持不变；`Shift+Tab` 反向。 | crates/xgent_ui/src/space/s2.rs:527-598 的 traverse_space_selection 在宇宙可见、无模态、无文本焦点且搜索关闭时处理 Tab/Shift+Tab，按 project_order(600-630) 循环并用 rem_euclid 回绕；仅写 pick.selected 与 orbit.target，yaw/pitch/dist 只在顺序重算时记录不改写。tests/space_s2.rs:140-166 覆盖前进/反向/回绕与相机不变，:169-186 覆盖文本焦点与搜索阻止。 |
| A2 | passed | brief.md | A2 **搜索飞行不打开编辑器**：按 `/` 输入文件名片段，`↑`/`↓` 选择，`Enter` 后搜索关闭、选中目标星体且相机飞到它；编辑器未打开。 | CRITICAL 已修复：搜索框改为 EditableText{allow_newlines:false}+TextInput（s2.rs:475-499），不再挂 xui::input::ChatInput——它是唯一在 Enter 时 clear 文本的观察者（crates/xui/src/input.rs:129-154）；bevy TextInput 对 allow_newlines=false 的 Enter 忽略并传播（../bevy/crates/bevy_ui_widgets/src/text_input.rs:241-260）。s2.rs:229-246 先按当前 EditableText 过滤，:249-281 再处理 Enter 选中 results[selected] 并 fly_to_node；view.rs:841-847 搜索打开时短路拾取，:939-946 以 suppress_enter 吞同帧 Enter。space_s2.rs:341-354 断言搜索框无 ChatInput，:188-224 断言飞行目标、相机与编辑器未打开。 |
| A3 | passed | brief.md | A3 **键盘完成找文件→打开**：在 A2 之后再次按 `Enter`，主区切换到 2D 编辑器并打开搜索选中的同一路径。 | 首次 Enter 关闭搜索并清 InputFocus（s2.rs:271-280、320-337），view.rs:845-847 吞掉同帧 Enter；再次 Enter 经 pick_space_stars(view.rs:939-946) 调 open_selected(983-1009) 按 pick.selected 星体 path 发 OpenFileRequest。space_s2.rs:188-224 断言第二次 Enter 只打开一次且路径以 src/file1.rs 结尾。 |
| A4 | passed | brief.md | A4 **2D→3D 选中同步**：在编辑器中切换活动文件（或从文件抽屉打开文件）后，3D 图选中同路径的 File 星体且相机不移动；图中不存在的文件清除选中。 | sync_selection_from_editor(s2.rs:633-677) 在 EditorTabs 变化时按绝对路径匹配 File 星体写 pick.selected，不触碰相机；图中不存在则清空，图刷新而用户已有 3D 选中时不覆盖。space_s2.rs:246-285 断言 file1 选中、相机 target 不变、src/not-in-graph.rs 清选中。 |
| A5 | passed | brief.md | A5 **Esc 逐级不穿透**：搜索打开时 Esc 只关搜索；编辑器/终端可见时 Esc 回宇宙并保留选中；宇宙有选中时 Esc 只清选中；只有宇宙无搜索无选中时 Esc 才中断对话。 | consume_space_escape(s2.rs:803-846) 优先关搜索（清输入焦点），否则宇宙有选中时只清选中并置 SpaceEscConsumed；crates/xgent_ui/src/shortcuts.rs:224-235 在 consumed 时整体 drain 让位，编辑器/终端/差异回宇宙走 :243-279。tests/space_panel.rs 三级 Esc 用例与 space_s2.rs:227-243 断言不穿透。 |
| A6 | passed | brief.md | A6 **回退隔离**：经典 2D 回退下 `/`、`Tab` 不改变布局或触发 3D 搜索；MVP-1 的对话/编辑器/终端/抽屉/设置/历史行为不回归。 | S2 键入口均以 space_visible 门控（s2.rs:209-217、196-226、543），经典回退由 layout_mode.rs:162-165 让 SPACE 失活；space_s2.rs:305-315 与 layout_mode.rs 4 项通过。本轮 cargo test -p xgent_ui -j 2 EXIT=0：153 lib + 28 个集成测试全绿（含 space_s2 11、space_panel 10、layout_mode 4），MVP-1 无回归。 |
| A7 | passed | brief.md | A7 **选中反馈**：悬停星体 120ms 后出现 label/path tooltip；单击后出现详情卡（label/path/类型/Enter 提示），不遮挡视口拾取，切换选中或清除选中时同步更新/消失。 | spawn_feedback_nodes(s2.rs:96-158) 在 HUD 下创建 tooltip 与详情卡（初始 Hidden、无 Pickable，require_markers=true 下不挡视口）；update_space_feedback(680-784) 按 pick.hovered+120ms 显示 label/path，按 pick.selected 更新详情卡并在清除/搜索打开时隐藏。space_s2.rs:288-302 与 tooltip_delay_ready 单测通过。 |
| A8 | passed | specs/spatial-s2-interaction/spec.md | Tab 遍历并回绕 - **WHEN** 用户连续按 `Tab` 或 `Shift+Tab` - **THEN** 选中态依次移动到下一个/上一个可见节点，到末尾后回绕 - 验收：A1 | space_s2.rs:140-166 同一用例覆盖 0→1、Shift+Tab 反向、头部回绕到 2，rem_euclid 保证回绕（s2.rs:583-588）。 |
| A9 | passed | specs/spatial-s2-interaction/spec.md | 遍历不旋转相机 - **WHEN** 用户用 `Tab` 切换选中节点 - **THEN** 相机 target 跟随选中节点，yaw、pitch 与 dist 保持不变 - 验收：A1 | space_s2.rs:146-150 断言 Tab 前后 yaw/pitch/dist 相等且 target 变化；实现只写 orbit.target 并 apply_orbit（s2.rs:592-597）。 |
| A10 | passed | specs/spatial-s2-interaction/spec.md | 搜索或输入焦点阻止遍历 - **WHEN** 搜索框打开或任意文本输入框拥有焦点 - **THEN** `Tab`/`Shift+Tab` 不改变 3D 选中态 - 验收：A1 | space_s2.rs:169-186 覆盖 EditableText 焦点与搜索打开两条阻止路径；text_focused(s2.rs:167-171) 判定任意 EditableText 焦点。 |
| A11 | passed | specs/spatial-s2-interaction/spec.md | 搜索并飞行定位 - **WHEN** 用户按 `/`，输入文件名片段并选择一条结果按 Enter - **THEN** 搜索关闭，该文件星体被选中，相机定位到它，编辑器保持关闭 - 验收：A2 | 同 A2：搜索框中立化后 Enter 目标稳定为 results[selected]（s2.rs:271-280），fly_to_node(283-302) 只调 target/dist 并 apply_orbit；space_s2.rs:188-216 断言 target=(20,0,0)、yaw/pitch 不变、未产生 OpenFileRequest。 |
| A12 | passed | specs/spatial-s2-interaction/spec.md | 搜索后再次 Enter 打开 - **WHEN** 搜索飞行完成后用户再次按 Enter - **THEN** 2D 编辑器打开搜索选中的同一路径 - 验收：A3 | 同 A3：无 ChatInput 清空/重排后，第二次 Enter 打开的是搜索选中的 src/file1.rs（space_s2.rs:220-223）。 |
| A13 | passed | specs/spatial-s2-interaction/spec.md | 搜索 Esc 只关搜索 - **WHEN** 搜索打开时按 Esc - **THEN** 搜索关闭，3D 选中态与对话均保持不变 - 验收：A5 | 搜索 Esc 分支(s2.rs:830-840)只关搜索、清输入焦点并置 consumed，不动 pick；shortcuts 因 SpaceEscConsumed 让位；space_s2.rs:227-235 断言选中保留。 |
| A14 | passed | specs/spatial-s2-interaction/spec.md | 无匹配结果 - **WHEN** 查询没有任何 File 节点匹配 - **THEN** 结果列表为空，Enter 不选中任何节点，Esc 仍可关闭搜索 - 验收：A2 | 无匹配时 refill_results 结果为空，handle_space_search_keys 的 results.get(selected) 为 None 直接 return，搜索保持打开且不选中（s2.rs:271-274）；space_s2.rs:318-337 覆盖 Enter inert 与 Esc 关闭。ChatInput 的 Enter 清空链路已移除，真实栈下也不会重填全量列表。 |
| A15 | passed | specs/spatial-s2-interaction/spec.md | 编辑器切换文件同步选中 - **WHEN** 用户在编辑器中切换活动文件，且该文件在空间图中 - **THEN** 对应 File 星体进入选中态，相机 target/yaw/pitch/dist 不变 - 验收：A4 | 同 A4；space_s2.rs:246-268 断言编辑器切到 file1 后选中对应星体且相机 target 不变。 |
| A16 | passed | specs/spatial-s2-interaction/spec.md | 图中不存在当前文件 - **WHEN** 编辑器活动文件不在空间图中 - **THEN** 3D 选中态被清除，不保留指向其他文件的选中 - 验收：A4 | 同 A4；切到 src/not-in-graph.rs 后 pick.selected=None（space_s2.rs:270-285）。 |
| A17 | passed | specs/spatial-s2-interaction/spec.md | 3D 选中打开同一文件 - **WHEN** 用户选中一个 File 星体并按 Enter 或双击 - **THEN** 2D 编辑器打开的路径与该星体路径一致 - 验收：A3 | open_selected(view.rs:983-1009) 仅对选中 File 星体按 star.path 发 OpenFileRequest；space_panel.rs::bracket_keys_select_and_enter_opens_file 断言打开的路径为当前选中的 src/file1.rs，单元/模块节点不打开（enter_on_module_does_not_open_editor 通过）。 |
| A18 | passed | specs/spatial-s2-interaction/spec.md | 搜索优先关闭 - **WHEN** 搜索打开时按 Esc - **THEN** 只关闭搜索，不关闭编辑器、不清选中、不中断对话 - 验收：A5 | 搜索 Esc 由 s2.rs:830-840 消费并置 SpaceEscConsumed；shortcuts.rs:232-235 有消费时 drain 后返回，不关闭编辑器、不清选中、不发 AbortMessage；space_s2.rs:227-235 通过。 |
| A19 | passed | specs/spatial-s2-interaction/spec.md | 工作区先回宇宙 - **WHEN** 编辑器、差异或终端可见时按 Esc - **THEN** 回到 3D 宇宙，选中态保留 - 验收：A5 | space_panel.rs 三级 Esc 第一步断言编辑器关闭、SPACE 重新可见、3D 选中保留；终端/差异分支与编辑器共用 shortcuts.rs:243-279 的 chat.abort 处理。 |
| A20 | passed | specs/spatial-s2-interaction/spec.md | 选中先清除 - **WHEN** 宇宙可见且有选中节点时按 Esc - **THEN** 只清除选中，不中断对话 - 验收：A5 | space_s2.rs:238-243 与 space_panel 第二步断言宇宙 Esc 清选中且 Messages<AbortMessage> 为空。 |
| A21 | passed | specs/spatial-s2-interaction/spec.md | 空宇宙才中断对话 - **WHEN** 宇宙可见、搜索关闭且没有选中节点时按 Esc - **THEN** 才向当前对话发送中断 - 验收：A5 | space_panel 第三步（空宇宙、搜索关闭、无选中）Esc 后 Messages<AbortMessage> 非空。 |
| A22 | passed | specs/spatial-s2-interaction/spec.md | 悬停 tooltip - **WHEN** 指针在星体上停留至少 120ms - **THEN** 显示该节点的 label/path tooltip，移开后消失 - 验收：A7 | update_space_feedback 在 hovered 且 now-hover.since>=0.12 时写 label/path 并置 Visible，移开或搜索打开即 Hidden（s2.rs:700-744）；tooltip_delay_ready 单测 0.119=false/0.12=true。真实窗口的 hover→可见与投影像素位置无集成断言。 |
| A23 | passed | specs/spatial-s2-interaction/spec.md | 选中详情卡 - **WHEN** 用户选中一个星体 - **THEN** 详情卡显示 label/path/类型与 Enter 提示；切换或清除选中时同步更新/消失 - 验收：A7 | 详情卡文本含 label、path、类型（space-kind-*）与 File 的 Enter 提示（space-detail-hint），选中变化逐帧更新、清选中/搜索打开即 Hidden（s2.rs:745-781）；space_s2.rs:288-302 断言 Visible 与文本；反馈节点无 Pickable，不遮挡视口拾取。 |
| A24 | passed | specs/spatial-s2-interaction/spec.md | 经典回退不触发 S2 - **WHEN** 用户处于经典 2D 回退（`three_d_home=false`）并按 `/` 或 `Tab` - **THEN** 不打开 3D 搜索、不切换 3D 选中，布局与 MVP-1 功能不受影响 - 验收：A6 | S2 开搜索/遍历均被 space_visible 拦截（s2.rs:209、543），经典回退由 layout_mode.rs:162-165 失活 SPACE；space_s2.rs:305-315 与 layout_mode.rs::classic_layout_restores_chat_as_center 通过，布局与 MVP-1 键位不变。 |

## 检查

_没有记录 Runtime 检查。_

### Builder 报告的证据

以下为 Builder 报告，不等同于 Runtime 检查凭据或独立验收结果。

- cargo test -p xgent_spatial -p xgent_ui -p xgent_app -j 2: passed — 修复后全量回归通过；xgent_ui lib 153 + 全部集成测试，space_s2 11 项。
- cargo test -p xgent_ui --test space_s2: passed — 含 search_input_is_not_a_chat_input 与 search_enter_with_real_input_stack_selects_current_result。
- cargo check -p xgent_app: passed — xgent_app/xgent_ui 编译通过。
- 已知限制: 真实窗口中的搜索覆盖层视觉/鼠标点击未做人工走查；headless 覆盖状态、UI 节点与 120ms tooltip 门控，未覆盖像素级定位。
- 已知限制: Tab 遍历在 headless 无窗口时按节点 id 排序；真实窗口按屏幕极角排序，极角顺序未做像素级测试。
- 已知限制: 搜索结果列表仅支持键盘选择（按确认范围）。
- 已知限制: Classic 下 space.show 仍为静默 no-op，未在本变更隐藏命令入口。
- 已知限制: 搜索结果上下键不会自动滚入可视区；搜索框无 I-beam 指针（范围内声明鼠标点选不支持）。

## 阻塞项

_无。_

## 风险与跳过的工作

- 新增的 real-stack 回归 tests/space_s2.rs:358-409 实际未覆盖 FocusedInput 派发：s2_app(space_s2.rs:59-88) 只有 MinimalPlugins、没有 PrimaryWindow，而 bevy_input_focus::dispatch_focused_input 需要 windows.single()（../bevy/crates/bevy_input_focus/src/lib.rs:401），故该用例即使重新挂上 ChatInput 也会通过。防回归真正依赖 space_s2.rs:341-354 的结构断言；建议补 WindowPlugin+PrimaryWindow，或断言 Enter 后 EditableText 值不变。
- Esc 消费依赖 xui::shortcuts::dispatch_hotkeys 与 xgent_ui::shortcuts::handle_hotkey_triggers 同帧且 dispatch 在前；两者无显式排序，若顺序反转，SpaceEscConsumed 仅保同帧，Esc 可能在下一帧泄漏为 chat.abort。
- A22 tooltip 的 hovered→120ms→Visible 与真实窗口投影位置无端到端测试；A1 的屏幕极角顺序只在无窗口时走 id 序回退，真实窗口角度序无像素级断言。
- traversal.order 只在初始/外部选中变化/相机变化时重算（s2.rs:567-573），SpaceGraph 刷新（增删节点）不会使其失效，刷新后可能短暂选中已删除 id 或漏掉新节点。
- 搜索结果上限 20（s2.rs:190）、上下键移动不自动滚入可视区，搜索框/结果项未支持鼠标点选（声明范围外）。
- 若打开搜索时会话输入框的 @ 补全浮层仍 active，completion_ui.rs:65-122 会清掉 ArrowUp/Down 的 just_pressed 并接收 Enter（接受候选），搜索结果的 ↑/↓ 选择可能被吞；需先 Esc 关闭浮层。
- 查询与 Enter 同帧时（最后键入字符的 apply_text_edits 在 PostUpdate，sync 在 Update）会使用上一帧查询；极端同帧输入新字符+Enter 可能选中旧结果。

## 之前的迭代

| 目标周期 | 迭代 | 尝试 | 结果 | 未解决项 | 摘要 | 完成时间 |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 1 | fail | A2, A3, A11, A12, A14 | fail：搜索飞行主链路在真实输入栈下失效。搜索框复用 ChatInput::single_line()（crates/xgent_ui/src/space/s2.rs:492），xui 的 Enter 提交观察者（crates/xui/src/input.rs:99/129-153）在 PreUpdate 清空 EditableText；而 crates/xgent_ui/src/space/s2.rs:226-243 的查询同步排在 crates/xgent_ui/src/space/view.rs:256-257 的 Enter 处理之前，导致 results 被重填为全部 File 节点、selected=0，s2.rs:268-277 选中并飞向/打开错误文件。故 A2/A3/A11/A12/A14 失败（A3/A12 二次 Enter 打开错误路径）；A14 的无匹配 Enter 也会选中首项。其余 A1/A4-A10/A13/A15-A24 通过，A7/A22 有测试覆盖缺口已列入风险。验证命令：cargo test -p xgent_ui --test space_s2 --test space_panel --test layout_mode（23 项全过）、cargo test -p xgent_ui -j 2（153 lib + 全部集成全绿）、cargo check -p xgent_app 通过。修复建议：移除搜索框的 ChatInput::single_line()（保留 EditableText allow_newlines=false），或把 sync_space_search_results 移到 handle_space_search_keys 之后（对齐 command_palette.rs:49-61 的 handle→sync 顺序），并补真实输入栈回归测试。 | 2026-11-02T03:00:01.966Z |
| 1 | 2 | 1 | pass | — | pass：上一轮 CRITICAL（搜索 Enter 被 ChatInput 提交观察者先 clear 查询、result 重填全量并选中首项）已修复。搜索框不再挂 ChatInput（s2.rs:475-499），bevy TextInput 在 allow_newlines=false 时忽略 Enter；sync 仍先读当前查询再处理 Enter，无匹配 Enter 保持 inert，二次 Enter 经 suppress_enter/pick_space_stars 打开同一路径。cargo test -p xgent_ui -j 2 EXIT=0（153 lib + 28 集成全绿），space_s2 11、space_panel 10、layout_mode 4 全过。A2/A3/A11/A12/A14 由代码路径 + 结构断言判定通过；real-stack 用例缺 PrimaryWindow 未真正派发 FocusedInput，已列入风险。 | 2026-11-02T03:33:43.204Z |



## 结论

pass：上一轮 CRITICAL（搜索 Enter 被 ChatInput 提交观察者先 clear 查询、result 重填全量并选中首项）已修复。搜索框不再挂 ChatInput（s2.rs:475-499），bevy TextInput 在 allow_newlines=false 时忽略 Enter；sync 仍先读当前查询再处理 Enter，无匹配 Enter 保持 inert，二次 Enter 经 suppress_enter/pick_space_stars 打开同一路径。cargo test -p xgent_ui -j 2 EXIT=0（153 lib + 28 集成全绿），space_s2 11、space_panel 10、layout_mode 4 全过。A2/A3/A11/A12/A14 由代码路径 + 结构断言判定通过；real-stack 用例缺 PrimaryWindow 未真正派发 FocusedInput，已列入风险。
