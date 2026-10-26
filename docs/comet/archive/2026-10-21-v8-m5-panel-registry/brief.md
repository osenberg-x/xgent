# 目标

把 UI v8 交互架构层重建推进到**第 5 步（迁移策略）**：用**槽位注册表**取代 closed enum（`SideViewContent`），用**派生可见性**取代各模块各自写 `Node.display`，并让 rail 选中态、页签条、分割尺寸全部**从状态派生**。

交付后应达到台账 `docs/plans/ui-v8-tasks.md` §M5 的目标：闭合缺陷 **D1–D6** 与 **I3**（P-1「状态是权威，呈现是派生」、P-2「区域是注册表，不是枚举」、P-5「子系统间只经 ECS Events/Messages」）。

**北极星前置**：本期之后，「新增一个 `SideDock` 面板」应当是「加一个描述符 + 加一个视图模块 + 发一条注册消息」，**既有布局文件（`layout.rs` / `editor/mod.rs`）零改动**——M8 的 Git 探针（北极星）依赖这一点成立。

# 范围

## 1. M5-T1 槽位注册表取代 closed 枚举（⚠ 本期改动面最大）

- `xgent_ui` 新增面板注册模型：
  - `Slot { Rail, Center, SideDock, BottomDock, SettingsSection, StatusSegment, Palette, Overlay }`——`Center` 本期只建模、**不接消费者**（3D 空间视图留给 F-16）。
  - `PanelDescriptor`：`id` / `slot` / `title_key`（**必填** i18n key）/ `icon` / `order` / `closeability` / `min_size` / `default_size`。
  - `PanelRuntime`：该面板的运行时状态（实体、是否可见、是否聚焦）。
  - `PanelRegistry`（`Resource`）：描述符 + 运行时状态的注册表，支持按 id / 按 slot 查询与排序。
- 注册与激活一律走 ECS `Message`（遵守 AGENTS.md §5.2 与架构 P-5，**禁直接方法调用**）：
  - `PanelRegister { desc, view }`、`PanelActivate { id }`、`PanelDeactivate { id }`。
- `SideViewContent`（`editor/mod.rs:69-81`）在本期**保留为临时 shim**：提供「枚举变体 → 描述符 id」的**一次性映射**（纯函数），使 M5-T1…M5-T5 可分小批合入且每批可编译；M5-T6 拆除 shim。
- 验收：新增一个 `SideDock` 面板**无需修改** `layout.rs` / `editor/mod.rs` 的枚举与页签条代码。

## 2. M5-T2 rail 与历史进槽位（修 D4 / D5）

- 删除 `ActiveActivity`（`activity_bar.rs:39-40`）；rail 选中态从「注册表 ∩ 槽位可见性」派生（`registry.iter_rail().find(|p| slots.is_visible(p.id))`）。
- `ActivityKind`（`activity_bar.rs:18-30`）改为描述符驱动：rail 项由注册的 `Slot::Rail` 面板描述符生成，图标与 tooltip 取自描述符。
- `session_history.rs` 从「自建浮层 + 遮罩 + 手写 `SessionHistoryState.open`」改为**可注册面板**（`Slot::Overlay`），与文件抽屉同属单例槽位 → 二者不再争层级。
- 验收：连点 Files 两次关掉抽屉后选中态回落正确；历史面板与文件抽屉不再争层级。

## 3. M5-T3 派生可见性单一系统（修 I3 / D2）

- `derive_panel_visibility` 成为**唯一**写「面板 / 区域」`Node.display` 的系统（`PanelRegistry` + `SlotLayout` → 期望 display → 仅在变化时写）。
- 归一现有 **7 处动态面板可见性写手**：
  - `layout.rs:287`（右侧分屏 `SideViewMarker`）、`layout.rs:291`（其右手柄）
  - `file_panel.rs:265`（抽屉遮罩）、`file_panel.rs:268`（抽屉面板）
  - `activity_bar.rs:369`（rail「展开面板」钮）
  - `editor/mod.rs:352`（编辑器视图）、`terminal/mod.rs:507`（终端视图）
- 另有 **3 处 spawn 期构造默认值**（`editor/mod.rs:186`、`editor/diff_view.rs:59`、`terminal/mod.rs:275`，均为「容器初始隐藏」）：改为构造期字面量（`Node { display: Display::None, .. }`），不再走运行时写手。
- **口径（用户 2026-10-21 裁决，取「收窄」方案）**：**5 处内容态可见性写手不动**——`chat_panel.rs:1054`（回到底部按钮）、`activity_bar.rs:234`（陪伴激活环）、`welcome.rs:190`（欢迎空态）、`welcome.rs:223`（最近会话标题）、`editor/mod.rs:398`（编辑器文档页签）。它们描述的是**内容状态**（会话是否为空、列表是否为空、滚动位置、当前文档页签）而非**面板 / 区域**，不属于槽位注册表的职责；把它们塞进面板注册表是类别错误，且会把本期扩到内容层（越出 M5 边界、逼近 M6/M7）。
- 验收：`grep -rn "\.display = " crates/xgent_ui/src/` 的命中**仅限**「派生系统 + 构造期字面量 + 上述 5 处内容态白名单」；白名单以源码级测试固化，每条注明属内容态而非面板态。

## 4. M5-T4 单页签条所有者（修 D3）

- 侧区顶部页签条由**槽位布局**（`SlotLayout`）拥有并渲染；页签项由注册表中的 `Slot::SideDock` 面板描述符生成，标题取 `title_key`。
- 编辑器内部 tab 条**降级为「文档列表」语义**（打开的代码文件列表，与槽位页签不是一回事，**不合并**）。
- 两套 `EditorBackButtonMarker`（`editor/mod.rs:295` 与 `:732`）**合一**（侧区顶部只有一个返回入口）。
- 验收：侧区顶部只有一条页签条、只有一个返回入口。

## 5. M5-T5 分割模型（修 D6）

- 建模 `SplitTree / Split / LeafSize`（`LeafSize { Fixed(f32), Min(f32, Preferred(f32)), Grow(f32) }`）。
- `resize.rs` 从「单字段 `PanelWidths { side_view }` + 只认 `ResizeEdge::Right`」改为**遍历分割树生成分割条**；每个分割条持 `HandleState { target }`，拖动写回相邻 leaf 尺寸。
- 抽屉宽度从定值 `DRAWER_W = 320.0`（`theme.rs:245`）改为 `Min(240, Preferred(320))`、**上限 480**。
- 验收：抽屉可拖宽；可注册纵向分割而无需改 `resize.rs`。

## 6. M5-T6 移除 `SideViewContent` 枚举

- shim 拆除；editor / preview / diff / terminal 全部为**原生注册项**。
- i18n：页签标题取 `title_key`；删除 `editor/mod.rs:668` 的 `let _ = loc;`（丢弃 `Localizer` 的写法）。
- 验收：`grep -rn "enum SideViewContent" crates/` 无命中（本期采用更严口径：全仓 `SideViewContent` **零引用**）；英文语料下页签为英文。

## 7. 测试（纯函数级为主）

新增 `crates/xgent_ui/tests/panel_registry_contract.rs`（必要时加单元测试），覆盖：描述符 → 槽位映射、shim 一致性、派生可见性、rail 选中态派生、分割树遍历；「抽屉可拖宽」「分割条」类断言**依赖真实视口**，需按既有 headless 装配清单给相机塞 `RenderTargetInfo`。

## 8. 文档同步

- `docs/plans/ui-v8-tasks.md`：§M5 逐条勾选 + §10 追加本轮勘误。
- `docs/plans/ui-v8-handoff.md`：交付表、基线、下一步。
- `docs/dev-tutorial.md`：新增 M5 小节（注册表模型、派生可见性、分割模型、坑）。

## Source coverage（来源覆盖）

**覆盖边界**：用户给出的 `docs/plans/next-session-m5-prompt.md` **全文**（用户未限定范围，整篇为需求来源）。其【必读】清单点名的依赖材料，在**理解与实现该范围内需求所必需**的范围内纳入边界（架构 §3.1–§3.4 与 §12、台账 §M5 / §0.2 / §10、交接 §1–§4.2 / §7、以及被点名的源码文件）。边界外的台账其它期次、架构其它章节、设计系统与审计报告不逐条映射。

| Source item and location | Read status | Content to preserve | Spec location | Acceptance ID | Coverage status | Reason or replacement |
| --- | --- | --- | --- | --- | --- | --- |
| S1: 提示词【任务】段 | complete | 目标 = M5 槽位注册表与派生可见性；修 D1–D6、I3；迁移第 5 步 | 《Purpose》《槽位与描述符模型》 | A1, A2 | covered | 需求来源 |
| S2.1: 提示词【工作流要求】1 | complete | 先 `comet workflow resolve` 再按 skill 走 | 不入 Spec | — | background | 流程约束，由 Comet 运行时执行 |
| S2.2: 提示词【工作流要求】2 | complete | 建变更 → Shape → 确认 → Build → 独立 Verifier → 用户确认 → Archive | 不入 Spec | — | background | 流程约束，由 Comet 运行时执行 |
| S2.3: 提示词【工作流要求】3 | complete | 提交粒度：每任务一提交（M5-T1…T6 各一条，`feat(ui): …`）；文档同步一条；归档 `chore(native): archive <change>` | 《不回归与交付纪律》 | A11, A12 | covered | 交付纪律 |
| S3: 提示词【必读】8 条材料清单 | complete | 读 `ui-v8-tasks.md`（§M5/§0.2/§10）、`ui-v8-handoff.md`（§1/§2/§3/§4.2/§7）、`ui-v8-layout-architecture.md`（§3.1–3.3/§12）、`editor/mod.rs`、`activity_bar.rs`、`layout.rs`、`file_panel.rs`、`terminal/mod.rs`、`session_history.rs`、`overlay.rs`、`resize.rs`、`xui/scroll_frame.rs` + `xui/layout.rs` | 见 D1–D8 各行 | — | background | 清单本身是**参考材料指引**，其内容由 D1–D8 行逐条承载 |
| S4: 提示词【起点事实】9 条（2026-10-21 实测） | complete | M1–M4 全交付；`PanelRegistry`/`Slot`/`derive_panel_visibility` 零存在；`SideViewContent` 仍 closed enum；`ActivityKind`/`ActiveActivity` 仍在；`.display =` 写手 15 处；两套 `EditorBackButtonMarker`；`PanelWidths` 单字段 + `DRAWER_W` 定值 320；`session_history` 仍 overlay；基线 84/103/33 + 15 项既有失败 | 《迁移 shim 与现状对照》 | A1, A2, A3, A5, A6, A7, A8, A11 | covered | 事实基线（已逐条复核，与代码一致） |
| S5.1: 提示词【范围】1 = M5-T1 | complete | `Slot` 集合 + `PanelDescriptor`（`title_key` 必填）+ `PanelRuntime`；三条 Message；`SideViewContent` 临时 shim；验收「新增 SideDock 面板无需改 layout.rs/editor/mod.rs」 | 《槽位与描述符模型》《注册与激活（ECS 消息）》《迁移 shim》 | A1, A2, A10 | covered | 核心实现要求 |
| S5.2: 提示词【范围】2 = M5-T2 | complete | 删 `ActiveActivity`；选中态从 `registry ∩ slots.is_visible()` 派生；`ActivityKind` 描述符驱动；`session_history` 改可注册面板；验收「连点 Files 两次回落正确」+「历史与文件抽屉不争层级」 | 《rail 选中态派生》《历史面板进槽位》 | A3, A4 | covered | 核心实现要求 |
| S5.3: 提示词【范围】3 = M5-T3 | complete | `derive_panel_visibility` 成为唯一写 `Node.display` 的系统；验收 grep 口径 | 《派生可见性（唯一写手）》 | A5 | covered | 核心实现要求；**精确口径待 Q1 确认** |
| S5.4: 提示词【范围】4 = M5-T4 | complete | 页签条由 `SlotLayout` 拥有并渲染；编辑器 tab 条降级为「文档列表」（不合并）；两套 `EditorBackButtonMarker` 合一；验收「只有一条页签条、只有一个返回入口」 | 《单页签条所有者》 | A6 | covered | 核心实现要求 |
| S5.5: 提示词【范围】5 = M5-T5 | complete | `SplitTree/Split/LeafSize`；`resize.rs` 遍历生成分割条；`DRAWER_W` → `Min(240, Preferred(320))` 上限 480；验收「抽屉可拖宽」+「可注册纵向分割而无需改 resize.rs」 | 《分割模型》 | A7 | covered | 核心实现要求 |
| S5.6: 提示词【范围】6 = M5-T6 | complete | shim 拆除；editor/preview/diff/terminal 全为原生注册项；页签标题取 `title_key`、删 `let _ = loc;`；验收「无 `enum SideViewContent`」+「英文语料下页签为英文」 | 《移除 closed 枚举》《i18n 页签标题》 | A8, A9 | covered | 核心实现要求 |
| S6: 提示词【⚠ 需你判断的一点】 | complete | M5-T3 验收口径收窄（动态面板/区域可见性）还是字面清 15 处（外扩到内容态可见性） | 《派生可见性（唯一写手）》 | A5 | covered | **已裁决**（2026-10-21）：取「收窄」方案——面板/区域可见性归一，内容态可见性不动；见「决策」D-7 |
| S7.1–S7.8: 提示词【环境坑】8 条 | complete | cargo 必带 `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`；Runtime 检查用 `cmd` 包装；headless App 装配清单（`UiPlugin`/`WindowPlugin`+相机/`PickingPlugin`/`ImagePlugin`+`TextureAtlasLayout`/`TextPlugin`/`OverlayPlugin`/`EditorPlugin`/`UiFonts::fallback()`）；headless 视口宽高全 0 → 手动塞 `RenderTargetInfo{1280,720}`；Bevy 系统元组上限 16；提交信息写文件再 `git commit -F`；禁 `Set-Content -Encoding UTF8`（BOM）；`cargo fmt` 只 `-p`；Comet 绑定纪律（改已确认工件要复确认、每轮修复走 `--revise-implementation` + 新起独立 Verifier）；既有 15 项失败不是回归 | 《不回归与交付纪律》 | A11, A12 | covered | 实现与验证约束（非功能需求） |
| S8.1–S8.6: 提示词【非目标】6 条 | complete | 不做 M6/M7/M8；不做 3D 空间视图（只建模 `Center`）；不做自由 docking / 亮色主题；不改 M1–M4 已交付行为；不改 `xui` 依赖边界；不做几何断言层 | 《非目标》 | — | non-goal | 明确排除 |
| S9: 提示词【交付物】 | complete | 实现 + 测试（纯函数级 5 类）+ 台账/交接/开发指南三处同步 + 一次独立 Verifier 复验 + 归档 | 《测试》《文档同步》《不回归与交付纪律》 | A7, A10, A11, A12 | covered | 交付清单 |
| D1: 台账 §M5 六个任务原文 | complete | M5-T1…M5-T6 的逐条要求与验收判据 | 同 S5.1–S5.6 | A1–A10 | covered | 与 S5 一致（台账为本轮权威任务书） |
| D2: 台账 §0.2 缺陷映射 D1–D6 / I3 | complete | D1→M5-T1、D2→M5-T3、D3→M5-T4、D4/D5→M5-T2、D6→M5-T5、I3→M5-T3 | 《Purpose》 | A1–A7 | covered | 缺陷闭合判据 |
| D3: 台账 §10 勘误 1–33 | complete | 已踩过的坑（假滚动、TextLayoutInfo ceil、BOM、headless 视口 0、max-content 定宽等） | 《不回归与交付纪律》 | A11 | background | 实施避坑清单（不产生新需求） |
| D4: 架构 §3.1/§3.2/§3.3/§3.4 | complete | `Slot`/`PanelDescriptor`/`PanelRegistry`/`PanelRuntime` 形态；`Center` 增补（互斥而非并列）；派生可见性系统签名；`region_fill` 契约；`SplitTree/Split/LeafSize` + `Min(240, Preferred(320))` 上限 480；不做自由 docking | 《槽位与描述符模型》《派生可见性（唯一写手）》《分割模型》 | A1, A3, A5, A6, A7 | covered | 技术设计依据 |
| D5: 架构 §12 迁移第 5 步与风险预案 | complete | 第 5 步 = `PanelRegistry` 取代 `SideViewContent`，消灭 D1–D6，改派生可见性（I3）；给 `SideViewContent` 留**临时 shim** 使改动可分小批合入、每批可编译；「文档列表与槽位页签分层，二者不合并」 | 《迁移 shim》《单页签条所有者》 | A2, A6, A10 | covered | 迁移策略与风险预案 |
| D6: 交接 §4.2「M5 范围与验收判据」表 | complete | 该表把 M5-T5 写作「拖拽排序 + 面板宽度持久化」，与台账 §M5-T5（分割模型）不一致 | 《分割模型》 | A7 | superseded | 以台账 §M5 原文 + 架构 §3.4 为准（提示词明示「台账 §M5 原文」；架构 §3.4 明说「不做自由 docking / 拖拽重排」）。交接表判据**过时**，本期在交接文档中订正并记台账 §10 勘误 |
| D7: 交接 §1 交付表 / §2 基线 / §3 环境坑 / §7 开工前 5 步 | complete | 基线数字（`xgent_ui` 84 / `xui` 103 / `xgent_terminal` 33；15 项既有失败）与环境坑清单 | 《不回归与交付纪律》 | A11 | background | 基线与避坑 |
| D8: 源码现状（`editor/mod.rs`/`activity_bar.rs`/`layout.rs`/`file_panel.rs`/`terminal/mod.rs`/`session_history.rs`/`overlay.rs`/`resize.rs`/`xui/scroll_frame.rs`/`xui/layout.rs`） | complete | 15 处 `.display =` 写手的实际分布与语义（7 动态面板 / 3 spawn 期默认 / 5 内容态）；`SideViewContent` 5 变体；两套 `EditorBackButtonMarker`；`PanelWidths` 单字段；`DRAWER_W` 定值；`session_history` 浮层；`ScrollFrame`/`region_fill` 已就位 | 《派生可见性（唯一写手）》《分割模型》 | A3, A5, A6, A7 | background | 实现参照（已逐处复核，行号以本轮实测为准） |

# 非目标

- **不做 M6（Transcript / Markdown / 输入区）、M7（快捷键单源 / 焦点栈 / i18n 扩面 / 几何断言层）、M8（Git 探针）**。M8 的北极星验证留待其自身一期；本期只把「新增面板不改既有布局文件」的能力**做出来**。
- **不做 3D 空间视图**：`Slot::Center` 本期只建模、不接消费者（首个消费者留给 F-16）。
- **不做自由 docking / 拖拽重排 / 面板宽度持久化**：`SplitTree` 只支持声明式注册的结构，用户只能改尺寸、不能改拓扑。
- **不做亮色主题**；不改 M1–M4 已交付行为（`ScrollFrame` / `region_fill` / `Ellipsis` / `VirtualList` / 等宽单元格 / `MonoMetrics` 按已交付契约使用）。
- **不改 `xui` 的依赖边界**（NU-4）：`xui` 不依赖任何 `xgent_*`，本期的注册表模型全部落在 `xgent_ui`。
- **不做几何断言层**（M7-I2）：本期只在必要处补**可判定**的自动化断言。
- **不接 `mono_cjk` 槽位**、不闭环硬门槛 A 的 macOS / Linux 确认（沿用 M4 的「不阻塞」决定）。
- **不改 `agent loop` / 协议 / 守护进程**。

# 验收示例

- A1 槽位与描述符模型：`Slot` 覆盖 `Rail/Center/SideDock/BottomDock/SettingsSection/StatusSegment/Palette/Overlay`；`PanelDescriptor.title_key` 必填（i18n key）；`PanelRegistry` 可注册并按 id / slot 查询（按 `order` 排序）；`PanelRegister` / `PanelActivate` / `PanelDeactivate` 是 ECS `Message`。
- A2 扩展性（北极星前置）：新增一个 `SideDock` 面板只需「一个描述符 + 一个视图模块 + 一条 `PanelRegister`」；`crates/xgent_ui/src/layout.rs` 与 `crates/xgent_ui/src/editor/mod.rs` 中不含 `SideViewContent`，也不含按面板 id 分派的 `match`。
- A3 rail 选中态派生：`ActiveActivity` 资源被删除；rail 选中态由「注册表 ∩ 槽位可见性」派生；连点 Files 两次关闭抽屉后，rail 上不残留 Files 选中态。
- A4 历史面板进槽位：`session_history` 由自建浮层改为注册面板（`Slot::Overlay`），与文件抽屉同属单例槽位，二者同一时刻至多一个可见（不再争层级）。
- A5 派生可见性单一系统：`derive_panel_visibility` 是**唯一**写「面板 / 区域」`Node.display` 的系统；原 7 处动态面板可见性写手（`layout.rs` 分屏与手柄、`file_panel.rs` 遮罩与面板、`activity_bar.rs` 展开钮、`editor/mod.rs` 编辑器视图、`terminal/mod.rs` 终端视图）全部并入该系统；3 处 spawn 期默认值改为构造期字面量；`crates/xgent_ui/src/` 内 `.display =` 的命中**仅限**「派生系统 + 构造期字面量 + 5 处内容态白名单」（`chat_panel.rs` 回到底部按钮、`activity_bar.rs` 陪伴激活环、`welcome.rs` 欢迎空态、`welcome.rs` 最近会话标题、`editor/mod.rs` 编辑器文档页签），白名单以源码级测试固化且逐条注明属内容态而非面板态。
- A6 单页签条所有者：侧区顶部只有一条页签条、且由槽位布局拥有并渲染；编辑器内部 tab 条降级为「文档列表」语义（不与槽位页签合并）；仓库内 `EditorBackButtonMarker` 只定义一次（源码级断言）。
- A7 分割模型：`SplitTree / Split / LeafSize` 建模；`resize.rs` 改为遍历分割树生成分割条（注册一个纵向分割**不需要**改 `resize.rs`）；抽屉宽度从定值 320 改为 `Min(240, Preferred(320))`、上限 480，且**可拖宽**并被钳制在 `[240, 480]`。
- A8 移除 closed 枚举：`crates/` 内 `SideViewContent` **零引用**（含 `enum SideViewContent`）；editor / preview / diff / terminal 全部为原生注册项。
- A9 i18n 页签标题：页签标题取自描述符 `title_key`（fluent 语料）；`editor/mod.rs` 中的 `let _ = loc;` 被删除；**英文语料**下页签显示英文（自动化断言：切 `en-US` 后取到的页签标题不含 CJK 码位）。
- A10 迁移 shim 一致性：在 shim 存在期间（M5-T1…M5-T5），「枚举变体 → 描述符 id」的映射是**纯函数**且与注册表一致（每个 shim 变体都能在注册表中查到唯一描述符）；M5-T6 拆除 shim 后该一致性由注册表本身承担（每个既有面板在注册表中有唯一描述符）。
- A11 既有基线不回归 + 交付纪律：`cargo check --workspace` 通过；`cargo fmt -p xgent_ui` 无 diff；`cargo clippy -p xgent_ui --all-targets` 在改动文件零新增警告；`cargo test --workspace` 的失败集合**不超出**既有 15 项基线（`xgent_tools` 6 / `xgent_plugin_host` 6 / `xgent_daemon` 1 / `xgent_settings_core` 1 / `xgent_agent` 1）。
- A12 文档同步与提交粒度：`docs/plans/ui-v8-tasks.md` §M5 逐条勾选 + §10 追加本轮勘误、`docs/plans/ui-v8-handoff.md` 交付表 / 基线 / 下一步（并订正 §4.2 的过时判据）、`docs/dev-tutorial.md` 新增 M5 小节；提交粒度为 M5-T1…M5-T6 各一条 `feat(ui): …` + 一条文档提交 + 一条 `chore(native): archive v8-m5-panel-registry`。

# 约束与不变量

- **ECS 契约（硬性）**：面板的注册 / 激活 / 失活全部经 ECS `Message`；业务代码不直接调用注册表方法。子模块间只经 Events / Messages 通信。
- **P-1 状态权威**：可见性 / 层级 / 选中态一律由单一系统从状态计算；业务代码不直接写 `Node.display`（口径见 A5）。
- **`xui` 依赖边界（NU-4）**：注册表模型全部落在 `xgent_ui`；`xui` 不依赖任何 `xgent_*` crate，不改 `xui` 既有原语契约（`ScrollFrame` / `region_fill` / `Ellipsis` / `VirtualList`）。
- **每个任务一批可编译**：M5-T1…M5-T5 期间 `SideViewContent` shim 在位，每批合入后 `cargo check --workspace` 通过；M5-T6 拆 shim 后仍通过。
- **Bevy 系统元组上限 16**：`add_systems(Update, (a…z))` 超过 16 个会编译失败。
- **cargo 环境**：所有 cargo 命令带 `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`；`cargo fmt` 只用 `-p <crate>`（`--all` 会改写 `../bevy` 上游检出）。
- **headless 集成测试装配**：App 需装齐 `bevy::ui::UiPlugin`、`bevy::window::WindowPlugin` + 相机、`bevy::picking::PickingPlugin`、`bevy::image::ImagePlugin` + `init_asset::<TextureAtlasLayout>()`、`bevy::text::TextPlugin`、`xgent_ui::overlay::OverlayPlugin`、`xgent_ui::editor::EditorPlugin`、`xgent_ui::fonts::UiFonts::fallback()`；并手动给相机塞 `RenderTargetInfo { physical_size: UVec2::new(1280, 720), scale_factor: 1.0 }`，否则 UI 视口宽高全 0。
- **文件编码**：禁 `Set-Content -Encoding UTF8` 回写源文件（PS 5.1 写 BOM，会污染静态断言）；需精确字节时用 `[System.IO.File]::WriteAllText($p, $text, (New-Object System.Text.UTF8Encoding($false)))`。
- **提交信息**：写文件后 `git commit -F <file>`（PowerShell 管道按 GBK 转码会破坏中文）。
- **既有 15 项失败不是回归**（见 A11）。

# 决策

- D-1 **本变更单变更、单会话推进，不拆 Supervisor 子变更**：M5 六个任务在 `xgent_ui` 的同一核心区（面板模型 / 显隐 / rail / 页签条 / 分割）反复改动，耦合紧、共享同一批类型与测试夹具，拆成并行子变更的协调成本高于收益。架构 §12 第 5 步的「分小批合入、每批可编译」由**每任务一提交**（而非多子变更）满足。
- D-2 **台账 §M5 为本轮任务书权威**；交接 §4.2 的 M5-T5 判据（「拖拽排序 + 面板宽度持久化」）与台账 §M5-T5（分割模型）冲突，**以台账 + 架构 §3.4 为准**（提示词明示「台账 §M5 原文」；架构 §3.4 明说「不做自由 docking / 拖拽重排」）。交接文档在本次交付时订正，并记台账 §10 勘误。
- D-3 **`Slot::Center` 本期建模但不接消费者**：提示词【非目标】明确「只把 `Center` 槽位建模好、不接消费者」；`Slot` 集合取台账 M5-T1 列表 ∪ 架构 §3.1 的 `Center` 增补（台账该行未含 `Center`，属滞后）。
- D-4 **历史面板归 `Slot::Overlay`（单例）**：文件抽屉与历史抽屉都是 DRAWER 层的覆盖层，同属 `Overlay` 单例槽位 → 「不争层级」由槽位唯一可见性保证，M1 建立的 `OverlayRegistry` 互斥裁决被注册表取代。
- D-5 **页签条归属槽位布局，编辑器 tab 条保留但降级**：二者语义不同（槽位页签 = 面板切换；文档页签 = 打开的代码文件），架构 §12 风险预案明确「二者不合并」。本期只保证「侧区顶部只有一条槽位页签条」+「`EditorBackButtonMarker` 只有一处定义」。
- D-6 **i18n 语料在 `xgent_settings/locales/{zh-CN,en-US}/main.ftl` 增补**：页签与新面板标题的 `title_key` 必须两种语料齐备（英文语料不得含 CJK），否则 A9 不成立。
- D-7 **`.display =` 的 grep 口径**（用户 2026-10-21 裁决，取「收窄」方案）：`derive_panel_visibility` 是唯一写**面板 / 区域**可见性的系统；**内容态**可见性（会话是否为空、列表是否为空、滚动位置、当前文档页签）**不纳入**本期归一——它们描述内容状态而非面板/区域，塞进槽位注册表是类别错误，且会把本期扩到内容层（越出 M5 边界）。台账 §M5-T3 的措辞在本次交付时订正为「面板 / 区域可见性口径 + 内容态白名单」，并记 §10 勘误。

# 开放问题

（无。本轮唯一待裁决项 Q1 —— M5-T3 的 `.display =` 归一口径 —— 已于 2026-10-21 由用户裁决为「收窄到面板 / 区域可见性」，结论见「决策」D-7 与 A5。）

# 验证期望

- **Runtime 检查计划**（`builder-handoff.verification_checks`）用 `cmd` 包装，规避 Windows 全量 debug info 的 OOM：
  `executable = "cmd"`，`argv = ["/c", "set CARGO_PROFILE_DEV_DEBUG=0&& set CARGO_PROFILE_TEST_DEBUG=0&& cargo test -p xgent_ui --no-fail-fast"]`。
- 提交前自检：`cargo fmt -p xgent_ui` → `cargo clippy -p xgent_ui --all-targets` → `cargo test -p xgent_ui --no-fail-fast` → `cargo check --workspace`。
- **必测的新增断言**（自动化、可判定）：描述符 → 槽位映射；shim 一致性（纯函数）；派生可见性（源码级：`.display =` 命中集合 = 派生系统 ∪ 构造期字面量 ∪ 内容态白名单，白名单外为空；行为级：开关抽屉 / 折叠分屏时 `display` 正确切换）；rail 选中态派生（连点两次回落）；分割树遍历（纵向分割注册后手柄数量与尺寸）；抽屉拖宽后宽度被钳制在 `[240, 480]`；英文语料下页签标题非 CJK。
- **独立 Verifier**：Comet 流程内启动**新的一轮只读 Verifier**，逐条评估 A1–A12；每轮修复后**重交候选 + 新起独立 Verifier**，不沿用上一轮结论。
- **既有失败不计入回归**：`cargo test --workspace` 的 15 项基线失败（`xgent_tools` 6 / `xgent_plugin_host` 6 / `xgent_daemon` 1 / `xgent_settings_core` 1 / `xgent_agent` 1）不在本变更责任范围。
