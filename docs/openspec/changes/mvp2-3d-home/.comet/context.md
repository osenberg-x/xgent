# Comet Design Handoff

- Change: mvp2-3d-home
- Phase: design
- Mode: compact
- Context hash: 2be8fb48e0243c4754aa29d0b22916eafb53b82ce32df6c9a39a72f2058b513e

Generated-by: comet-handoff.sh
Task hash policy: task-content-v1. Read tasks.md for live completion; excerpts are design-time context.

OpenSpec remains the canonical capability spec. This handoff is a deterministic, source-traceable context pack, not an agent-authored summary.

## docs/openspec/changes/mvp2-3d-home/proposal.md

- Source: docs/openspec/changes/mvp2-3d-home/proposal.md
- Lines: 1-36
- SHA256: c2fbad31c35863bbb3ac4431a394dbec48f137cb22ae861ec4a41d2b65e7b716

```md
# Proposal: MVP-2 3D 首页

## Why

XGent 的差异化来自 README 的 3D 代码宇宙，但当前产品仍是 2D agent IDE：`xgent_spatial` 只有 crate 级 L0 内核，UI 没有视口，布局仍以对话为主区。requirements v3 已把 3D 首页定为 MVP-2 当前主线（§4.2/§7.2），并钉死 L1 数据、轨道相机、G7 退化梯与布局反转；本次变更把这个边界落成可验收的实现。

工作区里 requirements v3、AGENTS、3D 视口文档等 14 份文档的改动是本变更的需求输入，已由本次会话完成；它们不是另一条实现变更。

## What Changes

- **G7 性能门**：离屏纹理 + 实例化节点，2k 节点 @60fps（默认 1280×800 窗口，优先集显）；不过降 800 节点/关特效 @60fps；仍不过回退 2D。记录测试机、GPU 型号/驱动与 1080p 压力值。
- **L1 空间图**：`xgent_spatial` 从 crate 级 L0 扩到文件/目录级 L1，任意项目可用；扫描忽略 `.gitignore`/`target`/`node_modules`；2k 节点上限与目录聚合；保留确定性布局与增量锚定。
- **3D 首页视口**：`xgent_ui` 新增 3D 视口；轨道相机；悬停高亮；单击选中、双击/Enter 打开文件；`Esc` 回宇宙；HUD 标签；所有首期操作有键盘路径。
- **布局反转**：3D 占 `Slot::Center`；对话移到 `SideDock` 常驻；编辑器/终端打开时接管 Center；2D fallback 偏好，关掉 3D 后 MVP-1 的全部功能可用。
- **性能降级**：2k → 800 → 2D 的节点梯；画质梯只在 800 档内使用；HUD 显示当前降级档位。
- **非目标**：完整 S2（键盘遍历/搜索飞行/2D↔3D 联动）、S3 时空回溯、S4 插件节点、L2 符号/import/调用边、自由飞行/惯性、MCP/宠物/成本/代码智能/TUI/Web。

## Capabilities

### New Capabilities

- `spatial-code-graph`: L1 文件/目录图、确定性布局、增量锚定、扫描忽略规则与节点上限。
- `3d-home-view`: 3D 视口的渲染、轨道相机、悬停/选中/打开/`Esc`、键盘路径、G7 性能与降级策略。
- `workspace-layout`: `Slot::Center`/`SideDock` 的面板排布、对话右栏、编辑器/终端接管主区、2D fallback。

### Modified Capabilities

无。项目当前没有已归档的 specs，三个能力均为新增。

## Impact

- **crates**：`xgent_spatial`（L1 扫描/聚合）、`xgent_ui`（3D 视口 + 面板注册 + 布局）、`xgent_app`（装配与偏好）；`xgent_core` 如需扩展 `SpatialGraph` 类型会同步。
- **依赖**：Bevy 0.20 的 `3d` 默认特性已在依赖图里，不新增渲染器依赖。
- **不改**：daemon、JSON-RPC、provider/tools/context/agent 契约。
- **文档**：requirements v3、3D 视口文档、layout 架构是本变更的输入；`docs/dev-tutorial.md` 在实现后同步。
- **风险**：G7 不过则本变更停在 gate，不进入 S1/布局；L1 大仓扫描与目录聚合是主要性能风险；编辑器/终端从 SideView 迁到 Center 需要面板注册表迁移。

```

## docs/openspec/changes/mvp2-3d-home/design.md

- Source: docs/openspec/changes/mvp2-3d-home/design.md
- Lines: 1-196
- SHA256: 1f2684efed9a0c7ff12105f964d09a1ed64df0cbfa9f0945a41302d754819c94

[TRUNCATED]

```md
---
comet_change: mvp2-3d-home
role: technical-design
canonical_spec: openspec
---

# Design

## Context

当前代码状态：

- `xgent_spatial` 只有 S0：从 `Cargo.toml` 生成 crate 级 L0 图与确定性布局，不读文件树；`SpatialNodeKind` 已有 `Module`/`File`，但没有生产者。
- `xgent_ui` 的 `Slot::Center` 当前挂对话；编辑器/预览/终端是 `Slot::SideDock` 的页签；没有 3D 视口模块。
- Bevy 0.20 的 `3d` 是默认特性，`bevy_pbr`/`bevy_core_pipeline` 已在依赖图中；不需要新增渲染依赖，但也没有任何 `Camera3d`/`RenderTarget` 装配。
- 需求边界见 `docs/design/requirements.md` §4.2/§7.2 与 `docs/design/ui-v8-3d-viewport.md` §7/§8；本设计只解释实现路径，不重复需求。

本变更是一个 full change，按 G7 → L1 → S1 → 布局反转的顺序推进；G7 是 go/no-go gate，未通过不进入后续 UI 工作。

## Goals / Non-Goals

**Goals:**

- 用独立的 G7 spike 先证明 2k 节点 / 60fps 可行，并给出 800 / 2D 的降级判据。
- 让 `xgent_spatial` 对任意项目产出 L1 文件/目录图，保持确定性、增量锚定与只读投影。
- 在 `xgent_ui` 内用离屏纹理承载 3D 视口，完成轨道相机、悬停、选中、双击/Enter 打开与 `Esc` 返回。
- 把 3D 变成默认首页：对话常驻右栏，编辑器/终端打开时接管主区，2D fallback 可用。

**Non-Goals:**

- 完整 S2（键盘遍历、搜索飞行、2D↔3D 联动）、S3 时空回溯、S4 插件节点。
- L2 符号/import/调用边、LSP、多语言解析。
- 自由飞行/惯性（OQ-13）、第一人称相机。
- 在 3D 内渲染源码文本或做 3D 编辑器。

## Decisions

### D1. G7 先做独立 spike，不先接产品 UI

用一个最小的 Bevy spike（离屏纹理 + 2k 个实例化 billboard/星体 + 轨道相机）测帧时间，测试机为默认 1280×800 窗口、优先集显，另记 1080p 压力值。判定顺序：2k@60fps → 800 节点/关特效@60fps → 停用 3D 首页回退 2D。

- **Why**：如果先写 S1/布局再发现跑不动，返工面覆盖 `xgent_ui` 与面板注册表；spike 的产物只是测量与结论，失败可以干净停止。
- **Alternative**：直接写 S1，边做边测。被否：失败时已经投入布局迁移。

### D2. 视口用离屏纹理，不用第二台摄像机直出

3D 摄像机渲染到 `RenderTarget::Image`，UI 用 `ImageNode` 承载；布局、裁剪、模态栈仍由 2D 系统权威。

- **Why**：直出摄像机只能铺满窗口，无法裁剪进面板；UI 容器会吞指针；离屏让视口成为普通面板。
- **Alternative**：`Camera3d` 全屏 + 2D UI 叠加。被否：清屏/排序/拾取与既有 2D 装配纠缠。

### D3. 拾取走 CPU 射线/屏幕距离，不用 mesh picking

视口自己把指针坐标映射到视口 UV，再用相机投影做节点中心/半径命中；不为节点建碰撞体，也不依赖 `mesh_picking`。

- **Why**：UI 节点先吃指针；2k 个节点的碰撞体维护成本高；CPU 判定对 billboard 足够。
- **Alternative**：Bevy `mesh_picking`。被否：与 UI picking 冲突，且与 LOD/实例化不同步。

### D4. L1 扫描放在 `xgent_spatial`，保持无 Bevy

扩展 `scan`：遍历文件树、应用忽略规则、生成 `Module`/`File` 节点与 `Contains` 边；有 `Cargo.toml` 时叠加 crate 节点/`DependsOn` 边。扫描与目录聚合在 `spawn_blocking` 中执行，结果回 UI 线程。

- **Why**：数据层可 headless 测试，未来 2D 依赖图可复用；不把 Bevy 拖进布局内核。
- **Alternative**：放在 `xgent_context`。被否：把可视化数据塞进检索层，边界变浑。

### D5. 忽略规则 = `.gitignore` + 内置跳过表

扫描时解析项目根的 `.gitignore`（含常见嵌套规则的最小实现），并始终跳过 `target/`、`node_modules/`、`.git/`、构建产物；解析失败只记一次警告，不阻塞打开。

- **Why**：任意项目都要有宇宙；只靠内置表会让 JS/Python 项目把依赖目录画进宇宙，节点数瞬间爆掉。
- **Alternative**：只做内置跳过。被否：覆盖不了用户自定义的生成目录。

### D6. 节点上限与目录聚合在扫描/布局之间完成

先按目录聚合（目录簇 + 文件星），再按 weight 裁剪到 2k；G7 降级时上限改为 800。被省略的节点数写入图统计，HUD/报告可见。

- **Why**：2k 是性能预算，不是文件数；聚合必须发生在布局前，否则布局本身就被大仓拖垮。
- **Alternative**：全量布局后 LOD 裁剪。被否：布局成本已经付出。

### D7. 视口是 `Slot::Center` 的注册面板，布局反转最后做

```

Full source: docs/openspec/changes/mvp2-3d-home/design.md

## docs/openspec/changes/mvp2-3d-home/tasks.md

- Source: docs/openspec/changes/mvp2-3d-home/tasks.md
- Lines: 1-50
- SHA256: f661b1412bcf50a52bc75723e4345a2a7beb424063492a5162b0d41d7ed61e66

```md
# Tasks

> G7 是 gate：若 2k 与 800 档都达不到 60fps，停在 group 1，记录 2D fallback 结论，不进入 group 3/4。

## 1. G7 性能 spike（gate）

- [ ] 1.1 建立独立 Bevy spike：3D 离屏纹理 + 2k 实例化 billboard/星体 + 轨道相机；验证：spike 可启动、渲染 2k 节点并输出一帧截图/日志
- [ ] 1.2 在默认 1280×800 窗口、优先集显下测 2k 与 800 档帧时间，另记 1080p 压力值；验证：记录含 GPU 型号/驱动、实测 fps 与帧时间
- [ ] 1.3 按 2k→800→2D 写 go/no-go 结论；验证：`docs/notes/g7-3d-perf.md` 存在且明确下一步；若 2k 不过记录 800 数据，若 800 不过记录 2D fallback 决定
- [ ] 1.4 把 G7 结论回写 3D 视口文档的测试基准；验证：`docs/design/ui-v8-3d-viewport.md` 的基准段与实测一致

## 2. L1 空间图

- [ ] 2.1 扩展 `xgent_spatial::scan` 生成 `Module`/`File` 节点与 `Contains` 边；验证：Rust 与非 Rust fixture 的节点类型/数量单测通过
- [ ] 2.2 实现忽略规则（`.gitignore` + 内置 `target/`、`node_modules/`、`.git/`、构建产物）；验证：fixture 中被忽略路径不出现在图中；规则解析失败只记一次警告
- [ ] 2.3 目录聚合 + 2k/800 上限 + 省略计数；验证：10k 文件 fixture 节点数不超过上限且 `omitted > 0`；小仓库不裁剪
- [ ] 2.4 保持确定性布局与增量锚定；验证：重复布局坐标逐位相同；新增节点时既有节点坐标不变（扩展现有 S0 测试）
- [ ] 2.5 扫描放到 `spawn_blocking` 并支持取消；验证：大 fixture 扫描期间主线程不被阻塞（计时/headless 测试），取消后不再回写图
- [ ] 2.6 在 `docs/dev-tutorial.md` 补 L1 扫描/忽略/聚合说明；验证：文档段落与实现一致

## 3. 3D 视口 S1 + 点击

- [ ] 3.1 用离屏纹理 + `ImageNode` 在 Center 面板承载视口；验证：视口可见，窗口 resize 后纹理尺寸正确（headless/截图）
- [ ] 3.2 实例化星体/边 + LOD0/L1 + HUD 标签（`Ellipsis`）；验证：2k fixture 渲染成功、长路径标签不溢出、主题切换后颜色更新
- [ ] 3.3 轨道相机（拖拽/滚轮/方向键/`+`/`-`，无惯性，数据刷新不漂移）；验证：输入测试与相机状态断言通过
- [ ] 3.4 CPU 拾取：悬停高亮、单击选中、双击/Enter 打开；验证：headless 投影命中测试 + 打开文件消息断言
- [ ] 3.5 `Esc` 从编辑器/终端返回宇宙，相机保持离开前位置；验证：快捷键测试与相机状态断言
- [ ] 3.6 运行期降级：800 档/静态首帧/HUD 档位；验证：强制低配档位时 HUD 显示且渲染循环按预期降级
- [ ] 3.7 在 `docs/dev-tutorial.md` 补 3D 视口使用与限制；验证：文档段落存在且与实际行为一致

## 4. 布局反转

- [ ] 4.1 注册 `space` 为 Center 面板并接通激活/失活；验证：`panel_registry_contract` 扩展通过，Center 单活跃
- [ ] 4.2 对话迁到 SideDock；编辑器/终端注册为中心面板；验证：打开文件/终端激活对应 Center，对话仍可见（headless 布局断言）
- [ ] 4.3 Center 默认改为 `space`，`Esc` 回 space；新增 `three_d_home` 偏好；验证：启动默认空间；关闭偏好后回 MVP-1 布局（集成测试）
- [ ] 4.4 对话侧栏默认 400px、可拖拽、320–560（或可用宽度 45%）限制、重启保留；验证：拖拽/持久化测试通过
- [ ] 4.5 2D fallback 全功能回归：对话、编辑器、终端、文件抽屉、设置、会话历史；验证：相关既有测试全绿 + 手工走查记录
- [ ] 4.6 同步 `docs/dev-tutorial.md` 与 `docs/design/ui-v8-layout-architecture.md` 的 MVP-2 布局说明；验证：文档段落与实现一致

## 5. 集成验收

- [ ] 5.1 端到端走查：打开项目 → 3D 首页 → 悬停/单击 → 双击打开 → `Esc` 返回；验证：集成测试或带时间戳的手工走查记录
- [ ] 5.2 用真实仓库（含非 Rust 项目）复测 L1 + 2k 上限 + 60fps；验证：G7 报告更新，记录实测
- [ ] 5.3 全量回归：`cargo test -p xgent_spatial -p xgent_ui -p xgent_app`；验证：命令输出全绿
- [ ] 5.4 文档一致性检查：requirements/3D 文档/dev-tutorial/README 无旧「2D only / 可选插件层 / 下一期 M8」表述；验证：grep 结果为空

## Workflow follow-up

- Archive the change after verification passes.
- Record the delivery method (local/push/pr) and verify it before closing.

```

## docs/openspec/changes/mvp2-3d-home/.openspec.yaml

- Source: docs/openspec/changes/mvp2-3d-home/.openspec.yaml
- Lines: 1-2
- SHA256: 6803193cd4b83f73d2d65d3e3ce1f3b116fb4dba2543ef44a3ad6cc9791547ed

```md
schema: spec-driven
created: 2026-10-30

```

## docs/openspec/changes/mvp2-3d-home/specs/3d-home-view/spec.md

- Source: docs/openspec/changes/mvp2-3d-home/specs/3d-home-view/spec.md
- Lines: 1-69
- SHA256: 9bbd937c8180b23183f9c65eaf287b4abbcfd771d5dd39c74336e4fdd3575203

```md
# Spec Delta

## Purpose

为用户提供默认首页的 3D 代码宇宙视口，支持轨道浏览、选中与打开文件，并在性能不足时明确降级或回退。

## ADDED Requirements

### Requirement: 3D 首页默认可见
系统 SHALL 在项目打开后把 3D 宇宙作为主区默认视图显示；没有打开文件/终端时，用户看到的第一个工作视图是 3D 宇宙。

#### Scenario: 启动进入宇宙
- **WHEN** 用户打开一个项目
- **THEN** 主区显示 3D 宇宙，而不是对话或空白页

### Requirement: 轨道相机
系统 SHALL 提供轨道相机：拖拽改变 yaw/pitch，滚轮改变距离，键盘方向键与 `+`/`-` 可完成同样的相机操作；相机不得因数据刷新或时间推移自动漂移。

#### Scenario: 拖拽与缩放
- **WHEN** 用户在视口内拖拽或滚动滚轮
- **THEN** 相机围绕当前注视点旋转或缩放，松手后立即停止

#### Scenario: 数据刷新不移动相机
- **WHEN** 文件监听或 Agent 状态更新导致空间图刷新
- **THEN** 相机位置与朝向保持不变

### Requirement: 悬停、选中与打开
系统 SHALL 在悬停时高亮节点；单击只选中节点；双击或按 Enter 打开选中文件到 2D 编辑器；按 Esc 从编辑器/终端返回宇宙。

#### Scenario: 单击选中
- **WHEN** 用户单击一个文件星体
- **THEN** 该节点进入选中态，不打开编辑器

#### Scenario: 双击打开
- **WHEN** 用户双击一个文件星体，或选中后按 Enter
- **THEN** 主区切换到 2D 编辑器并打开该文件

#### Scenario: Esc 返回
- **WHEN** 用户在编辑器或终端中按 Esc
- **THEN** 主区回到 3D 宇宙，相机保持离开前的位置

### Requirement: 首期操作键盘可达
系统 SHALL 为首期的选中、打开、返回与相机操作提供键盘路径，不要求鼠标才能完成这些操作。

#### Scenario: 键盘完成核心动作
- **WHEN** 用户不使用鼠标
- **THEN** 可以用键盘完成选中、打开文件、返回宇宙与相机调整

### Requirement: G7 性能门与降级
系统 SHALL 在 2000 节点场景达到 60fps；达不到时降为 800 节点并关闭特效，仍需达到 60fps；仍达不到时停用 3D 首页并回退 2D。降级档位必须在 HUD 上可见。

#### Scenario: 2k 达标
- **WHEN** 性能测试在 2000 节点下稳定达到 60fps
- **THEN** 3D 首页以完整档位运行

#### Scenario: 2k 不达标
- **WHEN** 2000 节点无法稳定 60fps
- **THEN** 节点上限降到 800 并关闭特效，HUD 显示降级档位

#### Scenario: 800 仍不达标
- **WHEN** 800 节点仍无法稳定 60fps
- **THEN** 3D 首页停用，主区回退到 2D 布局，并提示原因

### Requirement: 3D 不渲染源码
系统 SHALL 不在 3D 视口中渲染源码文本；阅读与编辑始终在 2D 编辑器完成。

#### Scenario: 打开文件交棒 2D
- **WHEN** 用户从宇宙打开一个文件
- **THEN** 源码只出现在 2D 编辑器中，3D 视口不显示源码文本

```

## docs/openspec/changes/mvp2-3d-home/specs/spatial-code-graph/spec.md

- Source: docs/openspec/changes/mvp2-3d-home/specs/spatial-code-graph/spec.md
- Lines: 1-58
- SHA256: 9063726e59863e9dc6a6732cbb7280d1227d28b2edcadfef0cac28ea6cb49275

```md
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

```

## docs/openspec/changes/mvp2-3d-home/specs/workspace-layout/spec.md

- Source: docs/openspec/changes/mvp2-3d-home/specs/workspace-layout/spec.md
- Lines: 1-55
- SHA256: ae534018ad74d501759bc31941c31a2d8162056272b8a936fd6b91970177b183

```md
# Spec Delta

## Purpose

定义 3D 首页下的主区与侧栏排布：3D 占据主区、对话常驻侧栏、编辑器/终端按需接管主区，并可随时回退 2D。

## ADDED Requirements

### Requirement: 3D 主区与对话侧栏
系统 SHALL 让 3D 宇宙占据主区，让对话常驻右侧栏；打开文件或终端时，主区由对应视图接管，对话仍可见。

#### Scenario: 默认排布
- **WHEN** 项目打开且没有打开文件/终端
- **THEN** 3D 宇宙位于主区，对话位于右侧栏

#### Scenario: 打开文件
- **WHEN** 用户从宇宙或文件抽屉打开一个文件
- **THEN** 编辑器接管主区，对话继续在右侧栏显示

#### Scenario: 打开终端
- **WHEN** 用户打开终端
- **THEN** 终端接管主区，对话继续在右侧栏显示

### Requirement: 主区单活跃
系统 SHALL 保证主区同一时刻最多只有一个活跃视图（3D/编辑器/终端）；激活新视图时旧视图隐藏，`Esc` 返回 3D 宇宙。

#### Scenario: 切换不叠加
- **WHEN** 用户从 3D 打开文件，再打开终端
- **THEN** 主区只显示终端，3D 与编辑器隐藏，不出现叠层

#### Scenario: 返回宇宙
- **WHEN** 用户在主区按 Esc
- **THEN** 当前视图让位，3D 宇宙重新成为主区

### Requirement: 2D fallback
系统 SHALL 提供关闭 3D 首页的偏好；关闭后布局回到 MVP-1（对话主区 + 编辑器/终端侧区），MVP-1 的全部功能仍可用。

#### Scenario: 关闭 3D
- **WHEN** 用户在偏好中关闭 3D 首页
- **THEN** 主区回到对话，编辑器/终端回到侧区，且无需重启

#### Scenario: 2D 功能完整
- **WHEN** 3D 首页被关闭
- **THEN** 对话、编辑器、终端、文件抽屉、设置与会话历史均可正常使用

### Requirement: 侧栏可调宽
系统 SHALL 允许用户调整对话侧栏宽度，并限制在可用的最小/最大范围内；调整后的宽度在重启后保留。

#### Scenario: 拖拽调宽
- **WHEN** 用户拖拽侧栏分隔条
- **THEN** 对话侧栏宽度改变，但不小于最小值、不大于可用宽度

#### Scenario: 重启保留
- **WHEN** 用户调整宽度后重启应用
- **THEN** 对话侧栏保持上次宽度

```
