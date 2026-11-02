# Proposal: MVP-2 3D 首页

## Why

XGent 的差异化来自 README 的 3D 代码宇宙，但当前产品仍是 2D agent IDE：`xgent_spatial` 只有 crate 级 L0 内核，UI 没有视口，布局仍以对话为主区。requirements v3 已把 3D 首页定为 MVP-2 当前主线（§4.2/§7.2），并钉死 L1 数据、轨道相机、G7 退化梯与布局反转；本次变更把这个边界落成可验收的实现。

工作区里 requirements v3、AGENTS、3D 视口文档等 14 份文档的改动是本变更的需求输入，已由本次会话完成；它们不是另一条实现变更。

## What Changes

- **G7 性能门**：离屏纹理 + 实例化节点，2k 节点 @60fps（默认 1280×800 窗口，优先集显）；不过降 800 节点/关特效 @60fps；仍不过回退 2D。记录测试机、GPU 型号/驱动与 1080p 压力值。
- **L1 空间图**：`xgent_spatial` 从 crate 级 L0 扩到文件/目录级 L1，任意项目可用；扫描忽略 `.gitignore`/`target`/`node_modules`；2k 节点上限与目录聚合；保留确定性布局与增量锚定。
- **3D 首页视口**：`xgent_ui` 新增 3D 视口；轨道相机；悬停高亮；单击选中、双击/Enter 打开文件；`Esc` 回宇宙；HUD 标签；所有首期操作有键盘路径。
- **布局反转**：3D 占 `Slot::Center`；对话移到 `SideDock` 常驻；编辑器/终端打开时接管 Center；2D fallback 偏好，关掉 3D 后 MVP-1 的全部功能可用。
- **性能降级**：2k → 800 → 2D 的节点梯；画质梯只在 800 档内使用；HUD 显示当前降级档位。
- **非目标**：完整 S2（键盘遍历/搜索飞行/2D↔3D 联动）、S3 时空回溯、S4 插件节点、L2 符号/import/调用边、自由飞行/惯性、MCP/宠物/成本/代码智能/TUI/Web。

## Capabilities

### New Capabilities

- `spatial-code-graph`: L1 文件/目录图、确定性布局、增量锚定、扫描忽略规则与节点上限。
- `3d-home-view`: 3D 视口的渲染、轨道相机、悬停/选中/打开/`Esc`、键盘路径、G7 性能与降级策略。
- `workspace-layout`: `Slot::Center`/`SideDock` 的面板排布、对话右栏、编辑器/终端接管主区、2D fallback。

### Modified Capabilities

无。项目当前没有已归档的 specs，三个能力均为新增。

## Impact

- **crates**：`xgent_spatial`（L1 扫描/聚合）、`xgent_ui`（3D 视口 + 面板注册 + 布局）、`xgent_app`（装配与偏好）；`xgent_core` 如需扩展 `SpatialGraph` 类型会同步。
- **依赖**：Bevy 0.20 的 `3d` 默认特性已在依赖图里，不新增渲染器依赖。
- **不改**：daemon、JSON-RPC、provider/tools/context/agent 契约。
- **文档**：requirements v3、3D 视口文档、layout 架构是本变更的输入；`docs/dev-tutorial.md` 在实现后同步。
- **风险**：G7 不过则本变更停在 gate，不进入 S1/布局；L1 大仓扫描与目录聚合是主要性能风险；编辑器/终端从 SideView 迁到 Center 需要面板注册表迁移。
