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
