# spatial-s0 · 3D 视口 S0 内核

## Purpose

为 F-16 3D 视口提供与渲染无关的第一层：把工作区的 crate 依赖投影成 `SpatialGraph`，并给出**确定性、可增量**的三维坐标。S1 的离屏视口只消费本能力产出的图与坐标，不重新计算结构。

本能力不依赖 Bevy，不读写 UI 状态，也不成为任何代码事实的真相源。

## Requirements

### 空间图类型

系统 SHALL 在 `xgent_core::spatial` 提供可序列化的纯类型：

- `SpatialGraph { generation: u64, nodes: Vec<SpatialNode>, edges: Vec<SpatialEdge>, clusters: Vec<SpatialCluster> }`
- `SpatialNodeId(u32)` 与 `ClusterId(u32)`：`Copy + Eq + Hash + Ord`，serde 透明。
- `SpatialNode { id, kind, label: String, path: Option<PathBuf>, cluster: ClusterId, weight: f32, status: SpatialStatus, anchor: Option<[f32; 3]> }`
- `SpatialCluster { id, label: String, path: Option<PathBuf>, weight: f32, anchor: Option<[f32; 3]> }`
- `SpatialEdge { from: SpatialNodeId, to: SpatialNodeId, kind: EdgeKind, weight: f32 }`
- `SpatialNodeKind { Crate, Module, File, Symbol, External }`
- `SpatialStatus { Idle, Changed, Running, Failed }`
- `EdgeKind { DependsOn, Contains, Imports, ChangedTogether, AgentTouched }`

类型不得引用任何 Bevy 类型；`anchor` 是布局内核回填的唯一可变字段，其余字段由数据源决定。

### Cargo.toml 解析

系统 SHALL 提供 `xgent_spatial::manifest::parse_manifest(source: &str) -> Result<ManifestInfo, SpatialError>`：

- 读取 `package.name`；缺失时返回 `SpatialError::MissingPackage`。
- 收集 `[dependencies]`、`[dev-dependencies]`、`[build-dependencies]` 的键名，保留 `kind`（Normal / Dev / Build）。
- 识别依赖项是否为 `{ path = "..." }` 或 `{ workspace = true }`，分别记录 `path` 与 `workspace` 标记。
- 不解析版本号、不联网、不展开 `[workspace.dependencies]`。

### 工作区扫描

系统 SHALL 提供 `xgent_spatial::workspace::scan(root: &Path) -> Result<SpatialGraph, SpatialError>`：

- 从 `root` 开始递归查找 `Cargo.toml`，跳过 `target`、`.git`、`.comet`、`node_modules`、`.worktrees` 目录。
- 只接纳含 `[package]` 的 manifest；纯 `[workspace]` manifest 仅用于发现根。
- 每个 package 生成一个 `SpatialNodeKind::Crate` 节点与一个 `SpatialCluster`；节点 `cluster` 指向自身 cluster。
- 以 package 名为唯一键；重名返回 `SpatialError::DuplicatePackage`。
- 把 Normal 与 Build 依赖中能解析到工作区内 package 的项，生成 `EdgeKind::DependsOn` 边（`from` = 依赖方，`to` = 被依赖方）；Dev 依赖只解析、不生成边。
- path 依赖通过规范化目录匹配工作区 package；`workspace = true` 的依赖按名字匹配。无法解析到工作区内的依赖视为外部依赖，不生成节点或边，不报错。
- 节点与边按稳定顺序输出：节点按 package 名排序，边按 `(from, to)` 排序；`generation` 由节点与边的稳定哈希得到，同一输入必须得到同一值。
- 所有节点 `weight = 1.0`、`status = Idle`、`anchor = None`。

### 确定性全量布局

系统 SHALL 提供 `xgent_spatial::layout::layout(graph: &mut SpatialGraph, config: &LayoutConfig) -> LayoutReport`：

- 初始方向由节点/簇的稳定键（`path` 优先，否则 `label`）经 SplitMix64 哈希生成三维坐标并归一化；只使用整数运算与 `sqrt`，不使用 `sin`/`cos`、时间或系统随机。
- 先布局簇：按 `SpatialGraph.clusters` 的顺序做固定轮数的斥力 + 依赖边引力迭代，再整体缩放到固定半径。
- 再把节点放到所属簇心周围：同一簇内节点做有限轮局部松弛；不同簇的节点之间不做全局迭代。
- 迭代顺序固定为 `Vec` 顺序；禁止以 `HashMap`/`HashSet` 的迭代顺序驱动计算。
- 结束后回填 `cluster.anchor` 与 `node.anchor`；`LayoutReport` 记录节点数、边数、迭代轮数与耗时。

#### Scenario: 同一张图连续两次布局坐标逐位相同

- **Given** 一张固定的 `SpatialGraph`（来自同一工作区快照或同一合成图）
- **When** 从 `anchor = None` 的初始状态连续执行两次全量布局
- **Then** 两次结果中每个节点的 `anchor` 三个分量的 `f32::to_bits()` 完全相等，簇坐标同理

验收：A1

### 增量布局

系统 SHALL 提供 `xgent_spatial::layout::layout_incremental(graph: &mut SpatialGraph, config: &LayoutConfig) -> LayoutReport`：

- 已存在 `anchor = Some(_)` 的节点视为钉住：不累加力、不写坐标。
- 新增节点按确定性初始位置放置，只与未钉住节点做局部松弛。
- 删除节点不移动任何邻居。

#### Scenario: 追加节点不移动既有节点

- **Given** 500 个节点已完成全量布局并写回 `anchor`
- **When** 追加 100 个节点后执行增量布局
- **Then** 原有 500 个节点的坐标逐位不变，新节点全部获得 `Some(anchor)`

验收：A3

### 性能

- 全量布局必须支持 10,000 节点、约 20,000 条边；斥力计算使用均匀网格或等价的空间划分，避免 O(n²) 全对遍历。
- 大图允许降低迭代轮数，但降轮策略必须由节点数确定性地决定，不得引入随机性。

#### Scenario: 一万节点全量布局在 300ms 内完成

- **Given** 一张 10,000 节点、约 20,000 条边的确定性合成图
- **When** 在 release 构建下执行一次全量布局
- **Then** 从进入 `layout` 到返回的耗时小于 300ms，且所有节点都有 `Some(anchor)`

验收：A2

### 错误处理

- `SpatialError` 使用 `thiserror`，至少覆盖：`Io`、`Toml`、`MissingPackage`、`DuplicatePackage`、`InvalidWorkspace`。
- 库代码不使用 `unwrap()`/`expect()`；扫描遇到不可读文件时返回 `SpatialError::Io`，由调用方决定降级。

## Constraints

- 新 crate 依赖仅 `xgent_core`、`toml`、`thiserror`；开发依赖 `tempfile`。
- 不依赖 Bevy、tokio、任何 `xgent_*` 之外的 UI crate。
- 公开 API 使用中文文档注释；测试可以 `unwrap`。
