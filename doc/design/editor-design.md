# XGent 内置编辑器设计文档

> 状态：草案 v1 · 待评审
>
> 范围：F-11 内置编辑器（P1）。基于 grill 会话决策落地，覆盖 UI 界面、架构边界、数据流、状态机、与 agent / 工具链 / 检索升级的协调。领域语言见 `CONTEXT.md` "编辑器（F-11，P1）" 小节。
>
> 与既有文档关系：本文档不重写 `ui-design.md` / `architecture.md` / `requirements.md`，只补充编辑器引入后的增量变更，并在文末列出对三份文档的具体修订点。

---

## 1. 设计决策摘要（grill 结论）

| 维度 | 决策 | 关键约束 |
|:---|:---|:---|
| 能力边界 | 中等：多行编辑 + 行号 + undo/redo + 查找替换 + tree-sitter 语法高亮 | 不含 LSP、不含 split view；完整 IDE 形态留后续 |
| 组件来源 | bevy_ui 自造为默认，调研并行 | `xui::CodeEditor` 可独立发布；egui 混合方案作 P1+ 备选 |
| agent 关系 | 双向：agent 可读编辑器状态 + 可驱动编辑器动作 | 状态经 @ 引用显式拉取，动作经 EditorCommand Event |
| agent 驱动安全 | 新增 UI-only Tier，默认 Approved | WriteFile 工具仍走 Write tier / NeedsConfirmation |
| 用户保存 | 直接 `fs::write`，不经 WriteFile 工具、不经确认 | 落盘后经 daemon 广播 peer.fileChanged |
| 布局焦点 | 对话主区 + 编辑器为可切换标签 | 兼容 `ui-design.md` 现有布局，不推翻"对话为中心" |
| D-06 grammar | P1 MVP 只内置 Rust 一种语言 grammar | 随二进制发布；不做按需下载 / lazy load |
| 多标签 | 多标签页，每个标签一个 EditorBuffer | 不含 split view（与中等边界一致） |
| 外部修改冲突 | 未脏静默重载 / 脏弹窗三选 | 协调 daemon 文件监听与本地 buffer |
| EditorState 桥接 | @ 引用语法显式请求 | 用户控制上下文边界，不主动注入 |
| 检索升级 | 调整 OQ-08：编辑器上线到 C，D 延后到 LSP 真接入 | 中等边界不含 LSP，原 C→D→E 路径分段 |

---

## 2. UI 界面

### 2.1 布局增量

现有布局（`ui-design.md` 2.1）保持不变：顶栏 + 文件面板（左，可折叠）+ 对话主区（右，flex:1）+ 状态栏。编辑器作为**对话主区内可切换的标签视图**引入，不另开区域。

```
┌──────────────────────────────────────────────────────────┐
│ 顶栏（高 40px）                                           │
│  项目名 · provider/model 标签 · 新建会话 · 设置 ⚙         │
├────────────┬─────────────────────────────────────────────┤
│            │  [对话] [编辑器] [文件预览]    ← 视图切换标签  │
│ 文件面板    │ ┌─────────────────────────────────────────┐  │
│（宽 240px）│ │                                          │  │
│ 可折叠      │ │  当前视图内容（flex:1）                  │  │
│            │ │  · 对话视图：消息列表 + 输入框            │  │
│            │ │  · 编辑器视图：多标签编辑器               │  │
│            │ │  · 文件预览视图：只读文本（现有）         │  │
│            │ └─────────────────────────────────────────┘  │
├────────────┴─────────────────────────────────────────────┤
│ 状态栏（高 24px）                                         │
└──────────────────────────────────────────────────────────┘
```

- 视图切换标签条放在对话主区顶部，高 32px，宽 100%。
- 默认视图为"对话"（保持现有 UX）。
- `Cmd+Shift+E` 切到编辑器视图；`Cmd+Shift+D` 切回对话视图（与 VSCode `Cmd+Shift+E` Explorer / `Cmd+Shift+D` Debug 对齐精神，但语义重定义为编辑器/对话切换）。
- 文件面板点击文件 → 若是代码文件，默认切到编辑器视图并打开该文件为新标签；若是图片/二进制，留在文件预览视图。

### 2.2 编辑器视图布局

```
┌─────────────────────────────────────────────────────────┐
│ [src/main.rs ×] [src/lib.rs ×] [src/ui/mod.rs ×]  ← 标签 │
├─────────────────────────────────────────────────────────┤
│  1 │ use bevy::prelude::*;                              │
│  2 │                                                     │
│  3 │ fn main() {                                        │
│  4 │     App::new().run();                              │
│  5 │ }                                                   │
│  ~ │                                                     │
├─────────────────────────────────────────────────────────┤
│ Ln 3, Col 5  ·  Rust  ·  ● 未保存           查找: [_]   │ ← 编辑器状态条
└─────────────────────────────────────────────────────────┘
```

| 元素 | 尺寸 / 行为 |
|:---|:---|
| 标签条 | 高 32px，标签宽自适应，最多展示可容纳数，溢出滚动；`×` 关闭；脏 buffer 显示 `●` |
| 行号列 | 宽 48px，右对齐，`text_dim` 色 |
| 代码区 | flex:1，等宽字体，虚拟滚动（大文件只渲染可见行） |
| 编辑器状态条 | 高 24px，光标位置 / 语言 / 脏标记 / 查找框 |

### 2.3 编辑器视图快捷键

| 快捷键 | 动作 |
|:---|:---|
| `Cmd/Ctrl+S` | 保存当前 buffer（直接 `fs::write`，不经确认） |
| `Cmd/Ctrl+W` | 关闭当前标签（脏 buffer 弹窗确认丢弃） |
| `Cmd/Ctrl+P` | 命令面板文件模式 → 在编辑器视图打开选中文件 |
| `Cmd/Ctrl+F` | 编辑器内查找 |
| `Cmd/Ctrl+H` | 查找替换 |
| `Cmd/Ctrl+Z` / `Cmd/Ctrl+Shift+Z` | undo / redo |
| `Cmd/Ctrl+Shift+E` | 切到编辑器视图 |
| `Cmd/Ctrl+Shift+D` | 切回对话视图 |
| `Cmd/Ctrl+Tab` | 编辑器标签间循环切换 |

### 2.4 外部修改冲突弹窗

当 daemon 推送 `FileChangedEvent` 且该文件当前有脏 buffer：

```
┌─────────────────────────────────────────┐
│  文件已被外部修改                         │
│                                         │
│  src/main.rs 在编辑器外被修改。           │
│  你有未保存的本地修改。                    │
│                                         │
│         [丢弃本地]  [保留本地]  [对比合并]  │
└─────────────────────────────────────────┘
```

- 未脏 buffer 检测到外部变更：静默重载，无弹窗。
- 三选：
  - 丢弃本地 → 重载磁盘内容到 buffer。
  - 保留本地 → 标记 buffer 为"本地优先"，下次保存将覆盖外部。
  - 对比合并 → 打开 diff 视图（MVP 可降级为并排只读 + 手动取舍）。

---

## 3. 架构边界

### 3.1 crate 归属

| 组件 | crate | 依赖 | 可发布性 |
|:---|:---|:---|:---|
| `CodeEditor` 裸件（多行/行号/undo/查找） | `xui` | bevy + xui_i18n | 可独立发布 |
| tree-sitter 语法高亮 | `xui`（作为 `CodeEditor` 的内置能力） | + tree-sitter + tree-sitter-rust | 可独立发布 |
| EditorBuffer / EditorState / EditorCommand | `xgent_ui` | xui + xgent_core + bevy | 业务层 |
| @ 引用解析器 | `xgent_ui::chat_panel`（输入预处理） | xgent_core + xgent_context | 业务层 |
| EditorContextProvider（把 @ 引用转 ContextChunk） | `xgent_context` | xgent_core + 注入 EditorState trait | 检索层 |
| EditorTool（UI-only Tier 工具：OpenFile/GoTo/...） | `xgent_tools` | xgent_core | 工具层 |
| EditorState trait（让 xgent_context 不依赖 xgent_ui） | `xgent_core` | 无 | 共享 |

**关键反转依赖**：`xgent_context` 不能依赖 `xgent_ui`（会成环）。EditorState 作为 trait 定义在 `xgent_core`，`xgent_ui` 实现该 trait，`xgent_context` 经 trait 查询。对齐 `xui_i18n::StringSource` 的反转依赖模式。

### 3.2 ECS 契约

| Event / Message | 方向 | 载荷 | 说明 |
|:---|:---|:---|:---|
| `EditorCommand` Event | agent → 编辑器 | `OpenFile{path,line}` / `GoTo{line,col}` / `SetSelection{range}` / `ScrollTo{line}` / `CloseTab{path}` | agent 经 EditorTool（UI-only Tier）发 |
| `FileChangedEvent` | daemon → 编辑器 | `FileChanged{path}` | 现有事件复用；编辑器订阅，触发冲突协调 |
| `BufferSavedEvent` | 编辑器 → daemon 桥接 | `{path}` | 用户保存后发，xgent_app 桥接转 IPC `fs.changed` 广播 |
| `EditorStatePolled` Resource | 编辑器 → context | 只读视图 | ContextProvider 查询，不事件化 |

### 3.3 数据流：用户保存

```
用户按 Cmd+S
  → 编辑器系统读 EditorBuffer.dirty
  → fs::write(path, buffer.text)          ← 直接落盘，不经 WriteFile 工具
  → buffer.dirty = false
  → 发 BufferSavedEvent{path}
  → xgent_app 桥接 → IPC daemon.fs.changed
  → daemon 广播 peer.fileChanged 给同项目其他客户端
  → 其他客户端 FileChangedEvent → 未脏静默重载
```

### 3.4 数据流：agent 驱动编辑器

```
agent 决定"打开 src/main.rs 并跳到第 42 行"
  → tool_call: EditorTool::OpenFile{path, line}
  → ToolExec 查 policy → UI-only Tier → 默认 Approved（无确认）
  → 执行发 EditorCommand::OpenFile{path, line}
  → 编辑器系统订阅 → 切到编辑器视图 + 打开标签 + 滚动到行
  → ToolResult 回灌 agent："已打开 src/main.rs:42"
```

### 3.5 数据流：@ 引用拉取编辑器状态

```
用户在输入框输入 "看一下 @cursor 这个函数有没问题"
  → chat_panel 输入预处理：解析 @cursor
  → 发 ContextQuery{ kind: CursorAt } 给 ContextProvider
  → ContextProvider 查 EditorState trait
      → 当前活跃 buffer 路径 + 光标行 + 所在符号（tree-sitter AST 查询）
  → 组装 ContextChunk::EditorState{ path, symbol, line, source_excerpt }
  → 注入 LLM context
  → 用户消息文本中的 @cursor 替换为展示标记（不发原文给 LLM）
```

### 3.6 数据流：外部修改冲突协调

```
daemon 文件监听 → FileChanged{path} → FileChangedEvent
  → 编辑器系统查 EditorBuffer{path}.dirty
  → 未脏：静默重载（fs::read → 替换 buffer.text → 重置 undo 栈 → dirty=false）
  → 脏：弹外部修改冲突弹窗 → 用户三选
      → 丢弃本地：同未脏路径
      → 保留本地：标记 buffer.local_preferred=true，下次保存覆盖
      → 对比合并：打开 diff 视图（MVP 降级为并排只读）
```

---

## 4. 状态机

### 4.1 EditorBuffer 状态

```
        open file (read)
Clean ────────────────────→ Dirty
  ↑                           │
  │                           │ 外部修改
  │ save (fs::write)          ↓
  │                      ConflictDetected
  │                           │
  │                           ├─ 丢弃本地 ─→ Clean（重载）
  │                           ├─ 保留本地 ─→ Dirty(LocalPreferred)
  │                           └─ 对比合并 ─→ Dirty（用户手动取舍后）
  │
  └──────────────────────────
```

- `Clean`：buffer 与磁盘一致。
- `Dirty`：有未保存本地修改。
- `ConflictDetected`：脏 buffer + 外部修改到达，等待用户决策。
- `Dirty(LocalPreferred)`：用户选保留本地，下次保存覆盖外部。

### 4.2 编辑器视图状态

| 状态 | 进入 | 退出 |
|:---|:---|:---|
| Hidden | 默认（对话视图激活） | `Cmd+Shift+E` / 文件面板点击代码文件 |
| Active | Hidden + 切换触发 | 切回对话视图 / `Cmd+Shift+D` |
| ConflictModal | Active + ConflictDetected | 用户三选决策 |

---

## 5. xui::CodeEditor 组件设计

### 5.1 模块职责

`xui::CodeEditor` 是通用代码编辑器裸件，纯依赖 bevy + xui_i18n + tree-sitter + tree-sitter-rust。不依赖任何 `xgent_*`。

提供：
- 多行文本编辑（基于官方 `EditableText` 扩展，处理多行 + 滚动）
- 行号列
- undo/redo 栈（组件内持久化）
- 查找替换
- tree-sitter 语法高亮（按 grammar 渲染富文本 span）
- 光标 / 选区视觉
- 虚拟滚动（大文件只渲染可见行，复用 `xui::VirtualList` 思路）

不提供：
- 多标签页（业务层 `xgent_ui::editor` 管理）
- 文件 IO（业务层负责 `fs::read` / `fs::write`）
- LSP / 诊断 / 跳转（完整能力边界，留后续）
- split view

### 5.2 关键类型（草签）

```rust
// crates/xui/src/code_editor.rs

#[derive(Component)]
pub struct CodeEditor {
    pub language: Language,           // Rust（MVP 唯一变体）
    pub readonly: bool,
    pub tab_size: u32,
    // undo 栈 / 选区 / 光标 由内部组件持有
}

pub enum Language { Rust }   // MVP 唯一

#[derive(Event)]
pub struct EditorSaveRequested { pub entity: Entity }  // Cmd+S 触发

#[derive(Event)]
pub struct EditorDirtyChanged { pub entity: Entity, pub dirty: bool }

pub struct CodeEditorPlugin;
impl Plugin for CodeEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            handle_editor_input,    // 光标/删除/IME 经官方 EditableText
            handle_editor_keys,     // Cmd+S / Cmd+F / Cmd+H / undo redo
            update_syntax_highlight, // tree-sitter 增量解析 → span
            render_editor,          // 行号 + 代码 + 光标
        ));
    }
}
```

### 5.3 tree-sitter 集成

- 依赖 `tree-sitter` + `tree-sitter-rust`（grammar 随二进制编译入）。
- 增量解析：buffer 改动时复用上一次 AST，只重解析受影响区间。
- 高亮：遍历 AST 节点 → 按节点类型映射到 span 样式（关键字/字符串/注释/函数名/...）→ 渲染为带颜色的 Text span。
- 性能：大文件（>10k 行）增量解析 O(改动量) 而非 O(全文件)。

### 5.4 Cargo.toml 增量

```toml
# crates/xui/Cargo.toml（增量）
[dependencies]
# 既有：bevy, xui_i18n
tree-sitter = "0.23"
tree-sitter-rust = "0.23"   # grammar 随二进制入
ropey = "1.6"                # 文本 rope：大文件 O(log n) 行/字符访问
```

注：版本以实际发布时最新稳定为准，上述版本号为占位。

### 5.5 文本数据结构（Rope）

`TextEditor` 内部文本载体为 `ropey::Rope`（非 `Vec<String>` 行表）：
- 行/字符访问 O(log n)，大文件（>10k 行）不再 O(n) 全文拷贝。
- cheap clone（O(1)），未来 undo 栈可存 `Rope` 快照而非 `String`。
- `spans_for_line` 用 `rope.line_to_byte(row)` 定位行首，`rope.get_line(row)` 取行切片。
- 与 `bevy::text::EditableText` 的关系：`EditableText` 是用户输入入口（IME/光标），
  `update_syntax_highlight` 系统从 `EditableText` 同步文本到 `rope`（可编辑态），
  或由业务层直接写 `rope`（只读虚拟化态，文件读取后）。`rope` 是渲染与解析的权威源。

**不引入 tree-house / helix-core**：二者均 MPL-2.0，作为库依赖虽不触发文件级 copyleft，
但 XGent 选择全 MIT 依赖栈（ropey MIT + tree-sitter MIT + tree-sitter-rust MIT），
代价是自造 Transaction/History/Selection/Movement（线性 undo 栈 + 单光标，够中等能力边界）。

### 5.6 渲染架构（单层 Text 流式布局）

放弃"每逻辑行一个绝对定位行容器"模型——该模型在长行软换行时崩溃：
一逻辑行被 parley 布局成多个视觉行，固定行高的行容器装不下，
溢出到下一逻辑行造成覆盖（实测 `text_layout_h=48 > container_h=44`）。

**新模型：单层 Text 流式布局**：
- `VirtualContentMarker` 占位节点撑高滚动范围（`height = line_count × line_height`）
- 其下挂一个 `VirtualTextMarker`（Text 节点），内容是可见行区间 `[start, end)`
  的所有行拼接（行间 `\n`），带 TextSpan 高亮
- Text 节点挂 `LineHeight::Px(line_height)`，parley 给每行精确像素高度——
  软换行的视觉行也在该行行盒内，不溢出到下一逻辑行。**这是消除行间覆盖的关键**
- Text 节点 `position: absolute, top = -(start × line_height)`，让可见行对齐视口顶部
- 视口外内容靠 buffer 容器 `overflow: Hidden` 裁剪

参考 helix-tui `Paragraph` widget + `WordWrapper`：单层文本流式布局，
软换行由布局引擎处理，行高由 `LineHeight` 精确控制，无逐行独立容器。

性能：只渲染可见行区间（视口高度/行高 + 2×overscan），内存 O(可见行)。
滚动时若区间不变则只更新 `top` 偏移，区间变化才重建 TextSpan 子树。

---

## 6. xgent_ui::editor 模块设计

### 6.1 模块职责

XGent 业务编辑器层，依赖 `xui::CodeEditor` + `xgent_core` + `xgent_agent`。

- 多标签页管理（EditorBuffer 集合）
- 文件 IO（`fs::read` / `fs::write`，tokio task 异步）
- 外部修改冲突协调（订阅 `FileChangedEvent`）
- `EditorState` Resource（impl `xgent_core::EditorState` trait，供 ContextProvider 查询）
- `EditorCommand` Event 订阅与执行
- 视图切换（对话/编辑器/文件预览）
- @ 引用解析（输入预处理）

### 6.2 目标文件结构（增量到 step11）

```
crates/xgent_ui/src/
├── editor/
│   ├── mod.rs               # EditorPlugin + 视图切换
│   ├── buffer.rs            # EditorBuffer 组件
│   ├── tabs.rs              # 多标签页管理
│   ├── io.rs                # 文件读写（tokio task）
│   ├── conflict.rs          # 外部修改冲突协调 + 弹窗
│   ├── state.rs             # EditorState Resource（impl trait）
│   ├── command.rs           # EditorCommand Event 订阅
│   └── at_syntax.rs         # @ 引用解析
```

### 6.3 EditorState trait（放 xgent_core）

```rust
// crates/xgent_core/src/editor.rs

/// 编辑器状态只读视图，供 ContextProvider 查询。
/// 定义在 xgent_core，xgent_ui 实现，xgent_context 经 trait 调用——
/// 避免 xgent_context 依赖 xgent_ui（成环）。
pub trait EditorState: Send + Sync {
    /// 当前活跃 buffer 的路径
    fn active_path(&self) -> Option<&Path>;
    /// 当前光标位置（行，列）
    fn cursor(&self) -> Option<(usize, usize)>;
    /// 当前选区文本（若有）
    fn selection(&self) -> Option<&str>;
    /// 指定路径 buffer 是否存在 + 是否脏
    fn buffer_status(&self, path: &Path) -> Option<BufferStatus>;
}

pub struct BufferStatus {
    pub open: bool,
    pub dirty: bool,
}

/// ContextProvider 查询 @ 引用的载荷
pub enum EditorQuery {
    File { path: PathBuf },
    Cursor,        // 当前光标所在符号/行
    Selection,     // 当前选区文本
}
```

### 6.4 EditorTool（UI-only Tier，放 xgent_tools）

```rust
// crates/xgent_tools/src/editor_tool.rs

/// agent 驱动编辑器的工具集，UI-only Tier，默认 Approved
pub enum EditorTool {
    OpenFile { path: PathBuf, line: Option<usize> },
    GoTo { line: usize, col: Option<usize> },
    SetSelection { start: usize, end: usize },
    ScrollTo { line: usize },
    CloseTab { path: PathBuf },
}

impl Tool for EditorTool {
    fn id(&self) -> &str { "editor.*" }
    fn tier(&self) -> ToolTier { ToolTier::UiOnly }   // 新增变体
    fn approval_for(&self, _input: &Value) -> Option<ToolTier> { Some(ToolTier::UiOnly) }
    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolResult {
        // 不实际执行 IO，只发 EditorCommand Event 给 ECS
        // ctx.emit(EditorCommand::from(self, &input))
        // ToolResult 回灌："已打开 X:Y" / "已跳转"
    }
}
```

`ToolTier` 枚举新增 `UiOnly` 变体，与 `Read`/`Write`/`Exec` 并列。`resolve_policy` 对 `UiOnly` 默认返回 `Approved`（不走 NeedsConfirmation）。

### 6.5 @ 引用解析

```rust
// crates/xgent_ui/src/editor/at_syntax.rs

/// 解析输入文本中的 @ 引用，替换为占位标记，并收集 ContextQuery
pub fn parse_at_references(input: &str) -> (String, Vec<EditorQuery>) {
    // @file:<path>      → EditorQuery::File
    // @cursor           → EditorQuery::Cursor
    // @selection        → EditorQuery::Selection
    // 其他 @xxx 不识别，按原样保留
}
```

MVP 三种 @ 引用：
- `@file:src/main.rs` — 拉取该文件内容作为上下文。
- `@cursor` — 拉取当前光标位置所在符号 + 周边若干行。
- `@selection` — 拉取当前选区文本。

不识别的 `@xxx` 原样保留，不做补全 UI（P1+ 再加）。

---

## 7. 检索升级路径调整（OQ-08 修订）

原 OQ-08（`requirements.md` 9 节）：编辑器上线后依次升级 C → D → E。

修订：编辑器上线只触发到 **C（向量 RAG）**。D（LSP/AST）延后到 LSP 真正接入时（即编辑器从中等边界升级到完整边界时）。E（混合检索）跟随 D。

理由：中等能力边界不含 LSP，D 阶段依赖 LSP。把 D 与编辑器中等边界解耦，避免编辑器上线就必须接 LSP 的硬依赖。tree-sitter AST 在中等边界已可用，作为 D 的弱化版可被 C 阶段复用（结构化切片喂向量库）。

| 阶段 | 触发 | 能力 | 与编辑器关系 |
|:---|:---|:---|:---|
| A | MVP | 无索引·按需读取 | 无编辑器 |
| B | P1 前 | tree-sitter repo map | 无编辑器 |
| C | **编辑器上线** | 向量 RAG（tree-sitter AST 切片喂向量库） | 编辑器提供 buffer/光标作为查询源 |
| D | **LSP 接入时**（编辑器升完整边界） | LSP/AST 深检索 | 编辑器内嵌 LSP 客户端 |
| E | D 后 | 混合检索 | 融合 C+D |

---

## 8. 对既有文档的修订点

### 8.1 `doc/design/requirements.md`

- **F-11** 描述更新：从"完整的代码编辑器（查看 + 编辑）"改为"中等能力边界：多行编辑 + 行号 + undo/redo + 查找替换 + tree-sitter 语法高亮（MVP 仅 Rust）。不含 LSP、不含 split view。LSP 接入等编辑器升级完整边界时再考虑"。
- **OQ-02** 状态更新：从"是否复用现有组件待实现时评估"改为"基于 bevy_ui 自造（xui::CodeEditor），egui 混合方案作 P1+ 备选；中等能力边界已重开自造路径"。
- **OQ-08** 状态更新：见本文档第 7 节，C 触发于编辑器上线，D 延后到 LSP 接入。

### 8.2 `doc/design/architecture.md`

- **4. crate 划分**：xui 的封装范围表新增 `K-08 CodeEditor（多行/行号/undo/查找/tree-sitter 高亮）` 行，阶段标 P1。xgent_ui 依赖列表不变（xui 已含）。
- **6.2 工具抽象**：`ToolTier` 枚举描述新增 `UiOnly` 变体；内置工具清单新增 `EditorTool`（UI-only Tier）。
- **6.3 上下文检索抽象**：`ContextProvider` trait 描述补充"实现 `EditorContextProvider` 查询 `EditorState` trait 处理 @ 引用"。`EditorState` trait 定义在 `xgent_core`，反转依赖避免成环。
- **13. 待决策点 D-06**：状态从"待定（B 阶段前）"改为"已决策（P1 编辑器 MVP 阶段只内置 Rust 一种语言 grammar，随二进制发布；不做按需下载/lazy load；多语言扩展待后续评估）"。

### 8.3 `doc/design/ui-design.md`

- **2. 布局**：2.1 总体结构图主区顶部新增"视图切换标签条 [对话] [编辑器] [文件预览]"，高 32px。
- **2.4 焦点管理**：新增焦点目标"编辑器视图"，进入 `Cmd+Shift+E`，退出 `Cmd+Shift+D` 或 `Esc`（回对话）。
- **11. 快捷键表**：追加编辑器视图快捷键（见本文档 2.3）。
- **13. 与架构的映射**：新增行"编辑器视图 | `xgent_ui::editor` + `xui::CodeEditor` | `EditorCommand` ↔ `BufferSavedEvent` / `FileChangedEvent` → UI"。
- **14. MVP 不做**：移除"内置编辑器 F-11"行（已在 P1 范围，但本表是 MVP 不做清单，编辑器本就不在 MVP；条目可保留作 P1 指针，备注"中等能力边界，详见 editor-design.md"）。

---

## 9. 验证方法（P1 实现时）

1. **编译检查**：`cargo check -p xui`（含 tree-sitter 依赖）、`cargo check -p xgent_ui`。
2. **独立性**：`cargo tree -p xui` 输出含 tree-sitter / tree-sitter-rust，不含任何 `xgent_*`。
3. **编辑器基础测试**：spawn CodeEditor，输入文本，断言 undo/redo、查找替换、行号渲染。
4. **语法高亮测试**：加载 Rust 代码，断言关键字/字符串/注释 span 颜色正确。
5. **多标签测试**：打开多文件，切换标签，断言各 buffer 独立 dirty 状态。
6. **保存测试**：编辑 + Cmd+S，断言文件落盘、dirty 清零、daemon 广播 peer.fileChanged。
7. **冲突协调测试**：模拟外部修改（直接 fs::write），未脏断言静默重载；脏断言弹窗出现且三选行为正确。
8. **@ 引用测试**：输入 `@file:src/main.rs` / `@cursor` / `@selection`，断言 ContextQuery 发出且 LLM context 含对应 chunk。
9. **agent 驱动测试**：mock agent tool_call `EditorTool::OpenFile`，断言编辑器视图切换 + 标签打开 + 滚动到行，且无确认弹窗（UI-only Tier Approved）。
10. **Rust grammar 嵌入测试**：断言二进制含 tree-sitter-rust grammar，离线可解析。

---

## 10. 未决与后续

- **多语言扩展**：Rust grammar 验证编辑器可行后，评估扩展到 TS/JS/Python/JSON/TOML/Markdown。届时重定 D-06 多语言策略（按需下载 vs lazy load vs 全内置）。
- **egui 混合方案调研**：并行评估 `egui_code_editor` crate 与 Bevy 集成成本，作为中等边界后续升级完整能力边界时的备选路径。
- **完整能力边界**：LSP 接入（跳转/重命名/hover/诊断）+ split view 等编辑器升完整 IDE 形态时考虑，触发检索 D 阶段。
- **diff 视图**：外部修改冲突"对比合并"选项 MVP 可降级为并排只读 + 手动取舍，真正的 3-way merge 留后续。
- **@ 引用补全 UI**：MVP 不做输入 `@` 时的补全弹窗，后续加。
