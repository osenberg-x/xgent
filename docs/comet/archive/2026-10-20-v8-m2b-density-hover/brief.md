# 目标

补上 UI v8 M2 剩下的两块（M2a 已交付主题权威与令牌收敛）：

1. **密度档位**：`preferences.density`（`comfortable` 缺省 / `compact`）成为可切换的偏好，设置面板有毒档位控件；切档后**全界面在下一帧完成重排**（`Node` 间距属性写回，不重建节点），不重启即生效，写回配置后重启仍保持。
2. **hover 全量化**：`kit.rs` 的 `hover_tint_system` 从「每帧遍历全部 `HoverTint`」改为「只处理状态变化的节点」，空闲帧零扫描；同时把「按下态释放」这一帧也纳入（否则按钮松手后停在按下色）。

# 范围

## 密度档位

- `xgent_settings_core::global::Preferences` 新增 `density`（枚举 `Density { Comfortable, Compact }`，serde snake_case，缺省 `Comfortable`，老 TOML 缺键即取缺省）。
- `xgent_ui::theme`：`ThemeHandle` 增加 `density`；`sync_theme_from_config` 同时读 `preferences.theme` 与 `preferences.density`，任一变化即 `generation += 1`。
- `xgent_ui::density`（新模块）：`DensityBase` 组件记录每个节点的基线 `padding/margin/row_gap/column_gap`；`apply_density_on_change` 在密度变化时按基线 × 系数写回（幂等、不累乘），新节点首帧捕获基线后立即按当前系数换算。
- 设置面板：「语言」区块之后新增「密度」区块，两枚分段按钮（舒适 / 紧凑），点击即生效（不进「保存」通路，与语言同形态），并写回 `preferences.density`。
- `xgent_app::settings_io`：新增 `SaveDensityMessage` 处理（经 `CONFIG_WRITE` 写 `preferences.density` 后重读快照），`read_global` / `drain_snapshot` / `apply_save_result` 覆盖该键；启动时读一次偏好让磁盘值即生效。
- `xgent_daemon::config_store`：`read` / `write` 白名单各加一条 `preferences.density`（用户已确认，见 D-5）。

## hover 全量化

- `hover_tint_system` 查询改为 `Or<(Changed<Hovered>, Changed<Pressed>)>` 过滤；`RemovedComponents<Pressed>` 补齐「松手」这一帧。
- 新增诊断资源 `HoverTintPollStats`（最近一帧处理的节点数与释放事件数），仅用于测试与排障。

## 文档

- `docs/dev-tutorial.md` 续写 §5.19（M2b），§5.18 末尾「M2b 待做」行标注已闭合。
- `docs/plans/ui-v8-tasks.md` 勾选 M2-T1 密度部分与 M2-T7，§10 追加勘误。

# 非目标

- 不缩放字号、图标尺寸、rail 宽度 52、`ICON_BTN` 40 等固定尺寸（D-2）。
- 不做亮色主题、不做密度快捷键、不做逐面板独立密度。
- 不改 provider / agent / tools 的任何逻辑；daemon 只改 `config_store.rs` 的配置键白名单，不动其余逻辑（D-5）。
- 不做 M3（滚动/省略）、M5（注册表）、M6（会话内容）。

# 验收示例

- A1 偏好缺省与兼容：`Preferences::default().density == Comfortable`；`density = "compact"` 可反序列化为 `Compact`；不含 `density` 键的老 TOML 反序列化后仍为 `Comfortable`（`xgent_settings_core` 单测）。
- A2 切档即时重排：切到 `Compact` 后同一帧或下一帧，rail（8→6 / 4→3）、对话区（24→18、row_gap 20→15）、设置面板卡片（16→12、row_gap 8→6）的 `Node.padding` / `row_gap` 按 0.75 收紧（集成测试断言这三处具体数值）。
- A3 幂等不累乘：连续 5 帧不切档数值不变；重复应用 `Compact` 不再继续缩小；切回 `Comfortable` 回到基线（单测）。
- A4 持久化：daemon 配置协调器写入 `preferences.density = "compact"` 成功（不再报「未知配置 key」）并可读回 `Compact`（`xgent_daemon` 单测）。
- A5 空闲帧零扫描：无 hover 变化的帧里 `HoverTintPollStats.visits == 0`（单测）。
- A6 悬停/按下/松手色序：`Hovered(true)` → hover 色；`Pressed` 插入 → 按下色；`Pressed` 移除 → 回到 hover 色（不停在按下色）；对应帧 `visits > 0`（单测）。
- A7 不回归：`xgent_ui` 既有 69 项 lib 测试与 `layout` / `settings_panel_values` / `settings_panel_i18n` / `toast` / `modal_settings` / `interaction_model` 集成测试全绿。
- A8 静态门禁：`cargo check --workspace` 通过；`cargo clippy -p xgent_ui --no-deps`（lib）无新增告警；`cargo fmt -p ...` 后无 diff。
- A9 文档同步：`docs/dev-tutorial.md` 有 §5.19 且 `grep -c "M2b 待做"` 为 0；`docs/plans/ui-v8-tasks.md` 勾选 M2-T1（密度）/M2-T7 并追加勘误。

# 决策

- D-1 密度以「间距属性写回」实现，而不是把 154 处 `space::` 调用点改成读密度：单一系统覆盖全部既有与后续节点，符合「状态是权威、呈现是派生」，也避免逐点改造的漏改风险。
- D-2 密度只缩放间距（padding / margin / row_gap / column_gap 的 PX 值），不缩放字号与固定尺寸；0.75 系数让 4px 网格落到整数（4→3、8→6、12→9、16→12、20→15、24→18、32→24），不引入半像素。用户已确认选「只缩放间距」。
- D-3 密度的切换入口与语言同形态（点击即生效，不进「保存」通路），因为它是「看到即所得」的显示偏好；写入失败时按既有 `SettingsSaveReportMessage` 语义如实报告，不做乐观假成功。
- D-4 hover 用 `Hovered` + `Pressed`（0.20 的 `Interaction` 已废弃），释放帧用 `RemovedComponents<Pressed>` 补，不引入每帧轮询。
- D-5 用户已确认：允许在 `xgent_daemon/src/config_store.rs` 的键白名单各加一条 `preferences.density`（读/写），仅此一处，daemon 其余逻辑不动——否则该键写入必然报「未知配置 key」，A4 无法达成。
- D-6 `ThemeHandle` 不为密度单开计数器：`generation` 同时表示主题与密度变化；颜色重绘在密度切换时多跑一遍但写入同一份主题值，视觉无差异（此点记入 `ui-v8-tasks.md` §10 勘误）。

# 验证期望

- 检查命令统一带 `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`（本机全量 debug info 链接极慢）。
- 新行为用「数值断言」而非耗时断言：间距收紧断言 `Node.padding` 具体像素；hover 断言用诊断计数与背景色，不用帧时间。
