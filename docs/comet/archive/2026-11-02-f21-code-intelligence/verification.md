---
generated_from_state_version: 8
---

# 验证

## 当前结果

- 结果: **已归档**
- 验证情况: **已完成检查，验证结果已确认**
- 目标周期: 1
- 迭代: 1
- 验证器尝试次数: 1
- 完成时间: 2026-11-02T07:48:45.182Z
- 摘要: 对候选 870dd545（f21-code-intelligence 未提交工作区）独立核对：新增 xgent_codeintel（符号/import/复杂度、确定性增量、忽略规则、只读）、xgent_context RepoMapContextProvider（策略 Ready、评分检索、预算、on_file_changed）、ContextHub 广播与 xgent_app fs 桥接接线、Cargo 与文档同步。实际运行 `CARGO_PROFILE_TEST_DEBUG=0 cargo test -p xgent_codeintel -p xgent_context -j 2`（9 + 54 全绿）与 `cargo check --workspace -j 2`（通过）；`cargo test -p xgent_app` 因 F: 盘满（环境问题）未能完成，已列入风险。A1–A23 逐项核对均有测试或代码级证据，判定 pass。

## 验收

| 编号 | 结果 | 来源 | 验收项 | 原因 |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | A1 **符号提取**：对 Rust fixture 解析出函数/结构体/枚举/trait/impl/module 的符号，名称、种类、文件与起止行正确；函数复杂度可计算；重复解析结果逐位一致。 | 独立运行 `CARGO_PROFILE_TEST_DEBUG=0 cargo test -p xgent_codeintel -p xgent_context` 全绿（codeintel 9/9、context 54/54）。`extracts_symbols_ranges_and_complexity` 覆盖函数/方法/结构体/枚举/trait/impl/module/const/static/type alias 的名称与 kind；`symbol_ranges_match_known_lines` 精确断言 struct 行 1-1、fn 行 3-5；`scan_is_deterministic_and_incremental` 断言同快照两次扫描 FileEntry 与 generation 逐位相等。代码侧 `parse_source` 用 tree-sitter 节点位置生成 1-based 起止行、file 字段写相对路径。 |
| A2 | passed | brief.md | A2 **import/use 解析**：工作区内 `use`/`mod` 解析到对应文件或模块；外部依赖标记 external 且不产生工作区文件边；无法解析时不报错。 | `resolves_workspace_imports_and_marks_external` 断言 `use crate::foo::Hello` 与 `use crate::foo::bar::Nested` 解析到 src/foo.rs、src/foo/bar.rs，`use external_crate::Thing` 记 External；`unresolved_imports_stay_unresolved_without_error` 断言 `crate::missing` 与 `super::up` 记 Unresolved 且无任何 Workspace 边、扫描不报错。`mod foo;` 由 `resolve_mod` 尝试 foo.rs / foo/mod.rs（代码核对）。 |
| A3 | passed | brief.md | A3 **增量更新**：修改一个文件后只更新该文件的符号/import，其他文件结果不变；删除文件后其符号消失；generation 随之变化且可重复。 | `scan_is_deterministic_and_incremental` 断言 update_file 后其他文件 FileEntry 逐位相等、新符号可查，remove_file 后符号消失且 generation 改变；`update_from_disk_handles_removed_files` 覆盖磁盘删除。update_file/remove_file 均调用 generation_of 重算（代码核对：路径 + 内容 FNV 哈希确定性聚合）。 |
| A4 | passed | brief.md | A4 **RepoMap 检索**：`build_context_provider` 对 `repo_map` 返回 Ready；以符号/文件/当前文件查询返回相关结构化结果，受 token 预算约束；无结果返回空而不报错。 | `repo_map_is_ready` 断言 build_context_provider(RepoMap)=Ready；`retrieves_matching_symbols`、`file_hints_and_path_queries_return_the_whole_file_map`、`current_file_is_prioritized_even_without_query_match`、`respects_token_budget`、`no_match_returns_empty_without_error` 全绿。实现按符号名精确 +20/包含 +8、路径命中 +3/词、当前文件 +50 评分，输出结构化 ContextChunk 并受 max_tokens 约束。 |
| A5 | passed | brief.md | A5 **变更通知**：`RepoMapContextProvider::on_file_changed` 对 `.rs` 文件增量刷新索引，后续查询看到新符号；非 Rust 文件与不存在路径不破坏索引。 | `on_file_changed_updates_index_and_ignores_non_rust` 断言 .rs 改名后检索到 new_name、README.md 与 missing.rs 后索引仍可用；`hub_broadcasts_file_changes_through_trait` 验证 ContextHub 广播。xgent_app 新增 ContextProviderHandle 与 forward_context_file_changed 把 FileChangedEvent 转发给 ContextHub（`cargo check --workspace` 通过，代码核对）。 |
| A6 | passed | brief.md | A6 **只读与后台**：索引过程不写项目文件；provider 首次构建在后台执行，查询调用不阻塞 UI 主线程；错误时返回空/降级而非 panic。 | `scan_is_read_only_and_skips_unreadable_files` 与 `retrieve_and_change_notification_do_not_write_files` 断言扫描/检索/通知前后源文件字节一致；`missing_root_degrades_to_empty` 断言根缺失返回空。代码核对：首次索引与增量更新都在 tokio spawn_blocking 执行，scan 失败仅 warn 后返回 None（降级为空），无 panic。 |
| A7 | passed | specs/code-intelligence/spec.md | 符号类别与范围 - **WHEN** 索引包含函数、结构体、枚举、trait、impl、module、const/static 的 fixture - **THEN** 每类符号都能按文件与名称查询，起止行覆盖对应语法节点 - 验收：A1 | `extracts_symbols_ranges_and_complexity` 的 fixture 同时含函数、方法（impl/trait 内）、结构体、枚举、trait、impl、module、const、static、type alias，逐项断言名称与 kind；`symbols_of`（按文件）与名称查找可用，impl 范围断言包含其方法。起止行由语法节点 position 直接生成。 |
| A8 | passed | specs/code-intelligence/spec.md | 函数复杂度 - **WHEN** 函数体内包含 if/match/循环/`&&`/`\|\|` 等决策点 - **THEN** 复杂度为 1 加决策点数量，且随源码变化可重算 - 验收：A1 | 复杂度 = 1 + 决策节点（if/match arm/while/loop/for/`?`/`&&`/`\|\|`，跳过嵌套 function_item）；`symbol_ranges_match_known_lines` 精确断言单 if 函数复杂度为 2；复杂方法（if+&&+match）断言 >=4。update_file 走同一 parse_source 重新解析，随源码变化可重算（代码核对）。 |
| A9 | passed | specs/code-intelligence/spec.md | 忽略目录 - **WHEN** 工作区存在 `target/`、`node_modules/` 或 `.gitignore` 命中的 Rust 文件 - **THEN** 这些文件不进入索引 - 验收：A1 | `skips_ignored_and_non_rust_files` 在同一 fixture 放置 target/、node_modules/、.gitignore 命中目录、隐藏 .hidden.rs 与非 Rust 文件，断言索引只保留 src/a.rs。扫描器启用 hidden/git_ignore/git_global/git_exclude/parents/require_git(false) 并叠加 SKIP_DIRS 过滤。 |
| A10 | passed | specs/code-intelligence/spec.md | 工作区 use 解析 - **WHEN** 源文件包含 `use crate::foo::Bar` 或 `mod foo;` 且对应模块文件存在 - **THEN** 产生从当前文件到目标文件/模块的 import 记录 - 验收：A2 | `resolves_workspace_imports_and_marks_external` 断言 `use crate::foo::Hello` 与 `use crate::foo::bar::Nested` 产生到 src/foo.rs、src/foo/bar.rs 的 Workspace import 记录；`mod foo;` 经 `resolve_mod` 解析到 foo.rs/foo/mod.rs（代码核对，与 use 共用 Workspace 断言）。 |
| A11 | passed | specs/code-intelligence/spec.md | 外部依赖 - **WHEN** 源文件引用工作区外的 crate 或无法解析的路径 - **THEN** 记录标记为 external 或未解析，不产生工作区文件边、不报错 - 验收：A2 | `unresolved_imports_stay_unresolved_without_error` 断言 external_crate 记 External、crate::missing 与 super::up 记 Unresolved，且不存在任何 Workspace 边；扫描成功、不返回错误、不 panic。 |
| A12 | passed | specs/code-intelligence/spec.md | 重复扫描确定 - **WHEN** 对同一快照连续扫描两次 - **THEN** 符号、import 与 generation 完全一致 - 验收：A1、A3 | `scan_is_deterministic_and_incremental` 对相同快照扫描两次，断言 generation 相等、FileEntry（含 symbols/imports/content_hash）逐位相等。实现用 BTreeMap + 符号按行/名、import 按 derive Ord 排序，保证顺序确定。 |
| A13 | passed | specs/code-intelligence/spec.md | 单文件更新 - **WHEN** 调用 `update_file` 修改一个文件或调用 `remove_file` 删除一个文件 - **THEN** 只有该文件相关条目变化，其他文件结果保持不变；generation 更新 - 验收：A3 | update_file 后只有目标文件 FileEntry 变化（其他文件逐位相等断言）、新符号可查；remove_file 后符号消失且 generation 断言改变；update_file 同样重算 generation（代码核对 generation_of）。 |
| A14 | passed | specs/code-intelligence/spec.md | 按文件查询 - **WHEN** 调用方查询一个已索引文件 - **THEN** 返回该文件的符号、import 与复杂度摘要 - 验收：A1、A6 | `CodeIndex::file/symbols_of` 返回该文件 FileEntry（symbols 含 complexity、imports）；测试用同一 FileEntry 断言符号/kind 与 imports，并断言复杂度；RepoMap chunk 渲染 path、kind/name/行范围/复杂度与工作区 import 摘要。 |
| A15 | passed | specs/code-intelligence/spec.md | 未索引文件 - **WHEN** 调用方查询不存在或未索引的文件 - **THEN** 返回空结果而不是 panic - 验收：A6 | `query_missing_file_returns_none_without_panic` 断言不存在/未索引路径 file 与 symbols_of 返回 None、symbols_named 返回空 Vec，不 panic。 |
| A16 | passed | specs/code-intelligence/spec.md | 策略可用 - **WHEN** 以 `repo_map` 策略构造上下文 provider - **THEN** 返回 Ready，而不是 Unsupported - 验收：A4 | `repo_map_is_ready` 断言 `ContextStrategy::RepoMap` 构造为 BuiltContextProvider::Ready；`unimplemented_strategies_report_unsupported` 已收窄为 Vector/Hybrid，仍禁止静默回退。 |
| A17 | passed | specs/code-intelligence/spec.md | 结构化检索 - **WHEN** 用户消息或 hints 提到已索引的符号/文件 - **THEN** 返回相关文件与符号的 repo map 文本片段，token 总数不超过请求预算 - 验收：A4 | `retrieves_matching_symbols`（符号名）、`file_hints_and_path_queries_return_the_whole_file_map`（@file: hint 与路径）、`current_file_is_prioritized_even_without_query_match`（当前文件）、`respects_token_budget`（max_tokens=1 时 total_tokens<=1）全绿；chunk 为 path + 符号 kind/name/行范围/复杂度 + imports 摘要的结构化文本。 |
| A18 | passed | specs/code-intelligence/spec.md | 无匹配 - **WHEN** 查询没有任何符号或文件匹配 - **THEN** 返回空 chunks，不报错、不阻塞 - 验收：A4 | `no_match_returns_empty_without_error` 断言无符号/路径匹配时 chunks 为空、total_tokens=0，不报错；实现无匹配直接返回默认 ContextResult，不 panic、不阻塞。 |
| A19 | passed | specs/code-intelligence/spec.md | 变更后检索 - **WHEN** 已构建索引后某 `.rs` 文件新增一个函数并触发 `on_file_changed` - **THEN** 后续查询能检索到新函数，其他文件结果不变 - 验收：A5 | `on_file_changed_updates_index_and_ignores_non_rust` 已构建索引后改写 src/a.rs 并调用 on_file_changed，后续检索命中 new_name；更新路径 clone 现索引后仅对变更文件 update_file_from_disk，其他条目保持（代码核对）。 |
| A20 | passed | specs/code-intelligence/spec.md | 非 Rust 变更 - **WHEN** `on_file_changed` 收到非 `.rs` 路径或不存在路径 - **THEN** 索引保持不变且不 panic - 验收：A5 | 同一测试对 README.md（非 .rs）与 missing.rs（不存在）调用 on_file_changed，随后检索仍命中且不 panic；extension 与前缀检查、NotFound 删除路径均不破坏索引。 |
| A21 | passed | specs/code-intelligence/spec.md | 不写盘 - **WHEN** 扫描与查询一个工作区 - **THEN** 项目文件字节不变 - 验收：A6 | `scan_is_read_only_and_skips_unreadable_files` 断言扫描前后 a.rs 与 bad.rs 字节一致；`retrieve_and_change_notification_do_not_write_files` 断言 retrieve + on_file_changed 后源文件字节一致。实现只读源码与 .gitignore。 |
| A22 | passed | specs/code-intelligence/spec.md | 后台首次构建 - **WHEN** 首次调用 RepoMap 检索 - **THEN** 扫描在阻塞线程池执行，异步调用方在完成前不被同步阻塞 - 验收：A6 | `ensure_index` 首次缺失时用 `tokio::task::spawn_blocking(CodeIndex::scan)` 并 await，扫描不占用 async worker、异步调用方不被同步阻塞；缓存存在时直接返回，build_lock 防并发重复扫描（代码核对，无专门时序测试）。 |
| A23 | passed | specs/code-intelligence/spec.md | 解析失败降级 - **WHEN** 某个 Rust 文件无法读取或解析 - **THEN** 该文件被跳过，索引继续构建，不 panic - 验收：A6 | `scan_is_read_only_and_skips_unreadable_files` 注入非法 UTF-8 的 src/bad.rs，断言扫描成功、坏文件被跳过、正常文件可查且不 panic；parse 失败路径返回空 FileEntry 继续（代码核对），符合 spec 的“跳过并降级为空/部分索引”。 |

## 检查

_没有记录 Runtime 检查。_

### Builder 报告的证据

以下为 Builder 报告，不等同于 Runtime 检查凭据或独立验收结果。

- cargo check --workspace -j 2: passed — F-21 worktree 全量编译检查通过（1m37s）。
- CARGO_PROFILE_TEST_DEBUG=0 cargo test --workspace -j 2: passed — 71 个测试套件全部 ok，0 failed；其中 xgent_codeintel 8、xgent_context 54、xgent_app 11 + ipc_integration。
- cargo test -p xgent_codeintel -j 2: passed — 新增 query_missing_file_returns_none_without_panic 后 9 passed, 0 failed。
- cargo test -p xgent_codeintel -p xgent_context -j 2: passed — 两个 crate 单测全绿（context 54 passed）。
- 已知限制: 首期仅 Rust；直接调用边、Git 变更频率、多语言、3D L2 符号卫星/虫洞不在本期范围。
- 已知限制: 首次索引为全量扫描且不设硬文件上限；超大仓库的首次 RepoMap 延迟取决于后台扫描时间（不阻塞 UI）。
- 已知限制: 未做真实 daemon → IPC fs.changed → FileChangedEvent → RepoMap 的进程级端到端测试；已单测 ContextHub 广播，并编译验证 app 桥接接线。
- 已知限制: 模块解析为 best-effort：不支持 #[path]、复杂嵌套 use 分组、宏生成模块；无 crate/self/super 前缀的首段按外部 crate 处理。
- 已知限制: 查询分词面向 ASCII 标识符；中文查询主要靠路径或 @file: hint 命中，不匹配 Rust 符号名。
- 已知限制: 复杂度按语法决策点计数（match 每个 arm 计 1），不是认知复杂度。

## 阻塞项

_无。_

## 风险与跳过的工作

- `cargo test -p xgent_app` 未能完成：编译 xgent_ui 测试变体时 rustc-LLVM 报 "IO failure on output stream: no space on device"（F: 盘 100% 满，Avail 0）。该次构建未设置 CARGO_PROFILE_TEST_DEBUG=0，产生了完整调试信息的大量产物，属验证过程中的环境资源问题而非候选缺陷；xgent_app 侧接线仅由 `cargo check --workspace`（通过）与代码核对覆盖，未跑其测试套件。F: 盘仍为 0 可用，后续构建/归档前需要清理空间。
- A3/A13 的“update_file 后 generation 变化”、A8 的“随源码变化可重算”、A19 的 provider 级“其他文件不变”、A22 的非阻塞时序只有代码级证据，无专门断言测试。
- 复杂度把每个 match arm 与 `?` 计为决策点（设计文档已注明）；测试仅对单 if 做精确数值断言，复杂函数用下界断言（>=4）。
- `mod foo;` 与 `use` 在同一测试中共享到 src/foo.rs 的 Workspace 边断言，未单独区分；mod 解析正确性来自代码核对。
- RepoMap 预算用 `max_tokens.max(1)` 作下限，若调用方传 max_tokens=0 仍可能返回 1 token；默认检索预算为 8000，当前未触发。
- `ContextStrategy` 的文档注释仍写“MVP 仅用 OnDemand…其余为后续阶段占位”，与 RepoMap 已实现略有出入（不在本次验收要求的两份文档内）。
- on_file_changed 每次深拷贝整个 CodeIndex 再原子替换，超大仓库下增量刷新有额外内存/CPU 成本（正确性无碍）。
- Builder handoff 的 known_limits 称“无 crate/self/super 前缀的首段按外部 crate 处理”，实际实现会先尝试工作区解析再记 External（说明性偏差，非仓库文档）。
- 无 daemon→IPC→RepoMap 的进程级端到端测试；ContextHub 广播与 app 桥接分别由单测和编译验证覆盖。

## 之前的迭代

| 目标周期 | 迭代 | 尝试 | 结果 | 未解决项 | 摘要 | 完成时间 |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 1 | pass | — | 对候选 870dd545（f21-code-intelligence 未提交工作区）独立核对：新增 xgent_codeintel（符号/import/复杂度、确定性增量、忽略规则、只读）、xgent_context RepoMapContextProvider（策略 Ready、评分检索、预算、on_file_changed）、ContextHub 广播与 xgent_app fs 桥接接线、Cargo 与文档同步。实际运行 `CARGO_PROFILE_TEST_DEBUG=0 cargo test -p xgent_codeintel -p xgent_context -j 2`（9 + 54 全绿）与 `cargo check --workspace -j 2`（通过）；`cargo test -p xgent_app` 因 F: 盘满（环境问题）未能完成，已列入风险。A1–A23 逐项核对均有测试或代码级证据，判定 pass。 | 2026-11-02T07:48:45.182Z |



## 结论

对候选 870dd545（f21-code-intelligence 未提交工作区）独立核对：新增 xgent_codeintel（符号/import/复杂度、确定性增量、忽略规则、只读）、xgent_context RepoMapContextProvider（策略 Ready、评分检索、预算、on_file_changed）、ContextHub 广播与 xgent_app fs 桥接接线、Cargo 与文档同步。实际运行 `CARGO_PROFILE_TEST_DEBUG=0 cargo test -p xgent_codeintel -p xgent_context -j 2`（9 + 54 全绿）与 `cargo check --workspace -j 2`（通过）；`cargo test -p xgent_app` 因 F: 盘满（环境问题）未能完成，已列入风险。A1–A23 逐项核对均有测试或代码级证据，判定 pass。
