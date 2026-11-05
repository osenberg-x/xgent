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

#### Scenario: 空历史或扫描失败
- **WHEN** 没有历史 usage 或扫描失败
- **THEN** 显示空态或可读错误，不 panic、不阻塞 UI

#### Scenario: 关闭面板
- **WHEN** 用户点击关闭或按 Esc
- **THEN** 面板关闭，状态栏与对话保持可用

### Requirement: 设置面板价格编辑

系统 SHALL 在设置面板提供默认输入/输出单价的编辑与保存；价格接受非负有限小数；模型覆盖通过配置文件提供，不在本期设置面板中逐模型编辑；保存后下一次统计按新价格计算。

#### Scenario: 保存默认价
- **WHEN** 用户在设置面板输入合法价格并保存
- **THEN** 配置写回全局 config.toml，统计使用新默认价

#### Scenario: 非法价格
- **WHEN** 输入负数、非数字或非有限值
- **THEN** 保存被拒绝并提示具体字段，不写入磁盘

### Requirement: 只读与兼容

系统 SHALL 只读消费会话 JSONL，不修改既有会话文件或持久化格式；价格配置缺失、会话文件不可读或历史为空时，成本面板与状态栏仍可正常显示，不阻塞对话与工具执行。

#### Scenario: 不写会话
- **WHEN** 打开面板或汇总历史
- **THEN** 会话 JSONL 字节不变

#### Scenario: 配置缺失
- **WHEN** 全局配置没有 pricing 段
- **THEN** 统计仍显示 token，成本显示未定价，不报错
