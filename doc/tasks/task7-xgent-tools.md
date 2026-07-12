# Task 7: xgent_tools

> 对应实现指导：`doc/plans/step7-xgent-tools.md`
> 前置：step1 xgent_core、step3 xgent_settings_core 已完成

## 任务清单

### 阶段一：脚手架

- [ ] T-7.1 创建 crate 目录与 Cargo.toml
  - 依赖：无
  - 验收：`crates/xgent_tools/Cargo.toml` 存在；依赖为 xgent_core、xgent_settings_core、serde、serde_json、tokio、async-trait、thiserror；**不依赖 bevy**；`cargo check -p xgent_tools` 通过。

- [ ] T-7.2 注册到 workspace
  - 依赖：T-7.1
  - 验收：`cargo metadata` 识别。

### 阶段二：抽象 trait 与安全策略

- [ ] T-7.3 实现 `tool.rs` 的 Tool trait 与辅助类型
  - 依赖：T-7.1
  - 验收：定义 `ToolCtx`（project_root + tool_policy）、`ToolResult`（output/success/side_effect）、`SideEffect`（FileWritten/CommandRun）、`SecurityPolicy`（Approved/NeedsConfirmation/Denied）、`Tool` trait（id/schema/policy/execute）；编译通过。

- [ ] T-7.4 实现 `security.rs` 的 resolve_policy
  - 依赖：T-7.3
  - 验收：`resolve_policy(tool_id, tool_default, policy)`：denied 列表→Denied，approved 列表→Approved，否则用 tool_default；编译通过。

- [ ] T-7.5 实现 `confirm.rs`
  - 依赖：T-7.3
  - 验收：定义 `ConfirmRequest`（tool_id/input/summary）、`ConfirmDecision`（Allow/AllowAll/Deny）；编译通过。

### 阶段三：执行器

- [ ] T-7.6 实现 `executor.rs` 的 ToolExecutor
  - 依赖：T-7.3, T-7.4, T-7.5
  - 验收：`execute(tool_id, input, ctx, confirm_fn)`：查工具→resolve_policy→Denied 拒绝/Approved 直执行/NeedsConfirmation 经 confirm_fn 取决策→执行或拒绝；编译通过。

- [ ] T-7.7 实现 AllowAll 会话级允许集合
  - 依赖：T-7.6
  - 验收：executor 维护 session 级 allowed 集合，ConfirmDecision::AllowAll 后同类工具本次会话不再确认；编译通过。

### 阶段四：内置工具

- [ ] T-7.8 实现 `builtins/read_file.rs`
  - 依赖：T-7.3
  - 验收：impl Tool，policy 返回 NeedsConfirmation；execute 读 `project_root.join(path)`，路径越界（`..` 逃逸）校验拒绝；返回内容或错误；编译通过。

- [ ] T-7.9 实现 `builtins/write_file.rs`
  - 依赖：T-7.3
  - 验收：impl Tool，policy NeedsConfirmation；execute 写文件，返回 `SideEffect::FileWritten(path)`；路径越界校验；编译通过。

- [ ] T-7.10 实现 `builtins/search_files.rs`
  - 依赖：T-7.3
  - 验收：impl Tool，policy NeedsConfirmation；execute 调系统 `rg`（不存在则降级内置子串搜索）返回匹配行；编译通过。

- [ ] T-7.11 实现 `builtins/run_command.rs`
  - 依赖：T-7.3
  - 验收：impl Tool，policy NeedsConfirmation；execute 用 `tokio::process::Command` 捕获 stdout/stderr，返回 `SideEffect::CommandRun`；编译通过。

### 阶段五：注册与测试

- [ ] T-7.12 实现 `lib.rs` 的 default_tools
  - 依赖：T-7.8~T-7.11
  - 验收：`default_tools() -> Vec<Arc<dyn Tool>>` 返回四个内置工具；编译通过。

- [ ] T-7.13 工具执行测试
  - 依赖：T-7.12
  - 验收：临时项目测 ReadFile（存在/不存在）、WriteFile（写后读回）、SearchFiles（造文件搜匹配）、RunCommand（`echo hello`）。

- [ ] T-7.14 安全策略测试
  - 依赖：T-7.4, T-7.6
  - 验收：默认 NeedsConfirmation；approved 提升后自动执行；denied 拒绝；NeedsConfirmation 在 Allow/Deny 下分别执行/拒绝。

- [ ] T-7.15 路径越界测试
  - 依赖：T-7.8, T-7.9
  - 验收：`read_file`/`write_file` 传 `../../etc/passwd` 被拒或裁剪到项目内。

- [ ] T-7.16 验证不依赖 Bevy
  - 依赖：T-7.12
  - 验收：`cargo tree -p xgent_tools` 不含 bevy。

## 完成标志

- `cargo check -p xgent_tools` 通过
- `cargo test -p xgent_tools` 全绿
- `cargo tree -p xgent_tools` 不含 bevy
- 四个内置工具可用，安全策略默认 NeedsConfirmation 且可配置覆盖
