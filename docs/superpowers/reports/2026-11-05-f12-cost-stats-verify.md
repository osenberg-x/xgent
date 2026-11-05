# F-12 成本统计验证报告

- Change: `f12-cost-stats`
- Date: 2026-11-05
- 验证模式：full
- 结论：**通过（0 CRITICAL / 0 IMPORTANT）**，可归档；保留已知限制。

## 摘要

| 维度 | 结果 |
|:---|:---|
| Completeness | 11/11 tasks；7 个 Requirement 均有实现 |
| Correctness | 价格、当前会话、成本公式、历史聚合、面板、设置、只读边界全部通过 |
| Coherence | Design Doc 与实现一致；复审 CRITICAL/IMPORTANT 已闭环 |

## 实际检查

- Build：`cargo check --workspace -j 2` 通过。
- Verify：`CARGO_PROFILE_TEST_DEBUG=0 cargo test -p xgent_settings_core -p xgent_daemon -p xgent_agent -p xgent_ui -p xgent_app -j 1 -- --test-threads=1` 通过。
- `comet classic openspec -- validate f12-cost-stats --strict`：通过。

## 独立复审与修复

第一轮复审发现并修复：

- **CRITICAL**：`settings_io` 读回 `pricing` 后未合并进 `GlobalConfigRes`（快照、保存、启动偏好三路）。三处已补 `global.0.pricing = g.pricing.clone()`，并新增 `snapshot_applies_pricing_to_resource` 回归。
- **IMPORTANT**：成本 overlay 遮罩缺少 `Pickable`；价格 4 位小数显示导致往返改写；历史标题未展示、空态按文件数判据；负/NaN 价格未告警。均已修复（精确 `format!("{value}")`、`models.is_empty`、`tracing::warn`、根节点 Pickable、历史标题）。
- 第二轮聚焦复审：0 CRITICAL / 0 IMPORTANT，可归档。

## 覆盖要点

- 价格：默认价/模型覆盖/字段级回落、0 未定价、非法值忽略并告警、成本公式与极小值。
- 当前会话：Conversation 重算、恢复会话、无 usage、价格变化、未定价 `—`。
- 历史：后台线程 + crossbeam_channel，损坏行/缺 usage/unknown model 跳过，按模型排序，包含当前会话，只读字节不变。
- 面板：打开/关闭、当前会话/历史/模型文本、加载/空/错误、标题与 Esc/关闭。
- 设置：小数默认价显示/保存、空值清 0、非法价格拦截；daemon `pricing.*` 读写与范围校验；快照合并 pricing。
- 状态栏：token/成本文本、未定价 `—`、点击打开面板。

## 已知限制（非阻塞）

- 模型价按 model 字符串匹配，跨 provider 同名模型共享价格；配置文件维护覆盖。
- 压缩会话的当前会话统计按恢复后消息，历史统计按 JSONL 全量消息，面板标注历史含当前会话。
- 历史扫描每次打开重扫，未做缓存/索引；大 sessions 目录显示加载态。
- 确认/遮罩点击穿透已修；状态栏点击在 overlay 打开时被遮罩拦截，Esc/关闭按钮可关。
- 单条未定价消息使总会话成本显示 `—`，未提供“已定价小计”展示。
- 全量测试默认并行时既有 `xgent_agent` 固定帧用例仍可能时序抖动；Verify 以串行命令留证。

## 复审结论

第一轮 1 CRITICAL + 4 IMPORTANT 已修复；第二轮 0 CRITICAL / 0 IMPORTANT。
