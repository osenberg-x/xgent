# Step 10: xui

## 模块职责

`xui` 是一个**可脱离 XGent 独立发布、被其他 Bevy 项目复用**的通用 UI 组件库，纯依赖 bevy + xui_i18n，不依赖任何 `xgent_*` crate。

**动机**：bevy_feathers / bevy_ui_widgets 官方明确标注 experimental、会 breaking、建议“copy into your own project”。`xui` 作为薄封装层隔离官方 breaking change，集中升级成本。

**封装策略**：官方已覆盖的（button/checkbox/slider/dialog/menu/popover 等基础 widget，以及 text_input 的 IME 支持）**直接用官方**，不重复造轮子；`xui` 只封装官方未覆盖或需增强的部分。

**MVP 范围**（K-02/K-03/K-05/K-06/K-07）：虚拟列表、命令面板、输入增强、快捷键体系、i18n 桥接。主题增强（K-01）与系统窗口管理（K-04）延后到 P1。

## 前置依赖

- bevy（仅此一项，**不依赖任何 xgent_* crate**，保证可独立发布）

## 目标文件结构

```
crates/xui/
├── Cargo.toml
└── src/
    ├── lib.rs                 # XuiPlugin + 模块导出
    ├── virtual_list.rs        # K-02 虚拟列表组件
    ├── command_palette.rs     # K-03 命令面板组件
    ├── input.rs               # K-05 输入增强（多行 + 发送语义）
    ├── shortcuts.rs           # K-06 快捷键体系
    ├── hotkeys.rs             # K-06 快捷键注册表与平台修饰键抽象
    └── i18n_bridge.rs        # K-07 i18n 桥接（与 Localizer 解耦的 trait）
```

## Cargo.toml

```toml
[package]
name = "xui"
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
description = "A small UI component kit for Bevy, isolating experimental bevy_feathers/bevy_ui_widgets breaking changes."
keywords = ["bevy", "ui"]
repository = "https://github.com/<owner>/xui"

[dependencies]
bevy = { workspace = true, features = ["ui"] }
xui_i18n = { path = "../xui_i18n" }
```

说明：
- **纯依赖 bevy + xui_i18n**，不依赖 xgent_core / xgent_settings 等。这是可独立发布的前提（xui_i18n 也是可独立发布的极小 crate）。
- 不启用 3d 等重型 feature，保持轻量。
- license 与 repository 字段为独立发布准备（实际值待你定）。

## 关键类型与接口

### 1. lib.rs — Plugin

```rust
use bevy::prelude::*;

pub struct XuiPlugin;

impl Plugin for XuePlugin {
    fn build(&self, app: &mut App) {
        app
            .add_plugins((
                VirtualListPlugin,
                CommandPalettePlugin,
                InputEnhancePlugin,
                ShortcutsPlugin,
            ))
            .init_resource::<HotkeyRegistry>();
    }
}
```

### 2. virtual_list.rs — K-02 虚拟列表

官方 `ListBox` / `ScrollArea` 非虚拟，大列表性能不足。`VirtualList` 只渲染可见项。

```rust
use bevy::prelude::*;

/// 虚拟列表：根据滚动位置只生成可见项的实体
/// 调用方提供 item count、item 高度、item 构造回调
#[derive(Component)]
pub struct VirtualList {
    pub item_count: usize,
    pub item_height: f32,
    pub first_visible: usize,   // 由系统更新
    pub visible_count: usize,   // 由系统更新
}

pub struct VirtualListPlugin;
impl Plugin for VirtualListPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_virtual_list);
    }
}

/// 系统每帧：读滚动位置 → 算可见区间 → spawn/despawn/回收 item 实体
/// 用一个 entity pool 复用节点，避免每帧创建销毁
fn update_virtual_list(
    mut commands: Commands,
    mut q: Query<(Entity, &mut VirtualList, &Children), Changed<VirtualList>>,
    // ... 滚动位置查询、pool 管理
) { /* ... */ }

/// 调用方注册 item 构造函数（item index -> 子节点）
pub trait VirtualItemBuilder: Send + Sync {
    fn build(&self, commands: &mut Commands, parent: Entity, index: usize);
}
```

**要点**：item 高度固定时最简（MVP 先支持固定高度）；变高需测量，留后续。用 entity pool 回收避免每帧 spawn/despawn 抖动。

### 3. command_palette.rs — K-03 命令面板

官方无此组件。Cmd+P / Cmd+Shift+P 风格。

```rust
use bevy::prelude::*;
use std::sync::Arc;

/// 命令定义
pub struct PaletteCommand {
    pub id: String,
    pub label: String,           // 已本地化后的字符串（由调用方提供，xui 不依赖 i18n）
    pub kind: CommandKind,        // File / Action
}

pub enum CommandKind { File, Action }

/// 命令注册表 Resource
#[derive(Resource, Default)]
pub struct CommandRegistry {
    pub commands: Vec<PaletteCommand>,
}

/// 命令面板状态
#[derive(Resource, Default)]
pub struct CommandPaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub filtered: Vec<usize>,   // 匹配的命令下标
}

pub struct CommandPalettePlugin;
impl Plugin for CommandPalettePlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<CommandRegistry>()
            .init_resource::<CommandPaletteState>()
            .add_systems(Update, (
                filter_commands,    // query 变化时模糊匹配
                render_palette,    // open 时渲染面板节点
                handle_palette_input,  // 上下选、Enter 确认、Esc 关闭
            ));
    }
}

/// 触发命令：调用方订阅 PaletteTriggered 事件执行实际逻辑
#[derive(Event)]
pub struct PaletteTriggered { pub command_id: String }
```

**要点**：xui 只提供面板与触发事件，**不执行命令逻辑**（命令 handler 由调用方在业务层订阅事件实现），保持 xui 与业务解耦。模糊匹配用简单子串+打分（MVP），后续可换 fzf 算法。

### 4. input.rs — K-05 输入增强（薄封装）

官方 `EditableText`（bevy_text）已支持 IME。xui 只做多行 + 发送语义封装。

```rust
use bevy::prelude::*;

/// 多行输入框：基于官方 EditableText 薄封装
/// - 多行模式（Enter 换行，Ctrl/Cmd+Enter 发送）
/// - 发送事件
#[derive(Component)]
pub struct ChatInput {
    pub multiline: bool,
    pub send_modifier: SendModifier,   // Cmd（macOS）/ Ctrl（其他）
}

pub enum SendModifier { Cmd, Ctrl }

#[derive(Event)]
pub struct ChatInputSubmitted { pub text: String }

pub struct InputEnhancePlugin;
impl Plugin for InputEnhancePlugin {
    fn build(&self, app: &mut App) {
        app
            .add_event::<ChatInputSubmitted>()
            .add_systems(Update, handle_chat_input_keys);
    }
}

/// 拦截 Enter：据 modifier 决定换行/发送；发送时清空并发事件
fn handle_chat_input_keys(/* ... */) { /* ... */ }
```

**要点**：**不重写输入核心**——光标、删除、IME 全部由官方 `EditableText` + text_input 系统处理。xui 只在 key event 层做"Enter 语义"判定。这是官方明确建议的“薄增强”而非替代。

### 5. hotkeys.rs + shortcuts.rs — K-06 快捷键体系

```rust
use bevy::prelude::*;
use std::collections::HashMap;

/// 平台无关修饰键抽象
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mod { Cmd, Ctrl }   // 运行时据 cfg(target_os) 选

pub fn platform_primary_mod() -> Mod {
    #[cfg(target_os = "macos")] { Mod::Cmd }
    #[else] { Mod::Ctrl }
}

/// 快捷键定义
#[derive(Debug, Clone)]
pub struct Hotkey {
    pub id: String,
    pub key: KeyCode,
    pub mod_shift: bool,
    pub mod_primary: bool,    // platform_primary_mod
    pub label: String,        // 已本地化字符串（调用方提供）
}

/// 注册表
#[derive(Resource, Default)]
pub struct HotkeyRegistry {
    pub bindings: HashMap<String, Hotkey>,
}

impl HotkeyRegistry {
    pub fn register(&mut self, h: Hotkey) -> Result<(), HotkeyConflict> {
        // 冲突检测：同 key+mods 报错
    }
    pub fn match_input(&self, key: KeyCode, mods: ...) -> Option<&Hotkey> { /* ... */ }
}

#[derive(Event)]
pub struct HotkeyTriggered { pub id: String }

pub struct ShortcutsPlugin;
impl Plugin for ShortcutsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, dispatch_hotkeys);  // 读 Input<KeyCode>，匹配，发事件
    }
}
```

**要点**：xui 只做匹配与触发事件，**不执行快捷键动作**（动作由调用方业务层订阅 `HotkeyTriggered` 实现）。参考 VSCode 默认绑定的实际映射表由调用方（xgent_ui）注册。冲突检测在注册时即报错，避免静默覆盖。

### 6. i18n_bridge.rs — K-07 i18n 桥接

xui 经 `xui_i18n::StringSource` trait 取字符串（trait 在 xui_i18n crate，见 step2），由宿主注入实现。xui 不依赖 fluent 或 xgent_settings。

```rust
use bevy::prelude::*;
use xui_i18n::StringSource;

/// 注入的 StringSource（运行期由宿主提供，如 xgent_settings::Localizer）
#[derive(Resource)]
pub struct Strings(pub Box<dyn StringSource>);

pub fn tr(res: &Strings, key: &str) -> String { res.0.get(key, &[]) }
pub fn tr_with(res: &Strings, key: &str, args: &[(&str, String)]) -> String { res.0.get(key, args) }
```

**要点**：trait 在 `xui_i18n`（step2），xui 与 xgent_settings 都依赖它。xgent_settings::Localizer impl 该 trait，xgent_app 注入到 `Strings` Resource。xui 自身可独立测试（mock impl）。这是可独立发布的关键设计——xui 不直接依赖任何 i18n 实现或 xgent 类型。

## 实现要点

1. **纯依赖 bevy + xui_i18n**：Cargo.toml 只有 bevy 与 xui_i18n，无 xgent_*。xui_i18n 本身零依赖。任何对 xgent 类型的需求都改用 trait 反转依赖（见 K-07）。
2. **不重复造轮子**：button/checkbox/slider/dialog/menu/popover 直接用官方；text_input 核心用官方，只薄增强发送语义。
3. **业务解耦**：命令面板、快捷键只发触发事件，不执行业务逻辑。调用方（xgent_ui）订阅事件实现。这是“通用”的核心。
4. **i18n 反转依赖**：xui 经 `xui_i18n::StringSource` trait 取字符串，trait 在独立的 xui_i18n crate。由上层（xgent_settings::Localizer）实现，xgent_app 注入。xui 不直接依赖 fluent 或 xgent_*，保持可被任意 i18n 方案的 Bevy 项目复用。
5. **虚拟列表 entity pool**：避免每帧 spawn/despawn，复用节点仅更新内容。
6. **平台修饰键**：cfg(target_os) 选 Cmd/Ctrl，运行期抽象给上层一致 API。
7. **冲突检测前置**：快捷键注册时即检测冲突报错，不静默覆盖。
8. **官方 breaking change 隔离**：所有对 bevy_feathers / bevy_ui_widgets 的直接调用集中在 xui 内部模块，业务层只依赖 xui API。升级 Bevy 时改 xui 内部，业务层不动。
9. **MVP 不实现 K-01/K-04**：主题直接用官方 `bevy_feathers::dark_theme::create_dark_theme()`；系统窗口用官方 `bevy_window::Window` 默认。这两项 P1 补。
10. **可发布性**：Cargo.toml 带 license/repository/description/keywords，命名 `xui`（独立身份，不带 xgent 前缀）。

## 验证方法

1. **编译检查**：
   ```bash
   cargo check -p xui
   ```
2. **独立性验证**：`cargo tree -p xui` 输出应只含 bevy 系列与 xui_i18n，不含任何 `xgent_*` crate。
3. **虚拟列表测试**：造 1000 项的 VirtualList，断言渲染的子节点数等于 visible_count 而非 1000。
4. **命令面板测试**：注册若干命令，open + 输入 query，断言 filtered 正确；Enter 发 PaletteTriggered；Esc 关闭。
5. **输入增强测试**：ChatInput 多行模式下，Enter 插入换行；Cmd/Ctrl+Enter 发 ChatInputSubmitted 且清空。
6. **快捷键测试**：注册 Hotkey，按对应键发 HotkeyTriggered；重复注册同键报冲突。
7. **i18n 桥接测试**：注入 mock impl `StringSource`，断言 tr() 返回 mock 值。

## 完成后下一步

xui 完成后 → 实现 **xgent_ui**（XGent 业务 UI：对话/工具/文件面板、状态栏、设置面板、确认弹窗），它依赖 xui 获取通用组件，订阅 agent 事件渲染，发送用户输入驱动 agent。
