---
generated_from_state_version: 8
---

# 验证

## 当前结果

- 结果: **未通过**
- 验证情况: **修复未通过的验收项后重新验证**
- 目标周期: 1
- 迭代: 1
- 验证器尝试次数: 2
- 完成时间: 2026-10-21T09:34:28.985Z
- 摘要: M5 的主体模型（Slot/PanelDescriptor/PanelRegistry/SlotLayout + 三条 ECS 消息、rail 选中态派生、历史面板进 Overlay 单例槽位、SideViewContent 全量拆除、SplitTree 分割模型与抽屉 240–480 可拖宽、title_key i18n 双语料齐备、基线不回归——Runtime 5 条检查全绿，workspace 失败集合恰为既有 15 项）均已核实落地，A1–A4、A7–A11 通过。但独立复核发现三处实质缺陷：① A5 归一把 rail『展开面板』钮的显隐语义反转（折叠时不显示、展开时常驻），违反 Spec『语义保持不变』；② A6 侧区顶部的返回入口被从 2 个删成 0 个，EditorBackButtonMarker 与 handle_back_button_click 沦为死代码，而台账与开发指南仍声称保留了一个；③ A12 明确要求的『订正交接 §4.2 过时判据』未执行（该节仍写着过时的 M5-T5『拖拽排序 + 面板宽度持久化』判据），且提交粒度为 4 条实现提交而非每任务一条（已记 §10 勘误 38，非静默接受）。因 A5/A6/A12 判定为 failed，整体结论为 fail，需修复后重交候选并另起独立 Verifier 复验。

## 验收

| 编号 | 结果 | 来源 | 验收项 | 原因 |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | A1 槽位与描述符模型：`Slot` 覆盖 `Rail/Center/SideDock/BottomDock/SettingsSection/StatusSegment/Palette/Overlay`；`PanelDescriptor.title_key` 必填（i18n key）；`PanelRegistry` 可注册并按 id / slot 查询（按 `order` 排序）；`PanelRegister` / `PanelActivate` / `PanelDeactivate` 是 ECS `Message`。 | Slot 覆盖 8 槽位（crates/xgent_ui/src/panels.rs:51-68：Rail/Center/SideDock/BottomDock/SettingsSection/StatusSegment/Palette/Overlay）；PanelDescriptor.title_key 为必填非 Option 字段（panels.rs:114，构造器 panels.rs:133-152 必传）；PanelRegistry 支持按 id 查询（panels.rs:236-239）与按槽位查询并按 order 升序（panels.rs:256-264）；PanelRegister/PanelActivate/PanelDeactivate 均为 #[derive(Message)]（panels.rs:438/447/454），注册/激活/失活只经消息（handle_panel_register/activate/deactivate panels.rs:556-583）。 |
| A2 | passed | brief.md | A2 扩展性（北极星前置）：新增一个 `SideDock` 面板只需「一个描述符 + 一个视图模块 + 一条 `PanelRegister`」；`crates/xgent_ui/src/layout.rs` 与 `crates/xgent_ui/src/editor/mod.rs` 中不含 `SideViewContent`，也不含按面板 id 分派的 `match`。 | crates/ 内 SideViewContent 零引用（grep 仅命中 tests/panel_registry_contract.rs 自身的断言文本）；layout.rs 只做一次注册（layout.rs:217-227 注册 ids::CHAT）且不含任何面板 id 字面量或分派（源码级断言 layout_file_has_no_panel_id_branches 通过）；editor/mod.rs 仅注册自身面板（editor/mod.rs:212-221）；页签由注册表自动生成（panels/tabs.rs:79-82 iter_slot(Slot::SideDock)）——新增一个 SideDock 面板 = 一个描述符 + 一个视图模块 + 一条 PanelRegister，layout.rs/editor/mod.rs 零改动成立。 |
| A3 | passed | brief.md | A3 rail 选中态派生：`ActiveActivity` 资源被删除；rail 选中态由「注册表 ∩ 槽位可见性」派生；连点 Files 两次关闭抽屉后，rail 上不残留 Files 选中态。 | ActiveActivity 类型已不存在（全仓仅 activity_bar.rs:5 文档提及，ActivityKind 亦已删除）；选中态由 PanelRegistry::active_rail 派生（panels.rs:342-360：只取目标面板经 layout.is_panel_visible 可见的 rail 入口，按 activated_seq 取最近激活者）；连点 Files 两次后抽屉失活 → rail-files 目标不可见 → 回落到 rail-chat（panels.rs 测试 active_rail_falls_back_when_target_hidden；tests/panel_registry_contract.rs:290-319 同逻辑；activity_bar.rs:360-407 的 update_active_indicators 只读 active_rail，不再有独立存储）。 |
| A4 | passed | brief.md | A4 历史面板进槽位：`session_history` 由自建浮层改为注册面板（`Slot::Overlay`），与文件抽屉同属单例槽位，二者同一时刻至多一个可见（不再争层级）。 | session_history 改为常驻注册面板：spawn_session_history 写 PanelRegister（session_history.rs:397-406，Slot::Overlay），遮罩与面板都挂 PanelVisibility::of_panel(SESSION_HISTORY, Slot::Overlay)（session_history.rs:303-306/332-335）；Overlay 单例语义由 PanelRegistry::activate 保证（同槽其它面板自动失活，panels.rs:293-300），文件抽屉同属 Slot::Overlay（file_panel.rs:231-240）故二者同一时刻至多一个可见、不再争层级（overlay.rs:56-70 的 DrawerToggle 统一转面板消息）。 |
| A5 | failed | brief.md | A5 派生可见性单一系统：`derive_panel_visibility` 是**唯一**写「面板 / 区域」`Node.display` 的系统；原 7 处动态面板可见性写手（`layout.rs` 分屏与手柄、`file_panel.rs` 遮罩与面板、`activity_bar.rs` 展开钮、`editor/mod.rs` 编辑器视图、`terminal/mod.rs` 终端视图）全部并入该系统；3 处 spawn 期默认值改为构造期字面量；`crates/xgent_ui/src/` 内 `.display =` 的命中**仅限**「派生系统 + 构造期字面量 + 5 处内容态白名单」（`chat_panel.rs` 回到底部按钮、`activity_bar.rs` 陪伴激活环、`welcome.rs` 欢迎空态、`welcome.rs` 最近会话标题、`editor/mod.rs` 编辑器文档页签），白名单以源码级测试固化且逐条注明属内容态而非面板态。 | 唯一写手与白名单口径成立，但归一过程改变了被合并写手的语义（Spec《派生可见性（唯一写手）》要求『原有各模块的动态面板可见性写手全部并入该系统，语义保持不变』，并明列『rail 的「展开面板」钮（activity_bar.rs）随分屏折叠态显隐』）。M5 前 activity_bar.rs 的 update_expand_visibility 为 `node.display = if side_collapsed.0 { Display::Flex } else { Display::None }`（折叠时显示；git show a5e438e:crates/xgent_ui/src/activity_bar.rs）；现实现把该钮挂成 PanelVisibility::of_slot(Slot::SideDock)（activity_bar.rs:132），而 derive_panel_visibility 对 of_slot 取 `layout.is_open(vis.slot)`（panels/visibility.rs:29-36）——语义被反转：展开钮在分屏打开时显示、折叠时隐藏，恰好在最需要它时不可用（其点击处理 activity_bar.rs:347-350 是 set_open(SideDock,true)，只在折叠态才有意义）；activity_bar.rs:110-111 的注释仍写『面板折叠/响应式收起时显示』，与实际行为相反，且因 SlotLayout 默认 open=true，该钮在启动默认态常驻可见。其余部分核验通过：crates/xgent_ui/src 内 `.display =` 仅 7 处命中（chat_panel.rs:1052/1054、activity_bar.rs:290、welcome.rs:190/223、panels/visibility.rs:35、editor/mod.rs:328），逐条核对确在白名单函数内（update_back_to_bottom / companion_ring_system / toggle_welcome / rebuild_recent_sessions / derive_panel_visibility / update_buffer_visibility），白名单真实；3 处 spawn 期默认值已改构造期字面量（editor/mod.rs:152、editor/diff_view.rs:57、terminal/mod.rs:278）；其余 6 处动态写手已并入派生系统（layout.rs:250、file_panel.rs:186-190/219-223、editor/mod.rs:163-167、terminal/mod.rs:286-289），语义与原实现一致。 |
| A6 | failed | brief.md | A6 单页签条所有者：侧区顶部只有一条页签条、且由槽位布局拥有并渲染；编辑器内部 tab 条降级为「文档列表」语义（不与槽位页签合并）；仓库内 `EditorBackButtonMarker` 只定义一次（源码级断言）。 | 单页签条与文档列表语义成立（编辑器旧页签条与 SideViewPage 已在 0141343 移除，槽位页签条由 panels/tabs.rs:41-67 渲染、页签项取注册表 Slot::SideDock 的 title_key（panels/tabs.rs:79-97），编辑器内部条保留为 EditorTabBarMarker 文档列表（editor/mod.rs:186-195）），EditorBackButtonMarker 全仓唯一定义成立（panels/tabs.rs:34，源码级断言通过）。但 Spec《单页签条所有者》要求『侧区顶部只有一个返回入口』，实现实为**零个**：编辑器顶栏 ✕ 在 0141343 被删除后没有任何替代节点挂 EditorBackButtonMarker（全仓 grep 仅命中 panels/tabs.rs:9/34/167/169 与测试文件自身），panels/tabs.rs 的 spawn_slot_tab_bar 也未 spawn 返回钮，于是 handle_back_button_click（panels/tabs.rs:168-186，含 set_open(SideDock,false) 与切回对话）成为死代码、侧区失去唯一关闭入口（编辑器/预览面板只剩快捷键 sideview.toggle / chat.view 可收起；终端另有自己的 ✕ terminal/mod.rs:395/517）。台账 ui-v8-tasks.md:268 与 dev-tutorial.md:895 均声称『侧区顶部只保留一个返回入口』，与实现不符。 |
| A7 | passed | brief.md | A7 分割模型：`SplitTree / Split / LeafSize` 建模；`resize.rs` 改为遍历分割树生成分割条（注册一个纵向分割**不需要**改 `resize.rs`）；抽屉宽度从定值 320 改为 `Min(240, Preferred(320))`、上限 480，且**可拖宽**并被钳制在 `[240, 480]`。 | SplitTree/Split/LeafSize/HandleState 已建模（panels/split.rs:22-135）；resize.rs 改为遍历树生成分割条（resize.rs:122-167 sync_split_handles 走 tree.handles()，resize.rs 内无按面板 id 的特判）；抽屉叶子为 Min(240, 320)（split.rs:155-158，theme.rs:245/247/249 = 320/240/480），拖拽写回并钳制在 [240,480]（resize.rs:278-282 + clamp_leaf resize.rs:290-300，单测 drawer_respects_min/max）；注册一个纵向分割不需要改 resize.rs（源码级断言 tests/panel_registry_contract.rs:379-419 通过）。附带风险另记：apply_split_sizes 一律写 node.width（resize.rs:192）、拖拽位移只用 motion.delta.x（resize.rs:254），纵向分割的尺寸/拖拽方向实际未按轴实现（当前无生产纵向分割，暂不触发）。 |
| A8 | passed | brief.md | A8 移除 closed 枚举：`crates/` 内 `SideViewContent` **零引用**（含 `enum SideViewContent`）；editor / preview / diff / terminal 全部为原生注册项。 | crates/ 内 SideViewContent 零引用（grep 结果仅测试文件自身文本；shim 文件 crates/xgent_ui/src/panels/shim.rs 已在 7eb83e9 删除）；editor / preview / diff / terminal 四个面板均为原生注册项：editor（editor/mod.rs:212-221）、diff（editor/diff_view.rs:88-97）、terminal（terminal/mod.rs:294-303）、file-drawer（file_panel.rs:231-240）+ session-history（session_history.rs:397-406），均以 PanelRegister 消息入表。 |
| A9 | passed | brief.md | A9 i18n 页签标题：页签标题取自描述符 `title_key`（fluent 语料）；`editor/mod.rs` 中的 `let _ = loc;` 被删除；**英文语料**下页签显示英文（自动化断言：切 `en-US` 后取到的页签标题不含 CJK 码位）。 | 页签标题取自描述符 title_key（panels/tabs.rs:94-97 tr(&loc, e.desc.title_key)）；两语料齐备六个 panel-* key（xgent_settings/locales/zh-CN/main.ftl:8-13 与 en-US/main.ftl:8-13，英文语料值无 CJK 码位）；editor/mod.rs 的 `let _ = loc;` 已删除（全仓 grep 无命中，0141343 同时移除 Localizer 导入）；自动化断言 english_corpus_has_english_panel_titles 通过。 |
| A10 | passed | brief.md | A10 迁移 shim 一致性：在 shim 存在期间（M5-T1…M5-T5），「枚举变体 → 描述符 id」的映射是**纯函数**且与注册表一致（每个 shim 变体都能在注册表中查到唯一描述符）；M5-T6 拆除 shim 后该一致性由注册表本身承担（每个既有面板在注册表中有唯一描述符）。 | shim 已在 M5-T6 随枚举拆除（panels/shim.rs 不再存在），A10 后半句『一致性由注册表自身承担』成立：六个既有面板各注册唯一描述符（ids::CHAT/FILE_DRAWER/SESSION_HISTORY/EDITOR/DIFF/TERMINAL，panels.rs:24-34；各模块的 PanelRegister 见上），PanelRegistry::register 对同 id 只补视图实体不产生重复条目（panels.rs:213-233），iter_slot 按 (order,id) 排序（panels.rs:262）。 |
| A11 | passed | brief.md | A11 既有基线不回归 + 交付纪律：`cargo check --workspace` 通过；`cargo fmt -p xgent_ui` 无 diff；`cargo clippy -p xgent_ui --all-targets` 在改动文件零新增警告；`cargo test --workspace` 的失败集合**不超出**既有 15 项基线（`xgent_tools` 6 / `xgent_plugin_host` 6 / `xgent_daemon` 1 / `xgent_settings_core` 1 / `xgent_agent` 1）。 | Runtime 记录（本轮 dispatch 的 5 条）全部符合基线：fmt（26088b8e-…-fmt.log，exit 0，无 diff）、clippy（…-clippy.log，exit 0，改动文件零新增警告，其余为 settings_panel_values/settings_input_focus/xgent_tools/xgent_settings_core/xgent_context 等既有警告）、test-ui（…-test-ui.log，exit 0，xgent_ui lib 95 项 + 全部集成测试全绿，含 panel_registry_contract 18 项）、check-workspace（…-check-workspace.log，exit 0，cargo check --workspace --all-targets 通过）、test-workspace（…-test-workspace.log，exit 101——因基线失败非零退出）：失败集合恰为既有 15 项基线（xgent_tools 6 项 run_command、xgent_plugin_host 6 项（ecs_registration 3 + git_plugin 3）、xgent_settings_core 1 项 daemon_socket_env_override、xgent_agent 1 项 max_parallel_tools_written_to_handle_bounds_next_turn、xgent_daemon 1 项 global_write_lands_inside_the_isolated_dir），无新增失败；xui 103 项、xgent_terminal 33 项、xgent_ui 95 项全绿。 |
| A12 | failed | brief.md | A12 文档同步与提交粒度：`docs/plans/ui-v8-tasks.md` §M5 逐条勾选 + §10 追加本轮勘误、`docs/plans/ui-v8-handoff.md` 交付表 / 基线 / 下一步（并订正 §4.2 的过时判据）、`docs/dev-tutorial.md` 新增 M5 小节；提交粒度为 M5-T1…M5-T6 各一条 `feat(ui): …` + 一条文档提交 + 一条 `chore(native): archive v8-m5-panel-registry`。 | 三处文档都记录了 M5：台账 §M5 六个任务逐条 [x]（ui-v8-tasks.md:250-278）+ §10 勘误 34–38（ui-v8-tasks.md:432-436）；交接 §1 交付表 M5 行（ui-v8-handoff.md:29）、§2 基线（45-48）、§4.0 落点速查（88-95）、§5 下一步 M6（103）、§8 开工前 5 步（160-166）；开发指南 §5.24（dev-tutorial.md:872-896）+ §1.1 M5 功能行（33）。但 A12 明确要求的『订正 §4.2 的过时判据』**未做**：交接 §4.2 仍是旧内容——标题仍为『下一期的任务范围（台账 §M5）』（ui-v8-handoff.md:113，尽管 M5 已交付、§5 已把下一期指向 M6），M5-T5 判据仍是过时的『拖拽排序 + 面板宽度持久化 / 拖拽后重启仍保持顺序与宽度』（ui-v8-handoff.md:121；brief 决策 D-2 明确要求本次交付订正并记 §10 勘误，而 §10 勘误 34–38 无对应条目），M5-T3 判据（line 119）也仍是未收窄的旧口径；文档节序也被插入破坏（§4.0 → §5 → §4.1 → §4.2 → §4.3）。提交粒度偏差：实际 4 条实现提交（0aeecc4 T1–T3 / 0141343 T4 / dc22458 T5 / 7eb83e9 T6）+ 1 条文档提交 0d76416，非 A12 要求的每任务一条（已记 §10 勘误 38，此处非静默接受）；`chore(native): archive v8-m5-panel-registry` 尚未产生，属归档阶段动作，Verify 阶段不计入。 |

## 检查

| 检查 | 命令 | 工作目录 | 状态 | 退出码 | 耗时 |
| --- | --- | --- | --- | ---: | ---: |
| 格式检查（xgent_ui + xgent_settings） | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo fmt -p xgent_ui -p xgent_settings -- --check | . | passed | 0 | 698 ms |
| clippy（xgent_ui，含测试目标） | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo clippy -p xgent_ui --all-targets | . | passed | 0 | 2749 ms |
| xgent_ui 测试（lib + 集成） | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo test -p xgent_ui --no-fail-fast | . | passed | 0 | 25800 ms |
| 全工作区编译检查 | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo check --workspace --all-targets | . | passed | 0 | 3956 ms |
| cargo test --workspace（A11 基线失败集合） | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo test --workspace --no-fail-fast | . | failed | 101 | 99625 ms |

### Builder 报告的证据

以下为 Builder 报告，不等同于 Runtime 检查凭据或独立验收结果。

- cargo fmt -p xgent_ui -p xgent_settings: passed — 无 diff
- cargo clippy -p xgent_ui --all-targets: passed — 改动文件零新增警告（其余为既有警告：settings_panel_values / settings_input_focus / xgent_tools / xgent_settings_core / xgent_context）
- cargo test -p xgent_ui --no-fail-fast: passed — lib 95 项 + 全部集成测试文件全绿（新增 panel_registry_contract 18 项）
- cargo check --workspace --all-targets: passed — 全工作区编译通过
- cargo test --workspace --no-fail-fast: passed — 失败集合 14 项，全部落在既有 15 项基线内（xgent_tools 6 / xgent_plugin_host 6 / xgent_settings_core 1 / xgent_agent 1），无新增失败
- 已知限制: 提交粒度偏差：M5-T1～T3 合并为一条实现提交（0aeecc4），T4/T5/T6 各一条（0141343 / dc22458 / 7eb83e9）；已记台账 §10 勘误 38。
- 已知限制: `.display =` 归一口径按 2026-10-21 用户裁决收窄到「面板/区域可见性」：5 处内容态写手（欢迎空态、最近会话标题、回到底部按钮、文档页签、陪伴激活环）保留在原模块，以显式白名单 + 源码级测试固化（台账 §10 勘误 34）。
- 已知限制: headless 集成测试的抽屉可见性必须经面板消息打开（FileDrawerOpen / SessionHistoryState 已是派生视图）；打开代码文件后面板可见性比旧枚举晚 1～2 帧，断言改为「一旦可见不得再消失」（台账 §10 勘误 37）。
- 已知限制: 本轮未做实机窗口截图对照（headless 环境无渲染管线）；M5 的视觉验收依赖几何/源码级断言。
- 已知限制: 硬门槛 A 的 macOS / Linux 的 mono 启动日志确认仍未闭环（沿用 M4 的「不阻塞」决定，不属本变更责任范围）。

## 阻塞项

_无。_

## 风险与跳过的工作

- A5 失败点：rail『展开面板』钮的显隐语义在归一中被反转（activity_bar.rs:132 + panels/visibility.rs:29-36 vs M5 前 update_expand_visibility）——该钮在分屏打开时常驻可见、在分屏折叠时不可用，用户失去恢复折叠分屏的 rail 入口。修复需引入『槽位关闭时显示』的反向跟随语义（或独立标记），并补行为级断言。
- A6 失败点：侧区顶部返回入口数量由 2 变 0（编辑器顶栏 ✕ 删于 0141343，新槽位页签条未 spawn 返回钮），EditorBackButtonMarker 与 handle_back_button_click 沦为死代码；台账 ui-v8-tasks.md:268 与 dev-tutorial.md:895 的『只保留一个返回入口』表述失实。
- A12 失败点：交接 §4.2 的过时 M5-T5 判据（拖拽排序 + 面板宽度持久化）与 M5-T3 旧口径未订正，且未按 brief 决策 D-2 记 §10 勘误；交接文档节序被打乱（§4.0 → §5 → §4.1 → §4.2 → §4.3）。
- 提交粒度偏差（非静默接受）：4 条实现提交而非 M5-T1…T6 各一条，已记台账 §10 勘误 38；A12 字面要求未满足。
- A7 附带风险：resize.rs 的 apply_split_sizes 一律写 node.width、拖拽位移只用 motion.delta.x，纵向分割虽然能生成纵向手柄且无需改 resize.rs，但其尺寸/拖拽方向实际未按轴实现（当前无生产纵向分割，暂不触发）。
- SlotLayout::set_tab_bar / tab_bar() 为死代码（页签条实体实际靠 SlotTabBarMarker 组件查询定位），『槽位布局拥有页签条实体』只做到模块/组件层面。
- Runtime 过程说明：首轮 dispatch（attempt 1, ref 4aeb4079-…）的本地执行在请求 workspace 全量测试时被外层超时中断，Runtime 报 portable-check-execution-stuck；已按 Runtime 指示用 `comet native doctor v8-m5-panel-registry --repair` 恢复（state_version 5→6，action=reverify，候选与代码未变），并在新的 dispatch（attempt 2, ref da0cabdc-…）下重跑了全部 5 条检查，本报告引用的 Runtime 日志均为该轮（26088b8e-…）记录。
- 本轮未做实机窗口截图对照（headless 环境无渲染管线），A5/A6 的两处显隐/入口问题即依赖源码推导而非实机观察。

## 之前的迭代

| 目标周期 | 迭代 | 尝试 | 结果 | 未解决项 | 摘要 | 完成时间 |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 2 | fail | A5, A6, A12 | M5 的主体模型（Slot/PanelDescriptor/PanelRegistry/SlotLayout + 三条 ECS 消息、rail 选中态派生、历史面板进 Overlay 单例槽位、SideViewContent 全量拆除、SplitTree 分割模型与抽屉 240–480 可拖宽、title_key i18n 双语料齐备、基线不回归——Runtime 5 条检查全绿，workspace 失败集合恰为既有 15 项）均已核实落地，A1–A4、A7–A11 通过。但独立复核发现三处实质缺陷：① A5 归一把 rail『展开面板』钮的显隐语义反转（折叠时不显示、展开时常驻），违反 Spec『语义保持不变』；② A6 侧区顶部的返回入口被从 2 个删成 0 个，EditorBackButtonMarker 与 handle_back_button_click 沦为死代码，而台账与开发指南仍声称保留了一个；③ A12 明确要求的『订正交接 §4.2 过时判据』未执行（该节仍写着过时的 M5-T5『拖拽排序 + 面板宽度持久化』判据），且提交粒度为 4 条实现提交而非每任务一条（已记 §10 勘误 38，非静默接受）。因 A5/A6/A12 判定为 failed，整体结论为 fail，需修复后重交候选并另起独立 Verifier 复验。 | 2026-10-21T09:34:28.985Z |



## 结论

M5 的主体模型（Slot/PanelDescriptor/PanelRegistry/SlotLayout + 三条 ECS 消息、rail 选中态派生、历史面板进 Overlay 单例槽位、SideViewContent 全量拆除、SplitTree 分割模型与抽屉 240–480 可拖宽、title_key i18n 双语料齐备、基线不回归——Runtime 5 条检查全绿，workspace 失败集合恰为既有 15 项）均已核实落地，A1–A4、A7–A11 通过。但独立复核发现三处实质缺陷：① A5 归一把 rail『展开面板』钮的显隐语义反转（折叠时不显示、展开时常驻），违反 Spec『语义保持不变』；② A6 侧区顶部的返回入口被从 2 个删成 0 个，EditorBackButtonMarker 与 handle_back_button_click 沦为死代码，而台账与开发指南仍声称保留了一个；③ A12 明确要求的『订正交接 §4.2 过时判据』未执行（该节仍写着过时的 M5-T5『拖拽排序 + 面板宽度持久化』判据），且提交粒度为 4 条实现提交而非每任务一条（已记 §10 勘误 38，非静默接受）。因 A5/A6/A12 判定为 failed，整体结论为 fail，需修复后重交候选并另起独立 Verifier 复验。
