# F-21 代码智能管线设计

- 日期：2026-11-02
- 状态：首期实现（Rust 符号 / import / 复杂度 + RepoMap）
- 需求：`docs/design/requirements.md` F-21；验收见 `docs/comet/changes/f21-code-intelligence/`

## 1. 目标与边界

为 Rust 工作区建立只读、确定性的代码结构索引，供 RepoMap 检索与后续 3D L2/诊断读取。

首期做：符号（函数/方法/结构体/枚举/trait/impl/module/const/static/type alias）、`use`/`mod` 解析、函数决策复杂度、单文件增量更新、RepoMap provider 与文件变更通知。

首期不做：直接调用边/完整调用图、Git 变更频率、多语言、LSP/类型推导、3D L2 可视化。

## 2. 分层

```
xgent_codeintel（无 Bevy）
  tree-sitter + tree-sitter-rust
  CodeIndex / FileEntry / Symbol / Import
        ↑
xgent_context::RepoMapContextProvider（懒构建 + 增量）
        ↑
xgent_app：ContextProviderHandle + FileChangedEvent → on_file_changed
```

- `xgent_codeintel` 不依赖任何 Bevy/xgent UI，可被 context 与未来 spatial/UI 复用。
- `RepoMapContextProvider` 首次 `retrieve` 在 `spawn_blocking` 扫描；后续查询读缓存。
- `ContextHub` 把 `on_file_changed` 广播给所有内置/插件 provider；`xgent_app` 的 fs 桥接把 UI 的 `FileChangedEvent` 转发给 ContextHub。

## 3. 数据模型

```rust
CodeIndex { generation, files: BTreeMap<PathBuf, FileEntry> }
FileEntry { symbols: Vec<Symbol>, imports: Vec<Import>, content_hash }
Symbol { name, kind, file, range: SourceRange, complexity, container }
Import { from, target: Workspace(PathBuf) | External(String) | Unresolved(String), raw }
```

- 路径相对项目根；符号/import 按文件、行、名称稳定排序。
- `generation` = 文件路径 + 内容 FNV 哈希的确定性聚合；同一快照可重复。
- 函数复杂度 = 1 + 决策节点数（if、match arm、循环、`&&`/`||`、`?`）；嵌套函数不重复计入外层。

## 4. 解析与解析边界

- tree-sitter-rust 0.23；每个文件 parse 后遍历节点。
- `use` 支持 `crate/super/self` 前缀、`as` 别名和简单 `{a,b}` 分组；模块解析尝试 `<path>.rs` 与 `<path>/mod.rs`。
- `mod foo;` 相对当前文件目录解析；内联 `mod foo {}` 只产生 Module 符号。
- 无法解析的路径记为 `External`（首段是外部 crate）或 `Unresolved`，不报错、不猜测。
- `target/`、`node_modules/`、`.gitignore` 命中的文件不进入索引。

## 5. RepoMap 检索

- 查询词来自 user_message 与 hints（`@file:` 取文件 stem）；current_file 作为独立高权重信号。
- 评分：当前文件 +50；符号名精确 +20、包含 +8；路径命中 +3；container 命中 +4。
- 符号命中时返回匹配符号；路径/hint 命中时返回该文件的符号结构。每个文件生成一个 `ContextChunk`：`path:` + 符号 kind/name/行范围/复杂度 + 工作区 import 摘要；按评分排序，遵守 `max_tokens`。
- 无匹配返回空 chunks；root 不存在/扫描失败返回空结果，不 panic。

## 6. 增量与并发

- `CodeIndex::update_file/remove_file` 只替换目标文件；`update_file_from_disk` 在文件不存在时删除条目。
- provider 的 `on_file_changed` 只处理根目录下 `.rs` 文件；在阻塞线程池解析，完成后原子替换 Arc；非 Rust/不存在路径不影响索引。
- 首次构建与增量更新共用一把异步 build lock，避免重复扫描；不持有 parking_lot 锁跨 await。

## 7. 性能与失败边界

- 首次扫描在 `spawn_blocking`，不阻塞 UI/async worker；单文件解析失败/读取失败跳过。
- 索引本身不写项目文件；只读取源码与 `.gitignore`。
- 当前不设硬文件上限；超大仓库的索引时间由后台任务吸收，后续可复用 L1 的目录聚合/上限策略。

## 8. 验证

- `xgent_codeintel`：符号/范围/复杂度、import 解析（工作区/外部/未解析）、忽略规则、确定性、增量、删除、只读与不可读文件跳过。
- `xgent_context`：`repo_map` 策略 Ready、RepoMap 命中/无匹配/预算/current_file、`on_file_changed` 增量与非 Rust 忽略、缺失 root 降级。
- `xgent_app`：`ContextProviderHandle` 与 `FileChangedEvent → on_file_changed` 接线编译。
