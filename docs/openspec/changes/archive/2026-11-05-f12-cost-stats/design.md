---
comet_change: f12-cost-stats
role: technical-design
canonical_spec: openspec
archived-with: 2026-11-05-f12-cost-stats
status: final
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
- 扫描使用 `std::thread` + `crossbeam_channel`，Update 中 `try_recv`，不阻塞主线程；失败/空数据落到面板状态。
- 每次打开面板重新扫描；扫描中重复点击不启动第二个任务。

### D5：成本面板

- 状态栏成本段改为 Button + `CostToggleMarker`；点击切换 `CostPanelState.open`。
- 新增常驻 overlay（Startup spawn、Visibility 控制），样式沿用 session_history：scrim、标题、关闭按钮、当前会话区、历史区、按模型行。
- 面板显示加载/空态/错误；Esc 与关闭按钮关闭；不改变对话、工具或编辑器焦点。
- 按模型展示前 20 个模型，按 token 降序；超出时显示“其余 N 个模型”汇总。

### D6：设置面板默认价

- 全局配置新增可写键 `pricing.input_per_million` / `pricing.output_per_million`，daemon 接受 JSON number/字符串并校验 `0..=10000` 的有限数。
- 设置面板新增“成本”分区，两个小数文本输入框（不是 u64 NumField）；保存时只写脏键，非法输入不落盘并提示字段。
- 模型覆盖不在设置面板编辑；配置文件中维护，保存后下一次统计生效。

### D7：i18n 与测试

- 所有状态栏/面板/设置文案走 fluent；新增 zh-CN/en-US 键。
- 测试：
  - settings_core：价格解析、优先级、非法值、成本公式。
  - xgent_ui：当前会话重算、恢复会话、历史聚合（临时 JSONL，含损坏行/缺 usage/unknown model）、面板加载/空态/错误、状态栏文本、设置面板保存脏键/非法值、会话文件字节不变。
  - daemon：`pricing.*` 读写与范围校验。

## Risks / Trade-offs

- [历史扫描成本] → 后台线程 + 每次打开重扫；会话很多时显示加载态。后续可加缓存/索引。
- [模型名跨 provider 冲突] → 价格按 model 字符串匹配，个人工具可接受；文档说明。
- [压缩会话] → 历史按 JSONL 全量消息统计，当前会话按恢复后消息统计；两者口径差异在面板文案中标注“历史累计（含当前会话）”。
- [小数 UI] → 设置面板新增专用价格输入，不复用 u64 数值字段；解析失败不写入。
- [旧会话无 usage] → 跳过计价，不猜测 token。

## Migration Plan

- 无数据迁移；新增 `[pricing]` 配置段。未配置时行为与当前一致（成本显示未定价）。
- 回滚：删除 `[pricing]` 或恢复旧二进制；不触碰会话文件。

## Open Questions

- 后续是否支持缓存 token 单价/批量折扣；本期不做。
- 历史聚合是否需要按项目/时间范围筛选；本期只做全量。
