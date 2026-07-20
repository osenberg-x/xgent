# XGent 文档目录

本目录存放 XGent 项目的各类设计与计划文档。文档使用中文编写，文件名采用 kebab-case。

> 代码现状（2026-07-19）：12 个 crate 全部实现，`cargo check --workspace` 通过，约 19k 行 Rust。MVP（step1~step12）+ optimization 方案 O1~O10 + F-11 内置编辑器（P1）已全部落地。

---

## 分类说明

| 目录 | 用途 |
|:---|:---|
| `plans/` | 分步实现计划。每步聚焦单个 crate 或模块，含目标结构、编码指导、验证方法。 |
| `tasks/` | 与 `plans/` 对应的可执行任务清单（带勾选状态）。 |
| `design/` | 架构设计与技术方案。跨 crate 的整体设计、数据流、ECS 契约、模块边界。 |
| `decisions/` | 技术决策记录（ADR）。记录"为什么这样选"，含背景、备选、结论。 |
| `notes/` | 调研、评估、参考材料。不直接指导编码的探索性内容。 |

## 命名约定

- 统一 kebab-case。
- 分步计划沿用 `stepN-<module>.md`，`N` 与实现顺序一致。
- ADR 编号 `NNNN-<主题>`，主题可含中文。

---

## 现有文档清单

### 一、plans/ — 实现计划（step1~step12 + optimization，均已落地）

入口：`plans/README.md` — 实现顺序总览与依赖关系图。

| 文件 | 对应 crate/主题 | 代码状态 | 文档同步状态 |
|:---|:---|:---|:---|
| `step1-xgent-core.md` | xgent_core | ✅ 实现 | ✅ 已对齐 ADR-0005/0006（ChatEvent 12 变体、StopReason、AgentMessage、ContentBlock、convert_to_llm） |
| `step2-xui-i18n.md` | xui_i18n | ✅ 实现 | ✅ |
| `step3-xgent-settings-core.md` | xgent_settings_core | ✅ 实现 | ✅ |
| `step4-xgent-settings.md` | xgent_settings | ✅ 实现 | ✅ |
| `step5-xgent-provider.md` | xgent_provider | ✅ 实现 | ✅ 已对齐 ADR-0005/0006（StopReason 映射、Stream 双层超时、message_to_json 按角色展开 + tool_call_id 修复） |
| `step6-xgent-daemon.md` | xgent_daemon | ✅ 实现 | ✅ |
| `step7-xgent-tools.md` | xgent_tools | ✅ 实现 | ✅ 已对齐 ADR-0007（Tool trait 新签名 tier/approval_for/concurrency/ToolError/signal；builtins 示例更新） |
| `step8-xgent-context.md` | xgent_context | ✅ 实现 | ✅ |
| `step9-xgent-agent.md` | xgent_agent | ✅ 实现 | ✅ 已对齐 ADR-0007/0008（双层 run_agent_loop、Steering/FollowUp、CancellationToken、SessionStore JSONL） |
| `step10-xui.md` | xui | ✅ 实现 | ✅ |
| `step11-xgent-ui.md` | xgent_ui | ✅ 实现 | ✅ |
| `step12-xgent-app.md` | xgent_app | ✅ 实现 | ✅ |
| `optimization-from-omp.md` | O1~O10 优化方案 | ✅ 全部落地 | ✅ 方案文档 |
| `optimization-execution-tasks.md` | T1~T10 执行任务 | ✅ 全部落地 | ✅ 任务清单（无 checkbox，靠代码验收） |

### 二、tasks/ — 任务清单（step1~step12，全部 ✅）

入口：`tasks/README.md` — 已标注 step1~step12 全部 ✅ 已完成（MVP）。每个 `taskN-<crate>.md` 对应同号 `stepN` 文件。

### 三、design/ — 设计文档

| 文件 | 范围 | 代码状态 |
|:---|:---|:---|
| `requirements.md` | 需求基线（F-xx / NF-xx / OQ-xx） | F-01~F-09 + F-11 已实现 |
| `architecture.md` | 架构、进程模型、ECS 契约、crate 划分 | ✅ §6.1 ChatEvent 已对齐 ADR-0006（12 变体 + StopReason）；§6.2 Tool trait 已对齐 ADR-0007；§6.4 会话存储 JSONL（ADR-0008） |
| `ui-design.md` | MVP UI 设计（布局/面板/交互/视觉/快捷键） | ✅ 实现（对话/文件/顶栏/状态栏/命令面板） |
| `editor-design.md` | F-11 内置编辑器设计（P1） | ✅ 实现（`xui::text_editor` + `xgent_ui::editor`） |

### 四、decisions/ — 技术决策记录（ADR-0001~0010，全部已定案并落地）

| 文件 | 主题 | 落地状态 |
|:---|:---|:---|
| `0001-provider-就绪闸门权威源定-daemon-侧.md` | provider 就绪闸门由 daemon 权威判定 | ✅ |
| `0002-model-落点为-provider-级-model-overrides.md` | model 作为 provider 级配置 | ✅ |
| `0003-错误反馈细分多类-ErrorKind.md` | ErrorKind 错误分类 | ✅ |
| `0004-kind-下拉选择-MVP-隐藏-Custom.md` | model kind 下拉 MVP 隐藏 Custom | ✅ |
| `0005-chatmessage-结构化-agentmessage-双层类型.md` | ChatMessage 结构化 + AgentMessage 双层 | ✅ clean cutover |
| `0006-chatevent-细粒度流式事件-clean-cutover.md` | ChatEvent 12 变体 + StopReason | ✅ clean cutover，旧 4 变体已删 |
| `0007-tool-trait-tier-approval-signal-破坏性重构.md` | Tool trait 重构（tier/approval_for/concurrency/ToolError/signal） | ✅ clean cutover |
| `0008-会话存储-jsonl-决策.md` | 会话存储 JSONL append-only | ✅ SessionStore 已实现 |
| `0009-编辑器保存绕过-writefile-工具链-ui-only-tier.md` | 编辑器保存直接 fs::write，agent 驱动走 UiOnly tier | ✅ |
| `0010-oq08-检索升级路径分段-编辑器到c-d延后.md` | OQ-08 检索升级路径分段，D 延后到 LSP 接入 | ✅ 决策已采纳 |

### 五、notes/ — 调研笔记（参考材料，不直接对应代码）

| 文件 | 用途 | 去向 |
|:---|:---|:---|
| `i18n-research.md` | i18n 方案调研 | 已用于 xui_i18n + fluent Localizer |
| `daemon-scope-research.md` | daemon 胖瘦评估 | 已用于 daemon MVP 瘦后台定调 |
| `context-retrieval-research.md` | 上下文检索方案评估 | 已用于方案 A（OnDemand）落地 |
| `bevy-ui-mode-assessment.md` | bevy_ui 模式评估 | 已用于 xui 组件设计 |
| `oh-my-pi-study.md` | oh-my-pi 学习报告 | 已被 `optimization-from-omp.md` 吸收 |
| `oh-my-pi-borrowing-analysis.md` | oh-my-pi 借鉴分析 | 同上 |
| `scroll-abstraction-design.md` | 滚动抽象设计 | 已实现（`xui::scroll_area` + StickToBottom） |

---

## 已完成：文档与代码对齐（2026-07-19）

以下 5 处原滞后于代码的 plan/design 文档已补齐，对齐 optimization 方案（O1~O10）与 ADR-0005~0008：

1. `step1-xgent-core.md` — 已补 ChatEvent 12 变体、StopReason、AgentMessage、ContentBlock、convert_to_llm（ADR-0005/0006）。
2. `step5-xgent-provider.md` — 已补 StopReason 映射、Stream 双层超时、message_to_json 按角色展开（tool_call_id 修复，ADR-0005/0006）。
3. `step7-xgent-tools.md` — Tool trait 示例签名已从旧 `policy()` 对齐到 `tier()`/`approval_for()`/`concurrency()`，executor 与 builtins 同步更新（ADR-0007）。
4. `step9-xgent-agent.md` — 已补双层循环 `run_agent_loop`、Steering/FollowUp Message、CancellationToken、SessionStore JSONL（ADR-0007/0008）。
5. `design/architecture.md` §6.1/§6.2 — ChatEvent 已更新为 12 变体 + StopReason（ADR-0006）；Tool trait 已更新为新签名（ADR-0007）。
