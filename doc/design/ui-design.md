# XGent UI 界面设计

> 状态：草案 v1 · 待评审
>
> 范围：MVP（仅 2D GUI）。3D 可视化（F-16）、宠物（F-15）、TUI（F-17）、Web（F-18）不在本文档范围。

---

## 1. 设计原则

| 原则 | 含义 |
|:---|:---|
| 对话为中心 | MVP 无内置编辑器，对话区是主交互区，占据屏幕最大面积。 |
| 键盘优先 | 所有操作有键盘快捷键；鼠标是辅助。参考 VSCode 体系（F-09）。 |
| 过程透明 | agent 的思考、工具调用、结果实时可见，不藏在后台。 |
| 渐进信息 | 只展示当前需要的信息；历史消息可滚动查看，不挤占屏幕。 |

---

## 2. 布局

### 2.1 总体结构

```
┌──────────────────────────────────────────────────────────┐
│ 顶栏（高 40px）                                           │
│  项目名 · provider/model 标签 · 新建会话 · 设置 ⚙         │
├────────────┬─────────────────────────────────────────────┤
│            │  [对话] [编辑器] [文件预览]    ← 视图切换标签  │
│ 文件面板    │ ┌─────────────────────────────────────────────┐ │
│（宽 240px）│ │  当前视图内容（flex:1）                      │ │
│ 可折叠      │ │  · 对话视图：消息列表 + 输入框（默认）     │ │
│            │ │  · 编辑器视图：多标签编辑器（P1，见下）    │ │
│            │ │  · 文件预览视图：只读文本（MVP 现有）       │ │
│            │ └─────────────────────────────────────────────┘ │
├────────────┴─────────────────────────────────────────────┤
│ 状态栏（高 24px）                                         │
│  provider/model · 会话状态 · token 指示                   │
└──────────────────────────────────────────────────────────┘
```

> P1 编辑器（F-11）上线后，对话主区顶部新增"视图切换标签条"（高 32px），支持对话/编辑器/文件预览三视图切换。MVP 阶段只有"对话"视图可见，标签条不渲染；编辑器视图随 F-11 上线启用。详见 `doc/design/editor-design.md` 2.1。

### 2.2 布局规则

| 区域 | 尺寸 | 行为 |
|:---|:---|:---|
| 顶栏 | 高 40px，宽 100% | 固定，不滚动 |
| 文件面板 | 宽 240px，高 flex:1 | 可折叠（`Cmd+B` 切换）；折叠后宽 0 |
| 对话主区 | flex:1，高 flex:1 | 始终可见，占据剩余空间 |
| 输入框 | 宽 100%，最小高 60px，最大高 200px | 输入增多时自动扩展 |
| 视图切换标签条 | 高 32px，宽 100%（P1 编辑器上线后） | 默认渲染"对话"标签；`Cmd+Shift+E` 切编辑器视图 |
| 状态栏 | 高 24px，宽 100% | 固定，不滚动 |

### 2.3 文件面板折叠

MVP 无编辑器，文件面板仅做只读预览。P1 编辑器（F-11）上线后，文件面板点击代码文件 → 切到编辑器视图打开该文件；点击图片/二进制 → 留在文件预览视图。大多数时间用户聚焦对话，文件面板可折叠以获得更大对话空间。

- 默认展开。
- `Cmd+B` 切换折叠/展开。
- 折叠状态持久化到项目配置。

### 2.4 焦点管理

| 焦点目标 | 进入方式 | 退出方式 |
|:---|:---|:---|
| 输入框 | 启动时自动聚焦；`Cmd+I` | `Esc`（仅在无活跃对话时移到消息列表） |
| 消息列表 | `Tab` 从输入框移出 | `Esc` 回到输入框 |
| 文件面板 | `Cmd+B` 展开时自动聚焦 | `Esc` 回到输入框 |
| 编辑器视图 | `Cmd+Shift+E`（P1 编辑器上线后） | `Cmd+Shift+D` 切回对话视图 / `Esc` |

---

## 3. 对话主区

### 3.1 消息类型与视觉

| 类型 | 来源 | 视觉 |
|:---|:---|:---|
| 用户消息 | `UserInputMessage` | 右对齐，`bubble_user` 背景色，圆角 |
| 助手消息 | `DeltaMessage` 流式累加 | 左对齐，`bubble_assistant` 背景色，圆角 |
| 工具调用卡片 | `ToolCallEvent` | 内联在助手消息后，带工具名 + 参数摘要 + 状态标识 |
| 工具结果 | `ToolResultEvent` | 附在对应工具调用卡片下方，折叠态只看摘要，展开看详情 |
| 错误消息 | `ErrorMessage` | `text_dim` 色 + 前缀图标，内联在对话流中 |

### 3.2 消息列表

- 垂直排列，新消息在底部，自动滚动到底部。
- 用户滚动向上查看历史时暂停自动滚动；新消息到达时显示"↓ 新消息"提示。
- MVP 用简单列容器；消息数超过 200 条时接入 `xui::VirtualList` 虚拟化。

### 3.3 输入框

- 多行输入（`xui::ChatInput::multiline()`），基于官方 `EditableText`。
- `Ctrl+Enter`（macOS `Cmd+Enter`）发送；`Enter` 换行。
- 空输入不发送。
- 发送后清空输入框，聚焦保持。
- 发送时 agent 正忙（`ConversationStatus != Idle`）则忽略，输入框边框闪红。

### 3.4 流式渲染

- `DeltaMessage` 到达时累加到当前助手消息节点的 `Text`，不重建列表。
- `DoneMessage` 到达时把当前节点固化为历史消息节点，新建空的当前节点。
- 流式期间显示光标动画（块状光标闪烁）。

### 3.5 中断

- `Esc` 发 `AbortMessage` 中断当前对话。
- 中断后助手消息追加"[已中断]"后缀。
- 状态栏显示"中断中…"。

---

## 4. 工具调用卡片

### 4.1 卡片结构

```
┌─────────────────────────────────────────┐
│ 🔧 ReadFile  · src/main.rs            ⏱ │  ← 工具名 + 参数摘要 + 状态
│  ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ │
│ ▸ 结果：32 行 · 点击展开                  │  ← 折叠态
└─────────────────────────────────────────┘
```

### 4.2 状态标识

| 状态 | 图标 | 含义 |
|:---|:---|:---|
| 待确认 | ⏸ 黄色 | `NeedsConfirmation`，等待用户决策 |
| 执行中 | ⏳ 旋转 | 正在执行 |
| 完成 | ✓ 绿色 | 执行成功 |
| 失败 | ✗ 红色 | 执行出错 |
| 已拒绝 | ⊘ 灰色 | 用户拒绝执行 |

### 4.3 确认弹窗

当工具策略为 `NeedsConfirmation` 时，弹出 overlay 确认框：

```
┌─────────────────────────────────────────┐
│  确认执行                                │
│                                         │
│  WriteFile 将写入: src/main.rs          │
│  ┌─────────────────────────────────┐    │
│  │ +use std::fs;                    │    │
│  │ -fn main() {                    │    │
│  │ +fn main() -> Result<()> {      │    │
│  │ ...                              │    │
│  └─────────────────────────────────┘    │
│                                         │
│              [拒绝]    [允许执行]         │
└─────────────────────────────────────────┘
```

- 弹窗为 modal overlay，居中，背景半透明遮罩。
- `Enter` 确认，`Esc` 拒绝。
- 有 diff 时展示 diff（绿增红删），无 diff 展示参数摘要。

---

## 5. 文件面板

### 5.1 文件树

- 从项目根遍历，按字母排序，目录优先。
- 点击目录展开/折叠。
- 点击文件 → 读取内容（tokio task 异步），在下方内容区展示。
- `.gitignore` 忽略的路径不展示（MVP 简单匹配 `target/`、`.git/`、`node_modules/`）。

### 5.2 文件内容预览

- 只读，等宽字体，行号显示。
- `FileChangedEvent` 到达时若当前展示的文件被修改，刷新内容。
- 长文件只渲染可见区域（虚拟滚动），MVP 简单截断到前 1000 行。

---

## 6. 顶栏

```
[项目名 ▾]  [openai / gpt-4o-mini ▾]              [新建会话] [设置 ⚙]
```

| 元素 | 行为 |
|:---|:---|
| 项目名 | 显示当前项目目录名；点击无操作（MVP） |
| provider/model 标签 | 点击 → 命令面板过滤到 provider 切换命令 |
| 新建会话 | 发 `NewSessionMessage`，清空对话区 |
| 设置 | 打开命令面板过滤到设置命令 |

顶栏极简，所有复杂操作经命令面板入口，避免顶栏按钮膨胀。

---

## 7. 状态栏

```
openai / gpt-4o-mini  ·  就绪  ·  ↑ 1.2k tokens
```

| 段 | 来源 | 更新时机 |
|:---|:---|:---|
| provider/model | `ProviderInfo` Resource | 切换 provider 时 |
| 会话状态 | `Conversation::status` | 每次 status 变化 |
| token 指示 | `DoneMessage` 的 usage 字段 | 每轮对话完成时 |

状态栏始终可见，提供"系统当前在做什么"的一行摘要。

---

## 8. 命令面板

### 8.1 布局

```
┌─────────────────────────────────────────┐
│ > 输入命令...                            │
├─────────────────────────────────────────┤
│  📁 new session        新建会话          │
│  🌐 switch to English  切换语言          │
│  ⚙  open settings      打开设置          │
│  📦 openai             切换到 OpenAI     │
│  📦 ollama             切换到 Ollama     │
└─────────────────────────────────────────┘
```

- 居中顶部 overlay，宽 500px。
- 输入即时模糊匹配命令 id 与 label。
- `↑↓` 导航，`Enter` 执行，`Esc` 关闭。

### 8.2 命令分类

| kind | 触发 | 示例 |
|:---|:---|:---|
| Action | `Cmd+Shift+P` | 新建会话、切换语言、打开设置 |
| File | `Cmd+P` | 文件快速打开（点击跳转到文件面板） |
| Provider | `Cmd+Shift+P` → 输入 provider 名 | 切换 provider/model |

---

## 9. 视觉规范

### 9.1 主题

MVP 仅暗色主题（`Theme::dark()`）。P1 加主题切换（K-01）。

| token | 色值 | 用途 |
|:---|:---|:---|
| `bg` | `rgba(25,28,33)` | 全局背景 |
| `panel` | `rgba(33,36,43)` | 面板/卡片背景 |
| `bar` | `rgba(20,23,28)` | 顶栏/状态栏 |
| `border` | `rgba(64,66,77)` | 边框/分隔线 |
| `text` | `#FFFFFF` | 主文本 |
| `text_dim` | `rgba(158,163,174)` | 次要文本 |
| `accent` | `rgba(92,158,235)` | 强调（选中、链接） |
| `bubble_user` | `rgba(51,92,143)` | 用户消息气泡 |
| `bubble_assistant` | `rgba(46,49,56)` | 助手消息气泡 |

### 9.2 字体

- 字号：14px 基准。
- 等宽字体：代码块、文件内容预览。
- MVP 用系统默认字体；`default_font` feature 提供嵌入式 fallback。

### 9.3 间距

| token | 值 | 用途 |
|:---|:---|:---|
| `XS` | 4px | 紧凑间距（图标与文字） |
| `SM` | 8px | 组件内间距 |
| `MD` | 12px | 面板内边距 |
| `LG` | 16px | 区块间间距 |
| `XL` | 24px | 大区块间距 |

### 9.4 圆角

- 气泡：6px
- 卡片：4px
- 按钮：4px
- 弹窗：8px

---

## 10. 交互状态机

### 10.1 会话状态

```
         UserInput                Delta
Idle ──────────────→ Thinking ────────→ Streaming
 ↑                                         │
 │                                         │ Done
 │                                         ↓
 │      ToolCall          ConfirmRequest
 │ Streaming ────────→ ToolRunning ───────→ Confirming
 │                        ↑           │
 │                        │  Decision │
 │                        └───────────┘
 │
 │  Done/Error/Abort
 └─────────────────────────────────────────
```

| 状态 | 状态栏显示 | 输入框行为 |
|:---|:---|:---|
| Idle | "就绪" | 可输入发送 |
| Thinking | "思考中…" | 禁用发送 |
| Streaming | "生成中…" | 禁用发送，`Esc` 可中断 |
| ToolRunning | "执行工具…" | 禁用发送 |
| Confirming | "等待确认" | 禁用发送，焦点移到确认弹窗 |
| Error | "出错" | 可输入重试 |
| Aborting | "中断中…" | 禁用 |

### 10.2 焦点切换

启动 → 输入框聚焦（`AutoFocus`）→ 用户输入 → 发送 → 焦点留在输入框（可 `Esc` 中断）→ 流式完成 → 焦点回输入框。

若 `ConfirmRequest` 到达 → 焦点移到确认弹窗 → 用户决策 → 焦点回输入框。

命令面板打开 → 焦点移到命令面板输入框 → 关闭 → 焦点回之前的焦点目标。

---

## 11. 快捷键表（MVP）

| 快捷键 | 动作 | 上下文 |
|:---|:---|:---|
| `Cmd/Ctrl+Shift+P` | 打开命令面板（动作模式） | 全局 |
| `Cmd/Ctrl+P` | 打开命令面板（文件模式） | 全局 |
| `Cmd/Ctrl+Enter` | 发送消息 | 输入框聚焦时 |
| `Enter` | 换行 | 输入框聚焦时 |
| `Esc` | 中断当前对话 / 关闭弹窗 / 关闭命令面板 | 全局 |
| `Cmd/Ctrl+B` | 切换文件面板 | 全局 |
| `Cmd/Ctrl+,` | 打开设置 | 全局 |
| `Cmd/Ctrl+I` | 聚焦输入框 | 全局 |
| `↑↓` | 消息列表滚动 / 命令面板导航 | 对应焦点时 |
| `Tab` | 焦点切换（输入框 → 消息列表 → 文件面板） | 全局 |

**P1 编辑器（F-11）上线后追加**：

| 快捷键 | 动作 | 上下文 |
|:---|:---|:---|
| `Cmd/Ctrl+Shift+E` | 切到编辑器视图 | 全局 |
| `Cmd/Ctrl+Shift+D` | 切回对话视图 | 全局 |
| `Cmd/Ctrl+S` | 保存当前 buffer（直接落盘不经工具） | 编辑器视图 |
| `Cmd/Ctrl+W` | 关闭当前标签（脏 buffer 弹窗确认） | 编辑器视图 |
| `Cmd/Ctrl+F` | 编辑器内查找 | 编辑器视图 |
| `Cmd/Ctrl+H` | 查找替换 | 编辑器视图 |
| `Cmd/Ctrl+Z` / `Cmd/Ctrl+Shift+Z` | undo / redo | 编辑器视图 |
| `Cmd/Ctrl+Tab` | 编辑器标签间循环切换 | 编辑器视图 |

详见 `doc/design/editor-design.md` 2.3。

---

## 12. i18n

- 所有用户可见字符串经 `xui::tr()` → `xgent_settings::Localizer` → fluent bundle。
- `.ftl` 资源内嵌（`include_str!`），按语言分目录：`locales/zh-CN/`、`locales/en-US/`。
- 语言切换经命令面板 → `Localizer::switch(lang)` → 发 `LanguageChanged` 事件 → 标记 UI 节点 dirty → 下帧重建。
- MVP 前期以中文为主，所有 key 必须有 `zh-CN` 翻译；`en-US` 可后续补。

---

## 13. 与架构的映射

| UI 设计概念 | 架构 crate/模块 | ECS 契约 |
|:---|:---|:---|
| 用户消息 | `xgent_ui::chat_panel` | `UserInputMessage` → agent |
| 助手流式消息 | `xgent_ui::chat_panel` | `DeltaMessage` → UI |
| 消息完成 | `xgent_ui::chat_panel` | `DoneMessage` → UI |
| 工具调用展示 | `xgent_ui::tool_panel`（计划中） | `ToolCallEvent` → UI |
| 确认弹窗 | `xgent_ui::confirm_dialog` | `ConfirmRequestMessage` ↔ `ConfirmDecisionMessage` |
| 会话状态 | `xgent_ui::status_bar` | `Conversation::status` Resource |
| 命令面板 | `xui::command_palette` + `xgent_ui::command_palette` | `PaletteTriggered` |
| 快捷键 | `xui::shortcuts` + `xgent_ui::shortcuts` | `HotkeyTriggered` |
| 文件面板 | `xgent_ui::file_panel` | `FileChangedEvent` → UI |
| 布局 | `xgent_ui::layout` | marker 组件 |
| 主题 | `xgent_ui::theme` | `Theme` Resource |
| 输入框 | `xui::input` + 官方 `EditableText` | `ChatInputSubmitted` → `UserInputMessage` |
| 编辑器视图（P1） | `xgent_ui::editor` + `xui::CodeEditor` | `EditorCommand` ↔ UI / `BufferSavedEvent` → daemon / `FileChangedEvent` → UI |

---

## 14. MVP 不做

| 不做 | 原因 | 何时做 |
|:---|:---|:---|
| Markdown 渲染 | 助手消息纯文本即可用 | P1 |
| 代码语法高亮 | 需 tree-sitter，B 阶段前定 D-06 | P1 |
| 内置编辑器 | F-11；MVP 不含 | P1（中等能力边界：多行+行号+undo+查找+tree-sitter 高亮，MVP 仅 Rust。详见 `doc/design/editor-design.md`） |
| 拖拽排序面板 | 收益低 | P2 |
| 多标签页 | 无编辑器无需求 | P1+编辑器 |
| 主题切换 | K-01 延后 | P1 |
| 成本统计面板 | F-12 | P1 |
| Git diff 查看 | F-10 | P1 |
