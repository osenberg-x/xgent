# Task 8: xgent_context

> 对应实现指导：`doc/plans/step8-xgent-context.md`
> 前置：step1 xgent_core、step3 xgent_settings_core 已完成

## 任务清单

### 阶段一：脚手架

- [ ] T-8.1 创建 crate 目录与 Cargo.toml
  - 依赖：无
  - 验收：`crates/xgent_context/Cargo.toml` 存在；依赖为 xgent_core、xgent_settings_core、serde、serde_json、tokio、async-trait、thiserror；**不依赖 bevy**；`cargo check -p xgent_context` 通过。

- [ ] T-8.2 注册到 workspace
  - 依赖：T-8.1
  - 验收：`cargo metadata` 识别。

### 阶段二：抽象 trait

- [ ] T-8.3 实现 `provider.rs` 的 ContextProvider trait 与类型
  - 依赖：T-8.1
  - 验收：定义 `ContextQuery`（user_message/current_file/hints/max_tokens）、`ContextChunk`（path/content/relevance/token_estimate）、`ContextResult`（chunks/tree_summary/total_tokens）、`ContextProvider` trait（retrieve/on_file_changed 空默认实现）；编译通过。

### 阶段三：方案 A 实现

- [ ] T-8.4 实现 `on_demand.rs` 的 OnDemandContextProvider 结构与 tree_summary
  - 依赖：T-8.3
  - 验收：`tree_summary(max_entries)` 用 tokio::fs 遍历，跳过 .git/target/node_modules/build/dist/.xgent，限深度与条目数，渲染树文本；编译通过。

- [ ] T-8.5 实现 rg_search
  - 依赖：T-8.4
  - 验收：`rg_search(pattern)` 调系统 `rg --files-with-matches`，返回 PathBuf 列表；编译通过。

- [ ] T-8.6 实现 read_file_chunk
  - 依赖：T-8.4
  - 验收：`read_file_chunk(path, max_tokens)` 读文件按 token 预算裁剪（粗估 1 token ≈ 4 字符），返回 ContextChunk；编译通过。

- [ ] T-8.7 实现 retrieve 端到端
  - 依赖：T-8.4, T-8.5, T-8.6
  - 验收：impl `retrieve(query)`：tree_summary + current_file 优先 + rg_search 关键词 + 填充，控制总 token 不超 max_tokens；编译通过。

### 阶段四：占位与构造函数

- [ ] T-8.8 实现 repo_map/vector/lsp/hybrid 占位
  - 依赖：T-8.3
  - 验收：四个占位 struct impl ContextProvider，retrieve 返回空 ContextResult；编译通过。

- [ ] T-8.9 实现 `lib.rs` 的 build_context_provider
  - 依赖：T-8.7, T-8.8
  - 验收：按 ContextStrategy 选实现返回 Box<dyn ContextProvider>；编译通过。

### 阶段五：测试

- [ ] T-8.10 目录树测试
  - 依赖：T-8.4
  - 验收：临时项目造文件结构，tree_summary 含预期文件、跳过 .git 等。

- [ ] T-8.11 搜索测试
  - 依赖：T-8.5
  - 验收：造含关键词文件，rg_search 返回正确文件列表。

- [ ] T-8.12 端到端检索测试
  - 依赖：T-8.7
  - 验收：给定 ContextQuery（含问题与 max_tokens），返回 chunks 不超预算、含相关文件。

- [ ] T-8.13 降级测试
  - 依赖：T-8.5
  - 验收：模拟 rg 不存在，降级路径仍工作（内置子串搜索）。

- [ ] T-8.14 trait 切换测试
  - 依赖：T-8.9
  - 验收：各 strategy 返回不同实现，占位返回空结果不报错。

- [ ] T-8.15 验证不依赖 Bevy
  - 依赖：T-8.9
  - 验收：`cargo tree -p xgent_context` 不含 bevy。

## 完成标志

- `cargo check -p xgent_context` 通过
- `cargo test -p xgent_context` 全绿
- `cargo tree -p xgent_context` 不含 bevy
- 方案 A（OnDemand）可用，B/C/D/E 占位不阻塞，trait 切换可配
