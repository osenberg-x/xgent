# Tasks

> G7 是 gate：若 2k 与 800 档都达不到 60fps，停在 group 1，记录 2D fallback 结论，不进入 group 3/4。

## 1. G7 性能 spike（gate）

- [x] 1.1 建立独立 Bevy spike：3D 离屏纹理 + 2k 实例化 billboard/星体 + 轨道相机；验证：spike 可启动、渲染 2k 节点并输出一帧截图/日志 <!-- comet-task:5e5c0500-a7bc-4fcd-af5a-e142625d56a5 -->
- [x] 1.2 在默认 1280×800 窗口、优先集显下测 2k 与 800 档帧时间，另记 1080p 压力值；验证：记录含 GPU 型号/驱动、实测 fps 与帧时间 <!-- comet-task:a3be7abb-4771-4dd6-9eb8-fb7d53440f23 -->
- [x] 1.3 按 2k→800→2D 写 go/no-go 结论；验证：`docs/notes/g7-3d-perf.md` 存在且明确下一步；若 2k 不过记录 800 数据，若 800 不过记录 2D fallback 决定 <!-- comet-task:6bc19bd5-56a4-4d6b-80e9-29bb43421d1e -->
- [x] 1.4 把 G7 结论回写 3D 视口文档的测试基准；验证：`docs/design/ui-v8-3d-viewport.md` 的基准段与实测一致 <!-- comet-task:75b731d5-8d6a-4ab1-9704-ece56d4c4266 -->

## 2. L1 空间图

- [x] 2.1 扩展 `xgent_spatial::scan` 生成 `Module`/`File` 节点与 `Contains` 边；验证：Rust 与非 Rust fixture 的节点类型/数量单测通过 <!-- comet-task:a917865e-9080-4cae-8c5d-b2b627af305a -->
- [x] 2.2 实现忽略规则（`.gitignore` + 内置 `target/`、`node_modules/`、`.git/`、构建产物）；验证：fixture 中被忽略路径不出现在图中；规则解析失败只记一次警告 <!-- comet-task:41850355-cb87-4ce9-b42d-1983d50bfdb2 -->
- [x] 2.3 目录聚合 + 2k/800 上限 + 省略计数；验证：10k 文件 fixture 节点数不超过上限且 `omitted > 0`；小仓库不裁剪 <!-- comet-task:c0c42ea9-5154-4663-9a8c-59c33efbaa7c -->
- [x] 2.4 保持确定性布局与增量锚定；验证：重复布局坐标逐位相同；新增节点时既有节点坐标不变（扩展现有 S0 测试） <!-- comet-task:48bbda3a-68fe-4a7f-8af0-945c1e0a2853 -->
- [x] 2.5 扫描放到 `spawn_blocking` 并支持取消；验证：大 fixture 扫描期间主线程不被阻塞（计时/headless 测试），取消后不再回写图 <!-- comet-task:83408bd8-8348-4839-8a46-b5516f48fc7e -->
- [x] 2.6 在 `docs/dev-tutorial.md` 补 L1 扫描/忽略/聚合说明；验证：文档段落与实现一致 <!-- comet-task:8b2f11e8-4c2f-498a-80fd-24f68f36b9cb -->

## 3. 3D 视口 S1 + 点击

- [x] 3.1 用离屏纹理 + `ImageNode` 在 Center 面板承载视口；验证：视口可见，窗口 resize 后纹理尺寸正确（headless/截图） <!-- comet-task:ca8e1f14-ece9-4a4f-8cfe-512d740a4815 -->
- [x] 3.2 实例化星体/边 + LOD0/L1 + HUD 标签（`Ellipsis`）；验证：2k fixture 渲染成功、长路径标签不溢出、主题切换后颜色更新 <!-- comet-task:c397d5ad-4c18-4d7f-b64a-bc448c47e05a -->
- [x] 3.3 轨道相机（拖拽/滚轮/方向键/`+`/`-`，无惯性，数据刷新不漂移）；验证：输入测试与相机状态断言通过 <!-- comet-task:d627c694-7570-4d0b-a339-5645787cc18d -->
- [x] 3.4 CPU 拾取：悬停高亮、单击选中、双击/Enter 打开；验证：headless 投影命中测试 + 打开文件消息断言 <!-- comet-task:9e3592e7-fa80-4ecc-a27e-75dab7b198fc -->
- [x] 3.5 `Esc` 从编辑器/终端返回宇宙，相机保持离开前位置；验证：快捷键测试与相机状态断言 <!-- comet-task:5cbbf458-24ec-4786-b74b-2c53e132605b -->
- [x] 3.6 运行期降级：800 档/静态首帧/HUD 档位；验证：强制低配档位时 HUD 显示且渲染循环按预期降级 <!-- comet-task:a1c3d02e-5c81-47a2-a369-65b32f534f8e -->
- [x] 3.7 在 `docs/dev-tutorial.md` 补 3D 视口使用与限制；验证：文档段落存在且与实际行为一致 <!-- comet-task:ce5a8e32-4039-434e-88f3-dc579b9907ab -->

## 4. 布局反转

- [x] 4.1 注册 `space` 为 Center 面板并接通激活/失活；验证：`panel_registry_contract` 扩展通过，Center 单活跃 <!-- comet-task:473f8fc2-0c1a-4b30-9184-b66fa8b2e490 -->
- [x] 4.2 对话迁到 SideDock；编辑器/终端注册为中心面板；验证：打开文件/终端激活对应 Center，对话仍可见（headless 布局断言） <!-- comet-task:088872cb-8aba-4ffe-86b0-278cf47707ab -->
- [x] 4.3 Center 默认改为 `space`，`Esc` 回 space；新增 `three_d_home` 偏好；验证：启动默认空间；关闭偏好后回 MVP-1 布局（集成测试） <!-- comet-task:f70fc6ec-d1ac-4969-99d7-2154d6e55583 -->
- [x] 4.4 对话侧栏默认 400px、可拖拽、320–560（或可用宽度 45%）限制、重启保留；验证：拖拽/持久化测试通过 <!-- comet-task:77f436b8-c9a9-4c20-839e-0c99bc51d9fe -->
- [x] 4.5 2D fallback 全功能回归：对话、编辑器、终端、文件抽屉、设置、会话历史；验证：相关既有测试全绿 + 手工走查记录 <!-- comet-task:e1451fb9-4330-457e-bf09-26791d5f0790 -->
- [x] 4.6 同步 `docs/dev-tutorial.md` 与 `docs/design/ui-v8-layout-architecture.md` 的 MVP-2 布局说明；验证：文档段落与实现一致 <!-- comet-task:99d10128-c5a5-4338-a5fe-3889279c4d7f -->

## 5. 集成验收

- [x] 5.1 端到端走查：打开项目 → 3D 首页 → 悬停/单击 → 双击打开 → `Esc` 返回；验证：集成测试或带时间戳的手工走查记录 <!-- comet-task:978fb922-60ce-4996-bc98-bf7649725001 -->
- [x] 5.2 用真实仓库（含非 Rust 项目）复测 L1 + 2k 上限 + 60fps；验证：G7 报告更新，记录实测 <!-- comet-task:b6be4072-7275-4488-9266-5b0e76dfbdbc -->
- [x] 5.3 全量回归：`cargo test -p xgent_spatial -p xgent_ui -p xgent_app`；验证：命令输出全绿 <!-- comet-task:be1ae731-7047-4d59-ac61-6826734f7678 -->
- [x] 5.4 文档一致性检查：requirements/3D 文档/dev-tutorial/README 无旧「2D only / 可选插件层 / 下一期 M8」表述；验证：grep 结果为空 <!-- comet-task:7fd0f875-fe15-42e0-a973-3de63c46ebb0 -->

## Workflow follow-up

- Archive the change after verification passes.
- Record the delivery method (local/push/pr) and verify it before closing.
