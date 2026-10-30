# G7 3D 性能 spike 记录

- 日期：2026-10-30
- 变更：`mvp2-3d-home`
- 目的：验证 2k 节点 + 离屏纹理在集显上能否 60fps，并按 2k → 800 → 2D 给出 G7 go/no-go。
- 工具：`crates/xgent_app/examples/g7_spike.rs`（一次性 spike，不进入产品代码）

## 测试环境

- 机器：Windows 11 Pro（kernel 26200）/ Intel Core i5-12400 / 31.7 GiB
- 集显（gate）：Intel UHD Graphics 730，driver 101.7088，Vulkan
- 独显（reference）：NVIDIA T400，driver 595.97，Vulkan
- 场景：2k 个同 mesh/material 的球体（半径 0.18），单方向光，轨道相机；`Camera3d` 渲染到 `RenderTarget::Image(1280×800)`，2D sprite 显示；预热 120 帧、采样 300 帧。
- 构建：`CARGO_PROFILE_RELEASE_DEBUG=0 cargo build -p xgent_app --example g7_spike --release`

## 命令

```bash
WGPU_ADAPTER_NAME=intel G7_NODES=2000 target/release/examples/g7_spike.exe
WGPU_ADAPTER_NAME=intel G7_NODES=800 target/release/examples/g7_spike.exe
WGPU_ADAPTER_NAME=intel G7_NODES=2000 G7_WIDTH=1920 G7_HEIGHT=1080 target/release/examples/g7_spike.exe
WGPU_ADAPTER_NAME=intel G7_NODES=2000 G7_VSYNC=0 target/release/examples/g7_spike.exe
WGPU_ADAPTER_NAME=nvidia G7_NODES=2000 target/release/examples/g7_spike.exe
```

## 结果

| GPU | 节点 | 分辨率 | VSync | avg fps | p50 ms | p95 ms | 说明 |
|:---|:---:|:---:|:---:|:---:|:---:|:---:|:---|
| Intel UHD 730 | 2000 | 1280×800 | on | 60.0 | 16.68 | 17.58 | **G7 gate：通过** |
| Intel UHD 730 | 800 | 1280×800 | on | 59.9 | 16.68 | 17.54 | 800 档更稳 |
| Intel UHD 730 | 2000 | 1920×1080 | on | 60.0 | 16.49 | 30.82 | 1080p 压力值，p95 有明显掉帧 |
| Intel UHD 730 | 2000 | 1280×800 | off | 129.5 | 7.47 | 10.03 | 余量约 2× |
| NVIDIA T400 | 2000 | 1280×800 | on | 59.9 | 16.67 | 17.36 | reference |

debug 初测（非 gate）：Intel 60.2 fps、T400 58.2 fps；release 数据以上表为准。

## 判定

- **G7 通过（2k 档）**：Intel UHD 730 在默认 1280×800 窗口稳定 60fps；no-vsync 129.5fps 说明还有约 2× 余量，vsync 下的 p95 17.58ms 属于呈现/调度抖动，不是 GPU 饱和。
- **800 档不需要启用**：2k 已过；800 节点/关特效仍保留为运行期退化梯的第二级。
- **2D fallback 不需要启用**：保留为最后一级，用于未来真实视口（HUD/边/拾取）集成后若性能回退的兜底。
- **1080p 不是 MVP-2 的默认目标**：1080p 压力值 p95 30.82ms，说明高分辨率下会掉帧；默认窗口仍为 1280×800，1080p 作为后续优化项。

## 后续

- S1 集成（HUD 标签、边、拾取、主题）完成后按同一命令复测；若 2k 掉出 60fps，按 800 → 2D 退化梯执行。
- spike 未包含标签/边/拾取/后台扫描，结论仅覆盖「离屏纹理 + 实例化节点 + 轨道相机」这部分预算。

## 真实仓库复测（2026-10-30，S1 集成后）

- 项目：`F:\ws\xgent`（本仓库，含 Rust + 非 Rust 文件）
- 版本：`CARGO_PROFILE_RELEASE_DEBUG=0 cargo build -p xgent_app -p xgent_daemon --release`
- 运行：`XGENT_AGENT_DIR=target/walkthrough/universe-agent XGENT_AUTOPLAY=no-such-file.rs target/release/xgent_app.exe --project F:\ws\xgent`
  （autoplay 只提供帧日志；目标不存在时跳过打开，主区保持 3D 首页）
- 窗口：1280×800，Intel UHD Graphics 730（Vulkan），vsync on

| 项 | 实测 |
|:---|:---|
| L1 节点 / 边 | 767 / 800 |
| 裁剪 | `omitted_nodes = 0`（未触发 2k 上限） |
| 扫描 + 布局耗时 | 打开项目后 **132ms** 完成（release） |
| 稳态帧间隔（120–1000 帧，n=761） | avg **16.69ms** / p50 16.60 / p95 18.40 / max 30.50 |
| 等效 fps | **59.9**（vsync 封顶） |
| >20ms 帧 | 20 / 761（2.6%） |

结论：真实仓库（767 节点，低于 2k 档）在 S1 集成后的产品内保持 60fps，无需启用 800 档或 2D 退化。
debug 版同场景 avg 28.8ms / 34.7fps，属未优化构建，不作为 gate。

截图：`target/walkthrough/shots/universe3.png`（3D 首页）、`target/autoplay_top.png`（打开文件后编辑器接管 Center）。
走查记录：`docs/notes/mvp2-layout-walkthrough.md`。
