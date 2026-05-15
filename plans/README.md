# 实现顺序总览

## 已完成

- [x] Step 1: Workspace 骨架

## 下一步按顺序实现

| Step | 模块 | 计划文件 | 依赖 |
|:---|:---|:---|:---|
| 2 | xgent_settings | plans/step2-xgent-settings.md | bevy_settings |
| 3 | xgent_provider | plans/step3-xgent-provider.md | xgent_settings |
| 4 | xgent_tools | plans/step4-xgent-tools.md | bevy_ecs |
| 5 | xgent_agent | 待编写 | xgent_provider + xgent_tools |
| 6 | xgent_ui | 待编写 | xgent_agent + xgent_tools |
| 7 | xgent_mcp | 待编写 | xgent_settings |
| 8 | xgent_app | 待编写 | all |

## 依赖关系图

```
xgent_settings ← xgent_mcp
       ↑
xgent_provider ← xgent_agent ← xgent_ui
                        ↑
xgent_tools ←───────────┘
```

## 建议实现顺序

1. **xgent_settings** — 最简单，~100行，配置持久化
2. **xgent_provider** — 核心，~400行，OpenAI SSE 流式解析
3. **xgent_tools** — ~300行，工具枚举 + 安全策略 + 执行器
4. **xgent_agent** — 最复杂，~500行，对话循环 + ECS 桥接
5. **xgent_ui** — ~600行，Bevy UI 组件
6. **xgent_mcp** — ~300行，MCP Client
7. **xgent_app** — ~100行，组装

每个模块的详细编码指导见对应的 plans/stepN-xxx.md 文件。

## 关键原则

- 先通后优：每个模块先实现最小可用版本，确保编译通过和基本功能
- 测试驱动：每个模块完成后写一个集成测试验证端到端
- 不提前引入 3D：MVP-1 只有 2D UI，3D 留到 MVP-2
