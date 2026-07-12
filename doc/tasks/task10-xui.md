# Task 10: xui

> 对应实现指导：`doc/plans/step10-xui.md`
> 前置：step2 xui_i18n 已完成（bevy 由 workspace 提供）

## 任务清单

### 阶段一：脚手架

- [ ] T-10.1 创建 crate 目录与 Cargo.toml
  - 依赖：无
  - 验收：`crates/xui/Cargo.toml` 存在；依赖为 bevy(ui feature) + xui_i18n；**不含任何 xgent_***；license/description/keywords 填好；`cargo check -p xui` 通过。

- [ ] T-10.2 注册到 workspace
  - 依赖：T-10.1
  - 验收：`cargo metadata` 识别。

- [ ] T-10.3 验证独立性
  - 依赖：T-10.1
  - 验收：`cargo tree -p xui` 仅含 bevy 系列与 xui_i18n，无任何 `xgent_*`。

### 阶段二：i18n 桥接

- [ ] T-10.4 实现 `i18n_bridge.rs`
  - 依赖：T-2.3
  - 验收：定义 `Strings(pub Box<dyn StringSource>)` Resource；`tr(res, key)` 与 `tr_with(res, key, args)`；编译通过。

### 阶段三：虚拟列表

- [ ] T-10.5 实现 `virtual_list.rs` 的 VirtualList 组件与系统
  - 依赖：T-10.1
  - 验收：定义 `VirtualList` Component（item_count/item_height/first_visible/visible_count）、`VirtualItemBuilder` trait；`VirtualListPlugin` 注册 `update_virtual_list` 系统（读滚动位置算可见区间、用 entity pool spawn/回收 item）；编译通过。

- [ ] T-10.6 验证虚拟列表性能
  - 依赖：T-10.5
  - 验收：造 1000 项 VirtualList，断言渲染子节点数 = visible_count 而非 1000。

### 阶段四：命令面板

- [ ] T-10.7 实现 `command_palette.rs` 的类型与注册表
  - 依赖：T-10.1
  - 验收：定义 `PaletteCommand`（id/label/kind）、`CommandKind`、`CommandRegistry` Resource、`CommandPaletteState` Resource（open/query/selected/filtered）、`PaletteTriggered` Event；编译通过。

- [ ] T-10.8 实现命令面板系统与渲染
  - 依赖：T-10.7
  - 验收：`CommandPalettePlugin` 注册 filter_commands（模糊匹配）、render_palette（open 时渲染）、handle_palette_input（上下选/Enter 触发/Esc 关闭）；编译通过。

- [ ] T-10.9 验证命令面板
  - 依赖：T-10.8
  - 验收：注册若干命令，open + 输入 query，filtered 正确；Enter 发 PaletteTriggered；Esc 关闭。

### 阶段五：输入增强

- [ ] T-10.10 实现 `input.rs` 的 ChatInput
  - 依赖：T-10.1
  - 验收：定义 `ChatInput` Component（multiline/send_modifier）、`SendModifier`（Cmd/Ctrl）、`ChatInputSubmitted` Event；`InputEnhancePlugin` 注册 `handle_chat_input_keys`；基于官方 EditableText 薄封装，不重写核心；编译通过。

- [ ] T-10.11 验证输入语义
  - 依赖：T-10.10
  - 验收：多行模式 Enter 插入换行；Cmd/Ctrl+Enter 发 ChatInputSubmitted 且清空。

### 阶段六：快捷键体系

- [ ] T-10.12 实现 `hotkeys.rs` 的 HotkeyRegistry
  - 依赖：T-10.1
  - 验收：定义 `Mod`（Cmd/Ctrl）、`platform_primary_mod()`（cfg target_os）、`Hotkey`（id/key/mod_shift/mod_primary/label）、`HotkeyRegistry` Resource（register 含冲突检测、match_input）、`HotkeyTriggered` Event、`HotkeyConflict` 错误；编译通过。

- [ ] T-10.13 实现 `shortcuts.rs` 的 dispatch
  - 依赖：T-10.12
  - 验收：`ShortcutsPlugin` 注册 `dispatch_hotkeys`（读 Input<KeyCode>，匹配，发 HotkeyTriggered）；编译通过。

- [ ] T-10.14 验证快捷键
  - 依赖：T-10.13
  - 验收：注册 Hotkey，按对应键发 HotkeyTriggered；重复注册同键报 HotkeyConflict。

### 阶段七：Plugin 集成

- [ ] T-10.15 实现 `lib.rs` 的 XuiPlugin
  - 依赖：T-10.5, T-10.8, T-10.10, T-10.13
  - 验收：Plugin 组装 VirtualListPlugin/CommandPalettePlugin/InputEnhancePlugin/ShortcutsPlugin，init HotkeyRegistry；编译通过。

- [ ] T-10.16 i18n 桥接测试
  - 依赖：T-10.4
  - 验收：注入 mock impl StringSource，断言 tr() 返回 mock 值。

## 完成标志

- `cargo check -p xui` 通过
- `cargo test -p xui` 全绿
- `cargo tree -p xui` 仅含 bevy + xui_i18n，无 xgent_*
- 虚拟列表、命令面板、输入增强、快捷键、i18n 桥接均可用
- 所有业务逻辑经 Event 触发，xui 不执行业务 handler
