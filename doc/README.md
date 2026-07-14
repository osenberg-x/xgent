# XGent 文档目录

本目录存放 XGent 项目的各类设计与计划文档。文档使用中文编写，文件名采用 kebab-case。

## 分类说明

| 目录 | 用途 |
|:---|:---|
| `plans/` | 分步实现计划。按实现顺序命名，如 `step2-xgent-settings.md`。每步聚焦单个 crate 或模块，包含目标结构、完整编码指导、验证方法。 |
| `design/` | 架构设计与技术方案。跨 crate 的整体设计、数据流、ECS 契约、模块边界说明。 |
| `decisions/` | 技术决策记录（ADR）。记录"为什么这样选"的关键决策，含背景、备选方案、结论。 |
| `notes/` | 杂项笔记、调研、参考材料。不直接指导编码的探索性内容。 |

## 命名约定

- 统一使用 kebab-case，如 `agent-ecs-bridge.md`。
- 分步计划沿用 `stepN-<module>.md` 格式，`N` 与实现顺序一致。
- ADR 建议带简短主题描述，如 `use-bevy-ecs-events-over-method-calls.md`。

## 现有文档

- `plans/README.md` — 实现顺序总览与依赖关系图
- `plans/quantum-nebula-tesla.md` — MVP-1 总体实现计划
- `plans/step2-xgent-settings.md` — xgent_settings 详细编码指导
- `plans/step3-xgent-provider.md` — xgent_provider 详细编码指导
- `plans/step4-xgent-tools.md` — xgent_tools 详细编码指导

- `design/ui-design.md` — UI 界面设计（布局、面板、交互流、视觉规范、快捷键表）