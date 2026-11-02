# Spec Delta

## Purpose

为 3D 首页提供任意项目可用的文件/目录空间图，并保证同一快照的布局确定、增量更新不抖动。

## ADDED Requirements

### Requirement: 任意项目生成 L1 空间图
系统 SHALL 为打开的项目生成目录与文件级空间图；有 `Cargo.toml` 的项目额外叠加 crate 依赖边；没有 manifest 的项目仍生成目录/文件节点。

#### Scenario: Rust 工作区
- **WHEN** 用户打开一个含 `Cargo.toml` 的 Rust 工作区
- **THEN** 空间图包含 crate、目录与文件节点，并包含工作区内部的 crate 依赖边

#### Scenario: 非 Rust 项目
- **WHEN** 用户打开一个不含 `Cargo.toml` 的项目
- **THEN** 空间图仍包含目录与文件节点，且不报错、不显示 crate 依赖边

### Requirement: 扫描忽略规则
系统 SHALL 在扫描文件树时忽略 `.gitignore` 命中的路径、`target/`、`node_modules/` 与常见构建产物；忽略规则不得阻塞项目打开。

#### Scenario: 依赖目录不进入宇宙
- **WHEN** 项目根目录存在 `node_modules/` 或 `target/`
- **THEN** 空间图不包含这些目录下的节点

#### Scenario: 忽略规则失败
- **WHEN** 项目没有 `.gitignore` 或规则无法解析
- **THEN** 扫描继续，使用内置忽略规则并报告一次警告

### Requirement: 节点上限与目录聚合
系统 SHALL 将空间图可见节点限制在 2000 个以内；G7 降级到 800 档时限制在 800 个以内；超出时按目录聚合或裁剪，并报告被省略的节点数量。

#### Scenario: 大仓库
- **WHEN** 项目的文件树超过节点上限
- **THEN** 空间图节点数不超过上限，且界面/报告显示被省略的数量

#### Scenario: 小仓库
- **WHEN** 项目的文件树小于节点上限
- **THEN** 空间图包含全部目录与文件节点（受忽略规则约束）

### Requirement: 确定性布局与增量锚定
系统 SHALL 保证同一代码快照与同一空间图生成相同的三维坐标；增量加入节点时，既有节点的坐标不得改变。

#### Scenario: 重复布局
- **WHEN** 对同一快照连续生成两次布局
- **THEN** 两次得到的每个节点坐标逐位相同

#### Scenario: 增量更新
- **WHEN** 在既有空间图中新增一个节点
- **THEN** 既有节点的坐标保持不变，只有新节点被求解

### Requirement: 空间图只读投影
系统 SHALL 将空间图视为只读投影；扫描、布局与视口交互不得修改项目文件、配置或会话数据。

#### Scenario: 浏览不写盘
- **WHEN** 用户打开项目并在 3D 宇宙中浏览
- **THEN** 项目文件与配置不被修改
