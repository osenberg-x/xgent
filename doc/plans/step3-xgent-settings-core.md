# Step 3: xgent_settings_core

## 模块职责

XGent 配置体系的纯类型层（不依赖 Bevy）：

1. **配置类型**：全局配置（provider 列表、API key、应用偏好）与项目配置（项目级覆盖）。
2. **存储**：全局配置用 TOML 存平台规范路径；项目配置用 TOML 存 `<project>/.xgent/config.toml`。
3. **平台路径工具**：跨平台配置目录与文件路径。
4. **TOML 读写**：`GlobalConfigStore` / `ProjectConfigStore`。

**关键**：本 crate 不依赖 Bevy，使 daemon 与 provider 可使用配置类型而不被 Bevy 拖重（解决 daemon 轻量性矛盾）。

## 前置依赖

- xgent_core（XgentResult 错误类型）

## 目标文件结构

```
crates/xgent_settings_core/
├── Cargo.toml
└── src/
    ├── lib.rs          # 模块导出
    ├── global.rs       # 全局配置类型
    ├── project.rs      # 项目配置类型
    ├── store.rs        # TOML 读写
    └── paths.rs        # 平台规范路径工具
```

## Cargo.toml

```toml
[package]
name = "xgent_settings_core"
version = "0.1.0"
edition = "2024"

[dependencies]
xgent_core = { path = "../xgent_core" }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
toml = "0.8"
dirs = "5"
```

说明：**不依赖 Bevy**。类型只 derive `Serialize`/`Deserialize`/`Debug`/`Clone`，不带 `Resource`/`Reflect`（那些在 `xgent_settings` 包装层做）。

## 关键类型与接口

### 1. paths.rs — 平台规范路径

```rust
use std::path::PathBuf;

/// 全局配置根目录（平台规范）
/// macOS: ~/Library/Application Support/xgent/
/// Windows: %APPDATA%/xgent/
/// Linux: ~/.config/xgent/
pub fn global_config_dir() -> PathBuf { /* dirs::config_dir() + "xgent" */ }

pub fn global_config_file() -> PathBuf { global_config_dir().join("config.toml") }
pub fn sessions_db_path() -> PathBuf { global_config_dir().join("sessions.db") }
pub fn project_config_dir(project_root: &Path) -> PathBuf { project_root.join(".xgent") }
pub fn project_config_file(project_root: &Path) -> PathBuf { project_config_dir(project_root).join("config.toml") }
/// daemon socket 路径（跨进程约定，UI 与 daemon 共用）
pub fn daemon_socket_path() -> PathBuf { /* dirs::cache_dir()/xgent/daemon.sock 或 Windows named pipe 名 */ }
```

### 2. global.rs — 全局配置类型

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct GlobalConfig {
    pub providers: HashMap<String, ProviderConfig>,
    pub default_provider: String,
    pub default_model: String,
    pub preferences: Preferences,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub api_base: String,
    pub api_key: String,           // MVP 明文存 TOML，未来考虑 keychain（D-02）
    pub model_overrides: HashMap<String, String>,
    pub timeout_secs: u64,
    pub max_retries: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum ProviderKind {
    OpenAiCompat,    // OpenAI compatible 接口
    ResponseApi,     // Response API 风格
    Anthropic,       // Anthropic 原生
    Ollama,          // 本地 Ollama（兼容模式）
    Custom,          // 用户自定义
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Preferences {
    pub language: String,   // "zh-CN" / "en-US"
    pub theme: String,       // MVP 仅 "dark"
}
```

### 3. project.rs — 项目配置类型

```rust
use serde::{Deserialize, Serialize};

/// 项目配置（本地隔离，不共享）
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ProjectConfig {
    pub project_root: String,
    pub provider_override: Option<String>,
    pub model_override: Option<String>,
    pub context_strategy: ContextStrategy,
    pub tool_policy: ToolPolicyConfig,   // 工具信任级别覆盖（见 step7）
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub enum ContextStrategy {
    #[default]
    OnDemand,   // MVP：方案 A 无索引·按需读取
    RepoMap,    // B 阶段
    Vector,     // C 阶段
    Hybrid,     // E 阶段
}

/// 工具策略配置：按工具 id 覆盖默认信任级别
/// 默认所有工具 NeedsConfirmation（见架构安全模型 11.1）
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ToolPolicyConfig {
    pub approved: Vec<String>,   // 提升为自动执行
    pub denied: Vec<String>,     // 降为拒绝
}
```

### 4. store.rs — TOML 读写

```rust
use crate::{global::GlobalConfig, paths, project::ProjectConfig};
use xgent_core::XgentResult;

pub struct GlobalConfigStore;
impl GlobalConfigStore {
    pub fn load() -> XgentResult<GlobalConfig> { /* 读 TOML，不存在则 Default */ }
    pub fn save(cfg: &GlobalConfig) -> XgentResult<()> { /* 写 TOML，确保目录存在 */ }
}

pub struct ProjectConfigStore;
impl ProjectConfigStore {
    pub fn load(project_root: &Path) -> XgentResult<ProjectConfig> { /* ... */ }
    pub fn save(cfg: &ProjectConfig) -> XgentResult<()> { /* ... */ }
}
```

## 实现要点

1. **不依赖 Bevy**：类型只 derive serde 标准 trait，不带 `Resource`/`Reflect`。
2. **平台路径**：用 `dirs` crate 获取平台规范配置目录。
3. **全局配置协调**：权威副本在 daemon（见 step6），UI 侧只读副本，写经 daemon。
4. **项目配置隔离**：存项目目录 `.xgent/`，天然隔离。
5. **socket 路径放此**：UI 与 daemon 共用 `daemon_socket_path()`，统一约定。
6. **API Key 暂存明文**：MVP 简化，D-02 留口子。
7. **ContextStrategy/ToolPolicyConfig 提前定义**：MVP 只用 OnDemand，工具默认 NeedsConfirmation，其余占位。

## 验证方法

1. **编译检查**：
   ```bash
   cargo check -p xgent_settings_core
   ```
2. **独立性验证**：`cargo tree -p xgent_settings_core` 不含 bevy。
3. **TOML 往返测试**：GlobalConfig → 存 TOML → 读回 → 断言相等。
4. **项目配置测试**：临时目录造项目，写/读 `.xgent/config.toml`。
5. **平台路径测试**：macOS 断言 `global_config_dir()` 以 `~/Library/Application Support/xgent/` 结尾。

## 完成后下一步

xgent_settings_core 完成后 → 实现 **xgent_settings**（Bevy Resource 包装 + fluent Localizer），它依赖 core 与 xui_i18n。
