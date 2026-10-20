# 目标

把 `mono`（等宽）槽位从「macOS 独占特权路径」改成**三平台一致**的候选栈装配：系统里第一个真实存在的等宽家族优先，全缺则回退到**内嵌 JetBrains Mono（OFL 1.1）**，永不回退到 `ui` 槽位的比例字体；解析结果写进启动日志，三平台可核对。

1. **硬门槛 A**（台账 M4-T1）：`mono` 必须解析到真实等宽资产。今天 `startup.rs:88` 的 `else { Handle::default() }` 在 Windows/Linux 上让「等宽」实际是 Inter（比例字体）——删除该分支。
2. **候选栈即数据**（C-FONT-3）：平台差异只存在于候选栈顺序这一处数据里，代码不分叉（禁止新增 `cfg!(target_os = …)`）。
3. **解析可核对**（C-FONT-2）：启动日志打出每个槽位实际命中的家族名与来源（system / embedded）；解析失败是**警告**不是静默。
4. **三槽位模型落地**（设计系统 §1.2）：`ui` / `ui_cjk` / `mono` / `mono_cjk` 四个槽位各有候选栈与解析结果，装配层统一产出。

# 范围

## 字体候选栈与解析（`xgent_ui::fonts`）

- 新增 `FontSlot`（`Ui` / `UiCjk` / `Mono` / `MonoCjk`）、`FontCandidate`（家族名 + 可选内嵌资产路径）、`FontSpec`（槽位 → 候选栈；平台顺序差异只在这份数据里）。
- 新增 `FontOrigin`（`System` / `Embedded`）与 `ResolvedFont`（解析结果：来源 + 家族名 + `FontSource`）。
- 新增纯函数 `resolve_slot(spec, available_families) -> ResolvedFont`：按候选顺序取第一个在系统字体集合里真实存在的家族；全缺且该槽位有内嵌资产 → 内嵌；全缺且无内嵌 → 失败标记 + 该槽位的声明兜底。
- `UiFonts` 扩展：`ui: Handle<Font>`（不变）+ `mono: FontSource`（解析产出）+ `ui_cjk: FontSource` + `mono_cjk: FontSource`。`mono_text` / `mono_text_weighted` 的调用点不变（`FontSource` 可直接进 `TextFont.font`）。

## 内嵌等宽资产

- `crates/xgent_app/assets/fonts/` 增加 **JetBrains Mono Regular**（OFL 1.1；官方 release 的 `JetBrainsMono-Regular.ttf`，约 264KB），与既有 Inter 同目录；`assets/fonts/` 补一份来源与许可说明（与 Inter 同为 OFL 1.1，`OFL.txt` 已在）。
- 该资产是 `mono` 槽位的**最后一位**候选（系统等宽优先），保证任何平台都有真实等宽。

## 启动装配

- `xgent_app::startup::load_fonts`：加载 Inter（不变）+ 加载内嵌 JetBrains Mono；用 parley 字体集合（`FontCx.context.collection`）判断候选家族是否真实存在；解析四个槽位并 `tracing::info!` 打出「槽位 → 家族名 → 来源」；解析失败 `tracing::warn!`。
- **删除** `else { Handle::default() }` 分支（C-FONT-1）。
- `xgent_app::settings_io` 里构造 `UiFonts` 的地方按新结构补齐。

# 非目标

- 不做 M4-T2 终端双宽单元格（`MonoMetrics` / `display_width` / 按格定位）——本轮之后独立的最重改动面，另开 change。
- 不做 M4-T3（代码/行号消费 `mono`、`type_scale::CODE` 档、`SMALL 12.5 → 12`）与 M4-T4（中文排版约束）——另开 change。
- 不改 `ui_text` 的调用签名（`ui` 槽位继续走默认字体句柄，不给约 50 处调用点加字体句柄参数）。
- 不做亮色主题与其它视觉变更。

# 验收示例

- A1 `mono` 槽位在全缺系统等宽时回退到内嵌 JetBrains Mono：给定候选集合里没有任何家族可用，解析结果的来源是 `Embedded`、家族名是 JetBrains Mono，且**不等于** `ui` 槽位的家族。
- A2 候选栈按顺序命中：给定系统里存在第二个候选家族，解析结果取第二个（不是第一个、不是最后一个）。
- A3 解析失败是警告不是静默：给定某槽位无任何可用家族且无内嵌资产，解析结果标记为失败（调用方据此 `warn!`），不静默返回 `ui` 槽位。
- A4 平台差异只在数据里：`crates/` 内字体相关代码的 `cfg!(target_os` 命中数为 0。
- A5 内嵌资产就位：`crates/xgent_app/assets/fonts/` 存在 JetBrains Mono 字体文件（OFL 1.1），并被装配为 `mono` 槽位的内嵌兜底候选。
- A6 解析在本机命中真实等宽：在本机系统字体集合上解析 `mono` 槽位，来源是 system 或 embedded，家族名**不是** `ui` 槽位的家族（Inter Variable）。
- A7 既有行为不回归：`xgent_ui` lib 76 项与既有集成测试全绿；`cargo check --workspace` 与 `cargo clippy -p xui -p xgent_ui -p xgent_app --all-targets` 通过。
- A8 文档同步：`docs/plans/ui-v8-tasks.md` 的 M4-T1 勾选 + §10 勘误、`docs/dev-tutorial.md` 新增 M4 小节、`docs/plans/ui-v8-handoff.md` 的交付表与下一步。

# 决策

- D-1 候选栈用 `FontSource::List`（有序来源列表）表达：parley 侧天然支持 fallback 链，代码不新增解析分支。
- D-2 `ui` 槽位保持「默认字体句柄」：`ui_text` 不带字体句柄（约 50 处调用点），CJK 由 parley 的系统回退承担；`ui_cjk` 槽位本轮只做**解析与日志**（保证 CJK 可用性可核对），专用 CJK 表面留给 M4-T4。
- D-3 内嵌资产用官方 release 的 **Regular** 单字重（264KB）：`mono_text_weighted` 的 510/590 字重由 parley 就近选面（无对应字面时回落 Regular），本轮不做子集化（环境无 fontTools），体积仍在设计系统的量级内。
- D-4 槽位解析失败时 `mono` 的兜底是内嵌 JetBrains Mono（**不是** `ui`）——C-FONT-1 明令「永不回退到 `ui` 槽位或比例字体」。
- D-5 `UiFonts.mono` 的类型从 `Handle<Font>` 改为 `FontSource`（候选栈解析结果）；`mono_text` / `mono_text_weighted` 的调用点不变（`FontSource` 可直接进 `TextFont.font`）。

# 验证要求

- 本机（Windows）可验：解析函数在本机系统字体集合上对 `mono` 槽位返回 system 来源的真实等宽家族（本机有 Cascadia Mono / Consolas），且不是 Inter Variable。
- macOS / Linux 的同一条解析需由用户在对应平台各启动一次确认（本变更无法在本机覆盖）。
- 全量测试带 `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`。
