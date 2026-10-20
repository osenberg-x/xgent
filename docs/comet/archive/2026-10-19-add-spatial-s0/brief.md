# Outcome

新建 `xgent_spatial` crate，交付 3D 视口（F-16）的 S0 内核：空间图类型、Cargo.toml 依赖提取、确定性布局。它是 3D 线的第一步，与 UI v8 M1 **并行、互不阻塞**；本轮不产生任何 UI、渲染或 Bevy 代码。

产物必须满足三条 S0 验收（确定性、10k 性能、增量零位移），并为 S1（只读视口）提供可直接消费的图与坐标。

# Scope

- 新 crate `crates/xgent_spatial`（lib，edition 2024）；workspace 的 `members = ["crates/*"]` 自动纳入。
- `xgent_core` 新增 `spatial` 模块：`SpatialGraph` / `SpatialNode` / `SpatialEdge` / `SpatialCluster` / `SpatialNodeId` / `ClusterId` / `SpatialNodeKind` / `SpatialStatus` / `EdgeKind`；serde 可序列化，无 Bevy。
- `xgent_spatial::manifest`：解析单个 `Cargo.toml` 的 package 名、normal/dev/build 依赖、path 与 `workspace = true` 标记。
- `xgent_spatial::workspace`：从工作区根扫描成员 manifest，把 path 依赖解析为内部 crate，生成 L0 `SpatialGraph`（crate 节点 + `DependsOn` 边 + cluster）。
- `xgent_spatial::layout`：确定性全量布局 + 增量布局（既有节点钉住）；不使用 HashMap 迭代顺序、不依赖时间或随机种子。
- 测试：解析、扫描、确定性、增量、10k 性能（`#[ignore]` + release）。
- 文档同步：`docs/dev-tutorial.md` 的 crate 拓扑（AGENTS.md §6.8）。

## Sources and their role

| Source item and location | Read status | Content to preserve | Spec location | Acceptance ID | Coverage status | Reason |
| --- | --- | --- | --- | --- | --- | --- |
| S1: 用户请求（2026-10-19）「先立 xgent_spatial 的 S0（类型 + Cargo.toml 依赖提取 + 确定性布局 + 三条验收）」 | complete | S0 范围与三条验收；S1 不得抢跑 | specs/spatial-s0/spec.md | A1–A3 | covered | 需求来源 |
| S2: `docs/design/ui-v8-3d-viewport.md` §3（数据契约）、§7 S0 行、§8 闸门 | complete | `SpatialGraph` 字段语义、S0 验收、与 v8 的边界 | specs/spatial-s0/spec.md | A1–A3 | covered | 设计依据 |
| S3: `AGENTS.md` §4/§5.7/§6.8（crate 划分、Rust 风格、文档同步） | complete | 不依赖 Bevy、中文文档、thiserror、库代码不 unwrap | specs/spatial-s0/spec.md | — | background | 项目约束 |
| S4: `docs/design/architecture.md` §10.3（3D 只读投影） | complete | 3D 不成为真相源；本 crate 只做投影数据与布局 | specs/spatial-s0/spec.md | — | background | 架构边界 |

# Non-goals

- 不做 S1 及以后：离屏纹理、渲染、拾取、相机、LOD、HUD、时间轴、2D↔3D 联动。
- 不改 `xgent_ui` / `xgent_app` 的依赖与代码；不把 Bevy 类型引入新 crate。
- 不做 tree-sitter / `mod`/`use` 解析（L1 耦合边），不做符号级（L2）图。
- 不计算 LOC 或文件级节点：S0 的节点是 crate，`weight` 暂为 1.0；文件节点留给 S1 的文件索引。
- 不做坐标持久化/缓存格式（另议）。
- 不改变任何现有 crate 的运行时行为。

# Acceptance examples

- 对同一张图连续两次执行全量布局，所有节点坐标的 `f32::to_bits()` 逐位相同（同一构建、同一机器）。
- 10,000 节点、约 20,000 条边的合成图，release 构建下全量布局耗时小于 300ms。
- 500 节点完成布局后追加 100 节点做增量布局，原有 500 个节点的坐标逐位不变。

# Constraints and invariants

- 新 crate 只依赖 `xgent_core`、`toml`、`thiserror`；不依赖 Bevy / tokio。
- 确定性：禁止 HashMap 迭代顺序参与计算；伪随机用固定种子的整数哈希（SplitMix64），不用时间、不用系统随机。
- 增量布局：已锚定节点不累加力、不写坐标，保证位移严格为 0。
- 库代码不 `unwrap()`/`expect()`（测试除外）；错误用 `thiserror`；公开项写中文文档注释。
- 验收口径：A2 必须在 release 下测；A1/A3 在 debug 下也必须通过。

# Decisions

- D1 类型放 `xgent_core::spatial`（与 `ui-v8-3d-viewport.md` §3.1 一致），布局算法放新 crate。
- D2 新 crate 名 `xgent_spatial`（对应 D-3D-02 推荐方案 A）。
- D3 布局为确定性力导向 + 锚定：初始位置由节点路径哈希决定；先做簇级布局，再把节点放到簇心周围，最后有限轮局部松弛。
- D4 性能验收以 release 为准；debug 只保证正确性。
- D5 S0 **包含**工作区扫描器：`xgent_spatial::workspace::scan(root)` 读文件系统、解析 path 依赖并组装 L0 图；仍不依赖 Bevy。

# Verification expectations

- `cargo test -p xgent_spatial`（debug：解析、扫描、确定性、增量）
- `cargo test -p xgent_spatial --release -- --ignored`（10k 性能）
- `cargo check --workspace`
- `cargo clippy -p xgent_spatial --all-targets -- -D warnings`
