# Brainstorm Summary

- Change: f12-cost-stats
- Date: 2026-11-05

## Confirmed Technical Approach

全局 `PricingConfig` 支持默认输入/输出单价与按模型覆盖（字段级回落，未定价不显示 0 元）；UI `CostStats` 从 `Conversation` 的 Assistant usage 重算当前会话；成本面板打开时用后台线程扫描 `sessions/*.jsonl` 汇总历史（含当前会话、按模型、损坏行/缺 usage 跳过）；状态栏成本段作为入口；设置面板新增两个小数输入编辑默认价，模型覆盖只在配置文件维护。

## Key Tradeoffs and Risks

- 历史扫描耗时：后台线程 + 每次打开重扫，加载态；后续可加缓存/索引。
- 模型名跨 provider 冲突：按 model 字符串匹配，文档说明。
- 压缩会话：当前会话按恢复消息重算，历史按 JSONL 全量消息；面板标注“含当前会话”。
- 价格小数输入不复用 u64 NumField，新增专用价格输入；非法值不落盘。
- 旧会话无 usage：跳过计价，不猜 token。

## Test Strategy

- settings_core：价格解析、默认/模型覆盖优先级、非法值、成本公式与极小值。
- daemon：`pricing.*` 读写与范围校验。
- xgent_ui：当前会话/恢复会话累计、状态栏文本、面板加载/空态/错误、历史聚合（临时 JSONL、损坏行、unknown model、字节不变）、设置面板脏键/非法值。
- 回归：相关 crate 测试 + workspace check。

## Spec Patch

None；Open 阶段 spec 已覆盖价格、当前会话、历史、面板、设置与只读边界。
