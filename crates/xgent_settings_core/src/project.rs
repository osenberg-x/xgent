//! 项目配置类型（本地隔离，存于 `<project>/.xgent/config.toml`）。

use serde::{Deserialize, Serialize};

/// 项目配置根结构。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ProjectConfig {
    /// 项目根目录（绝对路径）
    #[serde(default)]
    pub project_root: String,

    /// 覆盖默认 provider（None 表示用全局默认）
    #[serde(default)]
    pub provider_override: Option<String>,

    /// 覆盖默认模型
    #[serde(default)]
    pub model_override: Option<String>,

    /// 上下文检索策略
    #[serde(default)]
    pub context_strategy: ContextStrategy,

    /// 工具信任级别覆盖
    #[serde(default)]
    pub tool_policy: ToolPolicyConfig,

    /// agent 循环与上下文预算参数
    #[serde(default)]
    pub agent: AgentConfig,

    /// 工具执行参数
    #[serde(default)]
    pub tools: ToolConfig,

    /// 内置检索 provider 的遍历限额
    #[serde(default)]
    pub context: ContextLimits,
}

/// `[agent]` 段：agent 循环的预算与每帧工作量。
///
/// 数值键一律 `0` = 未设置，回落到本次转配置前的硬编码常量（R3-17）。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AgentConfig {
    /// 每轮对话向检索 provider 申请的 token 预算
    #[serde(default)]
    pub retrieval_max_tokens: u32,
    /// 单个 ECS 帧最多处理的 agent 事件数
    #[serde(default)]
    pub max_events_per_frame: u32,
    /// 同一轮 tool_calls 里最多同时执行的工具调用数（只读且无需确认的调用才并发）
    #[serde(default)]
    pub max_parallel_tools: usize,
}

/// `[tools]` 段：工具执行的超时与输出上限。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ToolConfig {
    /// 等待用户确认工具调用的秒数
    #[serde(default)]
    pub confirm_timeout_secs: u64,
    /// `run_command` 最长执行秒数
    #[serde(default)]
    pub command_timeout_secs: u64,
    /// `run_command` 捕获输出上限（字节）
    #[serde(default)]
    pub command_max_output_bytes: usize,
}

/// `[context]` 段：内置检索 provider（方案 A）的遍历限额。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ContextLimits {
    /// 目录树最大深度
    #[serde(default)]
    pub tree_max_depth: u32,
    /// 目录树最大条目数
    #[serde(default)]
    pub tree_max_entries: usize,
    /// ripgrep 超时秒数
    #[serde(default)]
    pub rg_timeout_secs: u64,
    /// 搜索返回的最大文件数
    #[serde(default)]
    pub max_search_files: usize,
    /// 关键词上限
    #[serde(default)]
    pub max_keywords: usize,
}

/// 内置缺省值——本次转配置前各 crate 的硬编码常量。
pub mod defaults {
    /// 每轮检索 token 预算（原 `xgent_agent::bridge` 字面量）。
    pub const RETRIEVAL_MAX_TOKENS: u32 = 8_000;
    /// 每帧 agent 事件上限（原 `xgent_agent::agent_loop` 字面量）。
    pub const MAX_EVENTS_PER_FRAME: u32 = 64;
    /// 同一轮并发执行的工具调用上限。
    ///
    /// 不是"越快越好"的调优值：并发的是子进程（rg）、socket 与文件描述符，
    /// 需要一个与"不限"区分的闸；缺省 4 覆盖"读几个文件 + 搜一次代码"这类常见批次。
    pub const MAX_PARALLEL_TOOLS: usize = 4;
    /// 确认等待秒数（原 `xgent_tools::executor` 字面量）。
    pub const CONFIRM_TIMEOUT_SECS: u64 = 300;
    /// 命令执行秒数（原 `run_command::COMMAND_TIMEOUT_SECS`）。
    pub const COMMAND_TIMEOUT_SECS: u64 = 60;
    /// 命令输出上限字节（原 `run_command::MAX_OUTPUT_BYTES`）。
    pub const COMMAND_MAX_OUTPUT_BYTES: usize = 64 * 1024;
    /// 目录树深度（原 `on_demand::TREE_MAX_DEPTH`）。
    pub const TREE_MAX_DEPTH: u32 = 4;
    /// 目录树条目（原 `on_demand::TREE_MAX_ENTRIES`）。
    pub const TREE_MAX_ENTRIES: usize = 200;
    /// rg 超时秒（原 `on_demand::RG_TIMEOUT_SECS`）。
    pub const RG_TIMEOUT_SECS: u64 = 10;
    /// 搜索文件数（原 `on_demand::MAX_SEARCH_FILES`）。
    pub const MAX_SEARCH_FILES: usize = 20;
    /// 关键词上限（原 `on_demand::MAX_KEYWORDS`）。
    pub const MAX_KEYWORDS: usize = 16;
}

/// `0` 哨兵下的实际取值。
fn or_default_u32(value: u32, fallback: u32) -> u32 {
    if value == 0 { fallback } else { value }
}

/// `0` 哨兵下的实际取值（`u64`）。
fn or_default_u64(value: u64, fallback: u64) -> u64 {
    if value == 0 { fallback } else { value }
}

/// `0` 哨兵下的实际取值（`usize`）。
fn or_default_usize(value: usize, fallback: usize) -> usize {
    if value == 0 { fallback } else { value }
}

impl AgentConfig {
    /// 生效的检索 token 预算。
    pub fn effective_retrieval_max_tokens(&self) -> u32 {
        or_default_u32(self.retrieval_max_tokens, defaults::RETRIEVAL_MAX_TOKENS)
    }

    /// 生效的每帧事件上限。
    pub fn effective_max_events_per_frame(&self) -> u32 {
        or_default_u32(self.max_events_per_frame, defaults::MAX_EVENTS_PER_FRAME)
    }

    /// 生效的同轮并发工具调用上限（`0` = 未设置 → 缺省 4，**不是**不限）。
    pub fn effective_max_parallel_tools(&self) -> usize {
        or_default_usize(self.max_parallel_tools, defaults::MAX_PARALLEL_TOOLS)
    }
}

impl ToolConfig {
    /// 生效的确认等待秒数。
    pub fn effective_confirm_timeout_secs(&self) -> u64 {
        or_default_u64(self.confirm_timeout_secs, defaults::CONFIRM_TIMEOUT_SECS)
    }

    /// 生效的命令执行秒数上限。
    pub fn effective_command_timeout_secs(&self) -> u64 {
        or_default_u64(self.command_timeout_secs, defaults::COMMAND_TIMEOUT_SECS)
    }

    /// 生效的命令输出上限字节。
    pub fn effective_command_max_output_bytes(&self) -> usize {
        or_default_usize(
            self.command_max_output_bytes,
            defaults::COMMAND_MAX_OUTPUT_BYTES,
        )
    }
}

impl ContextLimits {
    /// 生效的目录树深度上限。
    pub fn effective_tree_max_depth(&self) -> u32 {
        or_default_u32(self.tree_max_depth, defaults::TREE_MAX_DEPTH)
    }

    /// 生效的目录树条目上限。
    pub fn effective_tree_max_entries(&self) -> usize {
        or_default_usize(self.tree_max_entries, defaults::TREE_MAX_ENTRIES)
    }

    /// 生效的 rg 超时秒数。
    pub fn effective_rg_timeout_secs(&self) -> u64 {
        or_default_u64(self.rg_timeout_secs, defaults::RG_TIMEOUT_SECS)
    }

    /// 生效的搜索文件数上限。
    pub fn effective_max_search_files(&self) -> usize {
        or_default_usize(self.max_search_files, defaults::MAX_SEARCH_FILES)
    }

    /// 生效的关键词上限。
    pub fn effective_max_keywords(&self) -> usize {
        or_default_usize(self.max_keywords, defaults::MAX_KEYWORDS)
    }
}

/// 上下文检索策略。
///
/// MVP 仅用 `OnDemand`（方案 A 无索引·按需读取），其余为后续阶段占位。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContextStrategy {
    /// MVP：方案 A 无索引·按需读取
    #[default]
    OnDemand,
    /// B 阶段：基于仓库结构映射
    RepoMap,
    /// C 阶段：向量检索
    Vector,
    /// E 阶段：混合检索
    Hybrid,
}

/// 工具策略配置：按工具 id 覆盖默认信任级别。
///
/// 默认所有工具为 `NeedsConfirmation`（见架构安全模型 11.1）。
/// `approved` 提升为自动执行，`denied` 降为拒绝。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ToolPolicyConfig {
    /// 提升为自动执行的工具 id 列表
    #[serde(default)]
    pub approved: Vec<String>,

    /// 降为拒绝的工具 id 列表
    #[serde(default)]
    pub denied: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_config_default() {
        let cfg = ProjectConfig::default();
        assert_eq!(cfg.context_strategy, ContextStrategy::OnDemand);
        assert!(cfg.provider_override.is_none());
        assert!(cfg.tool_policy.approved.is_empty());
        assert!(cfg.tool_policy.denied.is_empty());
    }

    #[test]
    fn context_strategy_default_is_ondemand() {
        assert_eq!(ContextStrategy::default(), ContextStrategy::OnDemand);
    }

    #[test]
    fn context_strategy_serde_snake_case() {
        let j = serde_json::to_string(&ContextStrategy::RepoMap).unwrap();
        assert_eq!(j, r#""repo_map""#);
        let c: ContextStrategy = serde_json::from_str(r#""vector""#).unwrap();
        assert_eq!(c, ContextStrategy::Vector);
    }

    #[test]
    fn project_config_roundtrip() {
        let cfg = ProjectConfig {
            project_root: "/abs/proj".into(),
            provider_override: Some("anthropic".into()),
            model_override: None,
            context_strategy: ContextStrategy::RepoMap,
            tool_policy: ToolPolicyConfig {
                approved: vec!["read_file".into()],
                denied: vec!["rm_rf".into()],
            },
            ..Default::default()
        };
        let j = serde_json::to_string(&cfg).unwrap();
        let cfg2: ProjectConfig = serde_json::from_str(&j).unwrap();
        assert_eq!(cfg2.project_root, "/abs/proj");
        assert_eq!(cfg2.provider_override.as_deref(), Some("anthropic"));
        assert!(cfg2.model_override.is_none());
        assert_eq!(cfg2.context_strategy, ContextStrategy::RepoMap);
        assert_eq!(cfg2.tool_policy.approved, vec!["read_file".to_string()]);
        assert_eq!(cfg2.tool_policy.denied, vec!["rm_rf".to_string()]);
    }

    /// R3-17 不变量：新节缺省即「未设置」，生效值等于本次转配置前的硬编码常量。
    #[test]
    fn new_sections_default_to_previous_constants() {
        let cfg = ProjectConfig::default();
        assert_eq!(cfg.agent.effective_retrieval_max_tokens(), 8_000);
        assert_eq!(cfg.agent.effective_max_events_per_frame(), 64);
        assert_eq!(cfg.agent.effective_max_parallel_tools(), 4);
        assert_eq!(cfg.tools.effective_confirm_timeout_secs(), 300);
        assert_eq!(cfg.tools.effective_command_timeout_secs(), 60);
        assert_eq!(cfg.tools.effective_command_max_output_bytes(), 65_536);
        assert_eq!(cfg.context.effective_tree_max_depth(), 4);
        assert_eq!(cfg.context.effective_tree_max_entries(), 200);
        assert_eq!(cfg.context.effective_rg_timeout_secs(), 10);
        assert_eq!(cfg.context.effective_max_search_files(), 20);
        assert_eq!(cfg.context.effective_max_keywords(), 16);
    }

    /// 同一判据从 TOML 文本出发：老配置文件不受影响。
    #[test]
    fn toml_without_new_sections_resolves_to_builtin_defaults() {
        let toml_src = r#"
project_root = "/abs/proj"
context_strategy = "on_demand"

[tool_policy]
approved = ["read_file"]
"#;
        let cfg: ProjectConfig = toml::from_str(toml_src).unwrap();
        assert_eq!(cfg.agent.effective_retrieval_max_tokens(), 8_000);
        assert_eq!(cfg.agent.effective_max_parallel_tools(), 4);
        assert_eq!(cfg.tools.effective_command_timeout_secs(), 60);
        assert_eq!(cfg.context.effective_max_keywords(), 16);
        assert_eq!(cfg.tool_policy.approved, vec!["read_file".to_string()]);
    }

    /// 并发上限的三态：没写 = 4、写 `0` = 4（**不是**不限）、写 2 = 2。
    ///
    /// `0` 必须落回缺省：这个闸的存在意义就是给子进程/socket/fd 数量设上限，
    /// 把 `0` 解释成"完全不限"正好把它删掉。
    #[test]
    fn max_parallel_tools_zero_means_default_not_unlimited() {
        let absent: ProjectConfig = toml::from_str("[agent]\n").unwrap();
        assert_eq!(absent.agent.effective_max_parallel_tools(), 4);
        let zero: ProjectConfig = toml::from_str("[agent]\nmax_parallel_tools = 0\n").unwrap();
        assert_eq!(zero.agent.effective_max_parallel_tools(), 4);
        assert_eq!(
            zero.agent.max_parallel_tools, 0,
            "原始值仍可读，生效值才钳制"
        );
        let two: ProjectConfig = toml::from_str("[agent]\nmax_parallel_tools = 2\n").unwrap();
        assert_eq!(two.agent.effective_max_parallel_tools(), 2);
    }

    #[test]
    fn new_sections_values_are_honoured_and_roundtrip() {
        let toml_src = r#"
[agent]
retrieval_max_tokens = 2000
max_events_per_frame = 8

[tools]
confirm_timeout_secs = 2
command_timeout_secs = 5
command_max_output_bytes = 1024

[context]
tree_max_depth = 2
tree_max_entries = 5
rg_timeout_secs = 1
max_search_files = 1
max_keywords = 2
"#;
        let cfg: ProjectConfig = toml::from_str(toml_src).unwrap();
        assert_eq!(cfg.agent.effective_retrieval_max_tokens(), 2_000);
        assert_eq!(cfg.agent.effective_max_events_per_frame(), 8);
        assert_eq!(cfg.tools.effective_confirm_timeout_secs(), 2);
        assert_eq!(cfg.tools.effective_command_timeout_secs(), 5);
        assert_eq!(cfg.tools.effective_command_max_output_bytes(), 1_024);
        assert_eq!(cfg.context.effective_tree_max_depth(), 2);
        assert_eq!(cfg.context.effective_tree_max_entries(), 5);
        assert_eq!(cfg.context.effective_rg_timeout_secs(), 1);
        assert_eq!(cfg.context.effective_max_search_files(), 1);
        assert_eq!(cfg.context.effective_max_keywords(), 2);

        let s = toml::to_string_pretty(&cfg).unwrap();
        let cfg2: ProjectConfig = toml::from_str(&s).unwrap();
        assert_eq!(
            cfg2.agent.effective_retrieval_max_tokens(),
            cfg.agent.effective_retrieval_max_tokens()
        );
        assert_eq!(
            cfg2.context.effective_max_keywords(),
            cfg.context.effective_max_keywords()
        );
    }

    #[test]
    fn explicit_zero_means_unset_not_unlimited() {
        let toml_src = r#"
[agent]
retrieval_max_tokens = 0
max_events_per_frame = 0
"#;
        let cfg: ProjectConfig = toml::from_str(toml_src).unwrap();
        assert_eq!(cfg.agent.retrieval_max_tokens, 0);
        assert_eq!(cfg.agent.effective_retrieval_max_tokens(), 8_000);
        assert_eq!(cfg.agent.effective_max_events_per_frame(), 64);
    }
}
