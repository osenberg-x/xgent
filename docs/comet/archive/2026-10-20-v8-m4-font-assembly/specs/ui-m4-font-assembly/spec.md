# ui-m4-font-assembly — 跨平台字体候选栈与等宽兜底

## Purpose

把「等宽字体」从 macOS 独占的特权路径（`startup.rs` 的 `else { Handle::default() }`）改成**三平台一致**的候选栈装配：系统里第一个真实存在的等宽家族优先，全缺则回退到内嵌 JetBrains Mono（OFL 1.1），**永不**回退到 `ui` 槽位的比例字体。

- `mono` 槽位解析到真实等宽资产（硬门槛 A）。
- 平台差异只存在于候选栈顺序这一处**数据**里，代码不分叉（C-FONT-3）。
- 解析结果写进启动日志：实际命中的家族名 + 来源（system / embedded），解析失败是警告不是静默（C-FONT-2）。
- 四个槽位（`ui` / `ui_cjk` / `mono` / `mono_cjk`）各有候选栈与解析结果，由装配层统一产出（设计系统 §1.2）。

本变更只交付**装配层**（M4-T1）。终端双宽单元格（M4-T2）、代码与行号消费 `mono`（M4-T3）、中文排版约束（M4-T4）各自独立成 change。

## Requirements

### 槽位与候选栈

- `xgent_ui::fonts` 提供 `FontSlot`（`Ui` / `UiCjk` / `Mono` / `MonoCjk`）、`FontCandidate`（家族名 + 可选内嵌资产路径）、`FontSpec`（槽位 → 有序候选栈）。
- 候选栈按平台给出不同顺序，但**顺序差异只存在于数据**；运行期代码不含平台分支。
- `ui` 槽位的候选是内嵌 Inter Variable（已存在），`ui_cjk` 是系统 CJK 家族栈（Windows `Microsoft YaHei UI` → macOS `PingFang SC` → Linux `Noto Sans CJK SC` → `Source Han Sans SC` → `WenQuanYi Micro Hei`），`mono` 是系统等宽家族栈（Windows `Cascadia Mono` → `Consolas`；macOS `SF Mono` → `Menlo`；Linux `JetBrains Mono` → `DejaVu Sans Mono` → `Liberation Mono`），`mono_cjk` 是 `Sarasa Mono SC` → `Noto Sans Mono CJK SC`。

#### Scenario: 候选栈按顺序命中

- **Given** 某槽位的候选栈是 `[A, B, C]`，系统字体集合里存在 `B` 与 `C`
- **When** 解析该槽位
- **Then** 解析结果取 `B`（第一个真实存在的候选），来源为 system

验收：A2

### 解析与兜底

- 解析按候选顺序取第一个在系统字体集合里**真实存在**的家族（存在性以 parley 字体集合的家族索引为准）。
- 候选全缺且该槽位有内嵌资产 → 用内嵌资产，来源标记为 `Embedded`。
- 候选全缺且无内嵌资产 → 解析结果标记为**失败**，调用方记 `warn!`；**不得**静默回退到别的槽位。
- `mono` 槽位的失败兜底是内嵌 JetBrains Mono，**永不**是 `ui` 槽位或任何比例字体（C-FONT-1）。
- `ResolvedFont` 携带：来源（system / embedded）、家族名、`FontSource`。

#### Scenario: 全缺时回退到内嵌等宽资产

- **Given** `mono` 槽位的系统候选全部不可用，且该槽位有内嵌 JetBrains Mono
- **When** 解析该槽位
- **Then** 来源为 `Embedded`、家族名为 JetBrains Mono，且解析结果不等于 `ui` 槽位的家族

验收：A1

#### Scenario: 解析失败是警告不是静默

- **Given** 某槽位无任何可用候选且无内嵌资产
- **When** 解析该槽位
- **Then** 解析结果被标记为失败（调用方据此记警告），不返回 `ui` 槽位的家族

验收：A3

### 装配与日志

- 启动时加载内嵌 Inter Variable（既有）与内嵌 JetBrains Mono（新增），用 parley 字体集合判断候选家族是否存在。
- 四个槽位各解析一次，解析结果写进 `tracing::info!`：槽位 → 家族名 → 来源。
- 解析失败的槽位写 `tracing::warn!`。
- **删除** `startup.rs` 的 `else { Handle::default() }` 分支。

#### Scenario: 解析在本机命中真实等宽

- **Given** 一台装有系统等宽字体的机器
- **When** 对 `mono` 槽位解析
- **Then** 来源为 system 或 embedded，家族名不是 `ui` 槽位的家族（Inter Variable）

验收：A6

#### Scenario: 平台差异只在数据里

- **Given** 字体装配的全部代码
- **When** 检索 `cfg!(target_os`
- **Then** 命中数为 0（平台差异只在候选栈数据里）

验收：A4

### 内嵌等宽资产

- `crates/xgent_app/assets/fonts/` 含 JetBrains Mono Regular 字体文件（OFL 1.1，官方 release 产物），与 Inter 同目录。
- `assets/fonts/` 记录两个字体的来源与许可（Inter 与 JetBrains Mono 同为 OFL 1.1，`OFL.txt` 已在）。
- 该资产被装配为 `mono` 槽位候选栈的最后一位（内嵌兜底）。

#### Scenario: 内嵌资产就位

- **Given** 仓库检出
- **When** 查看 `crates/xgent_app/assets/fonts/`
- **Then** 存在 JetBrains Mono 字体文件，且装配把它登记为 `mono` 槽位的内嵌兜底候选

验收：A5

### `UiFonts` 资源

- `UiFonts` 携带 `ui: Handle<Font>`（不变）、`mono: FontSource`、`ui_cjk: FontSource`、`mono_cjk: FontSource`。
- `mono_text` / `mono_text_weighted` 把 `fonts.mono` 直接放进 `TextFont.font`（`FontSource` 即可），调用点签名不变。
- `xgent_app::settings_io` 构造 `UiFonts` 的地方按新结构补齐。

#### Scenario: 等宽文本使用解析后的 mono 槽位

- **Given** `UiFonts.mono` 是候选栈解析出的 `FontSource`
- **When** 业务代码用 `mono_text` / `mono_text_weighted` 造文本
- **Then** 文本的 `TextFont.font` 等于该 `FontSource`（不再是默认句柄）

验收：A2

### 不回归

- `xgent_ui` lib 76 项与既有集成测试全绿。
- `cargo check --workspace` 通过；`cargo clippy -p xui -p xgent_ui -p xgent_app --all-targets` 无新增警告；`cargo fmt -p xui -p xgent_ui -p xgent_app` 后无 diff。

#### Scenario: 既有测试不回归

- **Given** 实现完成的仓库
- **When** 跑 lib 与集成测试
- **Then** 全部通过（失败数为 0）

验收：A7

### 文档同步

- `docs/plans/ui-v8-tasks.md`：M4-T1 勾选 + §10 追加本轮勘误。
- `docs/dev-tutorial.md`：新增 M4 小节（候选栈模型、解析规则、内嵌资产与坑）。
- `docs/plans/ui-v8-handoff.md`：交付表 + 基线 + 下一步（硬门槛 A 的跨平台确认方式）。

#### Scenario: 文档同步

- **Given** 实现完成的仓库
- **When** 检索 `docs/dev-tutorial.md` 与 `docs/plans/ui-v8-tasks.md`
- **Then** 两处都记录了 M4-T1 的落地内容与硬门槛 A 的跨平台确认要求

验收：A8
