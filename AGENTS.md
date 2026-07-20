# AGENTS.md

本文件为 AI 编码助手提供在本项目中工作所需的全部背景信息。请先完整阅读本文件，再开始任何编码任务。

---

## 1. 项目简介

**XGent** — 面向个人开发者日常编码的桌面端 AI Code Agent 工具，基于 Bevy 游戏引擎构建。以完整可用的产品为目标，覆盖 AI 辅助编码、代码理解、自动化操作，并通过轻量创意的视觉与陪伴体验区别于同类工具。

四大设计支柱：实用优先、数据驱动、可扩展、轻量多开。

- 语言：Rust（edition 2024）
- 引擎：Bevy 0.19.0，依赖通过 path 指向本地源码 `../bevy`（便于调试）
- 当前阶段：MVP（仅 2D GUI，3D/TUI/Web/宠物留待后续）

**权威设计文档**（从零设计，不参考旧代码）：
- `doc/design/requirements.md` — 需求设计（功能 F-xx、非功能 NF-xx、关键约束、分期路线）
- `doc/design/architecture.md` — 架构设计（进程模型、分层、crate 划分、数据流、抽象接口、待决策点 D-xx）
- `doc/notes/` — 调研与评估报告（检索方案、守护进程胖瘦、i18n、bevy_ui 模式评估）
- `doc/plans/` — MVP 分步实现计划（step1~step12，按依赖顺序）
- `doc/dev-tutorial.md` — 开发指南（已实现功能总览、crate 拓扑、ADR 落地点、开发注意点；**功能变化时必须同步**）

---

## 2. 目录结构

```
xgent/
├── Cargo.toml                # workspace 根
├── Cargo.lock
├── AGENTS.md                # 本文件
├── src/main.rs              # 遗留 Hello world，待删除（xgent_app 接管入口）
├── crates/                  # 所有 crate（见第 4 节）
└── doc/                     # 文档（见下文）
```

### 文档目录 `doc/`

所有设计/计划文档统一存放于此，按类别分类（详见 `doc/README.md`）：

| 目录 | 用途 |
|:---|:---|
| `doc/plans/` | 分步实现计划，命名 `stepN-<crate>.md` |
| `doc/design/` | 架构设计与需求设计 |
| `doc/decisions/` | 技术决策记录（ADR） |
| `doc/notes/` | 调研、评估、笔记 |

文件名统一 kebab-case，中文撰写。

---

## 3. 构建与运行

使用标准 cargo 命令。所有命令在 workspace 根目录执行。

```bash
cargo check                       # 全量编译检查
cargo check -p <crate>            # 单 crate 编译检查
cargo build                       # 构建
cargo run -p xgent_app            # 运行 UI 进程（自动拉起 daemon）
cargo run -p xgent_daemon         # 单独运行守护进程
cargo test                        # 全量测试
cargo test -p <crate>            # 单 crate 测试
cargo tree -p <crate>             # 依赖树（验证 crate 独立性，如 xui 不含 xgent_*）
cargo fmt                         # 格式化
cargo clippy --workspace          # lint
```

**注意**：构建依赖本地 `../bevy` 源码（0.19.0），确保该目录存在。编码时按需查阅 `../bevy` 源码确认 API（bevy 仍在演进，有 breaking change）。参考实现：zed 源码位于 `/Users/xdo/ws/zed`（gpui + editor 滚动抽象可借鉴，但底层 UI 库不同，仅作设计参考）。

---

## 4. crate 划分与依赖

MVP 共 12 个 crate，按依赖关系自底向上实现（顺序见 `doc/plans/README.md`）：

| crate | 类型 | 依赖 | 职责 |
|:---|:---|:---|:---|
| xgent_core | lib | 无 | 跨进程共享类型：错误、JSON-RPC 协议、chat 事件、文件/配置事件、ID |
| xui_i18n | lib | 无 | 极小 i18n trait（StringSource），纯无依赖，反转依赖枢纽 |
| xgent_settings_core | lib | xgent_core | 配置纯类型 + TOML 读写 + 平台路径（不依赖 Bevy） |
| xgent_settings | lib | settings_core, xui_i18n | Bevy Resource 包装 + fluent Localizer（impl StringSource） |
| xgent_provider | lib | xgent_core, settings_core | LlmProvider trait + OpenAI compatible 适配器（不依赖 Bevy） |
| xgent_daemon | bin | core, provider, settings_core | 守护进程：provider 池、配置、文件监听、多客户端同步（纯 tokio，不依赖 Bevy） |
| xgent_tools | lib | xgent_core, settings_core | Tool trait + 安全策略 + 执行器（不依赖 Bevy） |
| xgent_context | lib | xgent_core, settings_core | ContextProvider trait + 方案 A 检索（不依赖 Bevy） |
| xgent_agent | lib | core, provider, tools, context, settings | agent loop + ECS 桥接 |
| xui | lib | bevy, xui_i18n | 通用 Bevy UI 组件库（可独立发布，不依赖任何 xgent_*） |
| xgent_ui | lib | xui, core, settings, agent | XGent 业务 UI（对话/工具/文件面板等） |
| xgent_app | bin | all UI-side | UI 进程入口：组装插件、daemon 拉起、IPC 客户端、项目打开 |

**依赖关系图**（无环）：

```
xgent_core ←──────── 一切共享类型的基础
     ↑
xui_i18n ← xui, xgent_settings            （纯 trait，无依赖）
xgent_settings_core ← xgent_daemon, xgent_provider, xgent_settings
xgent_provider ← xgent_daemon, xgent_agent
xgent_tools ← xgent_agent
xgent_context ← xgent_agent
xgent_settings ← xgent_daemon, xgent_agent, xgent_ui
     ↑
xgent_agent ← xgent_ui
xgent_pet ← xgent_ui（可选，P1）
     ↑
xui（纯依赖 bevy + xui_i18n，可独立发布） ← xgent_ui
     ↑
xgent_app → 组装所有 UI 侧 crate
```

**关键依赖原则**：
- daemon 不依赖 Bevy（纯 tokio，多开共享时轻量）——故 settings 拆 core（纯类型）+ Bevy 包装两层。
- xui 纯依赖 bevy + xui_i18n，**不依赖任何 xgent_* crate**，保证可独立发布被其他 Bevy 项目复用。
- i18n 用 trait 反转依赖：`StringSource` 在 `xui_i18n`，`xgent_settings::Localizer` impl 它，`xui` 经 trait 调用，`xgent_app` 注入。

---

## 5. 架构与编码约定

### 5.1 进程模型（多进程）

- **xgent-ui 进程**：每项目/窗口一个，承担 UI 渲染、交互、agent loop、工具执行（MVP）、上下文构建。
- **xgent-daemon 进程**：全局唯一、随用随启（首个 UI 进程拉起，末个退出后延迟退出）。承担 provider 连接池、全局配置协调、文件监听、多客户端文件状态同步（MVP 瘦后台）。
- **IPC**：JSON-RPC 2.0 over 本地 socket（Unix socket / named pipe）。
- **agent loop 放 UI 侧**（每客户端独立）。
- **守护进程演进**：MVP 瘦后台 → B 阶段升中后台（索引上移）→ Web 端强制升胖后台（工具执行上移）。所有可上移职责用 trait 抽象，切换不破坏调用方。

### 5.2 ECS 通信契约（硬性约束）

**所有子系统只通过 ECS Events（即时观察者）与 Messages（缓冲消息）通信，禁止直接方法调用。**

- Events：即时通知；Messages：缓冲队列。
- 目的：每个 Plugin 独立可测、可 headless、可录制/回放消息流。

### 5.3 Bevy ECS 使用

- 以 Plugin 为模块边界，每个 crate 暴露 `XgentXxxPlugin`。
- 配置类用 newtype 包装 core 类型加 `Resource`/`Reflect` 派生（见 `xgent_settings`），core 类型不带 Bevy 派生。
- 异步桥接：tokio runtime 作为 Bevy Resource，系统每帧非阻塞轮询 channel（agent/provider/tools/context 的异步调用在 tokio task，结果经 channel 回 ECS）。

### 5.4 安全模型（工具执行）

工具能力信任**可配置**，参考成熟 code agent：**默认所有工具调用（含只读类）均为 `NeedsConfirmation`**，用户可在配置中按工具 id 提升为 `Approved` 或降为 `Denied`。

| 级别 | 行为 | 默认 |
|:---|:---|:---|
| Approved | 自动执行 | 可配置提升 |
| NeedsConfirmation | 弹窗确认后执行 | **默认值** |
| Denied | 拒绝 | 可配置降级 |

### 5.5 UI 隔离封装

- 通用 UI 组件放 `xui`（纯依赖 bevy + xui_i18n，可独立发布），隔离 bevy_feathers/bevy_ui_widgets 的 breaking change。
- 官方已覆盖的（button/checkbox/slider/dialog/menu/popover、text_input IME）**直接用官方**，xui 只补官方未覆盖部分（虚拟列表、命令面板、输入增强、快捷键、i18n 桥接）。
- XGent 业务 UI 放 `xgent_ui`，依赖 `xui`。

### 5.6 i18n

- 从一开始内置（NF-05），所有用户可见字符串走 i18n，不硬编码。
- 采用 fluent（`.ftl` 资源，`fluent-rs`）。资源内嵌（`include_str!`）。
- 前期以中文为主，架构保证可翻译、可运行时切换。

### 5.7 Rust 风格

- edition 2024，workspace 依赖统一经 `[workspace.dependencies]` 声明，crate 内用 `{ workspace = true }` 引用。
- 公开类型与函数写文档注释 `///`，中文。
- 错误处理：用 `thiserror` 定义错误类型，库代码避免 `unwrap()`/`expect()`（测试除外）。
- 异步：tokio；流式用 eventsource-stream 解析 SSE；跨 ECS 与异步用 tokio mpsc channel。

### 5.8 注释与文档语言

代码注释、文档注释、提交信息、文档均使用**中文**。

---

## 6. 工作流程

1. **阅读背景**：开始任务前阅读本文件、`doc/design/`、`doc/plans/` 中相关 step 文件。编码时按需查阅 `../bevy` 源码确认 API。
2. **先通后优**：每个模块先实现最小可用版本，确保编译通过与基本功能，再迭代。
3. **接口先行**：先定义 trait 与协议类型，再实现具体逻辑，保证调用方无感于实现变化。
4. **测试驱动**：每个 crate 完成后写最小集成测试验证。
5. **不提前引入 3D/TUI/Web/宠物**：MVP 仅 2D GUI。
6. **产出文档放 doc/**：生成的设计/计划类文档放 `doc/` 对应分类目录（见第 2 节），不放项目根目录。
7. **遗留清理**：根目录 `src/main.rs` 是遗留 Hello world，按计划应删除（xgent_app 接管入口）。
8. **同步开发指南**：后续实现新功能或功能有变化，都需要更新 `doc/dev-tutorial.md`（已实现功能总览、crate 拓扑、ADR 落地点、开发注意点）。新增/变更功能、crate、ADR、trait 时必须同步该文档对应章节，避免文档与代码脱节。

---

## 7. 当前实现状态
项目处于 MVP + F-11 编辑器（P1）已落地阶段。12 个 crate 全部实现，`cargo check --workspace` 通过，约 19k 行 Rust。已实现功能总览、crate 拓扑、ADR 落地点与开发注意点见 **`doc/dev-tutorial.md`**。

实现顺序见 `doc/plans/README.md`（step1~step12，已全部完成）+ `doc/plans/optimization-from-omp.md`（O1~O10，已全部落地）。后续迭代从 `doc/design/requirements.md` §4.2 的 P1/P2 项推进。

---

## 8. 待决策点（不阻塞 MVP 起步）

详见 `doc/design/architecture.md` 第 13 节。摘要：

- D-02 API Key 是否用 OS keychain
- D-04 会话历史 SQLite schema（= OQ-07）
- D-05 命令面板注册机制
- D-06 tree-sitter grammar 分发（B 阶段前定）
- D-07 宠物等级度量（P1）

需求层开放问题（详见 `doc/design/requirements.md` 第 9 节）：OQ-05 宠物对话陪伴形式、OQ-07 会话持久化、OQ-10 成本统计细节。OQ↔D 映射见架构 13.1 节。
