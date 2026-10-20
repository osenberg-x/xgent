//! 全局配置权威副本与读写协调。
//!
//! daemon 持有全局配置的权威副本。`config.read` 返回对应字段的值，
//! `config.write` 更新内存副本、持久化到 TOML，并返回 [`ConfigChanged`]
//! 供调用方广播给所有客户端。
//!
//! 全局（`scope = global`）支持的 key：
//! - `default_provider` / `default_model` — 字符串
//! - `preferences.language` / `preferences.theme` — 字符串
//! - `preferences.density` — 字符串，只接受 `comfortable` / `compact`（v8 M2b）
//! - `providers` — 整个 providers map（读：JSON 对象；写：替换整个 map）
//! - `providers.<id>` — 单个 provider 配置（读：JSON 对象；写：插入/替换）
//! - `providers.<id>.api_base` / `api_key` / `kind` / `model_overrides` /
//!   `timeout_secs` / `max_retries` / `retry_*` / `context_window` /
//!   `max_output_tokens` / `first_event_timeout_secs` — 单字段
//!   （`model_overrides` 为插入式：设 map 键 `"default"`；
//!   `model_context_windows` 只读——它是按模型名建的表，不是单值）
//!
//! 项目（`scope = project`，必须带 `project_root`）支持的 key：
//! - `project` — 整个 [`ProjectConfig`]（只读，供面板一次取回真实值）
//! - `tool_policy` / `agent` / `tools` / `context` — 整段（只读）
//! - `tool_policy.approved` / `tool_policy.denied` — 字符串数组（整表替换）
//! - `agent.retrieval_max_tokens` / `agent.max_events_per_frame` /
//!   `agent.max_parallel_tools` — 整数
//! - `tools.confirm_timeout_secs` / `tools.command_timeout_secs` /
//!   `tools.command_max_output_bytes` — 整数
//! - `context.tree_max_depth` / `context.tree_max_entries` /
//!   `context.rg_timeout_secs` / `context.max_search_files` /
//!   `context.max_keywords` — 整数
//!
//! 项目作用域的写入按整份快照进行：先改副本、落盘成功、再换入内存副本，
//! 落盘失败时内存与磁盘不会各说各话。缺 `project_root` 或项目 TOML 损坏都是
//! 显式错误——静默按 global 处理会让用户以为项目配置被改了。

use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use xgent_core::config::{ConfigChanged, ConfigScope};
use xgent_core::error::{XgentError, XgentResult};
use xgent_settings_core::global::ProviderConfig;
use xgent_settings_core::{
    Density, GlobalConfig, GlobalConfigStore, ProjectConfig, ProjectConfigStore,
};

/// 配置权威副本（全局一份 + 按项目根缓存的项目配置）。
pub struct ConfigCoordinator {
    config: GlobalConfig,
    /// 项目配置缓存，键为请求携带的项目根。
    ///
    /// 首次访问某个根时从 `<root>/.xgent/config.toml` 载入。缓存而非每次读盘，
    /// 是为了让"同一 daemon 上两个窗口写同一项目"落到同一个内存副本上串行，
    /// 而不是各自读到对方写入之前的旧值。
    projects: HashMap<PathBuf, ProjectConfig>,
}

impl ConfigCoordinator {
    /// 从 TOML 加载（不存在则默认）。
    pub fn load() -> XgentResult<Self> {
        let config = GlobalConfigStore::load()?;
        Ok(Self {
            config,
            projects: HashMap::new(),
        })
    }

    /// 用指定配置构造（主要用于测试）。
    #[allow(dead_code)]
    pub fn with_config(config: GlobalConfig) -> Self {
        Self {
            config,
            projects: HashMap::new(),
        }
    }

    /// 当前配置的只读引用。
    pub fn config(&self) -> &GlobalConfig {
        &self.config
    }

    /// 读取指定 key 的值。未知 key 返回 `Value::Null`。
    pub fn read(&self, key: &str) -> Value {
        match key {
            "default_provider" => json!(self.config.default_provider),
            "default_model" => json!(self.config.default_model),
            "preferences.language" => json!(self.config.preferences.language),
            "preferences.theme" => json!(self.config.preferences.theme),
            "preferences.density" => json!(self.config.preferences.density.as_str()),
            "providers" => json!(self.config.providers),
            other if other.starts_with("providers.") => {
                let parts: Vec<&str> = other.split('.').collect();
                // parts[0]="providers", parts[1]=id, parts[2..]=可选字段
                if parts.len() < 2 || parts[1].is_empty() {
                    return Value::Null;
                }
                let id = parts[1];
                let Some(pc) = self.config.providers.get(id) else {
                    return Value::Null;
                };
                if parts.len() == 2 {
                    json!(pc)
                } else {
                    read_provider_field(pc, parts[2])
                }
            }
            _ => Value::Null,
        }
    }

    /// 写入指定 key 的值。
    ///
    /// 更新内存副本并持久化到 TOML，返回 [`ConfigChanged`] 供广播。
    pub fn write(&mut self, key: &str, value: Value) -> XgentResult<ConfigChanged> {
        match key {
            "default_provider" => {
                self.config.default_provider = parse_string(key, &value)?;
            }
            "default_model" => {
                self.config.default_model = parse_string(key, &value)?;
            }
            "preferences.language" => {
                self.config.preferences.language = parse_string(key, &value)?;
            }
            "preferences.theme" => {
                self.config.preferences.theme = parse_string(key, &value)?;
            }
            // v8 M2b：界面密度档位。只接受 `comfortable` / `compact`——
            // 未知值回落缺省会让「切了档但没保存上」这件事变得不可见，所以直接拒。
            "preferences.density" => {
                let raw = parse_string(key, &value)?;
                if raw != "comfortable" && raw != "compact" {
                    return Err(XgentError::Config(format!(
                        "{key} 只接受 comfortable / compact，收到 {raw}"
                    )));
                }
                self.config.preferences.density = Density::parse(&raw);
            }
            "providers" => {
                // 整体替换 providers map
                let map: std::collections::HashMap<String, ProviderConfig> =
                    serde_json::from_value(value.clone()).map_err(|e| {
                        XgentError::Config(format!("providers 值反序列化失败: {e}"))
                    })?;
                self.config.providers = map;
            }
            other if other.starts_with("providers.") => {
                let parts: Vec<&str> = other.split('.').collect();
                if parts.len() < 2 || parts[1].is_empty() {
                    return Err(XgentError::Config(format!("无效配置 key: {key}")));
                }
                let id = parts[1].to_string();
                if parts.len() == 2 {
                    // 写入/替换整个 ProviderConfig
                    let pc: ProviderConfig =
                        serde_json::from_value(value.clone()).map_err(|e| {
                            XgentError::Config(format!("provider 配置反序列化失败: {e}"))
                        })?;
                    self.config.providers.insert(id, pc);
                } else {
                    // 写入单个字段
                    let field = parts[2];
                    let pc = self.config.providers.entry(id).or_default();
                    write_provider_field(pc, field, &value)?;
                }
            }
            _ => {
                return Err(XgentError::Config(format!("未知配置 key: {key}")));
            }
        }
        // 持久化
        GlobalConfigStore::save(&self.config)?;
        Ok(ConfigChanged {
            scope: ConfigScope::Global,
            key: key.to_string(),
            value,
            project_root: None,
        })
    }

    /// 取某项目根的权威副本（首次访问从磁盘载入）。
    fn project(&mut self, root: &Path) -> XgentResult<&ProjectConfig> {
        if !self.projects.contains_key(root) {
            let mut cfg = ProjectConfigStore::load(root)?;
            // 磁盘上的 config.toml 可能没写 project_root（历史文件/手改）。
            // 补齐后 save() 才会写到正确的路径——它按 cfg.project_root 定位文件。
            if cfg.project_root.is_empty() {
                cfg.project_root = root.display().to_string();
            }
            self.projects.insert(root.to_path_buf(), cfg);
        }
        self.projects
            .get(root)
            .ok_or_else(|| XgentError::Config(format!("项目配置不可用: {}", root.display())))
    }

    /// 读取项目配置的指定 key。未知 key 返回 `Value::Null`（与全局一致）。
    pub fn read_project(&mut self, root: &Path, key: &str) -> XgentResult<Value> {
        let cfg = self.project(root)?.clone();
        Ok(read_project_value(&cfg, key))
    }

    /// 写入项目配置的指定 key。
    ///
    /// 顺序是「改副本 → 落盘 → 换入内存」：落盘失败时内存保持原值，
    /// 调用方看到的与磁盘上的是同一份。
    pub fn write_project(
        &mut self,
        root: &Path,
        key: &str,
        value: Value,
    ) -> XgentResult<ConfigChanged> {
        let mut cfg = self.project(root)?.clone();
        cfg.project_root = root.display().to_string();
        write_project_field(&mut cfg, key, &value)?;
        ProjectConfigStore::save(&cfg)?;
        self.projects.insert(root.to_path_buf(), cfg);
        Ok(ConfigChanged {
            scope: ConfigScope::Project,
            key: key.to_string(),
            value,
            project_root: Some(root.display().to_string()),
        })
    }
}

/// 项目作用域可读的 key。`None` 表示不认识这个 key。
fn read_project_value(cfg: &ProjectConfig, key: &str) -> Value {
    match key {
        "project" => json!(cfg),
        "tool_policy" => json!(cfg.tool_policy),
        "agent" => json!(cfg.agent),
        "tools" => json!(cfg.tools),
        "context" => json!(cfg.context),
        "tool_policy.approved" => json!(cfg.tool_policy.approved),
        "tool_policy.denied" => json!(cfg.tool_policy.denied),
        "agent.retrieval_max_tokens" => json!(cfg.agent.retrieval_max_tokens),
        "agent.max_events_per_frame" => json!(cfg.agent.max_events_per_frame),
        "agent.max_parallel_tools" => json!(cfg.agent.max_parallel_tools),
        "tools.confirm_timeout_secs" => json!(cfg.tools.confirm_timeout_secs),
        "tools.command_timeout_secs" => json!(cfg.tools.command_timeout_secs),
        "tools.command_max_output_bytes" => json!(cfg.tools.command_max_output_bytes),
        "context.tree_max_depth" => json!(cfg.context.tree_max_depth),
        "context.tree_max_entries" => json!(cfg.context.tree_max_entries),
        "context.rg_timeout_secs" => json!(cfg.context.rg_timeout_secs),
        "context.max_search_files" => json!(cfg.context.max_search_files),
        "context.max_keywords" => json!(cfg.context.max_keywords),
        _ => Value::Null,
    }
}

/// 写入项目单字段。未知 key 与类型不符都报错（不静默忽略）。
fn write_project_field(cfg: &mut ProjectConfig, key: &str, value: &Value) -> XgentResult<()> {
    match key {
        "tool_policy.approved" => cfg.tool_policy.approved = parse_string_list(key, value)?,
        "tool_policy.denied" => cfg.tool_policy.denied = parse_string_list(key, value)?,
        "agent.retrieval_max_tokens" => {
            cfg.agent.retrieval_max_tokens = parse_number(key, value)? as u32
        }
        "agent.max_events_per_frame" => {
            cfg.agent.max_events_per_frame = parse_number(key, value)? as u32
        }
        "agent.max_parallel_tools" => {
            cfg.agent.max_parallel_tools = parse_number(key, value)? as usize
        }
        "tools.confirm_timeout_secs" => cfg.tools.confirm_timeout_secs = parse_number(key, value)?,
        "tools.command_timeout_secs" => cfg.tools.command_timeout_secs = parse_number(key, value)?,
        "tools.command_max_output_bytes" => {
            cfg.tools.command_max_output_bytes = parse_number(key, value)? as usize
        }
        "context.tree_max_depth" => cfg.context.tree_max_depth = parse_number(key, value)? as u32,
        "context.tree_max_entries" => {
            cfg.context.tree_max_entries = parse_number(key, value)? as usize
        }
        "context.rg_timeout_secs" => cfg.context.rg_timeout_secs = parse_number(key, value)?,
        "context.max_search_files" => {
            cfg.context.max_search_files = parse_number(key, value)? as usize
        }
        "context.max_keywords" => cfg.context.max_keywords = parse_number(key, value)? as usize,
        _ => return Err(XgentError::Config(format!("未知配置 key: {key}"))),
    }
    Ok(())
}

/// 解析字符串数组（工具 id 列表）。非字符串元素逐个点名，便于面板如实报错。
fn parse_string_list(key: &str, value: &Value) -> XgentResult<Vec<String>> {
    let arr = value
        .as_array()
        .ok_or_else(|| XgentError::Config(format!("配置 {key} 期望字符串数组, 收到: {value}")))?;
    arr.iter()
        .map(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| XgentError::Config(format!("配置 {key} 含非字符串元素: {v}")))
        })
        .collect()
}

/// 解析非负整数。
fn parse_number(key: &str, value: &Value) -> XgentResult<u64> {
    value
        .as_u64()
        .ok_or_else(|| XgentError::Config(format!("配置 {key} 期望非负整数, 收到: {value}")))
}

/// 读取 provider 单字段。
fn read_provider_field(pc: &ProviderConfig, field: &str) -> Value {
    match field {
        "kind" => json!(pc.kind),
        "api_base" => json!(pc.api_base),
        "api_key" => json!(pc.api_key),
        "model_overrides" => json!(pc.model_overrides),
        "timeout_secs" => json!(pc.timeout_secs),
        "max_retries" => json!(pc.max_retries),
        "retry_mode" => json!(pc.retry_mode),
        "retry_initial_delay_ms" => json!(pc.retry_initial_delay_ms),
        "retry_max_delay_ms" => json!(pc.retry_max_delay_ms),
        "retry_backoff_factor" => json!(pc.retry_backoff_factor),
        "context_window" => json!(pc.context_window),
        "model_context_windows" => json!(pc.model_context_windows),
        "max_output_tokens" => json!(pc.max_output_tokens),
        "first_event_timeout_secs" => json!(pc.first_event_timeout_secs),
        _ => Value::Null,
    }
}

/// 写入 provider 单字段。
fn write_provider_field(pc: &mut ProviderConfig, field: &str, value: &Value) -> XgentResult<()> {
    match field {
        "kind" => {
            pc.kind = serde_json::from_value(value.clone())
                .map_err(|e| XgentError::Config(format!("provider.kind 反序列化失败: {e}")))?;
        }
        "api_base" => pc.api_base = parse_string(field, value)?,
        "api_key" => pc.api_key = parse_string(field, value)?,
        "model_overrides" => {
            // 插入式：设 model_overrides["default"] 的值，不覆盖 map 其他条目。
            // value 为字符串（模型名），写入键 "default"。
            let model_name = parse_string(field, value)?;
            pc.model_overrides.insert("default".to_string(), model_name);
        }
        "timeout_secs" => {
            pc.timeout_secs = value
                .as_u64()
                .ok_or_else(|| XgentError::Config(format!("timeout_secs 期望数字: {value}")))?;
        }
        "max_retries" => {
            // null → None（无限重试）；数字 → Some(n)
            pc.max_retries = if value.is_null() {
                None
            } else {
                Some(value.as_u64().ok_or_else(|| {
                    XgentError::Config(format!("max_retries 期望数字或 null: {value}"))
                })? as u32)
            };
        }
        "retry_mode" => {
            pc.retry_mode = serde_json::from_value(value.clone())
                .map_err(|e| XgentError::Config(format!("retry_mode 反序列化失败: {e}")))?;
        }
        "retry_initial_delay_ms" => {
            pc.retry_initial_delay_ms = value.as_u64().ok_or_else(|| {
                XgentError::Config(format!("retry_initial_delay_ms 期望数字: {value}"))
            })?;
        }
        "retry_max_delay_ms" => {
            pc.retry_max_delay_ms = value.as_u64().ok_or_else(|| {
                XgentError::Config(format!("retry_max_delay_ms 期望数字: {value}"))
            })?;
        }
        "retry_backoff_factor" => {
            pc.retry_backoff_factor = value.as_f64().ok_or_else(|| {
                XgentError::Config(format!("retry_backoff_factor 期望数字: {value}"))
            })?;
        }
        "context_window" => {
            pc.context_window = parse_number(field, value)? as u32;
        }
        "max_output_tokens" => {
            pc.max_output_tokens = parse_number(field, value)? as u32;
        }
        "first_event_timeout_secs" => {
            pc.first_event_timeout_secs = parse_number(field, value)?;
        }
        // `model_context_windows` 只读：它是「模型名 → 窗口」的表，按单字段写入
        // 一个数字没有确定语义（写哪个模型？）。面板显示解析后的生效值，
        // 要编辑这张表得先有表格式编辑器。
        "model_context_windows" => {
            return Err(XgentError::Config(
                "model_context_windows 只读, 请改 context_window 或编辑 TOML".to_string(),
            ));
        }
        _ => {
            return Err(XgentError::Config(format!("未知 provider 字段: {field}")));
        }
    }
    Ok(())
}

/// 把 [`Value`] 解析为字符串，类型不符则报错。
fn parse_string(key: &str, value: &Value) -> XgentResult<String> {
    value
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| XgentError::Config(format!("配置 {key} 期望字符串值, 收到: {value}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_util::isolate_agent_dir;

    fn empty_config() -> GlobalConfig {
        GlobalConfig::default()
    }

    #[test]
    fn read_returns_known_keys() {
        let mut cfg = empty_config();
        cfg.default_provider = "openai".into();
        cfg.default_model = "gpt-4".into();
        cfg.preferences.language = "zh-CN".into();
        cfg.preferences.theme = "dark".into();
        let c = ConfigCoordinator::with_config(cfg);
        assert_eq!(c.read("default_provider"), json!("openai"));
        assert_eq!(c.read("default_model"), json!("gpt-4"));
        assert_eq!(c.read("preferences.language"), json!("zh-CN"));
        assert_eq!(c.read("preferences.theme"), json!("dark"));
        // 未知 key
        assert_eq!(c.read("unknown.key"), Value::Null);
    }

    #[test]
    fn write_updates_memory_and_returns_changed() {
        isolate_agent_dir();
        let mut c = ConfigCoordinator::with_config(empty_config());
        let changed = c.write("default_model", json!("claude-3")).unwrap();
        assert_eq!(changed.key, "default_model");
        assert_eq!(changed.value, json!("claude-3"));
        assert_eq!(c.read("default_model"), json!("claude-3"));
    }

    #[test]
    fn write_unknown_key_errors() {
        isolate_agent_dir();
        let mut c = ConfigCoordinator::with_config(empty_config());
        let err = c.write("no.such.key", json!("x")).unwrap_err();
        assert!(matches!(err, XgentError::Config(_)));
    }

    #[test]
    fn write_wrong_type_errors() {
        isolate_agent_dir();
        let mut c = ConfigCoordinator::with_config(empty_config());
        let err = c.write("default_model", json!(42)).unwrap_err();
        assert!(matches!(err, XgentError::Config(_)));
    }

    #[test]
    fn write_persists_to_memory_and_reflects_in_config() {
        isolate_agent_dir();
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write("default_model", json!("gpt-5")).unwrap();
        assert_eq!(c.config().default_model, "gpt-5");
        c.write("preferences.language", json!("en-US")).unwrap();
        assert_eq!(c.config().preferences.language, "en-US");
    }

    /// A4（v8 M2b）：`preferences.density` 在白名单内，写入后可读回；
    /// 非法值必须报错而不是静默回落（回落会让「没保存上」变得不可见）。
    #[test]
    fn density_key_is_writable_and_readable() {
        isolate_agent_dir();
        let mut c = ConfigCoordinator::with_config(empty_config());
        assert_eq!(c.read("preferences.density"), json!("comfortable"));

        c.write("preferences.density", json!("compact")).unwrap();
        assert_eq!(c.config().preferences.density, Density::Compact);
        assert_eq!(c.read("preferences.density"), json!("compact"));

        c.write("preferences.density", json!("comfortable"))
            .unwrap();
        assert_eq!(c.config().preferences.density, Density::Comfortable);

        let err = c.write("preferences.density", json!("tight")).unwrap_err();
        assert!(matches!(err, XgentError::Config(_)), "非法档位应被拒绝");
        assert_eq!(
            c.config().preferences.density,
            Density::Comfortable,
            "被拒绝的写入不应改动权威副本"
        );
    }

    /// 全局写入的落盘点必须跟着 `XGENT_AGENT_DIR` 走。
    ///
    /// 这条守卫的不是"值写对没有"，而是隔离本身：`GlobalConfigStore::save`
    /// 自己解析路径，一旦它绕过环境变量直接算 `~/.xgent/agent/config.toml`，
    /// 本模块其余测试就会在开发机的真实配置上落盘（本轮验证前实际发生过一次，
    /// 用户的 provider 配置被测试夹具覆盖，对用户等价于删数据）。
    #[test]
    fn global_write_lands_inside_the_isolated_dir() {
        let dir = isolate_agent_dir();
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write(
            "providers.openai.api_base",
            json!("https://isolated.test/v1"),
        )
        .unwrap();

        let file = dir.join("config.toml");
        assert!(file.is_file(), "配置应落在 {}，而不是别处", file.display());
        let on_disk = GlobalConfigStore::load_from(&file)
            .unwrap_or_else(|e| panic!("{} 应可解析: {e}", file.display()));
        assert_eq!(
            on_disk.providers.get("openai").map(|p| p.api_base.as_str()),
            Some("https://isolated.test/v1"),
            "内存里的写入要真的进磁盘"
        );
    }

    /// 配置失败原因会被设置面板逐键显示（英文界面里也一样），所以这些串不能带
    /// 全角标点。daemon 没有 Localizer，串本身是数据不是文案；"不带全角标点"
    /// 是它对下游显示的唯一契约，这条把它钉住。
    #[test]
    fn config_write_errors_avoid_fullwidth_punctuation() {
        isolate_agent_dir();
        let root = tempfile::tempdir().expect("临时项目根");
        let mut c = ConfigCoordinator::with_config(empty_config());
        let global_cases = [
            ("default_model", json!(42)),
            ("no.such.key", json!(1)),
            ("providers.openai.model_context_windows", json!(1)),
        ];
        let project_cases = [
            ("tools.confirm_timeout_secs", json!("abc")),
            ("tool_policy.approved", json!("x")),
            ("no.such.key", json!(1)),
        ];
        let mut checked = 0;
        for (key, value) in global_cases {
            let err = c
                .write(key, value)
                .expect_err("这些写法都应被拒绝")
                .to_string();
            assert!(
                err.chars()
                    .all(|ch| !"，、；：！？（）《》「」…".contains(ch)),
                "全局配置错误串带了全角标点: {err}"
            );
            checked += 1;
        }
        for (key, value) in project_cases {
            let err = c
                .write_project(root.path(), key, value)
                .expect_err("这些写法都应被拒绝")
                .to_string();
            assert!(
                err.chars()
                    .all(|ch| !"，、；：！？（）《》「」…".contains(ch)),
                "项目配置错误串带了全角标点: {err}"
            );
            checked += 1;
        }
        assert_eq!(checked, 6, "两批用例都要真的跑到");
    }

    #[test]
    fn write_provider_field_creates_entry() {
        isolate_agent_dir();
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write(
            "providers.myai.api_base",
            json!("https://api.example.com/v1"),
        )
        .unwrap();
        c.write("providers.myai.api_key", json!("sk-xxx")).unwrap();
        assert_eq!(
            c.read("providers.myai.api_base"),
            json!("https://api.example.com/v1")
        );
        assert_eq!(c.read("providers.myai.api_key"), json!("sk-xxx"));
        // 默认 kind 是 openai_compat
        assert_eq!(c.read("providers.myai.kind"), json!("open_ai_compat"));
    }

    #[test]
    fn write_provider_model_overrides_inserts_default_key() {
        isolate_agent_dir();
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write(
            "providers.myai.api_base",
            json!("https://api.example.com/v1"),
        )
        .unwrap();
        // 写 model_overrides：插入式，设 model_overrides["default"]
        c.write("providers.myai.model_overrides", json!("gpt-4o"))
            .unwrap();
        let got = c.read("providers.myai.model_overrides");
        // 读回整个 map，应含 "default" 键
        assert_eq!(got["default"], json!("gpt-4o"));
        // 再写一次，仍是插入式（覆盖 default 值，不影响其他键）
        c.write("providers.myai.model_overrides", json!("gpt-4o-mini"))
            .unwrap();
        let got2 = c.read("providers.myai.model_overrides");
        assert_eq!(got2["default"], json!("gpt-4o-mini"));
    }

    #[test]
    fn read_nonexistent_provider_returns_null() {
        let c = ConfigCoordinator::with_config(empty_config());
        assert_eq!(c.read("providers.nonexistent.api_base"), Value::Null);
        assert_eq!(c.read("providers.nonexistent"), Value::Null);
    }

    #[test]
    fn write_whole_provider_config() {
        isolate_agent_dir();
        let mut c = ConfigCoordinator::with_config(empty_config());
        let pc = json!({
            "kind": "open_ai_compat",
            "api_base": "https://api.deepseek.com/v1",
            "api_key": "sk-deep"
        });
        c.write("providers.deepseek", pc).unwrap();
        let got = c.read("providers.deepseek");
        assert_eq!(got["api_base"], json!("https://api.deepseek.com/v1"));
        assert_eq!(got["api_key"], json!("sk-deep"));
    }

    #[test]
    fn write_provider_retry_fields() {
        isolate_agent_dir();
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write("providers.myai.api_base", json!("https://x.example.com"))
            .unwrap();
        // max_retries: 数字 → Some(n)
        c.write("providers.myai.max_retries", json!(5)).unwrap();
        assert_eq!(c.read("providers.myai.max_retries"), json!(5));
        // max_retries: null → None（无限重试）
        c.write("providers.myai.max_retries", json!(null)).unwrap();
        assert_eq!(c.read("providers.myai.max_retries"), json!(null));
        // retry_mode
        c.write("providers.myai.retry_mode", json!("exponential"))
            .unwrap();
        assert_eq!(c.read("providers.myai.retry_mode"), json!("exponential"));
        // retry_initial_delay_ms
        c.write("providers.myai.retry_initial_delay_ms", json!(1000))
            .unwrap();
        assert_eq!(c.read("providers.myai.retry_initial_delay_ms"), json!(1000));
        // retry_max_delay_ms
        c.write("providers.myai.retry_max_delay_ms", json!(60000))
            .unwrap();
        assert_eq!(c.read("providers.myai.retry_max_delay_ms"), json!(60000));
        // retry_backoff_factor
        c.write("providers.myai.retry_backoff_factor", json!(1.5))
            .unwrap();
        assert_eq!(c.read("providers.myai.retry_backoff_factor"), json!(1.5));
    }

    // ---- 全局：provider 限额字段可达（R3-13 的前半） ----

    /// 面板要能写 `context_window` / `max_output_tokens` /
    /// `first_event_timeout_secs`：写不进 daemon 就等于这些键仍只能手改 TOML。
    #[test]
    fn write_provider_context_limit_fields_reach_config() {
        isolate_agent_dir();
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write("providers.myai.context_window", json!(32000))
            .unwrap();
        c.write("providers.myai.max_output_tokens", json!(4096))
            .unwrap();
        c.write("providers.myai.first_event_timeout_secs", json!(45))
            .unwrap();
        assert_eq!(c.read("providers.myai.context_window"), json!(32000));
        assert_eq!(c.read("providers.myai.max_output_tokens"), json!(4096));
        assert_eq!(c.read("providers.myai.first_event_timeout_secs"), json!(45));
        let pc = c.config().providers.get("myai").expect("provider 应存在");
        assert_eq!(pc.context_window, 32000);
        assert_eq!(pc.max_output_tokens, 4096);
        assert_eq!(pc.first_event_timeout_secs, 45);
        // 解析后的生效值同步变化——面板报告的必须是生效值，不是原始存储值
        assert_eq!(pc.resolve_context_window("any"), (32000, false));
        assert_eq!(pc.effective_max_output_tokens(), 4096);
        assert_eq!(pc.effective_first_event_timeout_secs(), 45);
    }

    /// 写 `0` 是「清空该项、回到内置缺省」，不是把窗口设成 0 token。
    /// 这是 R3-17 定的哨兵语义，daemon 必须原样存 `0`。
    #[test]
    fn writing_zero_to_provider_limits_keeps_the_unset_sentinel() {
        isolate_agent_dir();
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write("providers.myai.context_window", json!(32000))
            .unwrap();
        c.write("providers.myai.context_window", json!(0)).unwrap();
        c.write("providers.myai.max_output_tokens", json!(0))
            .unwrap();
        let pc = c.config().providers.get("myai").expect("provider 应存在");
        assert_eq!(pc.context_window, 0);
        assert_eq!(pc.max_output_tokens, 0);
        assert_eq!(pc.resolve_context_window("any"), (128_000, true));
        assert_eq!(pc.effective_max_output_tokens(), 8_192);
    }

    /// `model_context_windows` 只读：按单字段写一个数字没有确定语义
    /// （写的是哪个模型？）。整 provider 写入仍然携带这张表。
    #[test]
    fn model_context_windows_is_readable_but_not_writable_as_a_field() {
        isolate_agent_dir();
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write(
            "providers.gpt",
            json!({
                "kind": "open_ai_compat",
                "model_context_windows": {"gpt-4o": 128000}
            }),
        )
        .unwrap();
        assert_eq!(
            c.read("providers.gpt.model_context_windows"),
            json!({"gpt-4o": 128000})
        );
        let err = c
            .write("providers.gpt.model_context_windows", json!(128000))
            .expect_err("按单字段写这张表应被拒绝");
        assert!(
            err.to_string().contains("只读"),
            "错误应说明是只读键: {err}"
        );
    }

    #[test]
    fn unknown_global_provider_field_still_errors() {
        isolate_agent_dir();
        let mut c = ConfigCoordinator::with_config(empty_config());
        let err = c
            .write("providers.myai.not_a_field", json!(1))
            .expect_err("未知字段应报错");
        assert!(err.to_string().contains("未知 provider 字段"), "{err}");
    }

    // ---- 项目作用域 ----

    #[test]
    fn project_write_lands_on_disk_and_reads_back() {
        let dir = tempfile::tempdir().expect("临时项目根");
        let root = dir.path();
        let mut c = ConfigCoordinator::with_config(empty_config());
        let changed = c
            .write_project(root, "agent.max_parallel_tools", json!(2))
            .unwrap();
        assert_eq!(changed.scope, ConfigScope::Project);
        assert_eq!(changed.key, "agent.max_parallel_tools");
        assert_eq!(changed.value, json!(2));
        assert_eq!(
            changed.project_root,
            Some(root.display().to_string()),
            "通知必须带上项目根，否则别的项目的窗口无法判断是否与己有关"
        );
        // 真的落盘了（既有全局测试全部走内存副本，项目侧要把这条钉住）
        let on_disk = ProjectConfigStore::load(root).expect("项目配置应可读");
        assert_eq!(on_disk.agent.max_parallel_tools, 2);
        assert_eq!(on_disk.project_root, root.display().to_string());
        assert_eq!(
            c.read_project(root, "agent.max_parallel_tools").unwrap(),
            json!(2)
        );
    }

    #[test]
    fn project_tool_policy_lists_replace_and_persist() {
        let dir = tempfile::tempdir().expect("临时项目根");
        let root = dir.path();
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write_project(root, "tool_policy.approved", json!(["read_file"]))
            .unwrap();
        c.write_project(root, "tool_policy.denied", json!(["run_command"]))
            .unwrap();
        let on_disk = ProjectConfigStore::load(root).expect("项目配置应可读");
        assert_eq!(on_disk.tool_policy.approved, vec!["read_file".to_string()]);
        assert_eq!(on_disk.tool_policy.denied, vec!["run_command".to_string()]);
        // 整表替换语义：第二次写 approved 不影响 denied
        c.write_project(root, "tool_policy.approved", json!([]))
            .unwrap();
        assert_eq!(
            c.read_project(root, "tool_policy.approved").unwrap(),
            json!([])
        );
        assert_eq!(
            c.read_project(root, "tool_policy.denied").unwrap(),
            json!(["run_command"])
        );
    }

    /// 一次取回整份项目配置（面板打开时用这条，避免十几次往返）。
    #[test]
    fn project_read_of_whole_config_covers_every_section() {
        let dir = tempfile::tempdir().expect("临时项目根");
        let root = dir.path();
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write_project(root, "context.max_keywords", json!(5))
            .unwrap();
        c.write_project(root, "tools.command_timeout_secs", json!(90))
            .unwrap();
        let whole = c.read_project(root, "project").unwrap();
        assert_eq!(whole["context"]["max_keywords"], json!(5));
        assert_eq!(whole["tools"]["command_timeout_secs"], json!(90));
        assert!(whole.get("tool_policy").is_some());
        assert!(whole.get("agent").is_some());
        assert_eq!(
            c.read_project(root, "context").unwrap()["max_keywords"],
            json!(5)
        );
    }

    #[test]
    fn project_read_seeds_from_disk_on_first_access() {
        let dir = tempfile::tempdir().expect("临时项目根");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".xgent")).expect("建 .xgent");
        std::fs::write(
            root.join(".xgent").join("config.toml"),
            "[agent]\nmax_parallel_tools = 3\n",
        )
        .expect("写项目配置");
        let mut c = ConfigCoordinator::with_config(empty_config());
        assert_eq!(
            c.read_project(&root, "agent.max_parallel_tools").unwrap(),
            json!(3),
            "daemon 的权威副本应从磁盘种子开始，而不是内存默认值"
        );
    }

    #[test]
    fn project_cache_is_per_root_not_shared() {
        let a = tempfile::tempdir().expect("项目 A");
        let b = tempfile::tempdir().expect("项目 B");
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write_project(a.path(), "agent.max_parallel_tools", json!(2))
            .unwrap();
        c.write_project(b.path(), "agent.max_parallel_tools", json!(9))
            .unwrap();
        assert_eq!(
            c.read_project(a.path(), "agent.max_parallel_tools")
                .unwrap(),
            json!(2)
        );
        assert_eq!(
            c.read_project(b.path(), "agent.max_parallel_tools")
                .unwrap(),
            json!(9)
        );
    }

    /// 未知键：读回 `Null`（与全局一致），写则报错。段落级写入不接受——
    /// 整段替换会让一次误发清掉用户所有该段设置。
    #[test]
    fn project_unknown_key_reads_null_and_write_errors() {
        let dir = tempfile::tempdir().expect("临时项目根");
        let root = dir.path();
        let mut c = ConfigCoordinator::with_config(empty_config());
        assert_eq!(
            c.read_project(root, "agent.not_a_field").unwrap(),
            Value::Null
        );
        let err = c
            .write_project(root, "agent.not_a_field", json!(1))
            .expect_err("未知项目键应报错");
        assert!(err.to_string().contains("未知配置 key"), "{err}");
        let err = c
            .write_project(root, "agent", json!({"max_parallel_tools": 1}))
            .expect_err("整段写入不应被接受");
        assert!(err.to_string().contains("未知配置 key"), "{err}");
    }

    /// 类型不符要点名是哪个字段，且旧值保持不变（内存与磁盘都不动）。
    #[test]
    fn project_write_wrong_type_names_the_field_and_keeps_previous_value() {
        let dir = tempfile::tempdir().expect("临时项目根");
        let root = dir.path();
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write_project(root, "tools.confirm_timeout_secs", json!(60))
            .unwrap();
        let err = c
            .write_project(root, "tools.confirm_timeout_secs", json!("abc"))
            .expect_err("字符串值应被拒绝");
        assert!(
            err.to_string().contains("tools.confirm_timeout_secs"),
            "错误应点名字段: {err}"
        );
        assert_eq!(
            c.read_project(root, "tools.confirm_timeout_secs").unwrap(),
            json!(60)
        );
        assert_eq!(
            ProjectConfigStore::load(root)
                .expect("项目配置应可读")
                .tools
                .confirm_timeout_secs,
            60
        );
        // 数组元素类型也要点名，面板才能报出可行动的原因
        let err = c
            .write_project(root, "tool_policy.approved", json!(["read_file", 7]))
            .expect_err("非字符串元素应被拒绝");
        assert!(err.to_string().contains("非字符串元素"), "{err}");
        assert_eq!(
            c.read_project(root, "tool_policy.approved").unwrap(),
            json!([] as [String; 0])
        );
    }

    /// 损坏的项目 TOML 是一条带路径的错误，不是「所有默认值」。
    #[test]
    fn corrupt_project_toml_reports_the_file_path() {
        let dir = tempfile::tempdir().expect("临时项目根");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".xgent")).expect("建 .xgent");
        std::fs::write(
            root.join(".xgent").join("config.toml"),
            "这不是 TOML：[[[\n",
        )
        .expect("写坏配置");
        let mut c = ConfigCoordinator::with_config(empty_config());
        let err = c
            .read_project(&root, "project")
            .expect_err("损坏配置应报错而不是返回默认");
        let msg = err.to_string();
        assert!(msg.contains("config.toml"), "错误应含文件路径: {msg}");
    }

    /// 落盘失败时内存副本保持不变：否则下一次读会显示一个磁盘上没有的值，
    /// 而运行时句柄又会按 daemon 已接受的值更新——三者各说各话。
    #[cfg(unix)]
    #[test]
    fn failed_project_save_leaves_memory_unchanged() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("临时项目根");
        let root = dir.path();
        let mut c = ConfigCoordinator::with_config(empty_config());
        c.write_project(root, "agent.max_parallel_tools", json!(2))
            .unwrap();
        // 把 .xgent 目录改成只读，后续 atomic_write 失败
        std::fs::set_permissions(root.join(".xgent"), std::fs::Permissions::from_mode(0o500))
            .expect("设为只读");
        let err = c
            .write_project(root, "agent.max_parallel_tools", json!(8))
            .expect_err("只读目录应让落盘失败");
        assert!(matches!(err, XgentError::Io(_)), "应为 IO 错误: {err}");
        assert_eq!(
            c.read_project(root, "agent.max_parallel_tools").unwrap(),
            json!(2),
            "落盘失败后内存副本必须仍是磁盘上的那份"
        );
        // 恢复权限以便 tempdir 清理
        std::fs::set_permissions(root.join(".xgent"), std::fs::Permissions::from_mode(0o700))
            .expect("恢复权限");
    }
}
