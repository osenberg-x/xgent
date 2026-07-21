# UI 原型对齐差距与后续实现方案

> 对照 `doc/design/ui-prototype.html` 原型图与当前 `xgent_ui` 实现的差距分析。
> A-C 阶段 + 本轮修补（光标位置/pending-deny 态/Aborting/确认 diff/预览高亮/顶栏 caret）已落地（2026-07-20）。
> 剩余 D（markdown 渲染）未实现。

---

## 已完成（A-C + 修补 + E/F/G/H）

### 阶段 A：主题 token 补全
- `Theme` 加状态色 5 色（`st_pending/running/ok/fail/deny`）与语法高亮色 7 色（`kw/fn_/str_/num/ty/com/punc`）。
- `FILE_PANEL_W` 从 260 改 240（对齐原型 `grid-template-columns:240px 1fr`）。

### 阶段 B：文件树视觉与交互
- `spawn_file_panel` 加 `fp-head` 标题头（「资源管理器」+ 折叠按钮 ◀）。
- `spawn_entry` 重写：目录行 = row(箭头 ▸/▾ + 图标 📁/📂 + 名称)，文件行 = row(占位 + 图标 📄 + 名称)，箭头/图标/名称分离为独立 Text 子节点。
- `handle_dir_click` 展开/折叠时单独切换 `DirArrowMarker` 与 `DirIconMarker` 子节点文本（用 `ParamSet` 避免双 `&mut Text` query 冲突）。
- 选中态：`FileSelectedMarker` + `update_file_entry_style` 系统据选中/悬停设背景色（半透明 accent）。

### 阶段 C：对话区视觉
- `spawn_chat_panel` 加 `viewtabs`（💬 对话标签 + 右侧会话信息 `ConversationInfoMarker`）。
- 消息气泡加 role 行（头像 + 角色名）：用户蓝底圆「你」、助手 accent 圆「✦」。
- 输入框下方加 `input-meta` 快捷键提示栏（Ctrl+Enter 发送 / Esc 中断 / Ctrl+Shift+P 命令 / Ctrl+\ 分屏 + 右侧 tokenhint）。

### 修补（2026-07-20，本轮）
- **#4 流式光标位置**：`update_streaming_cursor` 从 `ConversationInfoMarker` 迁到 `CurrentAssistantText`（助手气泡正文末尾）；`accumulate_delta`/`finalize_on_done` 剥离末尾 ▋ 避免光标错位。
- **#1/#2 工具卡片 pending/deny 态**：`ToolCardMarker` 加 `tool_call_id`；新增 `update_tool_pending` 系统订阅 `ConfirmRequestMessage` 切 ⏸ pending 态；`update_tool_result` 据 `ToolResultMessage.denied` 切 ⊘ denied 态（不展开结果区）vs ✗ failed 态。`ToolResult` 加 `denied` 字段，executor 的 Denied/Deny 路径标记 `denied: true`。
- **#6 确认弹窗 diff**：`ConfirmRequest` 加 `old_content`/`new_content`；`Tool` trait 加 `preview_diff`（WriteFile 实现读旧文件）；`confirm_dialog` 重写为 modal 结构（head/body/foot + 行级 diff 公共前缀后缀算法）。
- **#7 文件预览元信息+高亮**：`file_panel` 加 `FilePreviewMetaMarker`（字节数 · 只读预览）；`.rs` 文件用 `xui::highlight` + `span_color_for` 渲染语法高亮，其余纯文本。
- **#8 顶栏 caret 下拉**：provider 标签改 Button（`ProviderButtonMarker`），点击打开设置面板切换 provider。
- **#10 Aborting tokenhint**：新增 `update_token_hint` 系统据 `ConversationStatus` 更新输入框右侧状态文本（含 `Aborting` → 中断中…）。

### 阶段 E：状态栏分段 + 状态点（已落地）
- `spawn_status_bar` 改为 row 容器：`StatusDotMarker`（7px 圆点）+ `ProviderTextMarker` + 分隔 + `ConvStatusMarker` + 分隔 + `TokenTextMarker` + spacer + 编码段。
- `update_status_segments` 更新各分段文本；`update_status_dot` 忙时 running 色 + 正弦脉冲，空闲 ok 色，出错 fail 色。

### 阶段 F：顶栏品牌 + caret（已落地）
- `spawn_top_bar`：`xgent ▾` 品牌 + `📦 {provider/model} ▾` provider Button + spacer + `＋新建会话` btn + `⚙` icon-btn。

### 阶段 G：右侧分屏预览头 + 语法高亮（已落地）
- `spawn_file_preview`：`fv-head`（📄 路径 + 元信息 + ✕ 关闭）+ `fv-body`（可滚动）。
- `handle_file_click` 非代码文件：设 `FilePreviewMetaMarker`（字节数 · 只读预览）+ `FilePreviewPathMarker`；`.rs` 内容用 `xui::highlight` 高亮。

### 阶段 H：确认弹窗 diff 渲染（已落地）
- `confirm_dialog` 重写为 modal（head 确认执行 + ✕ / body 工具名+路径+diff 增删色 / foot 拒绝 btn-danger + 允许 btn-primary）。
- diff 由 `ConfirmRequest.old_content`/`new_content` 提供，UI 用 `line_diff`（公共前缀/后缀）渲染。

---

## 待实现（D）

### 阶段 D：助手消息 markdown 渲染（代码块 + 语法高亮）

**差距**：助手消息当前是纯 `Text` 节点，原型图含 `<pre class="code">` 代码块（带 `code-head` 文件名 + 语言标签 + tree-sitter 语法高亮 span）、列表、加粗。

**方案**：
1. 新增 `crates/xgent_ui/src/markdown.rs`：轻量 markdown 解析器，把助手文本拆成 `Vec<MarkdownChunk>`：
   - `Paragraph(String)` — 普通段落
   - `CodeBlock { lang: String, content: String }` — ``` 围栏代码块
   - `InlineCode(String)` — `` `行内代码` ``
   - `ListItem(String)` — `- ` / `1. ` 列表项
   - `Bold(String)` — `**加粗**`
2. `chat_panel.rs` `finalize_on_done`：助手历史消息从单一 `Text` 改为 `Column` 容器，按 chunk spawn 子节点：
   - 段落 → `Text` 节点
   - 代码块 → `Node`（`pre.code` 样式：暗底 #11141a + 边框 + 圆角）+ `code-head`（📖 文件名 + 语言标签）+ 语法高亮 `Text`（复用 `xui::HighlightCache` + tree-sitter）
   - 行内代码 → `Text` 段落内带背景色 span（bevy 0.19 Text 富文本 spans）
3. 流式期间（`accumulate_delta`）只渲染纯段落文本，**Done 后整体高亮**——避免半截代码块高亮闪烁。
4. 代码块语法高亮复用编辑器的 `xui::highlight` + tree-sitter grammar（仅 Rust，对齐 D-06）。

**风险**：bevy 0.19 的 Text 富文本 spans 对混合样式（背景色/前景色/字体）支持有限，代码块行内高亮可能需每行一个 `Text` 节点（性能可接受，单条消息代码块行数有限）。

**工作量**：大（新模块 + 解析器 + 渲染重构）。

---

## 优先级建议

| 阶段 | 价值 | 工作量 | 状态 |
|:---|:---|:---|:---|
| D（markdown/代码块） | 高（助手消息可读性核心） | 大 | 待实现 |
| E（状态栏分段） | 中 | 中 | ✅ 已落地 |
| F（顶栏品牌+caret） | 低 | 中 | ✅ 已落地 |
| G（预览头+高亮） | 中 | 中 | ✅ 已落地 |
| H（确认弹窗 diff） | 中（安全交互） | 大 | ✅ 已落地 |

**剩余仅 D**：markdown 渲染是助手消息可读性的核心差距，建议优先实施。
