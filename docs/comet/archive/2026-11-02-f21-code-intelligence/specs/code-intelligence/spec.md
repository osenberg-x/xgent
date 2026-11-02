# code-intelligence Specification

## Purpose

为 Rust 工作区提供只读、确定性的代码结构索引：符号、import/use 与复杂度；作为 RepoMap 上下文策略的数据源，并为后续 3D L2/诊断提供稳定查询 API。

## Requirements

### Requirement: Rust 符号索引

系统 SHALL 扫描工作区内 Rust 源文件并用 tree-sitter 提取符号；符号至少包含名称、种类、文件、起止行与函数复杂度；扫描遵守 `.gitignore`、`target/`、`node_modules/` 与隐藏文件规则。

#### Scenario: 符号类别与范围
- **WHEN** 索引包含函数、结构体、枚举、trait、impl、module、const/static 的 fixture
- **THEN** 每类符号都能按文件与名称查询，起止行覆盖对应语法节点
- 验收：A1

#### Scenario: 函数复杂度
- **WHEN** 函数体内包含 if/match/循环/`&&`/`||` 等决策点
- **THEN** 复杂度为 1 加决策点数量，且随源码变化可重算
- 验收：A1

#### Scenario: 忽略目录
- **WHEN** 工作区存在 `target/`、`node_modules/` 或 `.gitignore` 命中的 Rust 文件
- **THEN** 这些文件不进入索引
- 验收：A1

### Requirement: import 与模块解析

系统 SHALL 解析 `use` 与 `mod` 声明，尽可能解析到工作区内的文件或模块；工作区外的依赖标记为 external；无法解析的声明保持未解析且不报错。

#### Scenario: 工作区 use 解析
- **WHEN** 源文件包含 `use crate::foo::Bar` 或 `mod foo;` 且对应模块文件存在
- **THEN** 产生从当前文件到目标文件/模块的 import 记录
- 验收：A2

#### Scenario: 外部依赖
- **WHEN** 源文件引用工作区外的 crate 或无法解析的路径
- **THEN** 记录标记为 external 或未解析，不产生工作区文件边、不报错
- 验收：A2

### Requirement: 确定性增量索引

系统 SHALL 保证同一源码快照的索引顺序与结果逐位一致；单文件更新只替换该文件的符号与 import，删除文件移除其条目；generation 反映内容变化。

#### Scenario: 重复扫描确定
- **WHEN** 对同一快照连续扫描两次
- **THEN** 符号、import 与 generation 完全一致
- 验收：A1、A3

#### Scenario: 单文件更新
- **WHEN** 调用 `update_file` 修改一个文件或调用 `remove_file` 删除一个文件
- **THEN** 只有该文件相关条目变化，其他文件结果保持不变；generation 更新
- 验收：A3

### Requirement: 查询 API

系统 SHALL 提供按文件、符号名与路径查询的只读 API；查询返回稳定排序的结果，调用方不需要了解 tree-sitter 节点细节。

#### Scenario: 按文件查询
- **WHEN** 调用方查询一个已索引文件
- **THEN** 返回该文件的符号、import 与复杂度摘要
- 验收：A1、A6

#### Scenario: 未索引文件
- **WHEN** 调用方查询不存在或未索引的文件
- **THEN** 返回空结果而不是 panic
- 验收：A6

### Requirement: RepoMap 检索

系统 SHALL 实现 `RepoMapContextProvider`，使 `ContextStrategy::RepoMap` 可构造为可用 provider；检索按符号名、文件路径、当前文件与 hints 评分，返回受 token 预算约束的结构化 repo map 片段；无匹配时返回空结果。

#### Scenario: 策略可用
- **WHEN** 以 `repo_map` 策略构造上下文 provider
- **THEN** 返回 Ready，而不是 Unsupported
- 验收：A4

#### Scenario: 结构化检索
- **WHEN** 用户消息或 hints 提到已索引的符号/文件
- **THEN** 返回相关文件与符号的 repo map 文本片段，token 总数不超过请求预算
- 验收：A4

#### Scenario: 无匹配
- **WHEN** 查询没有任何符号或文件匹配
- **THEN** 返回空 chunks，不报错、不阻塞
- 验收：A4

### Requirement: 文件变更增量刷新

系统 SHALL 在 `ContextProvider::on_file_changed` 收到 Rust 文件变更时增量更新索引；非 Rust 文件、目录或不存在路径不破坏索引；后续检索看到新符号。

#### Scenario: 变更后检索
- **WHEN** 已构建索引后某 `.rs` 文件新增一个函数并触发 `on_file_changed`
- **THEN** 后续查询能检索到新函数，其他文件结果不变
- 验收：A5

#### Scenario: 非 Rust 变更
- **WHEN** `on_file_changed` 收到非 `.rs` 路径或不存在路径
- **THEN** 索引保持不变且不 panic
- 验收：A5

### Requirement: 只读与后台执行

系统 SHALL 将代码索引视为只读投影；扫描/解析不得写项目文件；provider 首次建索引在阻塞线程池执行，不阻塞 UI 主线程；工作区不可读或文件解析失败时跳过并降级为空/部分索引。

#### Scenario: 不写盘
- **WHEN** 扫描与查询一个工作区
- **THEN** 项目文件字节不变
- 验收：A6

#### Scenario: 后台首次构建
- **WHEN** 首次调用 RepoMap 检索
- **THEN** 扫描在阻塞线程池执行，异步调用方在完成前不被同步阻塞
- 验收：A6

#### Scenario: 解析失败降级
- **WHEN** 某个 Rust 文件无法读取或解析
- **THEN** 该文件被跳过，索引继续构建，不 panic
- 验收：A6
