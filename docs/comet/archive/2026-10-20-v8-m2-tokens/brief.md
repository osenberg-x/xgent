# 目标

把 UI 的**主题与尺寸令牌收敛为单一权威**：`ThemeHandle` 可替换、`Themed` 节点随 generation 重绘；删除影子定义与重复 `px()`；把散落的尺寸/圆角/色值收进 `theme`；最小宽度单源；窗口首帧有确定尺寸。

本 change 只做 M2a（令牌与主题权威 + 窗口基线），不改视觉风格、不加密度档位、不重构 hover。

# 范围

- `theme.rs`：
  - `ThemeHandle { theme: Theme, generation: u64 }` Resource；`ThemeRole { Canvas, Surface, Elevated, Border, Text, TextDim }`；`Themed { role, gen }` 组件；`repaint_on_theme_change` 系统（generation 变化时更新 `BackgroundColor`/`BorderColor`/`TextColor`，并同步旧 `Theme` 资源快照）。
  - `size` 新增：`ICON_BTN/RAIL_ITEM/BRAND/SCOPE_H/MSG_MAX_W/TOAST_BOTTOM/OVERLAY_PALETTE_W/OVERLAY_SETTINGS_W/OVERLAY_CONFIRM_W/MODAL_MIN_W`。
  - 新增主题色：`star`（活动栏星形）、`on_accent`（替代 `Color::WHITE`）。
  - `Theme::dark()` 可见性降为 `pub(crate)`（测试可用 `#[cfg(test)]` 辅助）。
- `layout.rs`：BASE/顶栏/状态栏/图标轨/对话区/上下文面板挂 `Themed`，颜色改从 `ThemeHandle` 取。
- `kit.rs`：`layer_root` 等保留；移除任何现场 `Theme::dark()` 构造。
- `confirm_dialog.rs`、`settings_panel.rs`：删除本地 `fn px()`，统一 `theme::px`。
- `resize.rs`：`SIDE_VIEW_MIN`/`CHAT_MIN`/`HANDLE_W` 改为引用 `theme::size`（新增 `HANDLE_W`）。
- 尺寸替换点：`chat_panel` 消息宽度、`top_bar` 品牌块、`activity_bar` 图标钮、`command_palette` overlay/面板、`settings_panel` 卡片、`confirm_dialog` 卡片、`editor/conflict` 卡片、`welcome` 圆角等（按 `ui-v8-tasks.md` M2-T3 清单）。
- 色值替换：`activity_bar` 星形、`editor/tabs.rs` 的 `Color::WHITE`；`terminal/output.rs` ANSI 表加白名单注释。
- `xgent_app`：窗口 `resolution: WindowResolution::new(1280, 800)`（逻辑像素，HiDPI 由 winit 处理）。
- `settings_io`/config bridge：`preferences.theme` 变化时更新 `ThemeHandle.theme` 并 `generation += 1`（当前仅 `dark`，未知值回落 dark）。
- 测试：主题重绘、令牌单源、窗口尺寸、色板白名单。

## 来源与用途

| 来源项与位置 | 读取状态 | 需保留的内容 | Spec 落点 | 验收 ID | 覆盖状态 | 理由 |
| --- | --- | --- | --- | --- | --- | --- |
| S1: 用户选择「先做 M2a」 | complete | 范围限定为令牌/主题权威/窗口，不含密度与 hover | specs/ui-m2a-tokens/spec.md | A1–A6 | covered | 需求来源 |
| S2: `docs/plans/ui-v8-tasks.md` M2-T1（主题部分）、T2–T6 | complete | ThemeHandle/Themed、影子清理、尺寸/圆角/色板、最小宽度、窗口基线 | specs/ui-m2a-tokens/spec.md | A1–A6 | covered | 任务定义 |
| S3: `docs/design/ui-v8-layout-architecture.md` §8.1 | complete | 主题热驱动与 generation 语义 | specs/ui-m2a-tokens/spec.md | A1 | covered | 技术设计 |
| S4: `docs/design/ui-v8-design-system.md` §2.3/§4 | complete | 尺寸/圆角/色板令牌值 | specs/ui-m2a-tokens/spec.md | A3, A5 | covered | 令牌来源 |
| S5: 现状代码 `theme.rs`、`resize.rs`、`kit.rs`、`settings_io.rs` | complete | 既有令牌与配置通路作为基线 | specs/ui-m2a-tokens/spec.md | A1–A6 | covered | 实现基线 |

# 非目标

- 密度档位（`preferences.density`）与全界面重排：留给 M2b。
- hover 全量化（M2-T7）：留给 M2b；`Interaction` 已废弃，M2b 用 `Hovered`/`Pressed` 重做。
- 新增 light 主题色板：本 change 只建立可替换机制，不交付第二套视觉。
- M3 及以后（滚动、省略、注册表、Markdown、焦点环）。
- 任何用户可见的视觉改版；令牌替换必须保持像素值不变。

# 验收示例

- 修改 `ThemeHandle.theme`（或配置 `preferences.theme`）后，`generation` 递增，所有 `Themed` 节点的背景/边框/文字颜色在下一帧更新，无需重启。
- `Theme::dark()` 仅命中 `theme.rs` 内定义与测试；`fn px(` 仅 `theme.rs` 一处。
- `size` 新增的 10 个常量被实际引用；区域尺寸不再出现裸 `Val::Px(<数字>)`（1px 边框等装饰需注释豁免）。
- `CONTEXT_W_MIN`/`CHAT_MIN` 只在 `theme::size` 定义，`resize.rs` 引用同一常量。
- 非 theme 色字面量只剩 ANSI 表白名单项（带注释）；星形与 `Color::WHITE` 已入 theme。
- 启动首帧窗口为 1280×800 逻辑像素；HiDPI 下不出现尺寸异常。

# 约束与不变量

- 只改 `xgent_ui`、`xgent_app`、`xgent_settings_core`（若需 theme 字段）与文档；不改协议与其他 crate 行为。
- 令牌替换必须保持现有像素值；不借机改视觉。
- `Theme` 资源保留为 `ThemeHandle` 的派生快照，避免一次性迁移所有 `Res<Theme>` 调用点。
- z 与层级仍走 M1 的 `theme::z`。
- 本机测试需 `CARGO_PROFILE_DEV_DEBUG=0`/`CARGO_PROFILE_TEST_DEBUG=0`。

# 决策

- D1 `ThemeHandle` 是唯一权威；`Theme` 资源在 generation 变化时同步，旧代码继续可用。
- D2 `Themed` 先覆盖 BASE 的六个表面节点（画布/顶栏/状态栏/图标轨/对话区/上下文面板），不一次性改所有文本节点。
- D3 密度与 hover 明确留给 M2b。
- D4 窗口只设默认尺寸，不做「记住上次尺寸」（需要新增持久化键，另议）。

# 验证预期

- `cargo test -p xgent_ui`（lib + 相关集成）
- `cargo check --workspace`
- `cargo clippy -p xgent_ui -p xgent_app --lib --no-deps -- -D warnings`
- `grep` 断言：`Theme::dark()`、`fn px(`、`Val::Px([0-9]`、`Color::WHITE`
