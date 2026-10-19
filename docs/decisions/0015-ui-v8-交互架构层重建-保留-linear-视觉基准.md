# ADR-0015（草案）：UI v8 采用交互架构层重建，保留 v7 视觉基准

> 状态为**草案**：待用户确认后转「接受」，并按 AGENTS.md §6.8 同步 `docs/dev-tutorial.md` 的 ADR 落地点章节。

- **状态**：提案（Proposed）
- **日期**：2026-10-16
- **相关**：取代 [ADR-0014](0014-ui-视觉基准采用-linear-设计系统-v7-原型.md) 的**实现范围**（视觉基准不变）；否弃 BSN 方案（`docs/design/ui-bsn-refactor-plan.md` 及其 4 份评审）
- **证据**：[ui-v8-audit.md](../notes/ui-v8-audit.md)　**需求**：[ui-v8-requirements-brief.md](../design/ui-v8-requirements-brief.md)　**设计**：[ui-v8-layout-architecture.md](../design/ui-v8-layout-architecture.md)　[ui-v8-design-system.md](../design/ui-v8-design-system.md)

---

## 背景

v7（Linear 基准，ADR-0014）已完整落地 M1–M7，视觉体系成立。但 as-built 审计暴露 30+ 条缺陷，其中 9 条 P0（见审计第三节），且**共同根因是"缺少架构原语"，不是"值写错"**：

1. **拾取与层级**：`UiPickingSettings::require_markers` 默认 `false`（✅ bevy 0.20 `picking_backend.rs:67`），无 `Pickable` 的实体默认阻挡下层（✅ `bevy_picking/src/lib.rs:212`）。全应用铺满全屏的容器与浮层根因此都在吞点击；`GlobalZIndex` 仅 8 处各写各的，设置面板/命令面板/冲突弹窗**根本没有 z 值** → 文件抽屉盖住设置面板并吃掉点击。
2. **滚动**：`ScrollPosition` 只在 `OverflowAxis::Scroll` 生效（✅ `docs/notes/scroll-abstraction-design.md:19`），而业务 9 处用 `Overflow::clip_y()` 配 `ScrollPosition` → 这些列表实际滚不动。`xui::ScrollArea` 的正确实现早已存在（`scroll_area.rs:104-105`），是**未被采用**而非不可用。
3. **省略**：bevy 0.20 **无 `TextTruncate`**（✅ 全仓 grep 证实），`TextBounds` 只按字符硬裁且官方自陈不可靠 → 省略号必须自建，而项目此前把它当作"待接线"。
4. **区域模型**：`SideViewContent` 是 closed 5 变体枚举（`editor/mod.rs:69-81`），五个内容互斥，可见性由 5 个独立系统各自写 `Node.display`。**Git 已实现为插件却无 UI 入口**，MCP/插件面板/成本统计都无处安放。
5. **平台**：等宽字体只在 macOS 解析成功，Windows/Linux 落回 Inter（比例字体）；终端按 `font × 0.6` 估格且无 CJK 双倍宽概念。三平台一阶的前提下这是阻断项。
6. **测试**：全仓无一条布局几何断言，上述缺陷在现有测试体系里不可能被发现。

同时，项目已有一整套**设计完但未落地**的方案：BSN（Bevy Scene Notation）声明式 UI + resvg 图标 + 新 `XuiTheme`，经 r1–r4 四轮评审。r4 的结论是 r3 的 10 条修复「只回写进 prose、从未落到代码」，并列出 A-1…A-9 九个未决架构问题。BSN 最终被 v7 路线放弃（`docs/plans/ui-v7-migration.md:25`、`docs/notes/bevy-0.20-migration.md:29`），但它提出的层级、按钮态、动态插入、state→render 管线问题**至今无人认领**。

## 决策

**采用「交互架构层重建」（UI v8），保留 v7/ADR-0014 的视觉基准不变。**

### 1. 新增六个架构原语，而非重写界面

| 原语 | 解决的问题 | 落点 |
|:---|:---|:---|
| **六静态层根 + `require_markers` 拾取模型** | 吞点击、层级混乱、tooltip 被裁 | `layout.rs`/`kit.rs`/`xgent_app` |
| **`ModalStack` 模态栈** | z 值散落、Esc 穿透、无 dismiss、焦点外逃 | 新 `layer.rs`/`modal.rs` |
| **`PanelRegistry` + `PanelDescriptor`** | closed 枚举、双页签条、陈旧选中态 | 新 `panel_registry.rs` |
| **`ScrollFrame` 滚动契约** | 9 处假滚动、38px 永久裁切 | `xui` |
| **`Ellipsis`/`Clamp` + `MonoMetrics`** | 无省略号、终端/编辑器宽度错算 | `xui`/`xgent_app` |
| **`Transcript` 有序消息树** | 流式顺序靠命令式挪子节点 | `chat_panel`/`tool_panel` |

### 2. 顺序权威化与状态派生呈现成为硬规则

- 状态在 Resource，**只有** `derive_panel_visibility` / `ModalStack` / `Transcript` 三类系统可写呈现属性。
- 子系统间只经 ECS Events/Messages（AGENTS.md §5.2），禁直接方法调用。
- 禁止 `add_child` 的调用顺序决定视觉顺序。

### 3. 三平台一阶，消灭 `cfg!` 特权路径

字体差异只存在于**候选栈顺序这一份数据**里，代码不分叉；`mono` 末位必须是内嵌 OFL 资产，**永不**回退比例字体。终端按实测 `latin_advance` + wcwidth `display_width` 手工定位每格，不依赖跨字体连续排版（与 alacritty/kitty 同构）。

### 4. 视觉沿用 ADR-0014，只收紧纪律

色板、明度阶梯、单一靛紫、零投影、圆角与间距网格**全部不变**。变更限于：`SMALL 12.5→12`（消除半像素模糊）、新增 `CODE` 字号档与 `line_height.CJK=1.7`、语义令牌层（为亮色铺结构）、尺寸令牌化收敛约 40 处魔法像素、焦点环消费既有但零使用的 `accent_glow`。

### 5. 验证层与实现同批交付

`layout_geometry.rs` 提供 6 个断言原语，覆盖填充、越界、可滚、截断、可拾取、空层不阻挡；每条 P0 配一条"先红后绿"回归；`ui-snapshot` 出图与原型逐状态人工对照。

### 6. 明确采纳 BSN 评审的问题清单，但否决 BSN 的实现路径

r4 提出的 A-1/A-3/A-8 是真问题，分别由「单一 `Theme` 权威 + 热驱动」「模态栈 + 分层常量」「状态资源权威 + 派生渲染」解决；A-2（动态插入）由 `Transcript` 顺序键解决；A-7（resvg 破坏 `xui` 纯净性）通过**沿用 v7 构建期 SVG→PNG 管线**规避；A-6（markdown 整段重建）由增量渲染 + `EllipsisCache` 回应。**不引入** `XuiTheme`、`SceneComponent`、`bsn!{}`。

## 备选方案与否决理由

| 方案 | 内容 | 否决理由 |
|:---|:---|:---|
| **A：换视觉语言** | 从 design-md 75 套另选风格重写 `theme.rs` | 审计显示问题不在色板而在架构；换肤会保留全部 P0 缺陷，且推翻 ADR-0014 已验证的 bevy_ui 适配性 |
| **B：缺陷逐条止血** | 就地补 9 处滚动、8 处 z 值、删死键 | 不建注册表则 Git/MCP/插件面板仍无落点，**下一个功能必然再次返工**；逐条补丁无法防回归（今天缺的正是几何断言层） |
| **C：启用 BSN** | 按 `ui-bsn-refactor-plan.md` 落地声明式 UI | 四轮评审未收敛且修复从未落地；引 resvg 破坏 `xui` 依赖边界（NU-4）；`XuiTheme` 与 `xgent_ui::Theme` 双份色板（r4 A-1 未决）；重写风险远高于 strangler 迁移 |
| **D：先做功能后补架构** | 直接上 Git/MCP/Markdown 界面 | 每个新界面都要改 `SideViewContent` 枚举 + 五处显隐写手，把缺陷复制到 3 倍规模；Markdown 若不在有序树上做，E2 的顺序问题会固化进消息渲染 |

**选定：交互架构层重建（本 ADR）**，采纳用户三项决定：主线为架构层重建、交付文档 + 可交互原型 v8、Windows/macOS/Linux 三平台一阶。

## 后果

### 正面

- 路线图界面**注册即落地**：北极星指标为「Git 面板探针 = 1 个注册项 + 1 个视图模块，零既有布局文件改动」（`docs/plans/ui-v8-tasks.md` M8-T1）。
- 9 条 P0 关闭且各有回归，缺陷不再靠 review 肉眼守。
- 三平台终端/编辑器/diff 对齐问题从根上消除，中文输出不再错位（A3 此前完全无人记录，本审计新增）。
- `xui` 依赖边界、ECS 通信契约、i18n 三项既有硬约束得到强化而非绕过。
- 为亮色主题、密度切换、`ui_panel` 插件扩展点（D-P3）铺好结构，不承诺引擎做不到的事。

### 负面与风险

- **改动面大**：M5（注册表取代 closed enum）与 M4-T2（终端双宽）风险最高，各留独立提交便于回滚；strangler 分 9 步、每步可编译可测（架构文档 §12）。
- **`require_markers = true` 会暴露既有 `Pickable` 缺失**（表现为控件点不动）。这是暴露缺陷而非引入缺陷，但设了硬门槛 B：若短期补不齐则回退该开关、另开子账逐区迁移，**不得带病进 M3**。
- **`Ellipsis` 两帧流水线**可能在下拉调整窗口宽度时产生轻微滞后，已用「宽度量化 1px + 迟滞 < 2px 不重算 + 结果缓存」约束。
- **Markdown 解析倾向引入 `pulldown-cmark`**（纯 Rust、无 Bevy 依赖），需 M6-T2 走一次依赖评审；退化方案为仅支持代码块/列表/粗体（覆盖约 90% 输出）。
- 字体资产体积增加（JetBrains Mono 约 400KB），计划 subset 到 ASCII + 常用符号；CJK 等宽走系统发现不入库。
- **本轮不解决**：diff 算法（R3-12）、`on_update` 流式回灌（D-P8）、插件字符串 i18n（D-P6）、宠物窗口（K-04）、3D/TUI/Web 界面。D-P8 期间渲染层须显示「执行中」而非空白。

### 需要同步的文档

`docs/dev-tutorial.md`（AGENTS.md §6.8 硬要求）、`docs/plans/README.md:85`（v7 误标「待实施」）、`docs/design/ui-design.md`（标注被取代章节）、`docs/design/ui-bsn-refactor-*`（顶部加「已放弃，见审计第七节」）、`docs/notes/agent-gap-review.md` 的 R3-5/-6/-7/-14/-21 台账指向 v8 承接任务。
