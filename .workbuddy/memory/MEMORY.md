# 项目长期记忆（XGent）

## 文档与台账约定

- **UI v8 的进度/基线与下一步统一看 `docs/plans/ui-v8-handoff.md`**（跨会话交接文档：已交付期数、提交号、测试基线、环境坑、下一步拆解）。
- 任务级勾选在 `docs/plans/ui-v8-tasks.md`（UI v8 唯一进度台账）。该台账的 M1/M2 复选框**滞后于实现**，判断「做没做」以归档 change（`docs/comet/archive/`）与代码实测为准。
- 功能变化要同步 `docs/dev-tutorial.md`（当前 UI v8 相关章节：§5.17 M1、§5.18 M2a、§5.19 M2b）。

## 构建/测试环境

- 本机所有 cargo 命令必须带 `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`（否则链接极慢/卡死）。
- `cargo fmt` 只用 `-p <crate>`；`--all` 会改写 `../bevy` 上游检出。
- Comet Native 的检查计划尾部常被标 `interrupted`：先用 `retry-checks` 补跑，不要直接 `doctor --repair`。
