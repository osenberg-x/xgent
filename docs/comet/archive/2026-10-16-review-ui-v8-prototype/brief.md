# Outcome

「继续 review 优化」针对交互原型 `docs/design/ui-prototype-v8.html`（v8 交互架构重建，保留 v7 Linear 视觉基准）。本会话对该 1125 行原型做了逐段静态复核（grep 证据，非目测），发现 10 组真实缺陷：全部集中在「原型自己的演示能力」上——关闭页签会直接抛 TypeError（致命）、停止按钮停不掉流式演示、`@` 补全时 Enter 仍发送、C2/C3/D2 三个缺陷复现名存实亡（开关文案声称可见但界面无差异或永不触发）、git 状态徽标样式永不生效、终端文本只清第一个 ESC、以及一批死代码/死 CSS。

本变更修复全部缺陷后，原型的每一条演示开关、每一句自述文案与其界面表现互相一致：复现模式能真实呈现 C1/C2/C3/D2/E2 的 as-built 行为，v8 模式下的滚动/省略/填满/模态栈/注册表探针交互全部可用，控制台零报错。**只改这一个文档文件（原型 HTML）与任务台账的验证记录段，不触碰任何 Rust 代码。**

# Scope

## No product behavior change: documentation-only wording（免 Spec 理由）

No product behavior change: this change is documentation-only wording. 本变更的交付物是设计文档目录下的交互原型（`docs/design/ui-prototype-v8.html`）与任务台账（`docs/plans/ui-v8-tasks.md`）的验证记录段。两者均在 `docs/` 下、均非运行代码；`crates/**` 零改动，不存在可改变的产品行为。验收只针对「原型文档自身」的可观察来源（静态文本断言 + 浏览器手动走查）。

## Sources and their role

用户直接以原型文件作为 review 对象（需求源）。coverage 边界 = 原型的**交互层与自述声明**（下表 S1–S10）；卡片区内的修复对照 prose、Grubbs 台账、注册表探针说明等静态内容属演示语境本身，本次不改其内容，只在对应缺陷被修复时保持其陈述成立。

| Source item and location | Read status | Content to preserve | Spec location | Acceptance ID | Coverage status | Reason or replacement |
| --- | --- | --- | --- | --- | --- | --- |
| S1: 控制条与演示开关（`.ctl`、`tBad/tInsp/tDense`，行 382-396） | complete | 开关语义：bad=回退 v7 as-built、inspect=图层、dense=密度；aria-pressed 同步 | 非-规范（原型内部，见下） | A4,A5,A6,A7 | covered | 本次重点：让开关声称的复现真实可见 |
| S2: 页签条与页切换（`renderTabs/renderPages`，行 705-728） | complete | 单一所有者页签条；closable 页签可关；关闭后选中态回落 | 同上 | A1 | covered | `const DESC` 重赋值致命缺陷修复 |
| S3: 模态栈与浮层（`pushModal/popModal/applyModal/applyLayers`，行 838-878） | complete | B1 分层：抽屉 100 < 模态 400+depth×8；Esc 走栈顶；scrim 点击关栈顶 | 同上 | —（已正确，作为不变量保留） | background | 仅确认，不改 |
| S4: 注册表探针（`regGit/regMcp/regCost`，行 1090-1104） | complete | M8-T1 验收：新增描述符即得页签/页/rail/状态段 | 同上 | A1,A8 | covered | 页签修复使探针「可关」成立；徽标样式修复 |
| S5: 流式演示（`startStream`，行 959-981） | complete | E2 有序消息树对照：bad=工具卡在流式文本之上 | 同上 | A2 | covered | 停止路径失效（`window.__iv`）修复 |
| S6: 终端与 wcwidth 双宽演示（`renderTerm/renderWc`，行 928-957） | complete | A3 对照：实测网格+wcwidth vs font×0.6 估算 | 同上 | A9 | covered | ESC 清除不彻底、死变量 `S.estCell` 修复 |
| S7: 文件树与截断演示（`bindTree`，行 824-836；CSS 行 207-221） | complete | C3 对照：bad=溢出无省略，good=尾部 `…` | 同上 | A6 | covered | 选择器失配（`.tree li.bad .lb` vs `span.lb.bad`）修复 |
| S8: 命令面板与 `@` 补全（`renderPalette/compCheck/moveComp`，行 897-917、1040-1059） | complete | 键盘可达：↑/↓ 选择、Enter 采纳/执行、Esc 关闭 | 同上 | A3,A4 | covered | Enter 与补全分支顺序错误修复；palette 键盘导航补齐 |
| S9: 图层检查器（`BANDS/renderInsp`，行 336-344、881-895） | complete | B1 六层 z 段可视化 | 同上 | —（已正确） | background | 仅确认，不改 |
| S10: 交付说明段（`docs/plans/ui-v8-tasks.md` 文末「附：原型验证」） | complete | 追笔本轮复评结论与修复，防止文档与原型脱节 | 同上 | A10 | covered | 文档同步（AGENTS.md §6.8） |

## In scope

- 修复 `const DESC` 重赋值（改 `let`）与关闭页签后的回落选中逻辑。
- 修复停止按钮/Esc 中断流式失效（interval 句柄存入共享状态并真正 `clearInterval`）。
- 修复 `@` 补全打开时 Enter 的优先级：先采纳补全、不再误发送；补全关闭时行为不变。
- 命令面板补齐 ↑/↓/Enter 键盘导航（复用 `sel` 状态与既有 onclick 动作）。
- 接通 C2 复现：bad 模式给 `#mlist` 加已有 `.bad2` 样式类。
- 修复 C3 复现选择器失配：bad 模式长文件名真实溢出无省略，good 模式尾部 `…`。
- 新增 D2 复现：bad 模式下终端底条被裁出视口 + 警告条（此前只存在于 toast 文案）；并将控制条 toast 文案与实际可复现清单对齐（C1/C2/C3/D2/E2）。
- 修复 git 徽标 M/A/D/U selector 失配（`.tree .git.*` → 在 `.srcline` 场景同样生效）。
- 终端文本 ESC 序列完整清除（regex 全量替换）。
- 清理死代码：`treeData()`、`S.estCell`、`window.__iv`、`slice(0,0)` 追加、`t.style.color=''` 无效操作、`$$('.lb.mono',)` 尾逗号、两参 `$` 调用；删除死 CSS：`.broken/.hint`、`.tree .lb.mid`、`.term .cw`。
- `docs/plans/ui-v8-tasks.md` 文末附段追加一条「2026-10-16 复评」记录（缺陷清单 + 修复），保持文档与原型一致。

# Non-goals

- 不触碰任何 Rust 代码（crates/** 零改动），不改构建与测试。
- 不做视觉重设计：色值/尺寸/字号沿用原型现有 v7-Linear 基准（ADR-0014）。
- 不给原型增加新功能面板/新演示场景（Git/MCP/成本/图层检查器维持现状）。
- 不给原型做 i18n 化（它是 docs/ 下的中文设计文档产物，AGENTS.md §5.8 中文撰写）。
- 不重写流式/终端/树等演示的数据内容，只修交互与复现保真度。

# Acceptance examples

- A1: 关闭任一 closable 页签（如「文件预览」，或注册后的 git/mcp/cost）不再抛 `TypeError: Assignment to constant variable`；页签与对应页同步消失；控制台零报错；关闭当前激活页签后选中态回落到相邻页签。
- A2: 流式响应中点击「停止」：打字立即停止，按钮变回「发送」，agent pill 回「空闲」；不再出现后续逐帧追加与完成 toast。
- A3: 输入 `@` 触发补全后按 Enter：采纳补全项（路径替换 `@…` 片段、菜单关闭）且消息不发送；补全菜单关闭时 Enter 行为不变（空文本不发、有文本发送）。
- A4: 命令面板内按 ↑/↓ 在 `.pitem` 间移动 `sel` 状态；Enter 执行当前选中项绑定的动作（与鼠标点击一致）；Esc 关闭面板（既有行为保持）。
- A5: 「缺陷复现」开 → 设置面板模型列表出现既有 `.bad2` 警告条且不可滚动；关 → 恢复可滚动且警告条消失（C2 对照两边均真实成立）。
- A6: 「缺陷复现」开 → 文件树长文件名真实溢出（无省略号）；关 → 出现尾部 `…`（C3 对照两边均真实成立）。
- A7: 「缺陷复现」开 → 终端页签底部状态条被裁出视口并出现 D2 警告条；关 → 底条完整可见；控制条 toast 文案列出的清单与实际可复现项一致（C1/C2/C3/D2/E2）。
- A8: git 面板变更列表 M/A/D/U 徽标分别呈黄/绿/红/绿配色（样式选择器对 `.srcline` 场景生效），注册表探针「注册 Git 面板」后直接可见。
- A9: 终端文本中所有 `\x1b` 控制符被清除（含第二处 ESC）；死代码（`treeData(`、`S.estCell`、`window.__iv`、`.slice(0,0)`、`t.style.color=''`、``$$('.lb.mono',)``）与死 CSS（`.broken`/`.hint`、`.tree .lb.mid`、`.term .cw`）在文件内 grep 零命中。
- A10: `docs/plans/ui-v8-tasks.md` 文末「附」段含本轮复评记录（日期 + 缺陷清单 + 已修复结论），与原型最终状态一致。

# Decisions

- D-1（实现选择）：页签移除用 `let` 重绑定或原位 splice 皆可，二选一不改变可观察结果（A1 不关心实现）。
- D-2（实现选择）：D2 补采用 CSS `.v8.bad` 域内规则 + 警告条模拟（与 C1/C2 同构的保真模拟），不引入新 DOM 结构；警告条文案与既有 `.fake::after` 风格一致。
- D-3（结论，非变更）：S3/B1 分层、S9 图层检查器、wcwidth 演示本身逻辑正确，不改。

# Open questions

（无 blocking。全部缺陷为原型内在一致性问题，修法唯一或实现细节级别，无需用户裁决。）

# Verification expectations

- 静态断言（可执行检查）：对最终 HTML 做模式断言——「修复后必须存在的模式」与「修复后必须归零的坏模式」各一组；两条 PowerShell 检查命令（详见 Runtime check plan）。
- 若本机存在 `node`：对内联 `<script>` 做 `node --check` 语法校验；不存在则跳过（Runtime 不可靠执行时不列入必需检查）。
- 浏览器手动走查（A1/A2/A3/A4 的行为项）留给用户在 Verify 结果接受前自查；静态断言覆盖其余项。
