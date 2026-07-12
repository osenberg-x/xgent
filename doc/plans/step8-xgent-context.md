# Step 8: xgent_context

## 模块职责

项目上下文检索层：根据当前对话决定把哪些代码/上下文喂给 LLM。

1. **抽象 trait `ContextProvider`**：统一检索接口，支持未来升级（A→B→C→D→E）。
2. **MVP 实现 `OnDemandContextProvider`**（方案 A：无索引·按需读取 + 目录树 + ripgrep）。
3. **ContextQuery**：描述一次上下文检索请求（用户问题、当前文件、相关文件线索）。
4. **ContextChunk**：检索返回的上下文片段（文件路径、内容片段、相关性说明）。
5. **演进占位**：RepoMap（B）、Vector（C）、Lsp（D）、Hybrid（E）的 trait 实现占位，MVP 不实现。

## 前置依赖

- xgent_core（错误类型）
- xgent_settings_core（ProjectConfig 的 ContextStrategy）

## 目标文件结构

```
crates/xgent_context/
├── Cargo.toml
└── src/
    ├── lib.rs              # 模块导出 + 构造函数（按 strategy 选实现）
    ├── provider.rs         # ContextProvider trait + ContextQuery + ContextChunk
    ├── on_demand.rs       # 方案 A：无索引·按需读取
    ├── repo_map.rs        # 方案 B 占位（P1）
    ├── vector.rs          # 方案 C 占位
    ├── lsp.rs             # 方案 D 占位
    └── hybrid.rs          # 方案 E 占位
```

## Cargo.toml

```toml
[package]
name = "xgent_context"
version = "0.1.0"
edition = "2024"

[dependencies]
xgent_core = { path = "../xgent_core" }
xgent_settings_core = { path = "../xgent_settings_core" }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
async-trait = { workspace = true }
thiserror = { workspace = true }
```

说明：MVP 不依赖 Bevy——context 是纯异步逻辑，agent 侧桥接到 ECS。方案 A 不需嵌入模型/向量库，依赖 ripgrep 子进程。tree-sitter（B 阶段）后续引入。

## 关键类型与接口

### 1. provider.rs — 抽象 trait

```rust
use async_trait::async_trait;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// 一次上下文检索请求
#[derive(Debug, Clone)]
pub struct ContextQuery {
    pub user_message: String,           // 用户当前问题
    pub current_file: Option<PathBuf>, // 当前打开文件（若有）
    pub hints: Vec<String>,             // 额外线索（文件名、符号名等）
    pub max_tokens: u32,                // 上下文预算
}

/// 检索返回的上下文片段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextChunk {
    pub path: PathBuf,
    pub content: String,         // 文件内容或片段
    pub relevance: String,       // 相关性说明，给 LLM 理解
    pub token_estimate: u32,
}

/// 检索结果
#[derive(Debug, Clone, Default)]
pub struct ContextResult {
    pub chunks: Vec<ContextChunk>,
    pub tree_summary: Option<String>,  // 目录树摘要（方案 A 用）
    pub total_tokens: u32,
}

/// 上下文提供者抽象
#[async_trait]
pub trait ContextProvider: Send + Sync {
    /// 检索上下文
    async fn retrieve(&self, query: &ContextQuery) -> ContextResult;

    /// 通知文件变更（供索引类实现增量更新，方案 A 空实现）
    async fn on_file_changed(&self, _path: &PathBuf) {}
}
```

### 2. on_demand.rs — 方案 A 实现

```rust
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use crate::provider::{ContextProvider, ContextQuery, ContextResult, ContextChunk};

pub struct OnDemandContextProvider {
    project_root: PathBuf,
}

impl OnDemandContextProvider {
    pub fn new(project_root: PathBuf) -> Self { Self { project_root } }
}

#[async_trait]
impl ContextProvider for OnDemandContextProvider {
    async fn retrieve(&self, query: &ContextQuery) -> ContextResult {
        // 策略：
        // 1. 生成项目目录树摘要（限制深度与文件数，token 预算内）
        //    - 跳过 .git、target、node_modules 等
        //    - 树结构作为 tree_summary
        // 2. 若有 current_file，优先读其内容
        // 3. 用用户问题关键词跑 ripgrep，取匹配文件
        //    - rg --files-with-matches <keyword> <project_root>
        //    - 读匹配文件内容（按 max_tokens 裁剪）
        // 4. 组装 chunks + tree_summary，控制总 token 不超 max_tokens
        ContextResult { /* ... */ }
    }
}

impl OnDemandContextProvider {
    /// 生成目录树摘要
    async fn tree_summary(&self, max_entries: usize) -> Option<String> { /* tokio::fs 遍历 */ }

    /// ripgrep 搜索
    async fn rg_search(&self, pattern: &str) -> Vec<PathBuf> { /* 调 rg 子进程 */ }

    /// 读文件内容（带 token 估算与裁剪）
    async fn read_file_chunk(&self, path: &Path, max_tokens: u32) -> Option<ContextChunk> { /* ... */ }
}
```

### 3. 占位实现（B/C/D/E）

```rust
// repo_map.rs (B 阶段占位)
pub struct RepoMapContextProvider { project_root: PathBuf }
#[async_trait]
impl ContextProvider for RepoMapContextProvider {
    async fn retrieve(&self, _q: &ContextQuery) -> ContextResult {
        ContextResult::default()  // P1 实现：tree-sitter 符号图
    }
}

// vector.rs (C 阶段占位) - 同上
// lsp.rs (D 阶段占位) - 同上
// hybrid.rs (E 阶段占位) - 同上
```

### 4. lib.rs — 构造函数

```rust
use std::path::PathBuf;
use xgent_settings_core::ContextStrategy;
use crate::provider::ContextProvider;

pub fn build_context_provider(
    strategy: ContextStrategy,
    project_root: PathBuf,
) -> Box<dyn ContextProvider> {
    match strategy {
        ContextStrategy::OnDemand => Box::new(OnDemandContextProvider::new(project_root)),
        ContextStrategy::RepoMap => Box::new(RepoMapContextProvider { project_root }),  // 占位
        ContextStrategy::Vector => Box::new(VectorContextProvider { project_root }),
        ContextStrategy::Hybrid => Box::new(HybridContextProvider { project_root }),
    }
}
```

## 实现要点

1. **不依赖 Bevy**：纯异步，agent 侧桥接 ECS。未来上移 daemon（B 阶段索引、Web 端检索）时可直接复用。
2. **方案 A 策略**：
   - 目录树摘要：遍历项目，跳过常见忽略目录（.git/target/node_modules/build/dist/.xgent），限制深度（如 3 层）与条目数（如 200），渲染成树文本。
   - ripgrep 搜索：用 `tokio::process::Command` 调系统 `rg`，`--files-with-matches` 取文件。若 rg 不存在，降级为内置简单遍历+子串匹配（性能差但可用）。
   - 文件读取：按 token 预算裁剪（简单估算：1 token ≈ 4 字符，或按字节粗估）。
3. **token 预算**：ContextQuery 带 max_tokens，实现需控制总输出不超预算（先树摘要 + 优先文件 + 搜索结果填充）。
4. **演进接口**：`on_file_changed` 在方案 A 是空实现（无索引），B+ 阶段做增量更新。trait 已定义，切换实现即升级。
5. **strategy 切换**：构造函数按 ProjectConfig 的 ContextStrategy 选实现，调用方（agent）无感于具体策略。
6. **目录树忽略规则**：用内置默认 + 项目 `.xgentignore` 文件（类似 .gitignore）覆盖，MVP 可先用内置默认。
7. **ripgrep 检测**：可在应用启动时检测 rg 是否可用，记录到配置，影响检索策略选择。

## 验证方法

1. **编译检查**：
   ```bash
   cargo check -p xgent_context
   ```
2. **目录树测试**：在临时项目造文件结构，断言 tree_summary 含预期文件、跳过 .git 等。
3. **搜索测试**：造几个含关键词的文件，rg_search 返回正确文件列表。
4. **端到端检索测试**：给定 ContextQuery（含用户问题与 max_tokens），断言返回 chunks 不超预算、含相关文件。
5. **降级测试**：模拟 rg 不存在，断言降级路径仍工作。
6. **trait 切换测试**：build_context_provider 各 strategy 返回不同实现（占位返回空结果不报错）。

## 完成后下一步

xgent_context 完成后 → 实现 **xgent_agent**（agent loop + 对话编排 + ECS 桥接），它组合 provider/tools/context 并接入 Bevy ECS。
