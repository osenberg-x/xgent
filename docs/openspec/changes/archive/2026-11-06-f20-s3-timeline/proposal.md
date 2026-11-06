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
