# Outcome

实现 M7 闭环：快捷键单一来源、焦点遍历与模态隔离、i18n 全覆盖、状态健壮性、布局几何断言层、文档同步。M7 是 UI v8 交互架构重建的最后一期，闭合 F1–F4/H1–H3/I1/I2 缺陷，建立布局几何断言层防止回归。

# Scope

## 任务范围（来源：`docs/plans/ui-v8-tasks.md` §M7）

| 任务 | 范围 | 验收（可机器判定） |
|:---|:---|:---|
| M7-T1 | 快捷键单一来源：`HotkeyAction` 注册即必填 handler；修 `palette.open_files` 死键与 `input.focus => {}` 空实现；启动期一致性断言 | 一致性断言全绿；死键数为 0；命令面板键位即真实绑定 |
| M7-T2 | 焦点遍历与模态隔离：装配 `TabNavigationPlugin` + `DirectionalNavigationPlugin` + `InputDispatchPlugin`；`TabIndex` 由布局顺序派生；模态层根挂 `TabGroup::modal()`；焦点环消费 `theme.accent_interactive` | 设置面板内 Tab 按视觉顺序遍历且可见焦点环；模态内 Tab 不外逃 |
| M7-T3 | i18n 全覆盖：硬编码点迁移（`activity_bar.rs`/`top_bar.rs`/`editor/mod.rs`/`status_bar.rs`/`chat_panel.rs`）；`xui::StringSource` 真正被消费；扫描扩为全模块 + EN 无 CJK + zh 无缺失 key | 英文语料下全界面无可读中文残留（白名单显式）；中文界面文案正确 |
| M7-T4 | 状态健壮性：删 `ChatPanelEntities` 等 Entity 缓存 Resource，改 marker 查询；`q.single_mut()` 静默早退改 `single()` + `debug_assert` + `tracing::warn!` | 布局重建后无静默失效；数量异常有日志 |
| M7-T5 | 布局几何断言层：新建 `tests/layout_geometry.rs`，含 `assert_fills_region`/`assert_no_overflow`/`assert_scrollable`/`assert_truncated_in_width`/`assert_pickable_leaf`/`assert_no_blocking_empty_layer`；覆盖 chat/palette/confirm/status/topbar/resize/滚动/重叠；每条 P0 缺陷配回归 | `cargo test --workspace` 全绿；故意引入 `clip_y` 假滚动会被测试抓住 |
| M7-T6 | 文档同步与勘误：`docs/dev-tutorial.md` 同步；`docs/plans/README.md:85` 勘误 v7；`docs/design/ui-design.md` 标注被 v8 取代；本文追加勘误条目 | 文档与代码事实一致；无过时引用 |

## 依赖

- M1–M6 全部已交付（分层/主题/滚动/字体/槽位/会话内容）
- Bevy 0.20.0 的 `TabNavigationPlugin`/`TabGroup::modal()`/`InputFocusVisible`（已验证引擎事实 §9.5）

# Non-goals

- M8 Git 探针（独立里程碑，不在 M7 范围）
- 新增 P1 功能（F-10/F-12/F-13/F-15，后续独立变更）
- macOS/Linux 的 `mono` 启动日志确认（M4 遗留，已降级不阻塞）

# Acceptance examples

- A1: `HotkeyAction` 注册时未提供 handler 编译失败；启动期断言注册表 id 集等于分发表 id 集
- A2: 设置面板内按 Tab 键，焦点按视觉顺序遍历（上→下→左→右），焦点环使用 `theme.accent_interactive` 色
- A3: 模态弹窗内按 Tab 键，焦点不跳出模态边界
- A4: 英文语料下 `activity_bar.rs:130-167` 无可读中文残留，显示 i18n 翻译
- A5: 删除 `ChatPanelEntities` Resource 后，`cargo test --workspace` 全绿，无编译错误
- A6: `tests/layout_geometry.rs` 包含 `assert_no_overflow`，故意让某节点溢出会被测试捕获
