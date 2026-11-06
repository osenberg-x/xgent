# Comet Design Handoff

- Change: f20-s3-timeline
- Phase: design
- Mode: compact
- Context hash: 24b725568827a15130bae715c2568865d143c2a34a7105c7c828e85ab1537ea1

Generated-by: comet-handoff.sh
Task hash policy: task-content-v1. Read tasks.md for live completion; excerpts are design-time context.

OpenSpec remains the canonical capability spec. This handoff is a deterministic, source-traceable context pack, not an agent-authored summary.

## docs/openspec/changes/f20-s3-timeline/proposal.md

- Source: docs/openspec/changes/f20-s3-timeline/proposal.md
- Lines: 1-31
- SHA256: 41c4c1c0ba465b3ee5ccddbc794eb1fe050f01b2d883b8a59e6d2cc54a1f9e3b

```md
# Proposal

## Why

F-20 时空回溯依赖 3D S3，但当前宇宙只有“现在”状态：没有时间轴，也没有历史星体回放。先交付 S3 的只读时间轴与状态回放，才能让 F-20 的时间锚点、跳转与二分定位有稳定的 3D 数据契约。

## What Changes

- 新增 3D 时间轴：主刻度为当前会话的 agent 回合，次刻度为 git 提交；按时间合并，git 不可用时退化为仅回合。
- 在 3D 视口底部增加 scrubber（时间标记、当前/历史标签、回到现在），拖动/点击切换历史锚点。
- 状态回放：按锚点文件集控制**当前 L1 图节点**的可见性；commit 锚点使用 `git ls-tree`，turn 锚点使用最近 commit 的文件集加检查点中新建文件；目录随子文件显隐，边随端点显隐。
- 历史视图显示边框并标注锚点；回到“现在”完全恢复；相机 yaw/pitch/dist/target 不因拖动改变。
- 时间轴数据在后台线程加载，只读，不写项目文件或 git。
- 本期不重建已删除文件的历史星体；完整 F-20 的时间锚点/二分/diff UI/分支探索、历史图重建、3D 编辑和多会话时间轴留后续。

## Capabilities

### New Capabilities

- `spatial-timeline`: 3D 时间轴锚点、scrubber 交互、历史状态回放与只读/降级行为。

### Modified Capabilities

（无。现有 3D 视口、L1 图和相机契约不变，只增加只读时间轴叠加。）

## Impact

- `crates/xgent_ui`：`space/s3.rs` 时间轴数据/状态回放/UI；读取 `Conversation`、检查点清单与会话节点路径；不写文件。
- `crates/xgent_tools`：只读消费 `TurnManifest`/`CheckpointEntry`（必要时导出只读查询）。
- git：后台执行只读 `git log`/`git ls-tree`，无 git 仓库时降级。
- 文档与测试：dev-tutorial、architecture、AGENTS；时间轴合并/回放/相机不变/只读/无 git 降级回归。

```

## docs/openspec/changes/f20-s3-timeline/design.md

- Source: docs/openspec/changes/f20-s3-timeline/design.md
- Lines: 1-77
- SHA256: c1f736a06ad96f68d5a6778d39b9f3c0883387ef78a7cc6297d8098f68ca7807

```md
---
comet_change: f20-s3-timeline
role: technical-design
canonical_spec: openspec
---

# Design

## Context

- S2 已交付轨道相机、选中/搜索/2D↔3D；S3 需要时间轴与状态回放。
- 现有数据：当前会话 `Conversation`（用户/助手时间戳）、`<project>/.xgent/checkpoints/turns/<session>/<turn>.json`（每轮改前条目）、git 仓库。
- D-3D-07 已定：agent 回合为主刻度、git 提交为次刻度。
- 本期只回放**当前 L1 图已有节点**；不重建已删除文件的历史星体。

## Goals / Non-Goals

**Goals：**
- 合并回合/提交锚点，后台加载，scrubber 可点击/拖动/键盘切换。
- commit 锚点按 `git ls-tree` 文件集回放；turn 锚点按最近 commit + 该回合前检查点新建文件回放。
- 相机不变、回到现在恢复、历史边框与标签、只读失败降级。

**Non-Goals：**
- 完整 F-20（时间锚点编辑、二分定位、diff UI、分支探索、跨会话时间轴）。
- 已删除文件的历史星体重建、3D 编辑、git 写操作。

## Decisions

### D1：模块与数据

- `xgent_ui::space::s3` 新增 `TimelineData`、`TimelineAnchor`、`TimelineLoader`、`TimelineState`。
- `TimelineAnchorKind::Turn { session, turn } | Commit { hash }`，`at_ms` 用用户消息时间戳 / git commit 秒×1000；合并排序，同一时刻提交在前。
- `TimelineData` 缓存每个 commit 的文件集（最多 50 个锚点）、每个 turn 的 `Created` 路径集、当前图文件集、是否有 git、警告。
- 后台线程通过 `std::thread` + `crossbeam_channel` 加载；`Update` 轮询，不阻塞 UI。

### D2：Git 读取

- `git -C <root> rev-parse --show-prefix` 得到项目在仓库内的前缀；`git log -n 50 --format=%H%x09%ct%x09%s`；对每个 commit 执行 `git ls-tree -r --name-only --full-tree <hash>`，剥掉前缀并转 `/`。
- 无 git、非仓库、命令失败或输出异常：降级为仅回合并记录 warning；不写 git。
- git 命令在后台线程执行，不进入 UI 主线程。

### D3：回合与检查点

- 当前会话 `Conversation` 的第 N 个用户消息对应检查点 turn N（`TurnKey.session = conv.id.0`）。
- 读取 `.xgent/checkpoints/turns/<session>/<turn>.json` 的 `TurnManifest`；只取 `EntryKind::Created` 路径作为该 turn 新建文件。损坏/缺失跳过并告警。
- 时间轴最多纳入最近 20 个有用户消息的回合（检查点保留上限）；git 提交最多 50 个。

### D4：回放可见性

- `now`：全部可见，恢复进入历史前的选中。
- commit 锚点：可见文件集 = 该 commit 的树文件集。
- turn 锚点：可见文件集 = 最近时间不晚于该 turn 的 commit 树（无则当前图文件集）∪ 该 turn 及之前所有 Created 路径；无 git 时从当前图文件集移除该 turn 之后 Created 的路径。
- 仅当前图已有路径参与；目录节点可见当且仅当存在可见子文件；`SpaceEdge` 记录 from/to，任一端隐藏则隐藏。
- 回放通过 `Visibility` 控制星体/边；不重建、不移动节点。进入历史时保存 `SpacePickState.selected` 并清空选中；回到 now 恢复选中。
- 相机 `OrbitCamera`/`Transform` 不写。

### D5：UI

- HUD 底部新增时间轴：rail、锚点标记、当前/历史标签、`回到现在` 按钮；历史时显示视口边框。
- 点击标记切换锚点；键盘 Left/Right 切换；rail 支持 `PointerDrag`（按拖动起点映射锚点）；`Esc` 先退出历史再交给 S2/对话（通过消耗资源）。
- 文案走 fluent；时间轴在 HUD 内，不覆盖状态栏/对话。

### D6：测试

- 纯数据：回合/提交合并排序、无 git、空历史、created 集合、commit 树回放、turn 前缀/后缀。
- 回放：节点/目录/边可见性、相机参数不变、now 恢复选中、只读字节不变。
- 检查点/ git fixture：临时 `.xgent/checkpoints` 与临时 git 仓库（git 不可用则跳过 git 用例）。
- UI：时间轴标记点击/键盘、标签/边框/回到现在、空/加载/错误。
- 回归：`cargo test -p xgent_ui -p xgent_tools -p xgent_spatial` + `cargo check --workspace`。

## Risks / Trade-offs

- [当前图缺失历史文件] → 明示限制，不重建；后续完整 F-20 再做历史图。
- [turn 文件集近似] → 只用 git 树 + Created；用户手改/删除不完整，面板标注“近似”。
- [git 性能] → 最多 50 个 commit，后台线程；失败降级。
- [检查点仅 20 轮] → 时间轴回合上限 20；更早历史需后续持久化索引。
- [UI 拖动] → 用 PointerDrag + rail 几何；headless 测试点击/键盘，拖动做单元映射测试。

```

## docs/openspec/changes/f20-s3-timeline/tasks.md

- Source: docs/openspec/changes/f20-s3-timeline/tasks.md
- Lines: 1-27
- SHA256: 92b4fa00134a7cf53d803a54c979037bb2eed8e191e13c1addcce65bfede1e7b

```md
# Tasks

## 1. 时间轴数据层

- [ ] 1.1 在 `xgent_ui::space::s3` 定义锚点/时间轴数据模型与合并逻辑（回合 + git commit，稳定排序，上限）；验证纯数据测试覆盖合并、空历史、无 git。
- [ ] 1.2 实现检查点清单读取（当前会话 turn→Created 路径，损坏/缺失跳过）与 git log/ls-tree 只读加载（前缀剥离、失败降级）；验证临时检查点与临时 git 仓库测试。

## 2. 回放逻辑

- [ ] 2.1 实现锚点文件集计算（commit 树；turn 最近提交 + Created；无 git 回退）；验证纯函数测试覆盖提交/回合/无 git/created 前后缀。
- [ ] 2.2 实现当前图可见性应用（文件/目录/边、now 恢复、选中保存、相机不变）；验证节点/目录/边/相机/选中测试。

## 3. UI 与交互

- [ ] 3.1 在 3D HUD 增加时间轴 rail/标记/标签/回到现在/历史边框；验证 UI 结构与 i18n 测试。
- [ ] 3.2 实现标记点击、键盘 Left/Right、PointerDrag 映射与 Esc 退出历史；验证交互测试（headless 点击/键盘/映射）。
- [ ] 3.3 后台加载/轮询/空态/加载/错误/刷新；验证异步加载与降级测试。

## 4. 文档与回归

- [ ] 4.1 同步 `docs/design/architecture.md`、`docs/dev-tutorial.md`、`AGENTS.md` 的 S3 范围、数据口径与限制；验证文档路径/测试名存在。
- [ ] 4.2 运行相关 crate 回归与 workspace check；验证 `cargo test -p xgent_ui -p xgent_tools -p xgent_spatial` 与 `cargo check --workspace` 通过。

## Workflow follow-up

- 归档前完成最终验证。
- 归档后按用户选择的交付方式推送/创建 PR；未获授权前不推送。

```

## docs/openspec/changes/f20-s3-timeline/.openspec.yaml

- Source: docs/openspec/changes/f20-s3-timeline/.openspec.yaml
- Lines: 1-2
- SHA256: 3e16994612e657a7bbaefe38b70ef46f3e8e9f382f092dcaceb60ebabe7a5da0

```md
schema: spec-driven
created: 2026-11-05

```

## docs/openspec/changes/f20-s3-timeline/specs/spatial-timeline/spec.md

- Source: docs/openspec/changes/f20-s3-timeline/specs/spatial-timeline/spec.md
- Lines: 1-95
- SHA256: 71e9eefcd20e7463325eb8d3be8040baa948baafe0eb4f606a75daee6c6bd5f6

[TRUNCATED]

```md
# Spec Delta

## Purpose

为 3D 宇宙提供只读时间轴：把当前会话的 agent 回合与 git 提交合并成时间锚点，按锚点回放当前 L1 图节点的历史可见状态，并保证相机与“现在”状态不被破坏。

## ADDED Requirements

### Requirement: 时间轴数据源

系统 SHALL 合并当前会话 agent 回合与 git 提交为按时间排序的锚点；agent 回合为主刻度、git 提交为次刻度；git 不可用或仓库为空时 MUST 退化为仅回合，不得阻塞视口。

#### Scenario: 回合与提交合并
- **WHEN** 当前会话有 agent 回合且仓库有 git 提交
- **THEN** 时间轴按时间合并两类锚点，回合与提交可区分显示

#### Scenario: 无 git 仓库
- **WHEN** 项目不是 git 仓库或 git 命令失败
- **THEN** 只显示 agent 回合，时间轴与回放仍可用，不报错

#### Scenario: 空历史
- **WHEN** 当前会话没有回合且没有 git 提交
- **THEN** 显示空态，不改变当前宇宙状态

### Requirement: 时间轴 scrubber

系统 SHALL 在 3D 视口提供可拖动的 scrubber；拖动或点击锚点切换历史位置；当前端点为“现在”；历史位置显示明确标签与历史视图边框；提供回到现在的入口。

#### Scenario: 拖动切换
- **WHEN** 用户拖动 scrubber 到某个锚点
- **THEN** 历史位置更新、视图边框与标签显示该锚点，未改变相机

#### Scenario: 回到现在
- **WHEN** 用户点击“回到现在”或拖到最右端
- **THEN** 历史边框消失，星体状态完全恢复到当前图

#### Scenario: 空态交互
- **WHEN** 时间轴没有锚点
- **THEN** scrubber 显示空态，拖动不改变状态

### Requirement: 历史状态回放

系统 SHALL 按锚点文件集控制当前 L1 图节点可见性：git 提交锚点使用该提交的 `git ls-tree` 文件集；agent 回合锚点使用最近提交文件集加该回合前检查点新建文件；目录节点随可见子文件显隐，边随端点显隐；当前图中不存在的历史文件不重建星体。

#### Scenario: 提交锚点
- **WHEN** 拖到某个 git 提交锚点且当前图存在对应文件节点
- **THEN** 该提交中不存在的当前文件节点隐藏，存在的显示

#### Scenario: 回合锚点
- **WHEN** 拖到 agent 回合锚点
- **THEN** 使用最近提交文件集与该回合前的新建文件决定可见性，不把之后新建的文件误显示

#### Scenario: 当前图缺失历史文件
- **WHEN** 历史锚点包含当前图没有的文件
- **THEN** 不重建该星体，仅回放当前图节点，并在面板/日志标注该限制

### Requirement: 相机与现在状态不变

拖动 scrubber 与切换历史锚点 MUST NOT 修改相机 yaw、pitch、distance 或 target；回到现在后当前图的所有节点、选中与相机恢复，历史回放不得写项目文件或执行 git 写操作。

#### Scenario: 相机不变
- **WHEN** 用户连续切换多个历史锚点
- **THEN** 相机参数与切换前完全一致

#### Scenario: 回到现在恢复
- **WHEN** 历史回放后回到现在
- **THEN** 节点可见性、选中与相机恢复到回放前状态

#### Scenario: 只读
- **WHEN** 加载时间轴或回放
- **THEN** 项目文件字节不变，git 只执行只读命令

### Requirement: 后台加载与失败降级

时间轴数据 SHALL 在后台加载，不阻塞 UI 主线程；加载中显示状态；损坏检查点、git 输出异常或部分数据缺失时跳过并降级，不 panic、不影响当前宇宙。

#### Scenario: 后台加载
- **WHEN** 首次打开时间轴
- **THEN** UI 先显示加载态，完成后显示锚点，不阻塞相机/对话操作


```

Full source: docs/openspec/changes/f20-s3-timeline/specs/spatial-timeline/spec.md
