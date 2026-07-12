# Task 3: xgent_settings_core

> 对应实现指导：`doc/plans/step3-xgent-settings-core.md`
> 前置：step1 xgent_core 已完成

## 任务清单

### 阶段一：脚手架

- [ ] T-3.1 创建 crate 目录与 Cargo.toml
  - 依赖：无
  - 验收：`crates/xgent_settings_core/Cargo.toml` 存在；依赖为 xgent_core、serde、serde_json、thiserror（workspace）、toml、dirs；`cargo check -p xgent_settings_core` 通过（空 lib.rs）。

- [ ] T-3.2 注册到 workspace
  - 依赖：T-3.1
  - 验收：`cargo metadata` 识别该 crate；依赖 xgent_core 解析正确。

### 阶段二：平台路径工具

- [ ] T-3.3 实现 `paths.rs`
  - 依赖：T-3.1
  - 验收：实现 `global_config_dir()`、`global_config_file()`、`sessions_db_path()`、`project_config_dir(root)`、`project_config_file(root)`、`daemon_socket_path()`；用 `dirs::config_dir()`/`cache_dir()` 拼平台路径；编译通过。

- [ ] T-3.4 验证平台路径
  - 依赖：T-3.3
  - 验收：测试断言 macOS 上 `global_config_dir()` 以 `Library/Application Support/xgent` 结尾；`daemon_socket_path()` 含 `xgent`（跨平台合理）。

### 阶段三：全局配置类型

- [ ] T-3.5 实现 `global.rs`
  - 依赖：T-3.1
  - 验收：定义 `GlobalConfig`、`ProviderConfig`、`ProviderKind`（OpenAiCompat/ResponseApi/Anthropic/Ollama/Custom）、`Preferences`；全部 derive `Serialize/Deserialize/Debug/Clone`，`GlobalConfig` derive `Default`；**不 derive Bevy 的 Resource/Reflect**；编译通过。

- [ ] T-3.6 验证 `GlobalConfig` 默认值合理
  - 依赖：T-3.5
  - 验收：`GlobalConfig::default()` 的 `default_provider`/`default_model` 为空串或合理占位（空串，由上层填），`providers` 空 map，`preferences.language` 空串。

### 阶段四：项目配置类型

- [ ] T-3.7 实现 `project.rs`
  - 依赖：T-3.1
  - 验收：定义 `ProjectConfig`、`ContextStrategy`（OnDemand[default]/RepoMap/Vector/Hybrid）、`ToolPolicyConfig`（approved/denied: Vec<String>）；全部 derive serde 标准 trait，不 derive Bevy；`ProjectConfig` derive `Default`；编译通过。

### 阶段五：TOML 读写

- [ ] T-3.8 实现 `store.rs` 的 `GlobalConfigStore`
  - 依赖：T-3.3, T-3.5
  - 验收：`load() -> XgentResult<GlobalConfig>`（读 TOML，文件不存在返回 Default）、`save(cfg)`（写 TOML，确保目录存在）；编译通过。

- [ ] T-3.9 实现 `store.rs` 的 `ProjectConfigStore`
  - 依赖：T-3.3, T-3.7
  - 验收：`load(project_root) -> XgentResult<ProjectConfig>`、`save(cfg)`；读 `<project>/.xgent/config.toml`，不存在返回 Default；编译通过。

- [ ] T-3.10 验证 TOML 往返
  - 依赖：T-3.8, T-3.9
  - 验收：构造含 provider 与 tool_policy 的 GlobalConfig → save → load → 断言相等；临时目录造项目，ProjectConfig save → load → 断言相等。

### 阶段六：lib 导出与一致性

- [ ] T-3.11 实现 `lib.rs` 模块导出
  - 依赖：T-3.3~T-3.9
  - 验收：导出所有公开类型与 store、paths 函数；`cargo doc -p xgent_settings_core` 无警告。

- [ ] T-3.12 验证不依赖 Bevy
  - 依赖：T-3.11
  - 验收：`cargo tree -p xgent_settings_core` 输出不含 bevy（仅 xgent_core/serde/toml/dirs 等）。

## 完成标志

- `cargo check -p xgent_settings_core` 通过
- `cargo test -p xgent_settings_core` 全绿
- `cargo tree -p xgent_settings_core` 不含 bevy
- 配置类型有 TOML 往返测试覆盖
- 类型不 derive 任何 Bevy trait（确认 core 纯净）
