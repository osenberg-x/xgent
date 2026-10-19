# Outcome

`ui-v8-model-dialog-redesign`：按用户新增需求，重新设计交互原型（`docs/design/ui-prototype-v8.html`）中的**模型对话框**（Chat 对话场景下的模型选择 UI）。目标：把「选择模型 + 思考等级（reasoning effort）设置」从全功能设置面板收进**对话输入框（composer）的右下角**——一个常驻的内联选择器 pill，点开为轻量 popover：上半部模型列表（搜索过滤 + 当前项标记），中段思考等级 segmented control，底部保留 XGent 自身特色（后续 3D 陪伴特性的身份区）。交互参照当前流行 agent 的成熟模式（Claude 的 composer 模型/思考组合器、Zed 的 composer 右侧下拉、Cursor 的模型下拉），但**不复制视觉**：全部沿用原型既有 v7-Linear 设计 Tokens（ADR-0014）与 v8 模态/popover 语言，保持产品身份。改完在浏览器中，Chat 页可直接完成「选模型 + 定思考等级」而不必打开全屏设置面板。

**只改设计文档产物**（原型 HTML + 任务台账附段），不触碰任何 Rust 代码。

# Scope

## No product behavior change: documentation-only wording（免 Spec 理由）

No product behavior change: this change is documentation-only wording. 本变更的交付物是 `docs/design/` 下的交互原型 HTML 与 `docs/plans/ui-v8-tasks.md` 的附段记录，均非运行代码；`crates/**` 零改动，不存在可改变的产品行为。验收只针对「原型文档自身」的可观察来源（静态文本断言 + 浏览器手动走查）。

## Sources and their role

| Source item and location | Read status | Content to preserve | Spec location | Acceptance ID | Coverage status | Reason or replacement |
| --- | --- | --- | --- | --- | --- | --- |
| S1: 用户需求原文（对话时模型的思考等级等设置放输入框右下角；重新设计模型对话框；参照流行 agent；保留 3D 特色） | complete | 需求语义按原文完整落进 Outcome 与验收项 | 无正式 Spec（原型内部） | A1-A5 | covered | 设计目标来源 |
| S2: 原型 composer 区域（`#input`、标签行、`#sendBtn`、`#charHint`，行 ~1041-1110） | complete | 发送按钮/字符计数/@ 补全/停止流式的既有交互全部不回归 | 同上 | A1,A4,A6 | covered | 新选择器不得遮挡或破坏既有 composer 行为 |
| S3: 既有模型选择 UI（设置面板 `#mlist` 模型列表、`provPill/gearBtn` 打开 settings、`.mlist.bad2` 复现） | complete | 设置面板入口与坏模式复现（C2）**原样保留**，不因新对话框而消失 | 同上 | A8 | covered | 新增 Chat 专用入口，与设置面板并存 |
| S4: 模态/popover 与 z 分层语言（`pushModal/popModal`、z=400+depth×8 抽屉 z=100、Esc 走栈顶） | complete | 新 popover 复用既有栈规则与关闭路径 | 同上 | A4 | covered | 不发明第二套浮层机制 |
| S5: XGent 身份与 3D 特色语境（hero 区产品自述、后续 3D/陪伴规划） | complete | 对话框中的特色区承载「3D 陪伴身份」但不引入新功能面 | 同上 | A5 | covered | 特色 = 身份呈现，非新交互 |
| S6: 设计 Tokens（v7-Linear 基准，ADR-0014：色/字号/圆角/间距变量） | complete | 新增组件只用既有 CSS 变量，不新增色值 | 同上 | A5,A6 | covered | 视觉一致性硬约束 |
| S7: `docs/plans/ui-v8-tasks.md` 文末「附：原型复审（2026-10-16，已完成）」 | complete | 保持不动；本轮重设计**另起**一段「附：模型对话框重设计（2026-10-16，已完成）」 | 同上 | A7 | covered | 文档同步（AGENTS.md §6.8） |

## In scope

- composer 右下角新增常驻选择器 pill（模型名 + 思考等级徽标），占位不遮挡发送按钮、不与 IME/补全冲突。
- 新增模型对话框（popover 形态）：模型列表（搜索过滤、当前模型标记、含「常用 / 其他」分组）、思考等级 segmented control（off/low/medium/high）、XGent 3D 陪伴身份区。
- 选择/切级即改即显：pill 文案与对话框内选中态联动；Esc/scrim/选择后自动关闭，遵循既有模态栈规则。
- 设置面板入口（provPill/gearBtn）与 C2 复现不变。
- 死代码/死 CSS 零新增；`node --check` 通过；控制台零新报错来源。
- 台账附段新增一段「模型对话框重设计」记录（动机、结构、参照、复测口径），不改动原有三段。

# Non-goals

- 不触碰任何 Rust 代码（crates/** 零改动），不改构建与测试。
- 不做视觉重设计：沿用 v7-Linear Tokens，不新增色值/字号/圆角；不重排 composer 现有元素位置（选择器 pill 只占用右下角既有空隙）。
- 不给原型引入真实数据流（模型/思考等级为演示态，不接 runtime）。
- 不移动/删除既有设置面板、模型列表演示、C1/C2/C3/D2/E2 复现开关。
- 不实现真正的 3D 渲染或宠物形象（特色区为身份占位呈现）。
- 不 i18n 化原型（docs/ 下中文设计文档产物，AGENTS.md §5.8）。

# Acceptance examples

- A1: composer 右下角出现常驻选择器 pill（显示当前模型名 + 思考等级徽标）；发送按钮、字符计数、@ 补全等既有行为完全不受影响；pill 用 absolute 定位锚定 composer 右下角，窗口缩放不遮挡输入区。
- A2: 点击 pill 打开模型对话框：上半部为可搜索的模型列表（新列表为 12 个典型模型的最小演示集：搜索即过滤、当前模型 ✓ 标记、「常用 / 其他」分组）；↑/↓ 移动高亮、Enter 采纳、点击采纳均可。
- A3: 思考等级为 segmented control（off/low/medium/high）：选中即改，pill 徽标与对话框内选中态同步；退出重开保持选择（页内状态）。
- A4: 全部关闭路径成立：Esc（栈顶优先）、点击 scrim、点选模型后自动收合；其余模态（settings/palette/drawer）与新 popover 的 z 顺序不冲突（沿用 400+depth×8 分层）。
- A5: 对话框底部有 XGent 特色区：3D 陪伴身份 mini 呈现（陪伴形象占位 + 「模型/思考等级将作用于 3D 陪伴演示」类文案），仅用既有 Tokens，无新功能面。
- A6: 静态断言全绿：新组件的关键选择器/DOM 模式的存在性断言 + 死代码/失配模式零命中；内联 `<script>` `node --check` 通过；控制台零新报错。
- A7: `docs/plans/ui-v8-tasks.md` 文末新增「附：模型对话框重设计（2026-10-16，已完成）」段：动机 + 结构描述 + 参照对象 + 复测口径；原有三段内容不动。
- A8: 旧入口并存无冲突：provPill/gearBtn 打开 settings 面板且其中 `#mlist` 列表 + C2 复现照旧；两处模型数据源一致（同一 JS 数据/常量）。

# Decisions

- D-1（交互选型）：思考等级集合 = **off / low / medium / high** 四档（对齐 OpenAI 系主流 agent 的档位语言）；文案「思考等级」。
- D-2（形态选型）：对话框采用**右下角锚定轻量 popover**（非全屏模态）——贴近 Claude/Zed composer 下拉的交互密度；z 轴沿用既有模态栈与 popover 语境（不第二套机制）。
- D-3（特色承载）：`3D 陪伴`身份区置于对话框底部：陪伴形象占位块 + 一行联动文案；强调「XGent 特色保留」，但**不**新增演示开关或数据面。
- D-4（参照对象，模式级不抄视觉）：Claude composer 的「模型名 pill + 思考等级」组合、Zed composer 右侧内联下拉、Cursor 的模型下拉；只取信息架构，不取样式。

# Open questions

（无 blocking。D-1 档位集合若用户想改（如两档 off/on），Shape 确认时可提；其余为可直接落地的实现细节。）

# Verification expectations

- 静态断言（PowerShell 模式断言，可入 Runtime check）：必现模式组（pill selector/anchor CSS、popover 结构、segmented control 绑定、关键文案锚点、台账识别）+ 禁止模式组（死代码、旧选择器、scope 违例痕迹如新增色值）；
- `node --check`：内联 `<script>` 语法校验；
- 浏览器手动走查（开合、键盘、联动回显、与 settings 并存）留 Verifier/用户自查边界，与既轮口径一致。
