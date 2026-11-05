# Comet Design Handoff

- Change: f12-cost-stats
- Phase: design
- Mode: compact
- Context hash: d401d760b9bc03e62d941bcb8a44711dc62f3ce066dbaf2f59faff4f4ca29f33

Generated-by: comet-handoff.sh
Task hash policy: task-content-v1. Read tasks.md for live completion; excerpts are design-time context.

OpenSpec remains the canonical capability spec. This handoff is a deterministic, source-traceable context pack, not an agent-authored summary.

## docs/openspec/changes/f12-cost-stats/proposal.md

- Source: docs/openspec/changes/f12-cost-stats/proposal.md
- Lines: 1-31
- SHA256: e4921cdec840354c4d990cd5c3a1204f679e643a6ac17122b67b52500b9ebba1

```md
# Proposal

## Why

状态栏目前只累计 token 总数，成本段固定显示 `$0.00`；用户看不到当前会话花了多少钱，也看不到历史会话的用量。F-12 在 P1 中要求 token 消耗与成本统计，OQ-10 需要在本期定下数据来源与展示形态。

## What Changes

- 新增全局价格配置：默认输入/输出单价（USD / 1M tokens），并支持按模型覆盖；设置面板编辑默认单价，模型覆盖在 `config.toml` 中配置。
- 当前会话：从 provider `DoneMessage` / `AssistantMessage.usage` 累计 prompt、completion 与成本；状态栏显示 token 与成本。
- 成本面板：点击状态栏成本段打开 overlay，展示当前会话分解、历史累计（sessions 目录 JSONL，包含当前会话）与按模型汇总；历史扫描在后台执行，展示加载/空态/错误。
- 历史扫描只读取现有会话 JSONL；缺失 usage、损坏行或未配置价格的模型跳过计价但仍可展示 token（或显示为未定价）。
- 非目标：缓存 token 单价、汇率换算、预算/告警、自动抓取价目表、按项目分组、导出报表。

## Capabilities

### New Capabilities

- `cost-accounting`: 价格配置、当前会话 token/成本累计、历史会话聚合与成本面板行为。

### Modified Capabilities

（无。现有会话持久化与状态栏布局契约不变，只新增读取与展示。）

## Impact

- `crates/xgent_settings_core`：全局 `PricingConfig`（默认价 + `models` 覆盖）与金额计算辅助。
- `crates/xgent_daemon`：新增 `pricing.*` 全局配置键读写，保持配置热更新。
- `crates/xgent_ui`：状态栏成本段可点击、成本 overlay、设置面板价格字段、历史 JSONL 后台聚合。
- `crates/xgent_core` / `xgent_agent`：只读消费既有 `TokenUsage`、`AssistantMessage.model/usage` 与会话存储，不改持久化格式。
- 文档与测试：dev-tutorial、architecture、AGENTS；价格/累计/历史/面板/设置面板回归。

```

## docs/openspec/changes/f12-cost-stats/design.md

- Source: docs/openspec/changes/f12-cost-stats/design.md
- Lines: 1-116
- SHA256: 1c49a6154168eae19cc0055579b1be1bcffe4ae1eafdb4bde4769e73cecaf1c0

[TRUNCATED]

```md
---
comet_change: f12-cost-stats
role: technical-design
canonical_spec: openspec
---

# Design

## Context

- `DoneMessage` 与 `AssistantMessage` 已携带 `TokenUsage { prompt, completion }`、`model`，并写入会话 JSONL；无需改持久化格式。
- 状态栏已有 token 累计与静态 `$0.00` 成本段；设置面板是固定键表单，全局配置由 daemon 写入。
- 现有设置框架的数值字段是 `u64`，价格需要小数；模型价格表是动态键，不适合直接塞进固定 NumField。
- F-12 选择：当前会话 + 历史累计；默认价 + 配置文件按模型覆盖；状态栏 + 成本面板。

## Goals / Non-Goals

**Goals:**

- 当前会话与历史会话的 token/成本可解释、可核对。
- 价格可配置，未定价不伪装成 0 元。
- 历史扫描不阻塞 UI，损坏数据可降级。
- 设置面板能编辑默认价；模型覆盖通过配置文件。

**Non-Goals:**

- 缓存 token、货币换算、预算/告警、自动价目表、按项目分组、导出。
- 修改会话 JSONL 格式或写入统计数据库。
- 在设置面板做动态模型价格表编辑器。

## Decisions

### D1：价格模型

`xgent_settings_core::global` 增加：

```rust
PricingConfig {
  input_per_million: f64,   // USD / 1M tokens；0 = 未设置
  output_per_million: f64,
  models: BTreeMap<String, ModelPrice>,
}
```

- `ModelPrice` 允许字段级覆盖：模型字段为 0/缺失时回落到默认价。
- `PricingConfig::price_for(model) -> Option<(f64, f64)>`：输入/输出任一为 0 即未定价；负值/NaN/Infinity 视为无效。
- 只支持 USD；不做汇率。
- 配置文件示例：
  ```toml
  [pricing]
  input_per_million = 2.5
  output_per_million = 10.0
  [pricing.models."gpt-4o-mini"]
  input_per_million = 0.15
  output_per_million = 0.6
  ```

### D2：成本计算

`PricingConfig::cost(prompt, completion, model) -> Option<f64>`：
`prompt/1e6*input + completion/1e6*output`，结果必须有限且非负；未定价返回 `None`。
展示格式：未定价显示 `—`；`< $0.01` 显示 4 位小数，否则 2 位；金额前加 `$`。

### D3：当前会话统计

- 新增 UI 侧 `CostStats` Resource，从 `Conversation.messages` 的 AssistantMessage 汇总 prompt/completion/cost；`Conversation` 或 `GlobalConfigRes` 变化时重算。
- 恢复会话后自动按恢复消息重算；没有 usage 的消息跳过。
- 状态栏 token 段显示当前 prompt+completion；成本段显示当前成本，并作为点击入口。
- 不依赖 `TokenUsage` 旧资源累加，避免恢复/压缩后重复或丢失。

### D4：历史聚合

- 新模块 `xgent_ui::cost_panel` 在面板首次打开时启动后台线程扫描 `<agent_dir>/sessions/*.jsonl`。
- 使用 `SessionEntry` 反序列化，仅统计 `Message(Assistant)` 的 usage/model；损坏行/无 usage/无 model 跳过，无 model 归入 `unknown`。
- 结果：总 prompt/completion/cost、按模型（BTreeMap，按 token 降序展示）、扫描文件数、跳过条数；包含当前会话，文案标注“含当前会话”。
- 扫描使用 `std::thread` + `std::sync::mpsc`，Update 中 `try_recv`，不阻塞主线程；失败/空数据落到面板状态。
- 每次打开面板重新扫描；扫描中重复点击不启动第二个任务。

### D5：成本面板


```

Full source: docs/openspec/changes/f12-cost-stats/design.md

## docs/openspec/changes/f12-cost-stats/tasks.md

- Source: docs/openspec/changes/f12-cost-stats/tasks.md
- Lines: 1-32
- SHA256: 2f18dc6824f962ac654b888f79f567a2faab0f70a0d4a7f4ddd63bfafbc9b976

```md
# Tasks

## 1. 价格配置与成本计算

- [ ] 1.1 在 `xgent_settings_core` 增加 `PricingConfig`/`ModelPrice`、默认价与模型覆盖优先级、价格合法性校验、成本公式；验证 `cargo test -p xgent_settings_core cost` 覆盖默认价、模型覆盖、未定价、非法值、极小额。
- [ ] 1.2 在 `docs/dev-tutorial.md` 增加 `[pricing]` 配置说明与示例；验证示例可由 1.1 的解析测试原样读取。

## 2. 配置写入与设置面板

- [ ] 2.1 在 daemon `ConfigStore` 增加 `pricing.input_per_million`/`pricing.output_per_million` 读写与 `0..=10000` 有限数校验；验证 daemon 单测覆盖合法/非法/往返。
- [ ] 2.2 在设置面板增加“成本”分区（两个小数输入框），只写脏键、非法值不落盘并提示；验证 UI 单测覆盖解析、脏检查、错误提示与 ConfigItem 键。

## 3. 当前会话与状态栏

- [ ] 3.1 新增 UI 侧 `CostStats` Resource，从 `Conversation` 的 Assistant usage 汇总 prompt/completion/cost，恢复会话与价格变化时重算；验证无 usage、恢复、价格切换测试。
- [ ] 3.2 状态栏 token/成本段接入 `CostStats`，成本段改为可点击按钮并打开成本面板；验证状态栏文本与按钮标记测试，未定价显示 `—`。

## 4. 成本面板与历史聚合

- [ ] 4.1 新增成本面板 overlay（scrim/标题/关闭/当前会话/历史/按模型行/加载/空态/错误），支持点击状态栏与 Esc 关闭；验证 panel 开关与状态渲染测试。
- [ ] 4.2 实现 sessions JSONL 后台聚合（线程 + mpsc、损坏行/缺 usage/unknown model 跳过、按模型排序、包含当前会话），不阻塞 UI、不写会话文件；验证临时会话聚合与字节不变测试。
- [ ] 4.3 接通面板与聚合结果、按模型前 20 行/其余汇总与 i18n；验证面板加载→结果→关闭集成测试。

## 5. 文档与回归

- [ ] 5.1 同步 `docs/design/architecture.md`、`docs/dev-tutorial.md` 与 `AGENTS.md` 的 F-12 状态、配置与边界；验证文档路径/测试名存在。
- [ ] 5.2 运行相关 crate 回归与 workspace check；验证 `cargo test -p xgent_settings_core -p xgent_agent -p xgent_ui -p xgent_app` 与 `cargo check --workspace` 通过。

## Workflow follow-up

- 归档前按项目评审要求完成最终验证。
- 归档后按用户选择的交付方式推送/创建 PR；未获授权前不推送。

```

## docs/openspec/changes/f12-cost-stats/.openspec.yaml

- Source: docs/openspec/changes/f12-cost-stats/.openspec.yaml
- Lines: 1-2
- SHA256: 3e16994612e657a7bbaefe38b70ef46f3e8e9f382f092dcaceb60ebabe7a5da0

```md
schema: spec-driven
created: 2026-11-05

```

## docs/openspec/changes/f12-cost-stats/specs/cost-accounting/spec.md

- Source: docs/openspec/changes/f12-cost-stats/specs/cost-accounting/spec.md
- Lines: 1-111
- SHA256: e3c6c058a096752dc52f330257f586c9c46a6a96ceda2a6c84c57406511def8a

[TRUNCATED]

```md
# Spec Delta

## Purpose

为 XGent 提供可解释的 token 与成本统计：当前会话、历史会话与按模型分解，价格可由用户配置，未配置时不猜金额。

## ADDED Requirements

### Requirement: 价格配置与优先级

系统 SHALL 支持全局默认输入/输出单价（USD / 1M tokens）与按模型覆盖；同一模型优先使用模型覆盖，否则使用默认价；价格缺失或为 0 时视为未定价；负值、非有限值 MUST 被拒绝或忽略并告警。

#### Scenario: 默认价生效
- **WHEN** 模型没有单独覆盖价，默认价已设置
- **THEN** 该模型的成本按默认输入/输出单价计算

#### Scenario: 模型覆盖优先
- **WHEN** 同一模型同时存在默认价与模型覆盖价
- **THEN** 使用模型覆盖价计算，默认价不参与

#### Scenario: 未定价模型
- **WHEN** 模型既无覆盖价，默认价也未设置或为 0
- **THEN** 只展示 token，成本显示为未定价，不产生 0 元假象

### Requirement: 当前会话统计

系统 SHALL 从当前会话的 AssistantMessage usage 汇总 prompt、completion、总 token 与成本；恢复的历史会话同样按消息 usage 重算；没有 usage 的消息不参与累计。

#### Scenario: 完成一轮后累计
- **WHEN** 一轮回复携带 prompt/completion usage
- **THEN** 当前会话的三项 token 与成本随之增加

#### Scenario: 恢复会话
- **WHEN** 会话从 JSONL 恢复且消息含 usage
- **THEN** 当前会话统计按恢复后的消息重算，不重复累加

#### Scenario: 无 usage
- **WHEN** 消息没有 usage
- **THEN** 该消息不计入 token 与成本，也不报错

### Requirement: 成本计算与格式

系统 SHALL 按 `prompt / 1_000_000 * input_price + completion / 1_000_000 * output_price` 计算成本；结果 MUST 为有限非负数；展示统一使用 USD，保留足够小数以显示小额成本。

#### Scenario: 正常计价
- **WHEN** usage 与价格均有效
- **THEN** 成本等于按公式计算的值，且与 token 同源更新

#### Scenario: 极小或为零
- **WHEN** token 为 0 或价格极低
- **THEN** 展示有限数值或未定价，不出现 NaN、Infinity 或负数

### Requirement: 历史会话聚合

系统 SHALL 扫描 sessions 目录下的 JSONL，汇总所有 AssistantMessage 的 usage 与 model；损坏行、缺失 usage 与无 model 的消息跳过或归入 unknown；聚合包含当前会话，结果按模型稳定排序。

#### Scenario: 多会话汇总
- **WHEN** sessions 目录存在多个含 usage 的会话
- **THEN** 历史累计等于各会话之和，并按模型给出分解

#### Scenario: 损坏与缺失数据
- **WHEN** 某行 JSON 损坏或某条消息没有 usage/model
- **THEN** 跳过该条并继续扫描，不中断整体聚合

#### Scenario: 包含当前会话
- **WHEN** 当前会话已经写入 JSONL
- **THEN** 历史累计包含它，并在面板标注“含当前会话”

### Requirement: 成本面板

系统 SHALL 提供从状态栏成本段打开的成本面板；面板展示当前会话 prompt/completion/成本、历史累计 token/成本与按模型分解；历史扫描在后台执行，面板显示加载、空态或错误状态；支持关闭并回到原界面。

#### Scenario: 打开面板
- **WHEN** 用户点击状态栏成本段
- **THEN** 面板打开并展示当前会话数据，历史区域先显示加载态

#### Scenario: 扫描完成
- **WHEN** 后台扫描返回结果
- **THEN** 历史累计与按模型分解显示，加载态消失


```

Full source: docs/openspec/changes/f12-cost-stats/specs/cost-accounting/spec.md
