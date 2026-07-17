# Step 11: xgent_ui

## 模块职责

XGent 的图形界面层，Bevy 全栈实现（bevy_ui + 必要的渲染）：

1. **布局**：顶栏 + 主区（文件预览/代码区 + 右侧对话侧栏）+ 状态栏。
2. **对话面板**：消息列表（流式渲染）、输入框、中断/重试。
3. **工具面板**：展示工具调用与结果、确认弹窗（NeedsConfirmation 工具）。
4. **文件面板**：项目文件树预览、当前文件内容（MVP 只读）。
5. **状态栏**：当前 provider/model、token 流式指示、会话状态。
6. **命令面板**（F-08）：Cmd+P / Cmd+Shift+P 风格全局命令/文件入口。
7. **快捷键体系**（F-09）：参考 VSCode 体系。
8. **设置面板**：provider 配置、语言切换、主题。
9. **i18n 集成**：所有字符串经 Localizer，语言切换实时刷新。

## 前置依赖

- xui（虚拟列表、命令面板、输入增强、快捷键、i18n 桥接等通用组件）
- xgent_core（类型）
- xgent_settings（Localizer、GlobalConfig、ProjectConfig，作为 xui StringSource 的实现提供者）
- xgent_agent（事件契约、Conversation Resource）

## 目标文件结构

```
crates/xgent_ui/
├── Cargo.toml
└── src/
    ├── lib.rs                  # XgentUiPlugin + 布局组装
    ├── theme.rs               # 主题：颜色、字体、间距
    ├── layout.rs              # 顶栏/主区/状态栏布局
    ├── chat_panel.rs          # 对话面板
    ├── tool_panel.rs          # 工具调用展示
    ├── file_panel.rs          # 文件树 + 文件内容
    ├── status_bar.rs          # 状态栏
    ├── command_palette.rs     # 命令面板
    ├── settings_panel.rs      # 设置面板
    ├── confirm_dialog.rs      # 确认弹窗
    ├── input.rs               # 文本输入组件（封装 bevy_ui 输入）
    ├── shortcuts.rs           # 快捷键体系
    ├── i18n.rs                 # Localizer 集成：字符串渲染辅助
    └── components/            # 可复用 UI 组件
        ├── mod.rs
        ├── button.rs
        └── editable_text.rs
```

## Cargo.toml

```toml
[package]
name = "xgent_ui"
version = "0.1.0"
edition = "2024"

[dependencies]
bevy = { workspace = true, features = [
    "ui",
    "bevy_gizmos",
    "serialize",
    "png",
] }
xgent_core = { path = "../xgent_core" }
xgent_settings = { path = "../xgent_settings" }
xgent_agent = { path = "../xgent_agent" }
xui = { path = "../xui" }
serde = { workspace = true }
serde_json = { workspace = true }
```

说明：UI 侧用 bevy_ui + xui 组件库。基础 widget（button/checkbox/slider/dialog/menu/popover）直接用官方 bevy_ui_widgets / bevy_feathers；通用增强组件（虚拟列表、命令面板、输入增强、快捷键）用 xui；XGent 业务组件（对话面板、工具面板、文件面板、状态栏、设置面板、确认弹窗）在本 crate 实现。MVP 不启用 3d feature（保持轻量），未来加 3D 时启用。不依赖 provider/tools/context（它们经 agent 的事件契约交互，UI 不直接调用）。

## 关键类型与接口

### 1. lib.rs — Plugin 与布局

```rust
use bevy::prelude::*;

pub struct XgentUiPlugin;

impl Plugin for XgentUiPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_plugins((
                LayoutPlugin,
                ChatPanelPlugin,
                ToolPanelPlugin,
                FilePanelPlugin,
                StatusBarPlugin,
                CommandPalettePlugin,
                SettingsPanelPlugin,
                ConfirmDialogPlugin,
                ShortcutsPlugin,
            ))
            .init_resource::<UiState>();
    }
}

#[derive(Resource)]
pub struct UiState {
    pub focused: FocusTarget,        // 当前焦点（输入框/文件树/命令面板）
    pub active_panel: ActivePanel,
    pub command_palette_open: bool,
    pub settings_open: bool,
}
```

### 2. layout.rs — 三区布局

```rust
// 顶栏（高 40px）：项目名、provider/model 选择、设置按钮
// 主区（flex:1）：左侧文件面板 + 右侧对话/工具侧栏（宽 360px）
// 状态栏（高 24px）：状态、token、快捷键提示
pub struct LayoutPlugin;
impl Plugin for LayoutPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_layout);
    }
}
```

### 3. chat_panel.rs — 对话面板

```rust
pub struct ChatPanelPlugin;
impl Plugin for ChatPanelPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_systems(Startup, spawn_chat_panel)
            .add_systems(Update, (
                render_messages,         // 订阅 DeltaEvent 累加渲染
                handle_input,            // 输入框：Enter 发 UserInputEvent
                handle_abort,            // Esc 或中断按钮发 AbortEvent
            ));
    }
}
```

### 4. tool_panel.rs / confirm_dialog.rs

```rust
// 订阅 ToolCallEvent / ToolResultEvent，展示工具执行过程
// 订阅 ConfirmRequestEvent，弹确认对话框，用户决策发 ConfirmDecisionEvent
```

### 5. file_panel.rs

```rust
// 文件树：从项目根遍历，缩进树形展示（详见 doc/design/ui-design.md 5.1）
//   - 双击目录展开/折叠，展开后在下方缩进展示子项（目录优先 + 字母序）
//   - 懒加载：初始只展开根第一层；深层目录首次双击才读（tokio task）
//   - 已展开目录内容缓存内存；折叠/再展开不重读，除非 FileChangedEvent 标 stale
//   - 节点超 500 接 xui::VirtualList 虚拟滚动 + entity pool 复用
//   - .gitignore 简单匹配（target/ .git/ node_modules/ .xgent/）；P1+ 接 ignore crate
//   - 加载中 / 空 / 读取失败 占位态
// 文件内容：单击文件 → 读（tokio task）→ 内容区渲染（MVP 只读）
//   - 双击文件：MVP 同单击；P1 编辑器上线后切到编辑器视图打开
// 订阅 FileChangedEvent → 沿 path 上溯找最近已展开祖先，标 stale 后台刷新子项
//   （保持已展开子目录的展开状态）；若该文件正在内容区展示，刷新内容
```

### 6. command_palette.rs

```rust
// 基于 xui::CommandPalette 组件，XGent 注册命令、订阅 PaletteTriggered 执行业务
// 命令注册：调用方调 xui::CommandRegistry::register 注册命令
// - Cmd+P：文件快速打开（注册文件命令）
// - Cmd+Shift+P：动作命令（新建会话、切换 provider、打开设置、切换语言...）
// handler：订阅 xui::PaletteTriggered 事件，据 id 执行
pub fn register_xgent_commands(reg: &mut CommandRegistry, strings: &Strings) { /* ... */ }
```

### 7. shortcuts.rs — 快捷键体系（参考 VSCode）

```rust
// 基于 xui::HotkeyRegistry + ShortcutsPlugin
// XGent 在此注册参考 VSCode 默认绑定的快捷键
// 订阅 xui::HotkeyTriggered 事件执行业务
pub struct ShortcutsPlugin;  // XGent 业务快捷键注册
impl Plugin for ShortcutsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_xgent_hotkeys)
            .add_systems(Update, handle_hotkey_triggers);
    }
}
// 平台修饰键、冲突检测由 xui 处理
// 例：Cmd+P 文件、Cmd+Shift+P 命令、Cmd+Enter 发送、Esc 中断、Cmd+, 设置
```

### 8. settings_panel.rs

```rust
// provider 列表编辑、默认 provider/model
// 语言切换（调 Localizer::switch + 触发 UI 刷新事件）
// 主题选择（MVP 仅 dark）
// 写操作经 daemon config.write（经 agent bridge 或直接 IPC client）
```

### 9. components/editable_text.rs

```rust
// bevy_ui 的文本输入能力较弱，封装一个 EditableText 组件
// 处理：光标、插入/删除、IME（中文输入法）、Enter/Ctrl+Enter 区分
// 这是 bevy_ui 的补轮子点，需投入
```

### 10. i18n.rs

```rust
// Localizer 集成辅助
// 渲染时用 localizer.tr(key, args) 取字符串
// 语言切换事件 → 标记需刷新的 UI 节点 → 重渲染
```

## 实现要点

1. **Bevy 全栈**：UI 用 bevy_ui；MVP 不开 3d feature 保持轻量。bevy_ui 即时模式，每帧重建必要节点。
2. **事件驱动**：UI 订阅 agent 的 Events（Delta、ToolCall、ToolResult、ConfirmRequest、Done、Error）渲染；发送 Events（UserInput、Abort、ConfirmDecision）驱动 agent。**禁止直接调用 agent 方法**。
3. **输入框用 xui::ChatInput**：基于官方 EditableText 薄封装，多行 + 发送语义由 xui 处理；IME（中文输入）由官方 text_input 系统已支持。这是 UI 层关键点，但风险已通过 xui + 官方能力化解。
4. **流式渲染**：DeltaEvent 累加到当前助手消息节点，避免每 delta 重建整个消息列表（性能）。
5. **消息列表用 xui::VirtualList**：只渲染可见项，大列表性能由 xui 保障，业务层只需提供 item 构造回调。
6. **i18n 渲染**：所有可见字符串经 `xui::tr()`（底层由 xgent_settings::Localizer impl `xui_i18n::StringSource` 注入）。不硬编码。语言切换时发事件触发刷新（标记节点 dirty，下帧重建）。
7. **快捷键**：参考 VSCode 默认绑定，按平台区分修饰键。快捷键表可配置（未来用户自定义）。
8. **命令面板注册**：调用 xui::CommandRegistry 注册命令，订阅 PaletteTriggered 执行。便于未来扩展（用户自定义命令）。
9. **多开**：UI 是 UI 进程内的渲染，多开=多进程，每个进程一个 XgentUiPlugin 实例。不依赖 Bevy MultiWindow。
10. **轻量**：不开 3d、不开 tonemapping_luts、最小 feature 集，降二进制体积与启动开销。

## 验证方法

1. **编译检查**：
   ```bash
   cargo check -p xgent_ui
   ```
2. **布局测试**：启动 App，断言三区（顶栏/主区/状态栏）节点存在。
3. **对话流测试**：mock agent 事件（用测试系统发 DeltaEvent 序列），断言对话面板渲染出消息。
4. **输入测试**：在输入框打字，断言 UserInputEvent 在 Enter 时发出。
5. **确认弹窗测试**：发 ConfirmRequestEvent，断言弹窗出现，点允许/拒绝发 ConfirmDecisionEvent。
6. **命令面板测试**：Cmd+P 打开，输入过滤，选中命令执行。
7. **i18n 切换测试**：从设置面板切语言，断言 UI 字符串变化。
8. **IME 测试**：在输入框输入中文（需真实 OS 测试，CI 跳过）。

## 完成后下一步

xgent_ui 完成后 → 实现 **xgent_app**（UI 进程入口：组装所有 UI 侧插件、daemon 拉起、IPC 客户端、项目打开）。
