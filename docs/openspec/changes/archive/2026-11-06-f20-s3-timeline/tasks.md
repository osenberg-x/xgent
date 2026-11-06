# Tasks

## 1. 时间轴数据层

- [x] 1.1 在 `xgent_ui::space::s3` 定义锚点/时间轴数据模型与合并逻辑（回合 + git commit，稳定排序，上限）；验证纯数据测试覆盖合并、空历史、无 git。 <!-- comet-task:a8717295-7804-4a5c-8dea-84d7d5af1015 -->
- [x] 1.2 实现检查点清单读取（当前会话 turn→Created 路径，损坏/缺失跳过）与 git log/ls-tree 只读加载（前缀剥离、失败降级）；验证临时检查点与临时 git 仓库测试。 <!-- comet-task:31908fad-563c-4487-bdb6-c3209a2cc92a -->

## 2. 回放逻辑

- [x] 2.1 实现锚点文件集计算（commit 树；turn 最近提交 + Created；无 git 回退）；验证纯函数测试覆盖提交/回合/无 git/created 前后缀。 <!-- comet-task:3194d536-d7bf-4bcf-bc31-fe171258f390 -->
- [x] 2.2 实现当前图可见性应用（文件/目录/边、now 恢复、选中保存、相机不变）；验证节点/目录/边/相机/选中测试。 <!-- comet-task:91c85ed4-c615-4af2-8873-3f7969caf2c4 -->

## 3. UI 与交互

- [x] 3.1 在 3D HUD 增加时间轴 rail/标记/标签/回到现在/历史边框；验证 UI 结构与 i18n 测试。 <!-- comet-task:b55f834d-77a0-45eb-ac01-672a38836623 -->
- [x] 3.2 实现标记点击、键盘 Left/Right、PointerDrag 映射与 Esc 退出历史；验证交互测试（headless 点击/键盘/映射）。 <!-- comet-task:7be0a35a-6c7e-40ee-976f-04b15080f3c8 -->
- [x] 3.3 后台加载/轮询/空态/加载/错误/刷新；验证异步加载与降级测试。 <!-- comet-task:55bb21bd-59ab-4b55-8783-ed0691d0791f -->

## 4. 文档与回归

- [x] 4.1 同步 `docs/design/architecture.md`、`docs/dev-tutorial.md`、`AGENTS.md` 的 S3 范围、数据口径与限制；验证文档路径/测试名存在。 <!-- comet-task:08823fce-74a8-44e5-91bd-60dcad45f6d8 -->
- [x] 4.2 运行相关 crate 回归与 workspace check；验证 `cargo test -p xgent_ui -p xgent_tools -p xgent_spatial` 与 `cargo check --workspace` 通过。 <!-- comet-task:35a93c3f-07d3-4992-a03a-49dd98f5e331 -->

## Workflow follow-up

- 归档前完成最终验证。
- 归档后按用户选择的交付方式推送/创建 PR；未获授权前不推送。
