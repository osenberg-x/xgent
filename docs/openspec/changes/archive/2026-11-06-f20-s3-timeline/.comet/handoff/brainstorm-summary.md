# Brainstorm Summary

- Change: f20-s3-timeline
- Date: 2026-11-06

## Confirmed Technical Approach

当前会话 agent 回合与 git 提交合并为时间轴；commit 锚点用 `git ls-tree` 文件集，turn 锚点用最近提交树 + 检查点 Created 路径；只控制当前 L1 图节点/目录/边可见性，不重建已删除文件。UI 为 HUD 底部 scrubber + 历史边框 + 回到现在；进入历史保存/清空选中，回到现在恢复；相机不动。后台线程只读加载，git/检查点缺失降级。

## Key Tradeoffs and Risks

- 当前图缺失历史文件：不重建星体，明示限制。
- turn 文件集是近似（git 树 + Created），用户手改/删除不完整。
- 检查点仅保留 20 轮、git 最多 50 提交。
- git 在后台执行，失败不影响当前宇宙。
- UI 拖动通过 PointerDrag + rail 几何；headless 测试点击/键盘/映射。

## Test Strategy

纯数据合并/排序/上限；检查点与临时 git fixture；锚点文件集与可见性/目录/边；相机/选中/只读；UI 标签/边框/点击/键盘/空加载错误；相关 crate 回归 + workspace check。

## Spec Patch

None；Open 阶段 spec 已覆盖数据源、scrubber、回放、相机、后台与 i18n。
