# 任务规划总览

本目录存放每个 step 的可执行任务清单，与 `doc/plans/` 的实现指导对应。每个 step 一份 `taskN-<crate>.md`。

## 与 plans 的关系

- `doc/plans/stepN-<crate>.md`：实现指导（模块职责、文件结构、关键类型与接口、实现要点、验证方法）。
- `doc/tasks/taskN-<crate>.md`：可执行任务清单（有序、可勾选的任务条目，带依赖标注与验收标准）。

plans 说明"做什么、怎么设计"，tasks 说明"按什么顺序执行、每步完成标志是什么"。

## 约定

- 任务粒度：中（每个 step 8~15 个任务，覆盖每个文件/模块/关键函数）。
- 任务格式：`- [ ] T-N.M 描述`，带依赖标注（`依赖 T-N.x`）与验收标准。
- 不含工时预估。
- 任务清单只管 step 内部依赖，跨 step 由 `doc/plans/README.md` 实现顺序保证。
- 任务规划中文撰写。

## 任务文件

| 文件 | 对应 step | 状态 |
|:---|:---|:---|
| `task1-xgent-core.md` | step1 xgent_core | 已制定 |
| `task2-xui-i18n.md` | step2 xui_i18n | 已制定 |
| `task3-xgent-settings-core.md` | step3 xgent_settings_core | 已制定 |
| `task4-xgent-settings.md` | step4 xgent_settings | 已制定 |
| `task5-xgent-provider.md` | step5 xgent_provider | 已制定 |
| `task6-xgent-daemon.md` | step6 xgent_daemon | 已制定 |
| `task7-xgent-tools.md` | step7 xgent_tools | 已制定 |
| `task8-xgent-context.md` | step8 xgent_context | 已制定 |
| `task9-xgent-agent.md` | step9 xgent_agent | 已制定 |
| `task10-xui.md` | step10 xui | 已制定 |
| `task11-xgent-ui.md` | step11 xgent_ui | 已制定 |
| `task12-xgent-app.md` | step12 xgent_app | 已制定 |

## 任务模板

每个任务文件遵循以下结构：

```
# Task N: <crate>

> 对应实现指导：doc/plans/stepN-<crate>.md
> 前置：step1~stepN-1 已完成

## 任务清单

### 阶段一：脚手架
- [ ] T-N.1 <描述>
  - 依赖：无
  - 验收：<标准>

### 阶段二：...
- [ ] T-N.2 <描述>
  - 依赖：T-N.1
  - 验收：<标准>

## 完成标志
- <整个 step 完成的标志>
```
