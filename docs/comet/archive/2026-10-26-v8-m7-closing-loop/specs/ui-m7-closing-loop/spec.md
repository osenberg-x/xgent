# UI M7: 闭环 — 快捷键/焦点/i18n/状态/断言/文档

## 能力概述

M7 是 UI v8 交互架构重建的最后一期，闭合 F1–F4/H1–H3/I1/I2 缺陷，建立布局几何断言层防止回归。依赖已交付的 M1–M6（分层/主题/滚动/字体/槽位/会话内容）。

---

## 1. 快捷键单一来源（M7-T1，修 F1/F2）

### 1.1 数据模型

- `HotkeyAction { id: &'static str, label_key: &'static str, default: KeyBinding, when: Option<fn() -> bool>, handler: fn(&mut Commands, ...) }`
- `handler` 为函数指针 → 注册即必填，语言层面消灭空实现
- 修 `palette.open_files`（`shortcuts.rs:44` 注册、全仓零消费者）与 `input.focus => {}`（`:196` 空实现）

### 1.2 启动期断言

- 注册 action 必有 handler
- 分发表 id 集等于注册表 id 集
- 命令面板显示的键位即真实绑定

**验收：A1** — 一致性断言对现有 action 全绿；死键数为 0；命令面板键位即真实绑定

---

## 2. 焦点遍历与模态隔离（M7-T2，修 F3/F4）

### 2.1 插件装配

- 装配 `TabNavigationPlugin` + `DirectionalNavigationPlugin` + `InputDispatchPlugin`
- `InputFocusPlugin` 已在 `default_plugins.rs:16`

### 2.2 TabIndex 派生

- `TabIndex` 由布局顺序派生（间隔 10），废除全 0
- 修 `tests/settings_input_focus.rs:154-167` 把错误当不变量的断言

### 2.3 模态隔离

- 模态层根挂 `TabGroup::modal()`（`tab_navigation.rs:94`）→ Tab 不跳出模态

### 2.4 焦点环

- 焦点环消费 `theme.accent_interactive` + `accent_glow`（`theme.rs:62-63` 今天零消费）
- 配合 `InputFocusVisible`（`bevy_input_focus/src/lib.rs:182`）只在键盘导航时显形

**验收：A2/A3** — 设置面板内 Tab 按视觉顺序遍历且可见焦点环；模态内 Tab 不外逃

---

## 3. i18n 全覆盖（M7-T3，修 H1/H2/H3）

### 3.1 硬编码点迁移

- `activity_bar.rs:130-167`
- `top_bar.rs:222,225`
- `editor/mod.rs:659-664`
- `status_bar.rs:140`（`"$0.00"`）
- `chat_panel.rs:385`（`"▼"`）

### 3.2 StringSource 消费

- `xui` 的 `StringSource` 真正被消费（R3-7/D5）
- 命令面板、虚拟列表、滚动条 aria 与空态文本

### 3.3 扫描扩面

- 从「只扫 settings」扩为全模块
- EN 无 CJK 码位
- zh 无缺失 key
- 纳入 CI

**验收：A4** — 英文语料下全界面无可读中文残留（白名单显式）；中文界面文案正确

---

## 4. 状态健壮性（M7-T4，修 I1）

### 4.1 删除 Entity 缓存 Resource

- 删 `ChatPanelEntities` 这类把 `Entity` 缓存进 Resource 的做法（`chat_panel.rs:85-92`）
- 改 marker 查询

### 4.2 静默早退改日志

- `q.single_mut()` 静默早退（`chat_panel.rs:796,889`、`top_bar.rs:293`）
- 改 `single()` + `debug_assert` + `tracing::warn!`

**验收：A5** — 布局重建后无静默失效；数量异常有日志

---

## 5. 布局几何断言层（M7-T5，修 I2）

### 5.1 断言函数

- 新建 `crates/xgent_ui/tests/layout_geometry.rs`
- `assert_fills_region` / `assert_no_overflow` / `assert_scrollable` / `assert_truncated_in_width` / `assert_pickable_leaf` / `assert_no_blocking_empty_layer`（架构 §9）

### 5.2 覆盖范围

- 当前完全无测模块：chat、palette、confirm、status、topbar、resize、滚动、重叠

### 5.3 回归测试

- 每条 P0 缺陷配一条"先红后绿"回归（≥ 9 条）

**验收：A6** — `cargo test --workspace` 全绿；故意引入 `clip_y` 假滚动会被测试抓住

---

## 6. 文档同步与勘误（M7-T6）

### 6.1 同步

- `docs/dev-tutorial.md`：已实现功能总览、crate 拓扑、ADR 落地点、开发注意点（AGENTS.md §6.8 硬要求）
- `docs/plans/README.md:85` 把 v7 从「待实施」勘误为已完成
- `docs/design/ui-design.md` 标注被 v8 取代的章节
- `ui-bsn-refactor-*` 顶部加「已放弃，见 ui-v8-audit.md 第七节」

### 6.2 勘误

- 本文（`ui-v8-tasks.md` §10）追加勘误条目记录与文档不符的事实

**验收** — 文档与代码事实一致；无过时引用

---

## 7. 依赖与约束

- 依赖 M1–M6 全部已交付
- 依赖 Bevy 0.20.0 的 `TabNavigationPlugin`/`TabGroup::modal()`/`InputFocusVisible`（已验证引擎事实 §9.5）
- 不引入 3D / TUI / Web / 宠物（MVP 仅 2D GUI）
- 所有用户可见字符串走 i18n（`fluent`）

---

## 8. 非目标

- M8 Git 探针（独立里程碑，不在 M7 范围）
- 新增 P1 功能（F-10/F-12/F-13/F-15，后续独立变更）
- macOS / Linux 的 `mono` 启动日志确认（M4 遗留，已降级不阻塞）
