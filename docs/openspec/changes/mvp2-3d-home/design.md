---
comet_change: mvp2-3d-home
role: technical-design
canonical_spec: openspec
---

# Design

## Context

当前代码状态：

- `xgent_spatial` 只有 S0：从 `Cargo.toml` 生成 crate 级 L0 图与确定性布局，不读文件树；`SpatialNodeKind` 已有 `Module`/`File`，但没有生产者。
- `xgent_ui` 的 `Slot::Center` 当前挂对话；编辑器/预览/终端是 `Slot::SideDock` 的页签；没有 3D 视口模块。
- Bevy 0.20 的 `3d` 是默认特性，`bevy_pbr`/`bevy_core_pipeline` 已在依赖图中；不需要新增渲染依赖，但也没有任何 `Camera3d`/`RenderTarget` 装配。
- 需求边界见 `docs/design/requirements.md` §4.2/§7.2 与 `docs/design/ui-v8-3d-viewport.md` §7/§8；本设计只解释实现路径，不重复需求。

本变更是一个 full change，按 G7 → L1 → S1 → 布局反转的顺序推进；G7 是 go/no-go gate，未通过不进入后续 UI 工作。

## Goals / Non-Goals

**Goals:**

- 用独立的 G7 spike 先证明 2k 节点 / 60fps 可行，并给出 800 / 2D 的降级判据。
- 让 `xgent_spatial` 对任意项目产出 L1 文件/目录图，保持确定性、增量锚定与只读投影。
- 在 `xgent_ui` 内用离屏纹理承载 3D 视口，完成轨道相机、悬停、选中、双击/Enter 打开与 `Esc` 返回。
- 把 3D 变成默认首页：对话常驻右栏，编辑器/终端打开时接管主区，2D fallback 可用。

**Non-Goals:**

- 完整 S2（键盘遍历、搜索飞行、2D↔3D 联动）、S3 时空回溯、S4 插件节点。
- L2 符号/import/调用边、LSP、多语言解析。
- 自由飞行/惯性（OQ-13）、第一人称相机。
- 在 3D 内渲染源码文本或做 3D 编辑器。

## Decisions

### D1. G7 先做独立 spike，不先接产品 UI

用一个最小的 Bevy spike（离屏纹理 + 2k 个实例化 billboard/星体 + 轨道相机）测帧时间，测试机为默认 1280×800 窗口、优先集显，另记 1080p 压力值。判定顺序：2k@60fps → 800 节点/关特效@60fps → 停用 3D 首页回退 2D。

- **Why**：如果先写 S1/布局再发现跑不动，返工面覆盖 `xgent_ui` 与面板注册表；spike 的产物只是测量与结论，失败可以干净停止。
- **Alternative**：直接写 S1，边做边测。被否：失败时已经投入布局迁移。

### D2. 视口用离屏纹理，不用第二台摄像机直出

3D 摄像机渲染到 `RenderTarget::Image`，UI 用 `ImageNode` 承载；布局、裁剪、模态栈仍由 2D 系统权威。

- **Why**：直出摄像机只能铺满窗口，无法裁剪进面板；UI 容器会吞指针；离屏让视口成为普通面板。
- **Alternative**：`Camera3d` 全屏 + 2D UI 叠加。被否：清屏/排序/拾取与既有 2D 装配纠缠。

### D3. 拾取走 CPU 射线/屏幕距离，不用 mesh picking

视口自己把指针坐标映射到视口 UV，再用相机投影做节点中心/半径命中；不为节点建碰撞体，也不依赖 `mesh_picking`。

- **Why**：UI 节点先吃指针；2k 个节点的碰撞体维护成本高；CPU 判定对 billboard 足够。
- **Alternative**：Bevy `mesh_picking`。被否：与 UI picking 冲突，且与 LOD/实例化不同步。

### D4. L1 扫描放在 `xgent_spatial`，保持无 Bevy

扩展 `scan`：遍历文件树、应用忽略规则、生成 `Module`/`File` 节点与 `Contains` 边；有 `Cargo.toml` 时叠加 crate 节点/`DependsOn` 边。扫描与目录聚合在 `spawn_blocking` 中执行，结果回 UI 线程。

- **Why**：数据层可 headless 测试，未来 2D 依赖图可复用；不把 Bevy 拖进布局内核。
- **Alternative**：放在 `xgent_context`。被否：把可视化数据塞进检索层，边界变浑。

### D5. 忽略规则 = `.gitignore` + 内置跳过表

扫描时解析项目根的 `.gitignore`（含常见嵌套规则的最小实现），并始终跳过 `target/`、`node_modules/`、`.git/`、构建产物；解析失败只记一次警告，不阻塞打开。

- **Why**：任意项目都要有宇宙；只靠内置表会让 JS/Python 项目把依赖目录画进宇宙，节点数瞬间爆掉。
- **Alternative**：只做内置跳过。被否：覆盖不了用户自定义的生成目录。

### D6. 节点上限与目录聚合在扫描/布局之间完成

先按目录聚合（目录簇 + 文件星），再按 weight 裁剪到 2k；G7 降级时上限改为 800。被省略的节点数写入图统计，HUD/报告可见。

- **Why**：2k 是性能预算，不是文件数；聚合必须发生在布局前，否则布局本身就被大仓拖垮。
- **Alternative**：全量布局后 LOD 裁剪。被否：布局成本已经付出。

### D7. 视口是 `Slot::Center` 的注册面板，布局反转最后做

先把 3D 视口注册为 Center 面板（可通过一个临时开关/命令打开），完成 S1 + 点击打开；最后再把 Center 默认从对话改为空间、对话迁到 SideDock、编辑器/终端迁到 Center。`Slot::Center` 的单活跃规则不变。

- **Why**：视口与布局迁移可以分开验证；先做布局会让 S1 在没有稳定视口时无法回归。
- **Alternative**：先反转布局再写视口。被否：中间状态没有可用的 3D 主视图。

### D8. 对话侧栏可调宽，默认 400px

`PanelWidths` 增加 chat 侧栏宽度：默认 400，最小 320，最大 `min(560, 可用宽度 × 0.45)`；拖拽沿用现有 resize 手柄，宽度持久化。

- **Why**：3D 主区需要宽度；对话不能固定 720（现有 SideDock 默认值），也不能窄到读不了工具卡。
- **Alternative**：固定 360/480。被否：不同窗口宽度下体验差异大。

### D9. 2D fallback 是一个偏好，不是隐藏开关

新增全局偏好 `three_d_home`（默认 true，G7 通过后生效）；关闭后 Center 默认对话、编辑器/终端回 SideDock，MVP-1 布局与功能完整保留。设置面板加一个开关，变更即时生效、不重启。

- **Why**：README 的渐进式沉浸要求 2D 始终可用；G7 失败时也能直接落到 2D，而不需要改代码。
- **Alternative**：只在启动参数/环境变量控制。被否：用户不可达。

### D10. 数据流：扫描 → 图 → 布局 → 视口，全部只读

```
file tree (ignore rules)
   -> SpatialGraph (L1 nodes + Crate nodes/edges)
   -> deterministic layout (background)
   -> viewport sync (instances + HUD labels)
```

文件监听/Agent 状态只触发增量图更新；视口不写文件、不改配置、不改会话。相机在数据刷新时保持不动（spec: 轨道相机）。

- **Why**：3D 永远不是真相源；刷新不能打断用户的空间记忆。
- **Alternative**：每次刷新重建整图并重置相机。被否：空间记忆失效，逐帧抖动。

## Implementation Shape

### G7 spike（不进入产品代码）

- 位置：`crates/xgent_app/examples/g7_spike.rs`；`Camera3d` + `RenderTarget::Image` + 2k 个同 mesh/material 实体 + 轨道相机。
- 采样：预热 60 帧，记录 300 帧的 `app.update()` 墙钟与 Bevy `FrameTimeDiagnostics`；报告 p50/p95 与 fps。
- 机器：Intel UHD 730（gate）、NVIDIA T400（reference）；窗口 1280×800，另测 1080p 压力值。
- 输出：`docs/notes/g7-3d-perf.md`（GPU/驱动/分辨率/节点数/档位/结论）。

### L1 扫描

```text
ScanOptions { l1, node_limit, ignore }
scan_with_options(root, options) -> SpatialGraph
  walk (ignore crate): .gitignore + hidden + .git + built-in target/node_modules
  -> dir nodes (Module) + file nodes (File) + Contains edges
  -> crate nodes/DependsOn edges when Cargo.toml exists
  -> aggregate/cap (2k or 800) + omitted count
  -> deterministic layout (S0) + anchors
```

- `scan` 保持 L0 行为；新增 `scan_with_options`，不破坏现有 S0 测试。
- 权重：文件字节数；目录权重为子节点聚合；不读文件内容（LOC 留给 F-21）。
- 聚合：先保目录簇；超限时按目录保留 weight 最高的文件；`omitted` 写入图统计。
- 线程：`spawn_blocking` + cancel；UI 先显示目录骨架，完成后替换。

### 视口

- `xgent_ui/src/space/{mod,view,camera,pick,labels,sync,degrade}.rs`。
- `view`：Center 面板，`ImageNode` + 离屏 `Image`；用 `UiGlobalTransform` 取视口矩形（不使用 `GlobalTransform`）。
- `camera`：轨道球坐标（target/yaw/pitch/dist），无惯性；方向键与 `+`/`-`。
- `pick`：屏幕 → 视口 UV → 相机投影；节点中心/半径命中；悬停/单击/双击共用同一判定。
- `labels`：2D 叠加，`Ellipsis`，同屏 ≤24，按距离/选中排序。
- `degrade`：`ViewQuality { Full, Reduced, Static }`；800 档关特效；HUD 显示档位；视口不可见/失焦暂停渲染。
- 3D 不渲染源码；双击/Enter 通过现有打开文件消息交棒 2D 编辑器。

### 面板迁移

| id | 旧 | 新 |
|:---|:---|:---|
| `chat` | Center | SideDock |
| `editor` / `preview` / `diff` / `terminal` | SideDock | Center |
| `space` | — | Center（默认） |

- `PanelActivate` 驱动；Center 单活跃；`Esc` 回 `space`。
- `preferences.three_d_home: bool` 默认 true；关闭后 `chat` 回 Center、2D 视图回 SideDock。
- `preferences.chat_side_width: f32` 默认 400，范围 320–560 或可用宽度 45%；`PanelWidths` 读写；拖拽结束保存。
- `settings_io` 白名单/apply 加两键；设置面板加 3D 开关；daemon global whitelist 加键。
- 新增 `space` 图标（或首期复用 `panel-right`）；`rail-space` 使用 FocusMain。

## Test Strategy

- `xgent_spatial`：Rust/非 Rust fixture、忽略规则、节点上限/聚合、确定性布局、增量锚定、取消；纯单测 + headless。
- `xgent_ui`：panel registry/visibility 契约、Center 单活跃、2D fallback 切换、chat 宽度持久化、拾取投影纯函数、视口 `ImageNode` 装配；headless Bevy 按现有装配清单（UiPlugin/WindowPlugin/PickingPlugin/ImagePlugin/OverlayPlugin/EditorPlugin/RenderTargetInfo）。
- 集成：打开项目 → 3D 首页 → 悬停/选中 → 双击/Enter 打开 → `Esc`；记录手工走查。
- G7：Intel UHD 730 / NVIDIA T400 手工跑，记录 GPU/驱动/分辨率/帧时间；不进 CI。
- 回归：`cargo test -p xgent_spatial -p xgent_ui -p xgent_app`；现有 `panel_registry_contract`、`scroll_contract`、`transcript_render_contract` 保持全绿。

## Risks / Trade-offs

- **`.gitignore` 规则边缘** → 使用 `ignore` crate 的标准过滤器（嵌套、否定、隐藏、二进制），并用 fixture 覆盖；新增依赖在 workspace 固定版本。
- **G7 不过** → 按 800 / 2D 退化梯执行；spike 先行，不进入 S1/布局。若 800 仍不过，3D 首页停用，保留 2D。
- **L1 扫描大仓卡顿** → 忽略规则 + `spawn_blocking` + 2k 聚合 + 取消边界；扫描期间先显示目录骨架。
- **离屏纹理 resize/DPI 抖动** → 按物理像素与 scale factor 计算纹理尺寸，窗口 resize 时延迟一帧重建；HUD 用 2D 层，不随 3D 缩放。
- **CPU 拾取偏差** → 用投影后的节点中心与半径，加容差；悬停/选中与点击共用同一判定；headless 测试锁定。
- **面板迁移破坏既有侧区** → 保留 2D fallback；先把 Center 面板注册与激活跑通，再迁移编辑器/终端；`panel_registry_contract` 扩展。
- **对话侧栏过窄** → 默认 400 + 可拖宽 + 最小 320；工具卡摘要用 `Ellipsis`，不在窄栏里强制横向滚动。
- **3D 渲染循环抢帧** → 视口不可见/窗口失焦时暂停渲染（spec 的性能降级）；帧预算 8ms。

## Migration Plan

1. **G7 spike**：独立实验，记录 GPU/驱动/分辨率/帧时间；产出 go/no-go。
2. **L1 数据**：先合入 `xgent_spatial` 的文件/目录图与忽略规则；无 UI，可 headless 验证。
3. **S1 视口**：注册 Center 面板，完成离屏纹理、轨道相机、拾取、打开/返回；用临时入口验证。
4. **布局反转**：切换 Center 默认、对话 SideDock、编辑器/终端 Center、2D fallback 偏好；跑完整回归。
5. **回滚**：任一步失败，关闭 `three_d_home` 或停在 G7 gate；2D 布局不受影响。

## Open Questions

- OQ-11：首次启动是否让用户选 2D/3D？当前默认 3D，保留偏好开关；引导形式待定。
- OQ-12：L1 目录聚合阈值（目录簇 vs 文件星）与 2k 上限的精确映射；实现时用真实仓库调参。
- OQ-13 / D-3D-08：自由飞行/惯性是否作为后续可选模式；不在本变更。
