---
generated_from_state_version: 12
---

# 验证

## 当前结果

- 结果: **已归档**
- 验证情况: **已完成检查，验证结果已确认**
- 目标周期: 1
- 迭代: 3
- 验证器尝试次数: 1
- 完成时间: 2026-10-20T10:44:01.959Z
- 摘要: 独立复核判定 pass：8/8 验收项通过。关键证据——（1）纯函数层：crates/xgent_ui/src/fonts.rs 的 resolve_slot 三级解析（系统按序命中 → 内嵌兜底 → Failed 记警告）实现与 spec 一致，crates/xgent_ui/tests/font_slots.rs 6 条测试我本机独立重跑全绿（内嵌兜底 / 候选顺序取第二个 / 失败标记 / 无平台特权路径 / 内嵌资产登记为 mono 最后一位 / 本机命中真等宽）。（2）资产层：crates/xgent_app/assets/fonts/JetBrainsMono-Regular.ttf 270224 字节，我用 parley/fontique 独立注册读出家族名确为 JetBrains Mono（真身非改名），OFL.txt 与 README.md 齐备，三平台 mono 候选栈均把它登记为最后一位内嵌兜底。（3）平台层：crates/ 内字体相关代码 `cfg!(target_os` 命中数为 0，旧 `else { Handle::default() }` 的 macOS 独占分支已从 startup.rs 删除；本机系统字体集合 163 家族含 Cascadia Mono/Consolas/Cascadia Code，mono 槽位实测解析为 Cascadia Mono（system，真实等宽，绝非 ui 槽位的 Inter Variable）。（4）不回归：xgent_ui lib 76 项 + 全部既有集成测试 0 失败，cargo check --workspace 通过，clippy 三 crate --all-targets 0 error 且本变更文件零告警，cargo fmt --check 退出码 0。（5）文档：ui-v8-tasks.md M4-T1 勾选 + §10 勘误 17~20 条、dev-tutorial.md §5.21、ui-v8-handoff.md 交付表 + §1.1 硬门槛 A 状态三处齐备。未闭环/风险项已列在 risks：最重要的一条是 Runtime 标记为「已通过」的 dd54cbc3 正式检查日志实为构建失败（rustc STATUS_STACK_BUFFER_OVERRUN，0 条 test result），绿色记录实为 59a56356；另 `ui` 槽位缺 FontSpec（与 spec 字面有偏差但不被 A1~A8 覆盖）、font_specs() 用 std::env::consts::OS 运行期选表、macOS/Linux 的硬门槛 A 仍需各启动一次确认。

## 验收

| 编号 | 结果 | 来源 | 验收项 | 原因 |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | A1 `mono` 槽位在全缺系统等宽时回退到内嵌 JetBrains Mono：给定候选集合里没有任何家族可用，解析结果的来源是 `Embedded`、家族名是 JetBrains Mono，且**不等于** `ui` 槽位的家族。 | crates/xgent_ui/src/fonts.rs:189-200 的第二轮「内嵌兜底」分支返回 FontOrigin::Embedded、family = 候选的家族名（EMBEDDED_MONO_FAMILY = "JetBrains Mono"）、source = FontSource::Handle(handle)；crates/xgent_ui/src/fonts.rs:232/263/294 把该内嵌候选登记为三平台 mono 候选栈的最后一位。测试 crates/xgent_ui/tests/font_slots.rs:29-54 mono_falls_back_to_embedded_asset 在我本机独立重跑通过（6/6 全绿）。独立取证：我用 parley/fontique 0.11（与 bevy_text 同一路径）把 crates/xgent_app/assets/fonts/JetBrainsMono-Regular.ttf 注册进 collection，读出家族名确为 "JetBrains Mono"（非改名的假文件），故该 family 名与资产一致；mono 槽位的 fallback_generic 是 GenericFontFamily::Monospace，解析结果不可能等于 ui 槽位的 Inter Variable。 |
| A2 | passed | brief.md | A2 候选栈按顺序命中：给定系统里存在第二个候选家族，解析结果取第二个（不是第一个、不是最后一个）。 | crates/xgent_ui/src/fonts.rs:176-187 第一轮严格按 spec.candidates 顺序遍历，返回第一个 available 的系统候选。测试 crates/xgent_ui/tests/font_slots.rs:58-71 candidates_are_hit_in_order 用 [Missing, Present, Also Present] 且后两个都存在，断言取第二个 "Present Mono"（不是第一个也不是最后一个），我独立重跑通过。spec 的第二个 A2 场景同时满足：crates/xgent_ui/src/fonts.rs:447 与 :469 把 fonts.mono（FontSource）直接放进 TextFont.font，调用点签名未变（crates/xgent_ui/src/kit.rs:387/609、src/terminal/mod.rs、src/terminal/output.rs:133、src/terminal/tabs.rs:236/253、src/confirm_dialog.rs:274 全部经 mono_text/mono_text_weighted）。 |
| A3 | passed | brief.md | A3 解析失败是警告不是静默：给定某槽位无任何可用家族且无内嵌资产，解析结果标记为失败（调用方据此 `warn!`），不静默返回 `ui` 槽位。 | crates/xgent_ui/src/fonts.rs:201-206 第三轮返回 FontOrigin::Failed + 该槽位声明的 fallback_generic（mono 为 Monospace，绝不回落 ui 的 Inter）；crates/xgent_ui/src/fonts.rs:129-134 的 FontOrigin::Failed 文档明确「调用方应记警告」。调用方 crates/xgent_app/src/startup.rs:138-142 对 Failed 分支打 tracing::warn!("字体槽位 {} 无可用候选，回退通用族 {}（来源 failed）")，非静默。测试 crates/xgent_ui/tests/font_slots.rs:75-83 failure_is_reported_not_silent（空候选栈 → Failed + Generic）我独立重跑通过。 |
| A4 | passed | brief.md | A4 平台差异只在数据里：`crates/` 内字体相关代码的 `cfg!(target_os` 命中数为 0。 | 全 crates/ 检索 `cfg!(target_os` / `cfg!(windows` / `cfg!(unix`：字体相关代码命中数为 0。crates/xgent_ui/src/fonts.rs 与 crates/xgent_app/src/startup.rs 内无任何 cfg 平台分支（fonts.rs:20 只是文档里引用这条禁令）；crates/ 内剩余命中全部与字体无关——xui/src/input.rs:157/161、shortcuts.rs:30/34、hotkeys.rs:21/25（`#[cfg(target_os="macos")]` 属性，非字体）、xgent_terminal/tests/integration.rs:15、xgent_ui/src/terminal/io.rs:249（`cfg!(windows)`，终端非字体）。旧的 macOS 特权路径已确实删除：git show HEAD -- crates/xgent_app/src/startup.rs 显示原 `if cfg!(target_os = "macos") { ... } else { Handle::default() }`（Menlo 独占）被整体移除，改为候选栈解析。 |
| A5 | passed | brief.md | A5 内嵌资产就位：`crates/xgent_app/assets/fonts/` 存在 JetBrains Mono 字体文件（OFL 1.1），并被装配为 `mono` 槽位的内嵌兜底候选。 | crates/xgent_app/assets/fonts/JetBrainsMono-Regular.ttf 存在，270224 字节（≈264KB，与 brief 一致），sfnt 头 00 01 00 00；同目录 OFL.txt（OFL 1.1）与 README.md（记 Inter 与 JetBrains Mono 两个字体的来源与许可）。装配：crates/xgent_ui/src/fonts.rs:210 定义 EMBEDDED_MONO_PATH="fonts/JetBrainsMono-Regular.ttf"、:213 EMBEDDED_MONO_FAMILY="JetBrains Mono"；:232/:263/:294 在 Windows/macOS/Linux 三张表里都把 FontCandidate::embedded(EMBEDDED_MONO_FAMILY, EMBEDDED_MONO_PATH) 作为 mono 候选栈的最后一位；crates/xgent_app/src/startup.rs:89-104 实际读盘并 fonts.add(Font::from_bytes(data))，读失败只记 warn 不静默。测试 crates/xgent_ui/tests/font_slots.rs:115-160 embedded_mono_asset_is_registered（断言资产存在、恰好 1 个内嵌候选、且是最后一位、mono.fallback_generic == Monospace）我独立重跑通过。 |
| A6 | passed | brief.md | A6 解析在本机命中真实等宽：在本机系统字体集合上解析 `mono` 槽位，来源是 system 或 embedded，家族名**不是** `ui` 槽位的家族（Inter Variable）。 | 测试 crates/xgent_ui/tests/font_slots.rs:164-200 mono_resolves_to_real_monospace_on_this_machine 在本机真实 parley 字体集合上解析 mono 槽位（origin != Failed、家族不含 Inter、家族必来自候选栈），我独立重跑通过。独立复核（自建临时探针 crate，依赖 parley 0.11 + system 特性，与 bevy_text 同一 fontique collection）：本机系统家族共 163 个，"Cascadia Mono" / "Consolas" / "Cascadia Code" 均存在；按 Windows mono 候选栈顺序首个命中的即 "Cascadia Mono" → 来源 system，家族名是真实等宽且不是 ui 槽位的 Inter Variable。 |
| A7 | passed | brief.md | A7 既有行为不回归：`xgent_ui` lib 76 项与既有集成测试全绿；`cargo check --workspace` 与 `cargo clippy -p xui -p xgent_ui -p xgent_app --all-targets` 通过。 | 我独立重跑（均带 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0）：`cargo test -p xgent_ui --lib` → 76 passed / 0 failed；`cargo test -p xgent_ui --tests` → 全部集成套件 0 失败（font_slots 6、settings_panel_values 19、scroll_contract 8、file_preview 7、interaction_model 7、file_tree_refresh 5、ellipsis_contract 6、density 4、conflict_save_guard 4、modal_settings 3、settings_panel_i18n 3、virtual_list_contract 3、toast 1、layout 2、settings_input_focus 2、editor_theme_sync 2）；`cargo check --workspace` → Finished、0 error；`cargo clippy -p xui -p xgent_ui -p xgent_app --all-targets` → 0 error，12 条 warning 全部落在本变更未改动的位置（settings_io.rs:342/345/458/604/703/1171、main.rs:265、tests/settings_input_focus.rs:94、tests/settings_panel_values.rs:*），本变更涉及的 fonts.rs / startup.rs / font_slots.rs 零告警；`cargo fmt -p xui -p xgent_ui -p xgent_app -- --check` 退出码 0。 |
| A8 | passed | brief.md | A8 文档同步：`docs/plans/ui-v8-tasks.md` 的 M4-T1 勾选 + §10 勘误、`docs/dev-tutorial.md` 新增 M4 小节、`docs/plans/ui-v8-handoff.md` 的交付表与下一步。 | docs/plans/ui-v8-tasks.md：:201 状态「M4-T1 已交付」、:204-211 M4-T1 四条 [x] 勾选（含 :210 Windows ✅ / :211 macOS-Linux ⏳ 的跨平台确认要求）、§10 勘误 :398-401 第 17~20 条。docs/dev-tutorial.md：:805 新增「### 5.21 跨平台字体候选栈（v8 M4-T1）」（四槽位模型、候选栈、resolve_slot 三级解析规则、内嵌 JetBrains Mono、Font::from_bytes alias 为空串、family_id/family_names 需 &mut 等坑）。docs/plans/ui-v8-handoff.md：:25 交付表 M4-T1 行、:27 §1.1 硬门槛 A 状态、:78 字体资产基线、:88 下一步表。三处都记录了硬门槛 A 的跨平台确认要求。 |

## 检查

| 检查 | 命令 | 工作目录 | 状态 | 退出码 | 耗时 |
| --- | --- | --- | --- | ---: | ---: |
| changed crate tests: xgent_ui / xui / xgent_app (debug info off) | /c set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo test -p xgent_ui -p xui -p xgent_app --no-fail-fast | . | passed | 0 | 147288 ms |

### Builder 报告的证据

以下为 Builder 报告，不等同于 Runtime 检查凭据或独立验收结果。

- cargo test -p xgent_ui -p xui --no-fail-fast: passed — xgent_ui lib 76 + 全部集成测试（含新增 font_slots 6 条）全绿；xui 103 全绿
- cargo test --workspace --no-fail-fast（与交接文档 §2 记录的既有基线对比：无回归）: passed — 本次改动涉及的 xgent_ui / xui / xgent_app 零失败；失败项全部落在未改动的 crate 且与既有基线完全一致（xgent_agent 1 时序偶发、xgent_daemon 1、xgent_plugin_host 6 git 插件 WASM 未构建、xgent_settings_core 1 Windows 路径、xgent_tools 6 依赖 POSIX shell）
- cargo check --workspace: passed — —
- cargo clippy -p xui -p xgent_ui -p xgent_app --all-targets: passed — 本次改动文件零新增警告；其余为改动前既有警告
- cargo fmt -p xui -p xgent_ui -p xgent_app: passed — 格式化后无 diff
- 已知限制: 硬门槛 A 的 macOS / Linux 部分无法在本机验证：需在对应平台各启动一次 xgent_app，确认启动日志里 mono 那行是 system/embedded 来源的真实等宽家族（台账 M4-T1 验收已标注为待确认）。
- 已知限制: 内嵌 JetBrains Mono 只提供 Regular 字面：mono_text_weighted 的 510/590 字重由 parley 就近选面（无对应字面时回落 Regular）——若要真实的多字重需另加字面资产（体积代价见设计系统）。
- 已知限制: ui_cjk / mono_cjk 两个槽位本轮只做解析与日志（不改变任何文本渲染路径）：ui 槽位受限于 ui_text 不带字体句柄（约 50 处调用点），CJK 仍由 parley 的系统回退承担；专用 CJK 表面留给 M4-T4。
- 已知限制: M4-T2（终端双宽单元格）、M4-T3（代码/行号消费 mono）、M4-T4（中文排版约束）按确认范围不在本变更内。
- 已知限制: 前两次 Runtime 检查失败均为环境问题而非代码问题：检查进程未设 CARGO_PROFILE_DEV_DEBUG=0/CARGO_PROFILE_TEST_DEBUG=0，全量 debug info 链接把 rustc 撑到 OOM（rustc-LLVM ERROR: out of memory），随后报 can't find crate。本轮按项目约定用 cmd /c "set ...&&" 包装检查命令。

## 阻塞项

_无。_

## 风险与跳过的工作

- Runtime 记录为「已通过（status=passed, exit_code=0）」的正式检查日志 dd54cbc3-1a73-4763-af47-c8d22197a938-changed-crates-tests.log 实际是构建失败：日志里 rustc 以 `-C debuginfo=2`（未设 CARGO_PROFILE_TEST_DEBUG=0）崩溃，exit code 0xc0000409 STATUS_STACK_BUFFER_OVERRUN，随后大面积 E0463 can't find crate，全程 0 条 `test result:`。真正的绿色记录是 59a56356 那份（23 条 test result、全 0 failed）。建议 Runtime 的 check 固定带 CARGO_PROFILE_DEV_DEBUG=0 / CARGO_PROFILE_TEST_DEBUG=0，并校验日志中出现 `test result:` 才记为通过，避免假阳性。
- `ui` 槽位没有 FontSpec/候选栈：crates/xgent_ui/src/fonts.rs:216-306 的三张平台表只含 UiCjk / Mono / MonoCjk 三个槽位，`ui` 仍是「Inter 插成默认 AssetId + 单独一行日志」，而 spec「槽位与候选栈」写的是「ui 槽位的候选是内嵌 Inter Variable」、装配要求「四个槽位各解析一次并打『槽位→家族名→来源』」。brief D-2 已授权 ui 保持默认句柄且 A1~A8 无一条直接考它，故不判失败，但与 spec 字面有偏差，建议 M4-T4 收敛时补一条 ui 的 FontSpec 或把 spec 措辞改齐。
- `font_specs()`（crates/xgent_ui/src/fonts.rs:311-317）用 `match std::env::consts::OS` 在运行期选表——形式上绕开了 `cfg!(target_os = …)` 的禁令，解析逻辑本身确实不分叉（差异只在数据），但 spec「运行期代码不含平台分支」的字面要求未严格满足。后续可改回编译期 cfg 选表以真正消除运行期分支。
- 硬门槛 A 的 macOS/Linux 部分仍未闭环：需在对应平台各启动一次 xgent_app，确认启动日志里 mono 那行是 system/embedded 来源的真实等宽家族。台账 ui-v8-tasks.md:211 与 handoff §1.1 已标注 ⏳。
- 内嵌 JetBrains Mono 仅 Regular 单字重（D-3 已知取舍）：mono_text_weighted 的 510/590 字重靠 parley 就近选面回落 Regular，本轮未做子集化。
- `mono_cjk` 候选（Sarasa Mono SC / Noto Sans Mono CJK SC）在本机都不存在，该槽位在本机会走 Failed 分支记 warn——符合「失败是警告不是静默」的设计，但等于本机没有等宽中文可用（属 M4-T4 范围）。
- `resolve_slot` 实际签名是 (spec, available, embedded) 三参（比 spec 写的 resolve_slot(spec, available_families) 多一个内嵌资产解析回调），功能等价，仅与 spec 字面不一致。
- crates/xgent_app/assets/fonts/README.md 的「内嵌兜底」列把 ui_cjk / mono_cjk 写成「系统发现」，与实现（无内嵌资产 → 声明兜底通用族 + warn）措辞不齐，属文档小瑕疵。

## 之前的迭代

| 目标周期 | 迭代 | 尝试 | 结果 | 未解决项 | 摘要 | 完成时间 |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 0 | recovery | — | Builder handoff Runtime checks failed: xgent-ui-regression | 2026-10-20T10:24:04.904Z |
| 1 | 2 | 0 | recovery | — | Builder handoff Runtime checks failed: changed-crates-tests | 2026-10-20T10:31:52.088Z |
| 1 | 3 | 1 | pass | — | 独立复核判定 pass：8/8 验收项通过。关键证据——（1）纯函数层：crates/xgent_ui/src/fonts.rs 的 resolve_slot 三级解析（系统按序命中 → 内嵌兜底 → Failed 记警告）实现与 spec 一致，crates/xgent_ui/tests/font_slots.rs 6 条测试我本机独立重跑全绿（内嵌兜底 / 候选顺序取第二个 / 失败标记 / 无平台特权路径 / 内嵌资产登记为 mono 最后一位 / 本机命中真等宽）。（2）资产层：crates/xgent_app/assets/fonts/JetBrainsMono-Regular.ttf 270224 字节，我用 parley/fontique 独立注册读出家族名确为 JetBrains Mono（真身非改名），OFL.txt 与 README.md 齐备，三平台 mono 候选栈均把它登记为最后一位内嵌兜底。（3）平台层：crates/ 内字体相关代码 `cfg!(target_os` 命中数为 0，旧 `else { Handle::default() }` 的 macOS 独占分支已从 startup.rs 删除；本机系统字体集合 163 家族含 Cascadia Mono/Consolas/Cascadia Code，mono 槽位实测解析为 Cascadia Mono（system，真实等宽，绝非 ui 槽位的 Inter Variable）。（4）不回归：xgent_ui lib 76 项 + 全部既有集成测试 0 失败，cargo check --workspace 通过，clippy 三 crate --all-targets 0 error 且本变更文件零告警，cargo fmt --check 退出码 0。（5）文档：ui-v8-tasks.md M4-T1 勾选 + §10 勘误 17~20 条、dev-tutorial.md §5.21、ui-v8-handoff.md 交付表 + §1.1 硬门槛 A 状态三处齐备。未闭环/风险项已列在 risks：最重要的一条是 Runtime 标记为「已通过」的 dd54cbc3 正式检查日志实为构建失败（rustc STATUS_STACK_BUFFER_OVERRUN，0 条 test result），绿色记录实为 59a56356；另 `ui` 槽位缺 FontSpec（与 spec 字面有偏差但不被 A1~A8 覆盖）、font_specs() 用 std::env::consts::OS 运行期选表、macOS/Linux 的硬门槛 A 仍需各启动一次确认。 | 2026-10-20T10:44:01.959Z |



## 结论

独立复核判定 pass：8/8 验收项通过。关键证据——（1）纯函数层：crates/xgent_ui/src/fonts.rs 的 resolve_slot 三级解析（系统按序命中 → 内嵌兜底 → Failed 记警告）实现与 spec 一致，crates/xgent_ui/tests/font_slots.rs 6 条测试我本机独立重跑全绿（内嵌兜底 / 候选顺序取第二个 / 失败标记 / 无平台特权路径 / 内嵌资产登记为 mono 最后一位 / 本机命中真等宽）。（2）资产层：crates/xgent_app/assets/fonts/JetBrainsMono-Regular.ttf 270224 字节，我用 parley/fontique 独立注册读出家族名确为 JetBrains Mono（真身非改名），OFL.txt 与 README.md 齐备，三平台 mono 候选栈均把它登记为最后一位内嵌兜底。（3）平台层：crates/ 内字体相关代码 `cfg!(target_os` 命中数为 0，旧 `else { Handle::default() }` 的 macOS 独占分支已从 startup.rs 删除；本机系统字体集合 163 家族含 Cascadia Mono/Consolas/Cascadia Code，mono 槽位实测解析为 Cascadia Mono（system，真实等宽，绝非 ui 槽位的 Inter Variable）。（4）不回归：xgent_ui lib 76 项 + 全部既有集成测试 0 失败，cargo check --workspace 通过，clippy 三 crate --all-targets 0 error 且本变更文件零告警，cargo fmt --check 退出码 0。（5）文档：ui-v8-tasks.md M4-T1 勾选 + §10 勘误 17~20 条、dev-tutorial.md §5.21、ui-v8-handoff.md 交付表 + §1.1 硬门槛 A 状态三处齐备。未闭环/风险项已列在 risks：最重要的一条是 Runtime 标记为「已通过」的 dd54cbc3 正式检查日志实为构建失败（rustc STATUS_STACK_BUFFER_OVERRUN，0 条 test result），绿色记录实为 59a56356；另 `ui` 槽位缺 FontSpec（与 spec 字面有偏差但不被 A1~A8 覆盖）、font_specs() 用 std::env::consts::OS 运行期选表、macOS/Linux 的硬门槛 A 仍需各启动一次确认。
