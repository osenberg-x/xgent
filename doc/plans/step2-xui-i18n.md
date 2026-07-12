# Step 2: xui_i18n

## 模块职责

极小 crate，只放 i18n 的 `StringSource` trait，纯无依赖。

**存在理由**：解决"反转依赖"的归属问题。`xui`（可独立发布的 UI 库）需要取本地化字符串，但不能依赖 `xgent_settings`（否则破坏独立发布）；`xgent_settings::Localizer` 要提供字符串给 `xui`。把 trait 放在本 crate：

- `xui` 依赖 `xui_i18n`（trait 使用方）
- `xgent_settings` 依赖 `xui_i18n`（trait 实现方）
- 两者不直接依赖，`xui` 仍可独立发布

本 crate 不依赖 Bevy、不依赖任何 `xgent_*` / `xui`，纯标准库 + serde（trait 参数用 `&str`，无需 serde，可零依赖）。

## 前置依赖

无。

## 目标文件结构

```
crates/xui_i18n/
├── Cargo.toml
└── src/
    └── lib.rs    # StringSource trait
```

## Cargo.toml

```toml
[package]
name = "xui_i18n"
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
description = "A minimal i18n string source trait, framework-agnostic."
keywords = ["i18n", "trait"]

[dependencies]
# 无依赖，纯标准库
```

## 关键类型与接口

### lib.rs — StringSource trait

```rust
/// i18n 字符串提供者 trait。
///
/// 由宿主应用实现（如 XGent 的 `xgent_settings::Localizer`），
/// UI 库（如 `xui`）通过此 trait 取本地化字符串，无需依赖具体 i18n 实现。
///
/// 设计为框架无关：实现可基于 fluent、gettext 或任何方案。
pub trait StringSource: Send + Sync {
    /// 取 key 对应的已翻译字符串。
    /// `args` 为命名参数列表 (name, value)，用于格式化（如复数、占位符）。
    fn get(&self, key: &str, args: &[(&str, String)]) -> String;

    /// 当前语言标识，如 "zh-CN"、"en-US"。
    fn current_lang(&self) -> &str;
}
```

## 实现要点

1. **零依赖**：只用标准库，trait 参数用 `&str` 与 `String`，不引入 serde 等。
2. **框架无关**：trait 不绑定 fluent 或任何 i18n 方案，任何实现都可接入。
3. **Send + Sync**：需跨线程（Bevy Resource、xui 系统访问）。
4. **不依赖 Bevy**：保持可被非 Bevy 项目复用。
5. **极简**：本 crate 只此一个 trait，不改名也不膨胀。

## 验证方法

1. **编译检查**：
   ```bash
   cargo check -p xui_i18n
   ```
2. **零依赖验证**：`cargo tree -p xui_i18n` 输出应只有自身，无任何依赖。
3. **trait 可实现性测试**：写一个 mock impl，调 `get` 返回固定串，断言通过。

## 完成后下一步

xui_i18n 完成后 → 实现 **xgent_settings_core**（配置纯类型），它依赖 xgent_core。同时 xui_i18n 也将被 step4（xgent_settings）与 step10（xui）依赖。
