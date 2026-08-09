# XGent UI v3 — 实施任务清单

> 来源：`doc/plans/v3-ui-migration.md`（经 5 轮 review 修订）
> 约定：每个 Task 对应方案的一个 Phase，内含可独立验证的子任务。任务间按依赖顺序串行，✅ 标记验收通过。

---

## Task 0 — 地基：令牌迁移 + 图标系统 + BSN 初始化

**先行依赖**：无（即可开始）
**预计影响**：`xgent_ui/src/theme.rs` 全域改动，`xui/Cargo.toml` 新依赖
**blocking**：Task 1-4 均依赖此 Task

### 子任务

- [ ] **T0.1** 在 `Theme` 新增 brand/warm/semantic 色系字段（`brand`, `brand_hover`, `brand_press`, `brand_bg`, `brand_glow`, `brand_2`, `line_focus`, `warm`, `warm_hover`, `warm_bg`, `warm_glow`, `warm_line`, `ok_bg`, `pend_bg`, `fail_bg`），`Theme::dark()` 赋 v3 令牌值
- [ ] **T0.2** 全局替换 `theme.accent` → `theme.brand`，`theme.accent_bg` → `theme.brand_bg`（两步法：先加后删）
- [ ] **T0.3** 新增 `typo`、`motion`、`radius` 模块常量；`space` 模块扩展 S1-S10；尺寸常量改 STATUS_BAR_H=32、SIDEBAR_W=242
- [ ] **T0.4** `xui/Cargo.toml`：bevy features 加 `"bevy_scene"`，新增 `bevy_resvg = "2.5"` + `pulldown-cmark` 依赖
- [ ] **T0.5** `xgent_ui/Cargo.toml`：bevy features 加 `"bevy_scene"`
- [ ] **T0.6** 复制 `doc/design/ui-v3-icons/*.svg` → `crates/xui/assets/icons/`
- [ ] **T0.7** 新建 `crates/xui/src/icon.rs`：实现 `IconKind` 枚举（32 个）+ `asset_path()` + `spawn_icon()` 场景函数
- [ ] **T0.8** 实现 `IconPlugin`：注册 `SvgPlugin`
- [ ] **T0.9** 全局 emoji 盘点（`grep` 列出待替换点，不替换）
- [ ] **T0.10** `cargo check --workspace` 通过

### 验收标准

- `cargo check` 无错误
- `spawn_icon()` 测试通过（spawn 32 个图标均不 panic）
- `theme.rs` 中 `accent` / `accent_bg` / `bar` / `bubble_user` / `bubble_assistant` 字段已删除或标记 deprecated

---

## Task 1 — 骨架：导航收敛 + 内容标签统一

**先行依赖**：Task 0
**预计影响**：`chat_panel.rs` ViewTabs 删除，新增 `content_tabs.rs`，`editor/` + `terminal/` 标签层重构
**blocking**：Task 2 需 ContentTabsBar 容器就位

### 子任务

- [ ] **T1.1** 新建 `crates/xgent_ui/src/content_tabs.rs`：`TabKind`, `ContentTab`, `ContentTabs` Resource, `ContentTabRequest` Event
- [ ] **T1.2** `ContentTabsPlugin`：注册 Resource + Event + spawn/update 系统
- [ ] **T1.3** `layout.rs`：SideView 顶部新增 `ContentTabsBarMarker` 空容器
- [ ] **T1.4** 实现 `spawn_content_tabs_bar()`（BSN 场景函数：标签栏 + 工具区）
- [ ] **T1.5** 实现 `render_content_tabs()` 更新系统（响应 `ContentTabs.changed()`）
- [ ] **T1.6** `chat_panel.rs`：删除 `spawn_views_tab_bar()` 及 ViewTabs 相关代码，对话面板直接从消息列表开始
- [ ] **T1.7** `editor/tabs.rs`：删除标签栏渲染层，保留 `EditorTabs` 数据模型与事件；关闭标签 → `ContentTabRequest::Close`
- [ ] **T1.8** `editor/mod.rs`：删除 EditorTabs 渲染引用；加 ContentTabs 桥接（active → pane 切换）
- [ ] **T1.9** `terminal/mod.rs`：删除 `TerminalTabBar` 渲染；保留 PTY 逻辑；加 ContentTabs 桥接
- [ ] **T1.10** `activity_bar.rs`：新增 Search 项，实现 1.3 联动状态机（6 行操作表 + 4 行 active 同步表），活动指示条改 `brand` 色
- [ ] **T1.11** `layout.rs`：状态栏高度 32px，顶栏底色 `bg`，侧栏宽度 242px
- [ ] **T1.12** `shortcuts.rs`：注册 Ctrl+Shift+F → search 快捷键
- [ ] **T1.13** `cargo check --workspace` 通过 + 编辑器/终端/预览功能回归

### 验收标准

- 活动栏 5 项图标 + 联动状态机逐项验证（1.3 表 11 条规则）
- 内容标签三种类型（file/preview/term）互斥切换正确
- dirty 关闭确认弹窗仍然工作
- PTY 终端多标签交互不受影响

---

## Task 2 — 陪伴：陪伴锚点 + 输入卡回声

**先行依赖**：Task 0, Task 1
**预计影响**：新增 `companion.rs`，状态栏左侧 + 输入卡工具栏

### 子任务

- [ ] **T2.1** 新建 `crates/xgent_ui/src/companion.rs`
- [ ] **T2.2** 定义 `AgentState` 枚举（Idle/Think/Tool/Pend/Fail）+ `CompanionState` Resource（pet_enabled 默认 true）
- [ ] **T2.3** 定义 `AgentStateChanged` Event
- [ ] **T2.4** 实现 `sync_agent_state()`：监听 `ConversationStatusChanged` + `ToolCallMessage`，按 8→5 映射表驱动（含 Aborting 500ms 回落）
- [ ] **T2.5** 实现 `companion_anchor()` BSN 场景函数（状态栏胶囊：暖色背景 + 字形 + 文字 + 色点）
- [ ] **T2.6** 实现 `update_companion_anchor()`：状态变更时切换字形/颜色/动画
- [ ] **T2.7** 实现 `spawn_input_echo()` + `spawn_input_hints()` BSN 场景函数（输入卡工具栏左侧回声 + 4 组 kbd 提示）
- [ ] **T2.8** 实现 `update_input_echo()` / `update_input_card_border()`（Pend/Fail 态边框联动）
- [ ] **T2.9** 实现宠物开关按钮 + `toggle_pet()`（点击切换 `pet_enabled`，胶囊退暖/回声退化，Toast 提示）
- [ ] **T2.10** `CompanionPlugin` 注册所有 system
- [ ] **T2.11** `cargo check --workspace` 通过

### 验收标准

- 陪伴锚点五态切换正确（idle/think/tool/pend/fail 逐态验证）
- Streaming 态显示"思考中"；Aborting 态显示"中断中"后 500ms 回到 Idle
- 宠物开关降级：胶囊退暖纯灰、字形 CpOff、回声同步退化
- 输入卡回声与状态栏胶囊同步变色
- 输入卡边框 Pend/Fail 态变色正确

---

## Task 3 — 仪表：状态栏运行时仪表

**先行依赖**：Task 0, Task 2（陪伴胶囊需就位）
**预计影响**：`status_bar.rs` 重写，`xgent_app/ipc_client.rs` 加 RTT

### 子任务

- [ ] **T3.1** 扩展 UI 侧 `TokenUsage` 为 `{prompt: u64, completion: u64}`（对齐 core 命名），修改 `track_token_usage`
- [ ] **T3.2** 新增 `CostEstimate` Resource + 内置模型价格表（方案 B，MVP）
- [ ] **T3.3** `xgent_app/ipc_client.rs`：新增 RTT 测量（复用最近请求往返时间），UI 侧新增 `DaemonStatus` Resource
- [ ] **T3.4** 实现 BSN 场景函数：`provider_pill()`, `token_meter()`, `cost_meter()`, `daemon_meter()`, `session_meter()`
- [ ] **T3.5** 重写 `spawn_status_bar()`（BSN 布局：左侧陪伴锚点 + 运行时信息，右侧会话 + 文件元信息）
- [ ] **T3.6** 重写 `update_status_segments()`：更新各仪表文本/色点/状态
- [ ] **T3.7** 会话信息仪：从 `Conversation.id` 取低 6 位 hex，从消息数推导轮次
- [ ] **T3.8** 将 Phase 2 陪伴胶囊集成到状态栏左侧
- [ ] **T3.9** `cargo check --workspace` 通过

### 验收标准

- token ↑↓ 数值正确累加（发送多轮对话验证）
- 成本估算对已知模型正确、未知模型显示 `≈ —`
- daemon 连接态色点 ok=绿 / fail=红，首帧显示 `daemon —`
- 会话 ID 和轮次随对话更新正确

---

## Task 4 — 对话：排版分级 + 留白 + Markdown

**先行依赖**：Task 0
**预计影响**：`chat_panel.rs` 排版调整，新增 `xui/src/markdown.rs`

### 子任务

- [ ] **T4.1** `chat_panel.rs`：对话流消息块间距 `S5→S6`，最大宽度 820px 居中
- [ ] **T4.2** `chat_panel.rs`：角色名/时间戳字号对齐 typo 令牌（FS_H3/FS_META）
- [ ] **T4.3** 新建 `crates/xui/src/markdown.rs`：`parse_markdown()`（pulldown-cmark → `Vec<MarkdownBlock>`）
- [ ] **T4.4** 实现 `spawn_markdown()`：块级元素渲染（段落/代码块/有序列表/无序列表）
- [ ] **T4.5** 代码块：`deep` 底 + 头部（图标+标签+语言+复制）+ 主体（tree-sitter Rust 高亮 / 非 Rust fallback）
- [ ] **T4.6** 行内样式渲染：code/bold/link
- [ ] **T4.7** `chat_panel.rs`：修改 `finalize_on_done`，从纯文本改为 Markdown spawn
- [ ] **T4.8** 流式期间保持纯文本 + 流式光标（现有逻辑不动）
- [ ] **T4.9** 流式光标：从 Unicode 字符改为 6×14px `BackgroundColor(st_running)` 节点，1Hz 闪烁
- [ ] **T4.10** 复制按钮：点击写入剪贴板 + toast
- [ ] **T4.11** `cargo check --workspace` 通过

### 验收标准

- 用户消息和助手消息排版符合 typo 令牌（字号/行高/间距）
- 代码块 Rust 高亮正确、非 Rust 纯文本 fallback
- 流式期间光标闪烁正常、完成后消失
- Markdown 段落/列表/粗体/行内代码渲染正确

---

## Task 5 — 信号：待确认界面级信号

**先行依赖**：Task 0, Task 2
**预计影响**：新增 `pend_card.rs`，修改 `tool_panel.rs` + `confirm_dialog.rs`

### 子任务

- [ ] **T5.1** 新建 `pend_card.rs`（或在 `tool_panel.rs` 新增模块）
- [ ] **T5.2** 实现 `spawn_pending_card()` BSN 场景函数（琥珀边框 + pend_bg + 脉冲圆点 + 标题 + 描述 + 按钮组）
- [ ] **T5.3** 实现 `despawn_pending_card()` + `resolve_pending()`（确认/拒绝后卡片消失，时间线节点转 done/deny）
- [ ] **T5.4** `tool_panel.rs`：Pend 态图标/卡片升级（pend_bg + pend 描边 + pendPulse 动画）
- [ ] **T5.5** `tool_panel.rs`：时间线连接脊（相邻工具节点间 1px line_strong 竖线）
- [ ] **T5.6** `confirm_dialog.rs`：弹窗顶部 2px pend 色状态条（signalBreathe 动画）
- [ ] **T5.7** `confirm_dialog.rs`：头部大号待确认标记（icon-info 24px 在 pend_bg 容器中）
- [ ] **T5.8** `confirm_dialog.rs`：按钮文案改 i18n（拒绝 Esc / 允许执行 ↵）
- [ ] **T5.9** 联动验证：`ToolCallMessage(NeedsConfirmation)` → 信号卡 + 陪伴锚点转 pend + 输入卡边框转 pend + 弹窗状态条
- [ ] **T5.10** `cargo check --workspace` 通过

### 验收标准

- 工具 Confirm 态：四落点同时转为 pend 琥珀色
- 拒绝后：信号卡消失、时间线节点 deny 灰、锚点回到上一个非 pend 态
- 确认后：信号卡消失、时间线节点 done 绿、工具结果展开
- 弹窗顶部状态条呼吸动画可见

---

## Task 6 — 动效：微交互体系

**先行依赖**：Task 0（动效令牌），Task 1-5（视觉模块就位）
**预计影响**：新增 `xui/src/animation.rs`，各交互模块加动画

### 子任务

- [ ] **T6.1** 新建 `crates/xui/src/animation.rs`：`AnimationState` 组件 + `AnimationPlugin`
- [ ] **T6.2** 实现颜色过渡系统（BackgroundColor / TextColor 渐变）
- [ ] **T6.3** 实现尺寸/透明度推进系统
- [ ] **T6.4** 实现 `ReducedMotion` Resource + `detect_reduced_motion()`（环境变量）
- [ ] **T6.5** 动画交互点逐项替换（对照 6.2 表 11 项）：
  - 状态文字 fade、工具卡展开、陪伴锚点状态切换、pendPulse、breathe、弹窗 riseIn、命令面板 riseIn、Toast、流式光标 blink、shake、标签激活
- [ ] **T6.6** reduced-motion 降级规则生效（duration→0.01ms，脉冲/闪烁→停止）
- [ ] **T6.7** `cargo check --workspace` 通过

### 验收标准

- 所有微交互 ≤240ms（肉眼观察无明显延迟）
- `XGENT_REDUCED_MOTION=1 cargo run` 下所有动画瞬切
- 呼吸光晕/脉冲动画视觉正常（无闪烁、无卡顿）

---

## Task 7 — 收尾：i18n 同步 + 清理 + 验证

**先行依赖**：Task 0-6 全部完成
**预计影响**：`.ftl` 文件、旧字段删除、文档更新

### 子任务

- [ ] **T7.1** `crates/xgent_ui/i18n/*.ftl`：新增 v3 字符串（24 条）
- [ ] **T7.2** 全局检查：所有新增用户可见字符串走 `tr()` 或 `tr_with()`
- [ ] **T7.3** 删除 `theme.rs` 废弃字段：`accent`, `accent_bg`, `bubble_user`, `bubble_assistant`, `bar`, `border` 旧名
- [ ] **T7.4** 全局 emoji/Unicode 图标替换核对（Phase 0 盘点清单逐项验收）
- [ ] **T7.5** 清理 `theme.accent` / `theme.border` 所有遗留引用
- [ ] **T7.6** `cargo clippy --workspace` 零警告
- [ ] **T7.7** `cargo test --workspace` 全量通过
- [ ] **T7.8** 对照 `ui-prototype-v3.html` 做全量视觉对比（布局/颜色/间距/图标/状态色），差异登记
- [ ] **T7.9** v2 功能回归测试（对话/工具/文件树/编辑器/终端/命令面板/设置/会话历史）
- [ ] **T7.10** v3 新功能验收（16 项清单，见 migration.md §7.4）
- [ ] **T7.11** 文档同步：更新 `doc/dev-tutorial.md`（令牌/陪伴锚点/内容标签/Markdown/图标/BSN）

### 验收标准

- 全部 checklist 勾选通过
- `cargo clippy --workspace` 零警告
- 原型视觉对比无严重偏差（颜色/间距/图标）
- v2 功能无回归

---

## 附录：Task 依赖图

```
Task 0 (令牌+图标+BSN)
  ├──→ Task 1 (导航收敛+内容标签)
  │       └──→ Task 2 (陪伴锚点) ──┐
  ├──→ Task 2 (陪伴锚点) ← 可与 Task 1 并行       │
  ├──→ Task 3 (状态栏仪表) ← 需 Task 2 陪伴胶囊   │
  ├──→ Task 4 (排版+Markdown) ← 可与 Task 2/3 并行 │
  └──────────────────────────────→ Task 5 (待确认) ← Task 2
                                       └──→ Task 6 (动效) ← Task 1-5
                                                └──→ Task 7 (收尾)
```

**建议执行顺序**：0 → 1 → (2, 4 并行) → 3 → 5 → 6 → 7

Task 2 可与 Task 1 并行（不同文件），但 Task 3 需 Task 2 的陪伴胶囊就位。Task 4 独立于 Task 1-3。
