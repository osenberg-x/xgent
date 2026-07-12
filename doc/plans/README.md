# MVP 实现顺序总览

本目录存放 XGent MVP 的分步实现计划。每个文件对应一个 crate，按依赖关系自底向上实现。

架构与需求见：
- `doc/design/requirements.md` — 需求设计
- `doc/design/architecture.md` — 架构设计

---

## 实现顺序

| Step | Crate | 类型 | 依赖 | 计划文件 |
|:---|:---|:---|:---|:---|
| 1 | xgent_core | lib | 无 | `step1-xgent-core.md` |
| 2 | xui_i18n | lib | 无 | `step2-xui-i18n.md` |
| 3 | xgent_settings_core | lib | xgent_core | `step3-xgent-settings-core.md` |
| 4 | xgent_settings | lib | xgent_settings_core, xui_i18n | `step4-xgent-settings.md` |
| 5 | xgent_provider | lib | xgent_core, xgent_settings_core | `step5-xgent-provider.md` |
| 6 | xgent_daemon | bin | xgent_core, xgent_provider, xgent_settings_core | `step6-xgent-daemon.md` |
| 7 | xgent_tools | lib | xgent_core | `step7-xgent-tools.md` |
| 8 | xgent_context | lib | xgent_core | `step8-xgent-context.md` |
| 9 | xgent_agent | lib | xgent_core, xgent_provider, xgent_tools, xgent_context | `step9-xgent-agent.md` |
| 10 | xui | lib | bevy, xui_i18n（不依赖任何 xgent_*） | `step10-xui.md` |
| 11 | xgent_ui | lib | xui, xgent_core, xgent_settings, xgent_agent | `step11-xgent-ui.md` |
| 12 | xgent_app | bin | all UI-side crates | `step12-xgent-app.md` |

## 依赖关系图

```
xgent_core ←───────────── 一切共享类型的基础
     ↑
xui_i18n ← xui, xgent_settings   （纯 trait，无依赖）
xgent_settings_core ← xgent_daemon, xgent_provider, xgent_settings
     ↑
xgent_provider ← xgent_daemon, xgent_agent
xgent_tools ← xgent_agent
xgent_context ← xgent_agent
xgent_settings ← xgent_daemon, xgent_agent, xgent_ui
     ↑
xgent_agent ← xgent_ui
                         ↑
                    xui（纯依赖 bevy + xui_i18n，可独立发布） ← xgent_ui
                         ↑
                    xgent_app（组装 UI 侧）
                    xgent_daemon（独立 bin，UI 经 IPC 调用）
```

## 关键原则

- **先通后优**：每个 crate 先实现最小可用版本，确保编译通过和基本功能，再迭代优化。
- **接口先行**：先定义 trait 与协议类型，再实现具体逻辑，保证调用方无感于实现变化。
- **测试驱动**：每个 crate 完成后写最小集成测试验证端到端。
- **不提前引入 3D**：MVP 仅 2D UI，3D 相关内容留待 P2。
- **守护进程瘦后台起步**：MVP daemon 仅承担 provider 池、全局配置、文件监听、多客户端同步；工具执行与 agent loop 在 UI 侧。
- **i18n 从一开始内置**：所有用户可见字符串走 fluent。
- **检索方案 A 起步**：MVP 用无索引·按需读取（ripgrep + 目录树），经 `ContextProvider` trait 抽象，未来可升级。
- **UI 隔离封装**：通用 UI 组件放 `xui` crate（纯依赖 bevy + xui_i18n，可独立发布），隔离 bevy_feathers / bevy_ui_widgets 的 breaking change；XGent 业务 UI 放 `xgent_ui`，依赖 `xui`。
- **i18n trait 反转依赖**：`StringSource` trait 放极小 crate `xui_i18n`，`xgent_settings::Localizer` impl 它，`xui` 调用。使 `xui` 不依赖 `xgent_*` 仍可独立发布。
- **settings 拆分**：`xgent_settings_core`（纯类型，不依赖 Bevy）供 daemon/provider 用；`xgent_settings`（Bevy Resource 包装 + Localizer）供 agent/ui 用。daemon 不被 Bevy 拖重。
- **不重复造轮子**：官方 bevy_ui_widgets / bevy_feathers 已覆盖的（button/checkbox/slider/dialog/menu/popover、text_input IME）直接用，xui 只补官方未覆盖部分。

## MVP 范围

见 `doc/design/requirements.md` 第 7.1 节。核心功能：F-01~F-09 + NF-01~NF-04。

不含（后续迭代）：宠物、3D、TUI、Web、Git、内置编辑器、成本统计、MCP、自定义工具。

## 每个 step 文件的结构

- 模块职责
- 前置依赖
- 目标文件结构
- Cargo.toml
- 关键类型与接口（代码级指导，非完整实现）
- 实现要点
- 验证方法（编译检查 + 最小测试）
- 完成后下一步
