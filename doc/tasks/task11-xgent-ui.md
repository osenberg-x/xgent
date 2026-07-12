# Task 11: xgent_ui

> 对应实现指导：`doc/plans/step11-xgent-ui.md`
> 前置：step1 xgent_core、step4 xgent_settings、step9 xgent_agent、step10 xui 已完成

## 任务清单

### 阶段一：脚手架

- [ ] T-11.1 创建 crate 目录与 Cargo.toml
  - 依赖：无
  - 验收：`crates/xgent_ui/Cargo.toml` 存在；依赖为 bevy(ui/bevy_gizmos/serialize/png)、xgent_core、xgent_settings、xgent_agent、xui、serde、serde_json；`cargo check -p xgent_ui` 通过。

- [ ] T-11.2 注册到 workspace
  - 依赖：T-11.1
  - 验收：`cargo metadata` 识别。

### 阶段二：主题与布局

- [ ] T-11.3 实现 `theme.rs`
  - 依赖：T-11.1
  - 验收：MVP 直接用官方 `bevy_feathers::dark_theme::create_dark_theme()`；定义薄封装 Resource 持有主题引用；编译通过。

- [ ] T-11.4 实现 `layout.rs` 三区布局
  - 依赖：T-11.3
  - 验收：`LayoutPlugin` + `spawn_layout`：顶栏(40px) + 主区(flex:1, 文件区 + 对话侧栏 360px) + 状态栏(24px)；编译通过。

- [ ] T-11.5 实现 `i18n.rs` 的 Localizer 桥接
  - 依赖：T-4.8, T-10.4
  - 验收：把 `xgent_settings::Localizer`（impl StringSource）注入 `xui::Strings` Resource 的辅助系统；语言切换发刷新事件；编译通过。

### 阶段三：对话面板

- [ ] T-11.6 实现 `chat_panel.rs` 的渲染
  - 依赖：T-11.4, T-10.5
  - 验收：`ChatPanelPlugin` + `spawn_chat_panel` + `render_messages`：订阅 DeltaEvent 累加渲染当前助手消息；消息列表用 `xui::VirtualList`；编译通过。

- [ ] T-11.7 实现对话输入处理
  - 依赖：T-10.10, T-9.3
  - 验收：输入框用 `xui::ChatInput`；订阅 `xui::ChatInputSubmitted` 发 `UserInputEvent`；Esc/中断按钮发 `AbortEvent`；编译通过。

### 阶段四：工具面板与确认弹窗

- [ ] T-11.8 实现 `tool_panel.rs`
  - 依赖：T-9.3
  - 验收：订阅 `ToolCallEvent`/`ToolResultEvent`，展示工具执行过程与结果；编译通过。

- [ ] T-11.9 实现 `confirm_dialog.rs`
  - 依赖：T-9.3, T-7.5
  - 验收：订阅 `ConfirmRequestEvent` 弹确认对话框（工具 id/摘要）；用户决策发 `ConfirmDecisionEvent`；编译通过。

### 阶段五：文件面板与状态栏

- [ ] T-11.10 实现 `file_panel.rs`
  - 依赖：T-11.4
  - 验收：文件树遍历项目根；点击文件异步读（tokio task）渲染内容（MVP 只读）；订阅 FileChangedEvent 刷新打开文件；编译通过。

- [ ] T-11.11 实现 `status_bar.rs`
  - 依赖：T-11.4
  - 验收：展示当前 provider/model、token 流式指示、会话状态；编译通过。

### 阶段六：命令面板与快捷键（业务层）

- [ ] T-11.12 实现 `command_palette.rs` 的 XGent 命令注册
  - 依赖：T-10.7, T-11.5
  - 验收：调用 `xui::CommandRegistry::register` 注册 XGent 命令（新建会话/切换 provider/打开设置/切换语言/Cmd+P 文件）；订阅 `xui::PaletteTriggered` 据 id 执行；编译通过。

- [ ] T-11.13 实现 `shortcuts.rs` 的 XGent 快捷键绑定
  - 依赖：T-10.12, T-11.5
  - 验收：`register_xgent_hotkeys` 注册参考 VSCode 的默认绑定（Cmd+P/Cmd+Shift+P/Cmd+Enter/Esc/Cmd+,）；订阅 `xui::HotkeyTriggered` 执行业务；编译通过。

### 阶段七：设置面板与可复用组件

- [ ] T-11.14 实现 `settings_panel.rs`
  - 依赖：T-11.5
  - 验收：provider 列表编辑、默认 provider/model、语言切换（调 Localizer::switch + 刷新事件）、主题选择（MVP 仅 dark）；写操作经 daemon config.write；编译通过。

- [ ] T-11.15 实现 `components/` 可复用组件
  - 依赖：T-11.1
  - 验收：button.rs/editable_text.rs 等 XGent 特有包装（如对话输入框样式与 placeholder i18n）；编译通过。

### 阶段八：Plugin 集成与测试

- [ ] T-11.16 实现 `lib.rs` 的 XgentUiPlugin
  - 依赖：T-11.4~T-11.14
  - 验收：组装 XuiPlugin + 各业务面板 Plugin + ShortcutsPlugin；init UiState；编译通过。

- [ ] T-11.17 布局测试
  - 依赖：T-11.16
  - 验收：启动 App，断言三区节点存在。

- [ ] T-11.18 对话流测试
  - 依赖：T-11.6, T-11.16
  - 验收：用测试系统发 DeltaEvent 序列，断言对话面板渲染出消息。

- [ ] T-11.19 输入测试
  - 依赖：T-11.7, T-11.16
  - 验收：输入框打字，Enter 时 UserInputEvent 发出。

- [ ] T-11.20 确认弹窗测试
  - 依赖：T-11.9, T-11.16
  - 验收：发 ConfirmRequestEvent，弹窗出现；点允许/拒绝发 ConfirmDecisionEvent。

- [ ] T-11.21 命令面板与快捷键测试
  - 依赖：T-11.12, T-11.13, T-11.16
  - 验收：Cmd+P 打开命令面板，输入过滤，选中执行；快捷键触发对应业务。

- [ ] T-11.22 i18n 切换测试
  - 依赖：T-11.5, T-11.14, T-11.16
  - 验收：设置面板切语言，UI 字符串变化。

## 完成标志

- `cargo check -p xgent_ui` 通过
- `cargo test -p xgent_ui` 全绿
- 三区布局 + 对话/工具/文件/状态/设置/确认面板可用
- 命令面板与快捷键参考 VSCode 体系
- i18n 字符串经 xui::tr，语言切换实时生效
- 所有 UI 订阅 agent Event 渲染、发送 Event 驱动 agent，无直接方法调用
