# UI 原型对齐差距与后续实现方案

> 对照 `doc/design/ui-prototype.html` 原型图与当前 `xgent_ui` 实现的差距分析。
> A-C 阶段已落地（2026-07-20），D-H 阶段为后续实现方案。

---

## 已完成（A-C）

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
- 流式光标：`update_streaming_cursor` 系统据 `ConversationStatus` 在会话信息文本末尾闪烁 `▋`（1Hz）。

---

## 待实现（D-H）

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
4. 代码块语法高亮复用编辑器的 `xui::HighlightCache` + tree-sitter grammar（仅 Rust，对齐 D-06）。

**风险**：bevy 0.19 的 Text 富文本 spans 对混合样式（背景色/前景色/字体）支持有限，代码块行内高亮可能需每行一个 `Text` 节点（性能可接受，单条消息代码块行数有限）。

**工作量**：大（新模块 + 解析器 + 渲染重构）。

### 阶段 E：状态栏分段 + 状态点

**差距**：当前 `status_bar.rs` 是单一 `StatusText` 文本节点拼接；原型图是分段布局（`status-dot` 圆点 + provider/model + 分隔 + 会话状态 + 分隔 + token/cost + spacer + `UTF-8·LF·Rust`），状态点忙时脉冲动画。

**方案**：
1. `spawn_status_bar` 改为 row 容器，spawn 多个分段子节点：
   - `StatusDotMarker`（小圆点 `Node`，7px 圆）+ `ProviderTextMarker` + 分隔 + `ConvStatusMarker` + 分隔 + `TokenTextMarker` + spacer + `EncodingTextMarker`（`UTF-8 · LF · Rust`）
2. `update_status_text` 拆为更新各分段：
   - `StatusDotMarker` 的 `BackgroundColor`：忙时 `st_running` + 脉冲（每帧 opacity 正弦或 `AnimationPlayer`），空闲 `st_ok`
   - `ConvStatusMarker` 文本：就绪/思考中/生成中/执行工具/等待确认/出错（已有 i18n key）
   - `TokenTextMarker`：`↑ 2.4k tokens · $0.003`（cost 需 agent 层提供，MVP 可只显示 token）
3. 状态点脉冲：用 `Time` 累计 + `Color::srgba` alpha 正弦，或 bevy `AnimationPlayer` + `AnimationClip`。

**工作量**：中。

### 阶段 F：顶栏品牌 + 下拉 caret + icon 按钮样式

**差距**：当前 `top_bar.rs` 是 `Button+Text` 简陋样式；原型图有 `xgent ▾` 品牌字样 + `📦 openai/gpt-4o-mini ▾` provider 下拉 caret + `＋新建会话` 按钮 + `⚙` icon 按钮（`tb-item` hover panel 底样式）。

**方案**：
1. `spawn_top_bar` 重排：
   - `tb-brand`：`xgent` + `▾` caret（Text，hover 时 panel 底）
   - `tb-provider`：`📦 {provider/model}` + `▾` caret（带边框 panel 底）
   - spacer
   - `btn`：`＋ 新建会话`
   - `icon-btn`：`⚙`（28px 方形，hover panel 底）
2. provider 下拉 caret 点击 → 打开 provider 切换菜单（MVP 可复用命令面板过滤 provider，或新增下拉 Popover）。
3. 样式：`tb-item` hover 态用 `Interaction::Hovered` 设 `BackgroundColor(theme.panel)`。

**工作量**：中（下拉菜单交互需额外组件，MVP 可先只做视觉 caret 不做实际下拉）。

### 阶段 G：右侧分屏预览头 + 语法高亮

**差距**：当前 `file_panel.rs` `spawn_file_preview` 预览区是无标题的纯 `Text` 节点；原型图是 `fv-head`（📄 路径 + 元信息 + ✕ 关闭）+ `fv-body`（`<pre class="code">` 带语法高亮）。

**方案**：
1. `spawn_file_preview` 重构为 `fv-head` + `fv-body`：
   - `fv-head`：row(📄 + `FvPathMarker` 路径 Text + `·` + `FvMetaMarker` 元信息 Text + spacer + ✕ 关闭按钮)
   - `fv-body`：可滚动容器，预览内容动态挂入
2. `handle_file_click` 非代码文件填充时：
   - 设 `FvPathMarker` 文本为文件名
   - 设 `FvMetaMarker` 文本为 `{字节数} · 只读预览`
   - 内容用 `xui::HighlightCache` 语法高亮（据扩展名选 grammar）
3. ✕ 关闭按钮点击 → `closeSideView`（设 `SideViewCollapsed=true` + `SideViewContent=None`）。

**工作量**：中。

### 阶段 H：确认弹窗 diff 渲染

**差距**：当前 `confirm_dialog.rs` 是单文本 + 两按钮；原型图是 `modal`（head「确认执行」+ ✕ / body 工具名 + 路径高亮 + diff 区增删改色 / foot 拒绝 + 允许按钮）。

**方案**：
1. **前置依赖**：`xgent_agent::ConfirmRequestMessage` 当前只有 `summary`，需扩展携带 diff 数据：
   - 加 `old_content: Option<String>` + `new_content: Option<String>`（或 `diff: Vec<DiffLine>`）
   - 由 `xgent_tools::executor` 在发起 `ConfirmRequest` 时填充（WriteFile 工具读旧内容 + 新内容）
2. `show_on_request` 重构为 `modal` 结构：
   - `modal-head`：「确认执行」+ ✕ 关闭
   - `modal-body`：`<strong>WriteFile</strong> 将写入文件：` + `path` 高亮（monospace + bar 底）+ `diff` 区（`diff` 容器 + 每行 `line add/del/ctx`，增绿/删红/灰 ctx）
   - `modal-foot`：`btn` 拒绝（Esc）+ `btn-primary` 允许执行（Enter）
3. diff 计算：UI 侧用简单的行级 diff（`similar` crate 或手写 LCS），或 agent 层直接传 `Vec<DiffLine>`。
4. 按钮样式：`btn-danger`（拒绝，红底）vs `btn-primary`（允许，accent 底）。

**风险**：跨层改动（agent → tools → ui），`ConfirmRequest` 结构变更影响序列化（IPC）。需确保 daemon 透传新字段。

**工作量**：大（跨层 + diff 算法）。

---

## 优先级建议

| 阶段 | 价值 | 工作量 | 建议 |
|:---|:---|:---|:---|
| D（markdown/代码块） | 高（助手消息可读性核心） | 大 | 优先，但可先做代码块不做完整 markdown |
| E（状态栏分段） | 中 | 中 | 次优，视觉提升明显 |
| F（顶栏品牌） | 低 | 中 | 可延后，当前功能可用 |
| G（预览头+高亮） | 中 | 中 | 次优，与 D 共用高亮基础设施 |
| H（确认弹窗 diff） | 中（安全交互） | 大 | 延后，跨层改动需评估 |

**推荐顺序**：D → G（共用语法高亮）→ E → F → H。
