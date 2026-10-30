# MVP-2 布局手工走查记录

- 日期：2026-10-30
- 构建：`cargo build -p xgent_app -p xgent_daemon`
- 隔离方式：`XGENT_AGENT_DIR=target/walkthrough/{universe,classic}-agent`，不触碰开发机真实配置
- 截图：`target/walkthrough/shots/universe3.png`、`target/autoplay_top.png`

## 走查结果

| 场景 | 结果 |
|:---|:---|
| 宇宙默认（`three_d_home = true`） | Center 显示 3D 空间（本仓库 766 节点 / 799 边）；对话常驻右侧 400px；顶部「完整」档位角标正常 |
| 经典回退（`three_d_home = false`） | 对话接管 Center；右侧分屏无可见面板时整块收起；侧栏页签不含「空间」 |
| 经典回退 + 打开文件（`XGENT_AUTOPLAY`） | 编辑器落在右侧分屏（默认 720px），对话仍在 Center；侧栏页签为 预览 / 差异 / 终端 |
| 文件抽屉 / 设置 / 会话历史 | Overlay 层，不受布局模式影响（既有测试全绿） |
| 终端 | `Slot::SideDock`（经典）/ `Slot::Center`（宇宙），激活与关闭路径由 `space_panel` 与终端测试覆盖 |

## 走查中发现并修复

1. **3D 主区全黑**：相机初始距离 30，而默认布局簇半径 420，整张图落在视锥外。
   改为首次加载按节点包围球取景（中心 + 半径 × 2.8），并让星体尺寸 / 边粗细随图半径缩放；
   后续数据刷新不再移动相机。
2. **经典回退暴露「空间」页签**：`space` 在回退模式下被搬到 SideDock 但未从槽位页签过滤，
   点击会在侧栏打开死视图。已在 `sync_slot_tab_bar` 按模式过滤。
3. **回退编辑器被 400px 挤窄**：对话 400px 只属于宇宙模式；经典回退的编辑器坞保持 720 默认，
   对话宽度偏好只在宇宙模式落树。
4. **400px 对话栏顶部挤压**：「上下文」标签被挤成竖排、文件 chip 把 token 计数挤出视口。
   标签改为不收缩，chips 收进 `flex_grow + clip_x` 容器，token 计数固定保留。

## 5.1 端到端路径

| 步骤 | 证据 |
|:---|:---|
| 打开项目 → 3D 首页 | `target/walkthrough/shots/universe3.png`（767 节点；对话右栏 400px） |
| 悬停 / 单击 / 双击 / Enter | CPU 拾取与打开文件由 `space_panel` 投影命中、Enter `OpenFileRequest` 测试覆盖；双击与 Enter 共用 `open_selected` 分支 |
| 打开文件 → 编辑器接管 Center | `target/walkthrough/shots/universe-editor.png`（宇宙：编辑器占 Center、对话仍在右栏）；`target/autoplay_top.png`（经典：编辑器在右侧、对话在 Center） |
| Esc → 回 3D 首页、相机不漂移 | `space_panel::escape_returns_from_editor_to_space_and_keeps_camera`：断言编辑器失活、`space` 可见、相机 `Transform.translation` 逐位相同 |

说明：本机自动化会话无法稳定给 Bevy 窗口注入鼠标/键盘焦点，双击与 Esc 的**人工点击**
未作为截图证据；两条路径分别由上述 headless 投影/消息测试与 Esc 集成测试覆盖。
5.2 的真实仓库性能复测见 `docs/notes/g7-3d-perf.md`「真实仓库复测」。

## 自动化回归

```bash
cargo test -p xgent_ui            # 全部通过（29 个测试二进制）
cargo test -p xgent_app           # 全部通过
cargo test -p xgent_daemon config_store
cargo test -p xgent_settings_core
```

布局专项：

- `crates/xgent_ui/tests/layout_mode.rs`：默认宇宙、关闭偏好回经典、重新打开、经典页签不含空间。
- `crates/xgent_ui/tests/space_panel.rs`：`Esc` 从编辑器回 space 且相机位置不变。
- `crates/xgent_ui/src/resize.rs`：320–560 / 45% 钳制、偏好落树、仅宇宙拖拽持久化。