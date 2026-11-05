# Tasks

## 1. 价格配置与成本计算

- [x] 1.1 在 `xgent_settings_core` 增加 `PricingConfig`/`ModelPrice`、默认价与模型覆盖优先级、价格合法性校验、成本公式；验证 `cargo test -p xgent_settings_core cost` 覆盖默认价、模型覆盖、未定价、非法值、极小额。 <!-- comet-task:10a56886-7ef9-4b6e-adbc-0aa6d10051f7 -->
- [x] 1.2 在 `docs/dev-tutorial.md` 增加 `[pricing]` 配置说明与示例；验证示例可由 1.1 的解析测试原样读取。 <!-- comet-task:2099f992-5a82-4978-acfc-29a2395681b3 -->

## 2. 配置写入与设置面板

- [x] 2.1 在 daemon `ConfigStore` 增加 `pricing.input_per_million`/`pricing.output_per_million` 读写与 `0..=10000` 有限数校验；验证 daemon 单测覆盖合法/非法/往返。 <!-- comet-task:54fee8a9-ec10-4451-9728-bb5c58a5dcf4 -->
- [x] 2.2 在设置面板增加“成本”分区（两个小数输入框），只写脏键、非法值不落盘并提示；验证 UI 单测覆盖解析、脏检查、错误提示与 ConfigItem 键。 <!-- comet-task:5b3034c0-ce85-48d8-8188-0e2a81c57b78 -->

## 3. 当前会话与状态栏

- [x] 3.1 新增 UI 侧 `CostStats` Resource，从 `Conversation` 的 Assistant usage 汇总 prompt/completion/cost，恢复会话与价格变化时重算；验证无 usage、恢复、价格切换测试。 <!-- comet-task:05a42766-88a4-4640-9296-9fe669945e49 -->
- [x] 3.2 状态栏 token/成本段接入 `CostStats`，成本段改为可点击按钮并打开成本面板；验证状态栏文本与按钮标记测试，未定价显示 `—`。 <!-- comet-task:4bd3ad98-824f-44d7-85fa-ef6a866d2c34 -->

## 4. 成本面板与历史聚合

- [x] 4.1 新增成本面板 overlay（scrim/标题/关闭/当前会话/历史/按模型行/加载/空态/错误），支持点击状态栏打开、关闭按钮/Esc 关闭；验证 panel 开关与状态渲染测试。 <!-- comet-task:8a251206-bc6c-4aa8-8db0-5e32b308a661 -->
- [x] 4.2 实现 sessions JSONL 后台聚合（线程 + crossbeam_channel、损坏行/缺 usage/unknown model 跳过、按模型排序、包含当前会话），不阻塞 UI、不写会话文件；验证临时会话聚合与字节不变测试。 <!-- comet-task:cfd9e079-e4f0-4721-8c66-28305c89bc96 -->
- [x] 4.3 接通面板与聚合结果、按模型前 20 行/其余汇总与 i18n；验证面板加载→结果→关闭集成测试。 <!-- comet-task:671b554b-ec0c-47bd-a996-d4437251e8c8 -->

## 5. 文档与回归

- [x] 5.1 同步 `docs/design/architecture.md`、`docs/dev-tutorial.md` 与 `AGENTS.md` 的 F-12 状态、配置与边界；验证文档路径/测试名存在。 <!-- comet-task:22bc8772-8ef3-4c6b-9818-41253d56b8e3 -->
- [x] 5.2 运行相关 crate 回归与 workspace check；验证 `cargo test -p xgent_settings_core -p xgent_agent -p xgent_ui -p xgent_app` 与 `cargo check --workspace` 通过。 <!-- comet-task:87979838-f7e2-4d8c-962e-828d31ace9b0 -->

## Workflow follow-up

- 归档前按项目评审要求完成最终验证。
- 归档后按用户选择的交付方式推送/创建 PR；未获授权前不推送。
