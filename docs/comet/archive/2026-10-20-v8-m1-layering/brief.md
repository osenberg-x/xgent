# 目标

把 UI v8 的**交互架构层**落到代码：六静态层根、`require_markers` 拾取模型、`ModalStack`、抽屉统一裁决、tooltip 顶层化、设置面板可 dismiss。目标是关闭审计 B1–B4 四条 P0/P1 缺陷，并为 M2（令牌）、M3（滚动）、M5（注册表）提供可依赖的层与键盘基础。

本 change 不改变任何视觉呈现，不新增用户可见功能；它改变的是「浮层放在哪、谁吃点击、Esc 关谁」。

# 范围

- `theme.rs` 新增 `z` 模块：`BASE/DOCK/DRAWER/DROPDOWN/MODAL_BASE/TOOLTIP/TOAST/MODAL_STRIDE/MODAL_MAX_DEPTH`（值取 `ui-v8-layout-architecture.md` §2.2）。
- `layout.rs`：`UiRoot` 下并列 spawn 六个全屏绝对定位层根（无 `Pickable`、`BackgroundColor::NONE`）；`TopBar`/`MainArea`/`StatusBar` 迁入 `BASE` 层。
- `kit.rs`：新增 `layer_root(band)` bundle；toast 改用 `z::TOAST`；tooltip 从「宿主子节点」改为挂 `TOOLTIP` 层根，按宿主 `ComputedNode` 定位并做右缘翻转。
- 新模块 `modal.rs`：`ModalStack`、`ModalLayer`、`Dismiss`、`ModalPush`/`ModalPop` Message、`ModalTopChanged` Event、`derive_modal_z`、`route_modal_input`。
- `confirm_dialog.rs`、`settings_panel.rs`：迁入模态栈，z 由栈深派生（`MODAL_BASE + depth * MODAL_STRIDE`）；设置面板挂 `Dismiss::EscAndScrim`。
- `shortcuts.rs`：Esc 不再对 confirm/terminal/editor 做 if-else 特判；栈非空时先交 `ModalStack`，栈空才走原业务。
- 抽屉：`activity_bar.rs` 的手工互斥改为「已注册可覆盖层集合」+ `DrawerToggle` 消息；`context_scope.rs`「+ 添加」、rail 钮、`filepanel.toggle` 走同一入口。
- `xgent_app`：装配 `UiPickingSettings { require_markers: true, ..default() }`；为全部可交互叶子补 `Pickable`（按钮、输入、列表项、可点卡片、页签、rail 项）。
- 测试：层根/z、拾取结构、模态栈与 Esc、抽屉互斥、tooltip 归属与翻转、设置面板 dismiss。

## 来源与用途

| 来源项与位置 | 读取状态 | 需保留的内容 | Spec 落点 | 验收 ID | 覆盖状态 | 理由 |
| --- | --- | --- | --- | --- | --- | --- |
| S1: 用户请求（2026-10-19）「继续」，按 ui-v8 计划推进 M1 | complete | 先做 M1，不抢跑 S1/3D 视口 | specs/ui-m1-layering/spec.md | A1–A6 | covered | 需求来源 |
| S2: `docs/plans/ui-v8-tasks.md` M1-T1…M1-T6 | complete | 六层根、拾取硬门槛、ModalStack、抽屉互斥、tooltip、设置 dismiss 的具体任务与验收 | specs/ui-m1-layering/spec.md | A1–A6 | covered | 任务定义 |
| S3: `docs/design/ui-v8-layout-architecture.md` §2.1–2.3、§3 | complete | `require_markers` 机制、z 段数值、`layer_root` 形状、`ModalStack`/`Dismiss` 语义 | specs/ui-m1-layering/spec.md | A1–A4 | covered | 技术设计 |
| S4: `docs/design/ui-v8-requirements-brief.md` U-02 | complete | 五类浮层 z 段、模态栈唯一权威、Esc 不穿透、tooltip 不受裁剪 | specs/ui-m1-layering/spec.md | A1, A3, A5 | covered | 需求验收口径 |
| S5: 现状代码 `layout.rs`、`kit.rs:235`、`shortcuts.rs:143`、`activity_bar.rs:274`、`confirm_dialog.rs:91`、`settings_panel.rs:683` | complete | 现有行为与测试作为回归基线 | specs/ui-m1-layering/spec.md | A1–A6 | covered | 实现基线 |

# 非目标

- M2 及以后：主题热驱动、令牌收敛、尺寸令牌、滚动契约、省略原语、槽位注册表、Markdown、快捷键注册校验、焦点环与 TabIndex。
- 命令面板与冲突弹窗迁入模态栈：M1 只迁设置与确认，其余留给 M5/M6；本 change 不改变它们的现有行为。
- 任何视觉变化（色值、字号、圆角、间距）与用户可见新功能。
- 3D 视口 / `xgent_spatial` / daemon / provider / agent / tools 的任何改动。
- 面板拖拽重排、自由 docking、多窗口。

# 验收示例

- 六个静态层根存在且 z 值来自 `theme::z`；业务代码中 `GlobalZIndex(` 只出现在 `layout.rs`/`kit.rs` 的层根 spawn 与 `modal.rs` 的栈深派生处。
- `UiPickingSettings.require_markers == true`；层根与布局壳不带 `Pickable`；所有带 `Button` 组件的实体都带 `Pickable`；空层根不吞点击。
- 设置面板打开时按 Esc 只关闭面板、不触发 `chat.abort`；确认弹窗在栈顶时独占键盘；模态 z 等于 `400 + depth*8`。
- 通过 rail 文件钮、「+ 添加」、`filepanel.toggle` 三个入口打开文件抽屉时，历史抽屉自动关闭；反向亦然；设置面板打开时抽屉不遮挡模态。
- 最右齿轮钮的 tooltip 完整可见、不被 `MainArea` 的 `Overflow::clip()` 裁切；右缘空间不足时翻转到左侧；tooltip 实体不在宿主子树内。
- 设置面板按 Esc 与点击遮罩均可关闭；关闭后焦点回到打开前的实体。

# 约束与不变量

- 只改 `xgent_ui` 与 `xgent_app`；不动 `xgent_core` 的协议类型与其他 crate。
- 层根、布局容器**不得**带 `Pickable`；可交互叶子**必须**带。遮罩用 `Pickable { should_block_lower: true, is_hoverable: true }`；穿透装饰层用 `should_block_lower: false, is_hoverable: false`。
- 抽屉与下拉不带遮罩；只有模态带 scrim（对齐 L-2）。
- z 值只走 `theme::z` 常量或 `ModalStack` 派生，禁止业务裸写数字。
- 不新增色值/字号；沿用现有 `Theme` 字段。
- **硬门槛 B**：`require_markers = true` 后若出现大面积控件点不动且短期补不齐，回退该开关、另开子账逐区迁移；不得带病进入 M3。

# 决策

- D1 六静态层根（BASE/DOCK/DRAWER/DROPDOWN/TOOLTIP/TOAST）+ 模态动态层，共 7 个 z 段。
- D2 拾取采用 `require_markers = true` + 显式 `Pickable` 白名单，而不是继续依赖默认「无 Pickable 即阻挡」。
- D3 `ModalStack` 是 Esc 与 z 的唯一权威；`shortcuts.rs` 的业务特判删除。
- D4 抽屉互斥用 `OverlayRegistry`（已注册可覆盖层集合）+ `DrawerToggle` 消息，单一系统裁决。
- D5 命令面板/冲突弹窗暂不入栈，保持现状；M1 的验收不覆盖它们。

# 验证预期

- `cargo test -p xgent_ui`（含新增 layer/modal/drawer/tooltip/settings 测试）
- `cargo test -p xgent_app`（若装配改动影响现有测试）
- `cargo check --workspace`
- `cargo clippy -p xgent_ui -p xgent_app --all-targets -- -D warnings`
- `grep -rn "GlobalZIndex(" crates/xgent_ui/src/`：仅命中 `layout.rs`/`kit.rs` 层根与 `modal.rs`
