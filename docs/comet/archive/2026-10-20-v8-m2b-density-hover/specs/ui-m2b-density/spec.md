# ui-m2b-density · 密度档位与 hover 量化刷新

## Purpose

让「界面疏密」成为一个可切换、可持久、切换即生效的偏好，并让 hover 换色从每帧全量轮询改为只处理发生变化的节点。

- 密度档位：`comfortable`（缺省）与 `compact`。切档后**不重建节点、不重启**，在下一帧按间距基线写回并重排。
- hover 量化：空闲帧不扫描任何 `HoverTint` 节点；按下与松手都在同一帧内得到正确的背景色。

本能力不改变 `comfortable` 档位下的任何像素值与字号；不引入新的色值。

## Requirements

### 偏好类型

- `xgent_settings_core::global::Preferences` 新增 `density: Density`，serde 用 snake_case 字符串（`"comfortable"` / `"compact"`），缺省 `Comfortable`。老配置文件无该键时取缺省，不报错。
- `Density` 提供 `scaling() -> f32`（`Comfortable = 1.0`、`Compact = 0.75`）、`as_str()`、`parse(&str) -> Density`（未知值回落 `Comfortable`）、`scale(base: f32) -> f32`（按系数换算并取整到整像素；0.75 系数下 4px 网格全部落在整数上）。

#### Scenario: 缺省与老配置兼容

- **Given** 一份不含 `density` 键的全局配置 TOML，以及一份 `density = "compact"` 的 TOML
- **When** 分别反序列化为 `GlobalConfig`
- **Then** 前者 `preferences.density == Comfortable`，后者 `== Compact`；`Preferences::default().density == Comfortable`

验收：A1

### 主题权威承载密度

- `ThemeHandle` 增加 `density: Density`；`generation` 同时表示「主题或密度发生了变化」。
- `sync_theme_from_config` 读 `GlobalConfigRes.preferences.{theme, density}`；任一值与 `ThemeHandle` 不同即写回并 `generation += 1`。
- 颜色重绘系统（`repaint_on_theme_change`）语义不变：密度变化时它也会跑一遍，写入的是同一份主题值，不改变视觉。

### 间距基线重排

- 新模块 `xgent_ui::density`：
  - `DensityBase` 组件记录节点的基线 `padding` / `margin` / `row_gap` / `column_gap`（只关心 `Val::Px`）。
  - `capture_density_bases`：对尚无 `DensityBase` 的节点，记录基线后立即按当前系数写回（新节点首帧即合档，不出现一帧的「舒适尺寸」）。
  - `apply_density_on_change`：`ThemeHandle` 变化时，对全部有基线的节点按 `基线 × 系数` 写回。
  - 两系统按序执行；**写入始终以基线为源**，因此重复应用不会累乘。
- 间距换算只作用于 `padding` / `margin` / `row_gap` / `column_gap` 的 `Val::Px`；`Val::Percent` / `Val::Auto` / 0 值原样保留。字号、图标尺寸、`Node.width/height`、边框宽度不参与换算。

#### Scenario: 切档即时收紧

- **Given** 界面以 `comfortable` 启动，rail（`padding: 8/4`）、对话区（`padding: 24`、`row_gap: 20`）、设置面板卡片（`padding: 16`、`row_gap: 8`）已布局
- **When** 把 `ThemeHandle.density` 改为 `Compact` 并跑一帧
- **Then** 上述节点的 `Node.padding` / `row_gap` 变为基线的 0.75 倍（8→6、24→18、20→15、16→12、4→3、8→6），无需重启

验收：A2

#### Scenario: 重复应用不累乘

- **Given** 某节点已按 `Compact` 写回间距
- **When** 连续多帧不切档，或再次应用 `Compact`
- **Then** 该节点的间距数值保持不变；切回 `Comfortable` 后回到基线值

验收：A3

### 设置入口与持久化

- 设置面板在「语言」区块之后新增「密度」区块，两枚分段按钮（舒适 / 紧凑），选中态用 `accent` 底（与语言按钮同形态）。
- 点击即生效：面板写 `GlobalConfigRes.preferences.density` 并发 `SaveDensityMessage`；不进入「保存」通路。
- `xgent_app::settings_io`：
  - `save_density` 经 `CONFIG_WRITE` 写 `preferences.density`（字符串），随后重读快照。
  - `read_global` / `drain_snapshot` / `apply_save_result` 覆盖 `preferences.density`，与 `language`、`theme` 同列。
  - 启动时读一次偏好（`preferences.language` / `preferences.theme` / `preferences.density`）并落到资源，使磁盘上的密度在启动即生效；该路径**不**触发项目配置回填与运行时句柄重写（避免开机把 `LiveSettings` 冲成默认值）。
- `xgent_daemon::config_store`：`read` / `write` 的键白名单各增一条 `preferences.density`，`write` 用与 `preferences.theme` 相同的字符串解析与校验。仅此一处，daemon 其余逻辑不动。

#### Scenario: 写入并读回

- **Given** 配置协调器已加载
- **When** 写入 `preferences.density = "compact"`，再读该键
- **Then** 写入成功（不再报「未知配置 key」），读出为 `"compact"`，`config().preferences.density == Compact`

验收：A4

### hover 量化刷新

- `kit.rs::hover_tint_system` 只处理状态发生变化的节点：
  - 查询过滤 `Or<(Changed<Hovered>, Changed<Pressed>)>`；
  - 用 `RemovedComponents<Pressed>` 补齐「松手」这一帧：节点从按下色回落到「仍悬停则 hover 色、否则基础色」。
- 新增诊断资源 `HoverTintPollStats { visits: u32, releases: u32 }`，每帧记录本轮实际处理过的节点数与释放事件数，供测试与排障读取；不面向用户展示。
- 写入前仍做「值不同才写」，避免无谓的组件变更。

#### Scenario: 空闲帧零扫描

- **Given** 场景内存在若干 `HoverTint` 节点且指针未移动
- **When** 连续跑若干帧
- **Then** `HoverTintPollStats.visits == 0`

验收：A5

#### Scenario: 悬停与松手色序

- **Given** 一个挂 `HoverTint` 的按钮，初始 `Hovered(false)`
- **When** 依次把 `Hovered` 置 `true`、插入 `Pressed`、移除 `Pressed`
- **Then** 背景依次变为 hover 色、按下色、悬停色（不是停在按下色）；对应帧的 `visits` 均 > 0

验收：A6

### 回归与门禁

- `xgent_ui` 既有 69 项 lib 测试与 `layout` / `settings_panel_values` / `settings_panel_i18n` / `toast` / `modal_settings` / `interaction_model` 集成测试全绿。
- `cargo check --workspace` 通过；`cargo clippy -p xgent_ui --no-deps`（lib）无新增告警；`cargo fmt -p xgent_ui -p xgent_settings_core -p xgent_app -p xgent_daemon` 后无 diff。

#### Scenario: 既有测试不回归

- **Given** 完成实现后的工作区
- **When** 跑 lib 测试与上述集成测试
- **Then** 全部通过，失败数为 0

验收：A7

#### Scenario: 静态门禁

- **Given** 完成实现后的工作区
- **When** 跑 `cargo check --workspace` 与 `cargo clippy -p xgent_ui --no-deps`
- **Then** 均通过且无新增告警

验收：A8

### 文档同步

- `docs/dev-tutorial.md` 新增 §5.19「密度档位与 hover 量化（v8 M2b）」，并把 §5.18 末尾的「M2b 待做」行改为已闭合。
- `docs/plans/ui-v8-tasks.md` 勾选 M2-T1 的密度部分与 M2-T7，并在 §10 记一条勘误（`ThemeHandle` 用一个 `generation` 同时表示主题与密度变化）。

#### Scenario: 文档同步

- **Given** 实现完成
- **When** 检查 `docs/dev-tutorial.md` 与 `docs/plans/ui-v8-tasks.md`
- **Then** 两处均有 M2b 记录，`grep -c "M2b 待做" docs/dev-tutorial.md` 为 0

验收：A9
