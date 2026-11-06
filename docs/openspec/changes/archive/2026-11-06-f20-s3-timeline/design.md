---
comet_change: f20-s3-timeline
role: technical-design
canonical_spec: openspec
archived-with: 2026-11-06-f20-s3-timeline
status: final
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
