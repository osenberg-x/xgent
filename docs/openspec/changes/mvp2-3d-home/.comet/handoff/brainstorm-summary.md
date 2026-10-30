# Brainstorm Summary

- Change: mvp2-3d-home
- Date: 2026-10-30
- Status: confirmed（2026-10-30，用户已批准）

## Confirmed Technical Approach

### 1. G7 spike（gate）

- 位置：`crates/xgent_app/examples/g7_spike.rs`，只用于测量，不进入产品代码。
- 场景：`Camera3d` 渲染到 `RenderTarget::Image`，2k 个同 mesh/material 的实例化星体（billboard/球体）+ 轨道相机 + 单盏灯。
- 测量：预热后采样 N 帧；记录 `app.update()` 墙钟与 Bevy `FrameTimeDiagnostics` 帧时间；测试机为 Intel UHD 730（集显）为主、NVIDIA T400 为参考；窗口默认 1280×800，另记 1080p 压力值。
- 判定：2k@60fps → 800/关特效@60fps → 2D fallback；结果写入 `docs/notes/g7-3d-perf.md`。

### 2. L1 空间图（`xgent_spatial`）

- 新增 `ScanOptions { l1: bool, node_limit: usize, ignore: bool }` 与 `scan_with_options`；`scan` 保留 L0 行为，避免破坏现有 S0 测试。
- 遍历使用 `ignore` crate（ripgrep 同款 walker）：`.gitignore`、隐藏文件、`.git/`；再叠加内置 `target/`、`node_modules/`、构建产物。
- 节点：目录 → `Module`，文件 → `File`，`Contains` 边；有 `Cargo.toml` 时叠加 crate 节点/`DependsOn` 边。
- 权重：首期用文件字节数（不读内容）；目录权重为子节点聚合。LOC/复杂度留给 F-21。
- 聚合：从目录层开始保留目录簇；若节点数仍超上限，按目录保留 weight 最高的文件，记录 `omitted`；上限 2k，G7 降级时 800。
- 布局：复用 S0 确定性布局 + 增量锚定；同一快照坐标逐位一致。
- 执行：`spawn_blocking` + 取消 token；扫描中先显示目录骨架，完成后替换。

### 3. 3D 视口（`xgent_ui/src/space/`）

- 模块：`mod.rs`（插件/资源）、`view.rs`（Center 面板 + `ImageNode`）、`camera.rs`（轨道相机）、`pick.rs`（CPU 拾取）、`labels.rs`（HUD）、`sync.rs`（图→实例）、`degrade.rs`（档位）。
- 离屏：`Camera3d.target = RenderTarget::Image(handle)`；`ImageNode` 承载；窗口 resize/DPI 变化按物理像素重建纹理。
- 拾取：用 `UiGlobalTransform` 取视口矩形（0.20 UI 节点没有渲染侧 `GlobalTransform`），屏幕坐标 → 视口 UV → 相机射线/半径判定；悬停/单击/双击共用一套命中。
- 交互：轨道相机（拖拽/滚轮/方向键/`+`/`-`，无惯性）；单击选中；双击/Enter 打开；`Esc` 回宇宙；HUD 标签走 `Ellipsis`，上限 24。
- 降级：`ViewQuality` 资源（Full/Reduced/Static）；节点上限 2k/800；HUD 显示档位；视口不可见/失焦暂停渲染。

### 4. 布局反转（panel registry）

- 新 id：`space`（Center，默认可见）、`rail-space`（FocusMain）；`chat` 改 SideDock；`editor`/`preview`/`diff`/`terminal` 改 Center。
- `PanelActivate` 驱动：打开文件 → 编辑器 Center；打开终端 → 终端 Center；`Esc` → `space`。
- 2D fallback：`preferences.three_d_home: bool`（默认 true）；关闭后 Center 默认 `chat`、编辑器/终端回 SideDock；设置面板加开关，`settings_io` 与 daemon 白名单加键。
- 侧栏宽度：`preferences.chat_side_width: f32`（默认 400，范围 320–560 或可用宽度 45%）；`PanelWidths` 读取/写回；拖拽结束时保存。
- 图标：新增 `space` 图标或复用现有 `panel-right` 作为首期占位；rail 入口与 i18n `title_key` 补齐。

## Key Tradeoffs and Risks

1. **G7 harness**：`xgent_app/examples/g7_spike.rs`（推荐）vs `xgent_ui` feature 模块。前者不污染产品代码，后者便于复用 UI 但增加 feature 分支。
2. **`.gitignore` 实现**：新增 `ignore` crate（推荐）vs 手写最小解析器。前者覆盖嵌套规则/隐藏/二进制，代价是一个小依赖；后者零依赖但边缘情况多。
3. **文件权重**：文件字节数（推荐，首期便宜）vs 行数/LOC（更接近“代码量”但需读全部文件）。LOC 留给 F-21。
4. **面板迁移**：注册表迁移（推荐）vs 保留 SideView 再重挂到 Center。前者与 v8 M5 模型一致；后者会出现两套可见性权威。
5. **侧栏宽度持久化**：`preferences.chat_side_width`（推荐）vs 仅会话内 `PanelWidths`。spec 要求重启保留，必须落配置。

## Test Strategy

- `xgent_spatial`：Rust/非 Rust fixture、忽略规则、节点上限/聚合、确定性布局、增量锚定、取消；纯单测 + headless。
- `xgent_ui`：panel registry/visibility 契约、Center 单活跃、2D fallback 切换、chat 宽度持久化、拾取投影纯函数、视口 `ImageNode` 装配；headless Bevy 测试按现有装配清单。
- 集成：打开项目 → 3D 首页 → 悬停/选中 → 双击打开 → `Esc`；记录手工走查。
- G7：Intel UHD 730 / NVIDIA T400 手工跑，记录 GPU/驱动/分辨率/帧时间；不进 CI。
- 回归：`cargo test -p xgent_spatial -p xgent_ui -p xgent_app`；现有 `panel_registry_contract`、`scroll_contract`、`transcript_render_contract` 保持全绿。

## Spec Patch

None。三个 delta spec 已有验收场景；设计不改变需求边界。

## Open Questions

- OQ-11：首次启动是否引导 2D/3D；默认 3D + 偏好开关，先不阻塞。
- OQ-12：目录聚合阈值与 2k 上限的精确映射；实现时用真实仓库调参。
- OQ-13 / D-3D-08：自由飞行/惯性后续可选模式；不在本变更。
