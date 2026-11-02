# 目标

建立 Rust 代码智能管线：用 tree-sitter 从工作区源码提取符号、import/use 关系与复杂度元数据，形成可复用的只读代码索引，供 RepoMap 检索与后续 3D L2/诊断使用。

# 范围

- 仅 Rust（复用 D-06：tree-sitter + tree-sitter-rust 随二进制，不做按需下载）。
- 新建无 Bevy 的数据层 crate `xgent_codeintel`：符号、import/use、复杂度、确定性索引与查询 API。
- 符号：自由函数/方法/结构体/枚举/trait/impl/module/const/static/type alias；记录名称、种类、文件、起止行与函数决策复杂度。
- import/use：解析 `use` 与 `mod`，尽可能解析到工作区文件/模块；外部 crate 标记 external，不报错；无法静态解析的保持未解析，不猜测。
- 索引：只读、确定性；同一源码快照重复结果逐位一致；`update_file`/`remove_file` 增量更新，未受影响文件结果不变。
- 集成：实现 `RepoMapContextProvider` 并让 `build_context_provider` 对 `ContextStrategy::RepoMap` 返回 Ready；`ContextProvider::on_file_changed` 增量刷新索引；导出查询 API 供后续 3D L2/诊断使用。
- 性能：索引构建在 `spawn_blocking`/后台执行，不阻塞 UI；扫描遵守 `.gitignore`/`target`/`node_modules`/隐藏文件规则。

# 非目标

- 直接调用边与完整调用图（本期留后续）。
- Git 历史变更频率、changed-together、热力图/诊断雷达 UI。
- 3D L2 符号卫星/虫洞的可视化；S3 时空回溯、S4 插件节点。
- LSP、类型推导、跨 crate 宏展开、多语言。

# 验收示例

- A1 **符号提取**：对 Rust fixture 解析出函数/结构体/枚举/trait/impl/module 的符号，名称、种类、文件与起止行正确；函数复杂度可计算；重复解析结果逐位一致。
- A2 **import/use 解析**：工作区内 `use`/`mod` 解析到对应文件或模块；外部依赖标记 external 且不产生工作区文件边；无法解析时不报错。
- A3 **增量更新**：修改一个文件后只更新该文件的符号/import，其他文件结果不变；删除文件后其符号消失；generation 随之变化且可重复。
- A4 **RepoMap 检索**：`build_context_provider` 对 `repo_map` 返回 Ready；以符号/文件/当前文件查询返回相关结构化结果，受 token 预算约束；无结果返回空而不报错。
- A5 **变更通知**：`RepoMapContextProvider::on_file_changed` 对 `.rs` 文件增量刷新索引，后续查询看到新符号；非 Rust 文件与不存在路径不破坏索引。
- A6 **只读与后台**：索引过程不写项目文件；provider 首次构建在后台执行，查询调用不阻塞 UI 主线程；错误时返回空/降级而非 panic。

# Decisions

- Q1：首期数据范围为符号 + import/use + 每符号复杂度；调用边与 git 变更频率留后续。
- Q2：首期实现 RepoMap 检索与数据 API；不做 3D L2 符号卫星/虫洞可视化。

# Verification expectations

- 新增数据层 crate/模块的 fixture 单测 + 确定性/增量测试；RepoMap provider 集成测试；只读与后台行为测试。
- `cargo test -p xgent_context -p xgent_ui -p xgent_spatial` 等相关 crate 全绿。
