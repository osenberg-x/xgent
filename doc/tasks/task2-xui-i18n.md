# Task 2: xui_i18n

> 对应实现指导：`doc/plans/step2-xui-i18n.md`
> 前置：无（与 step1 无依赖，可并行）

## 任务清单

### 阶段一：脚手架

- [ ] T-2.1 创建 crate 目录与 Cargo.toml
  - 依赖：无
  - 验收：`crates/xui_i18n/Cargo.toml` 存在；`[dependencies]` 为空（零依赖）；license/description/keywords 字段填好（为独立发布准备）；`cargo check -p xui_i18n` 通过。

- [ ] T-2.2 注册到 workspace
  - 依赖：T-2.1
  - 验收：`cargo metadata` 能识别该 crate。

### 阶段二：trait 定义

- [ ] T-2.3 实现 `lib.rs` 的 `StringSource` trait
  - 依赖：T-2.1
  - 验收：定义 `pub trait StringSource: Send + Sync`，含 `fn get(&self, key: &str, args: &[(&str, String)]) -> String` 与 `fn current_lang(&self) -> &str`；文档注释说明"由宿主实现、UI 库经此 trait 取字符串、框架无关"；编译通过。

### 阶段三：测试

- [ ] T-2.4 编写 mock 实现测试
  - 依赖：T-2.3
  - 验收：写一个测试用 `MockStrings` impl `StringSource`（`get` 返回固定串或插值 key），断言 `get("welcome", &[])` 返回预期；`cargo test -p xui_i18n` 通过。

- [ ] T-2.5 验证零依赖
  - 依赖：T-2.3
  - 验收：`cargo tree -p xui_i18n` 输出只有自身，无任何依赖。

## 完成标志

- `cargo check -p xui_i18n` 通过
- `cargo test -p xui_i18n` 全绿
- `cargo tree -p xui_i18n` 零依赖
- trait 定义稳定（后续 xui 与 xgent_settings 都依赖它，不应再改动）
