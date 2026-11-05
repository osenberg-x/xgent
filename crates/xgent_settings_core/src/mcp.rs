//! MCP Server 配置（全局 `~/.xgent/agent/mcp.toml` + 项目 `.xgent/mcp.toml`）。
//!
//! 项目配置默认被全局 `allow_project_servers = false` 忽略，避免打开不可信仓库
//! 即执行其中定义的命令。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// 单个 MCP Server 文件中的 `[[servers]]` 项。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerConfig {
    /// Server 标识（用于工具 id 前缀）。
    #[serde(default)]
    pub id: String,
    /// stdio 启动命令。
    #[serde(default)]
    pub command: String,
    /// 启动参数。
    #[serde(default)]
    pub args: Vec<String>,
    /// 额外环境变量。
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// 工作目录；相对路径按项目根解析。
    #[serde(default)]
    pub cwd: Option<String>,
    /// 是否启用。
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 是否随应用启动自动连接。
    #[serde(default = "default_true")]
    pub auto_start: bool,
}

fn default_true() -> bool {
    true
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            command: String::new(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            enabled: true,
            auto_start: true,
        }
    }
}

/// 合并后的 MCP 配置。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpConfig {
    /// 是否读取项目级配置；仅全局文件可设置。
    pub allow_project_servers: bool,
    /// 按 id 稳定排序的 Server。
    pub servers: BTreeMap<String, McpServerConfig>,
}

/// 加载结果：配置 + 可诊断的跳过原因。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpLoadOutcome {
    /// 合并后的配置。
    pub config: McpConfig,
    /// 解析/校验过程中产生的警告。
    pub warnings: Vec<String>,
}

/// 从全局/项目两个路径加载配置；`env` 用于 `${VAR}` 展开（测试可注入）。
pub fn load_mcp_config(
    global_path: &Path,
    project_path: &Path,
    env: &BTreeMap<String, String>,
) -> McpLoadOutcome {
    let mut warnings = Vec::new();
    let global = match read_config_file(global_path, "全局", &mut warnings) {
        Some(file) => file,
        None => {
            return McpLoadOutcome {
                config: McpConfig::default(),
                warnings,
            };
        }
    };
    let allow_project_servers = global.allow_project_servers;
    let mut servers = BTreeMap::new();
    insert_servers(global.servers, env, &mut servers, &mut warnings);

    if allow_project_servers {
        if let Some(project) = read_config_file(project_path, "项目", &mut warnings) {
            insert_servers(project.servers, env, &mut servers, &mut warnings);
        }
    }

    McpLoadOutcome {
        config: McpConfig {
            allow_project_servers,
            servers,
        },
        warnings,
    }
}

#[derive(Debug, Default, Deserialize)]
struct McpFileConfig {
    #[serde(default)]
    allow_project_servers: bool,
    #[serde(default)]
    servers: Vec<McpServerConfig>,
}

fn read_config_file(path: &Path, scope: &str, warnings: &mut Vec<String>) -> Option<McpFileConfig> {
    if !path.exists() {
        return None;
    }
    match std::fs::read_to_string(path) {
        Ok(source) => match toml::from_str::<McpFileConfig>(&source) {
            Ok(config) => Some(config),
            Err(error) => {
                warnings.push(format!(
                    "{scope} MCP 配置解析失败（{}）：{error}",
                    path.display()
                ));
                None
            }
        },
        Err(error) => {
            warnings.push(format!(
                "{scope} MCP 配置读取失败（{}）：{error}",
                path.display()
            ));
            None
        }
    }
}

fn insert_servers(
    servers: Vec<McpServerConfig>,
    env: &BTreeMap<String, String>,
    out: &mut BTreeMap<String, McpServerConfig>,
    warnings: &mut Vec<String>,
) {
    for mut server in servers {
        if server.id.trim().is_empty() {
            warnings.push("MCP server 缺少 id，已跳过".to_string());
            continue;
        }
        if server.command.trim().is_empty() && server.enabled {
            warnings.push(format!("MCP server `{}` 缺少 command，已跳过", server.id));
            continue;
        }
        match expand_server(&mut server, env) {
            Ok(()) => {
                out.insert(server.id.clone(), server);
            }
            Err(name) => warnings.push(format!(
                "MCP server `{}` 引用了未设置的环境变量 `{name}`，已跳过",
                server.id
            )),
        }
    }
}

fn expand_server(
    server: &mut McpServerConfig,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    server.command = expand_env(&server.command, env)?;
    for arg in &mut server.args {
        *arg = expand_env(arg, env)?;
    }
    for value in server.env.values_mut() {
        *value = expand_env(value, env)?;
    }
    if let Some(cwd) = &server.cwd {
        server.cwd = Some(expand_env(cwd, env)?);
    }
    Ok(())
}

/// 展开 `${VAR}`；变量缺失返回变量名。
fn expand_env(input: &str, env: &BTreeMap<String, String>) -> Result<String, String> {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            output.push_str(&rest[start..]);
            return Ok(output);
        };
        let name = &after[..end];
        let Some(value) = env.get(name) else {
            return Err(name.to_string());
        };
        output.push_str(value);
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("创建目录");
        }
        fs::write(path, contents).expect("写文件");
    }

    fn loads(global: &str, project: Option<&str>) -> McpLoadOutcome {
        let temp = tempfile::tempdir().expect("临时目录");
        let global_path = temp.path().join("mcp.toml");
        let project_path = temp.path().join("project-mcp.toml");
        write(&global_path, global);
        if let Some(project) = project {
            write(&project_path, project);
        }
        let env = BTreeMap::from([
            ("TOKEN".to_string(), "secret".to_string()),
            ("HOME_DIR".to_string(), "/tmp/home".to_string()),
        ]);
        load_mcp_config(&global_path, &project_path, &env)
    }

    #[test]
    fn project_is_ignored_by_default() {
        let outcome = loads(
            r#"
[[servers]]
id = "global"
command = "npx"
args = ["-y", "pkg"]
"#,
            Some(
                r#"
[[servers]]
id = "project"
command = "node"
"#,
            ),
        );
        assert!(!outcome.config.allow_project_servers);
        assert!(outcome.config.servers.contains_key("global"));
        assert!(
            !outcome.config.servers.contains_key("project"),
            "默认不得读取项目配置：{:?}",
            outcome.config.servers
        );
    }

    #[test]
    fn project_overrides_and_appends_when_allowed() {
        let outcome = loads(
            r#"
allow_project_servers = true
[[servers]]
id = "shared"
command = "npx"
args = ["global.js"]
[[servers]]
id = "global-only"
command = "global"
"#,
            Some(
                r#"
[[servers]]
id = "shared"
command = "node"
args = ["project.js"]
[[servers]]
id = "project-only"
command = "project"
"#,
            ),
        );
        assert!(outcome.config.allow_project_servers);
        let shared = outcome.config.servers.get("shared").expect("shared");
        assert_eq!(shared.command, "node");
        assert_eq!(shared.args, vec!["project.js"]);
        assert!(outcome.config.servers.contains_key("global-only"));
        assert!(outcome.config.servers.contains_key("project-only"));
        assert_eq!(outcome.config.servers.len(), 3);
    }

    #[test]
    fn env_vars_expand_and_missing_ones_skip_server() {
        let outcome = loads(
            r#"
[[servers]]
id = "ok"
command = "${HOME_DIR}/bin/server"
args = ["--token", "${TOKEN}"]
env = { TOKEN = "${TOKEN}" }
[[servers]]
id = "missing"
command = "server"
args = ["${NOT_SET}"]
"#,
            None,
        );
        let ok = outcome.config.servers.get("ok").expect("ok");
        assert_eq!(ok.command, "/tmp/home/bin/server");
        assert_eq!(ok.args, vec!["--token", "secret"]);
        assert_eq!(ok.env.get("TOKEN").map(String::as_str), Some("secret"));
        assert!(
            !outcome.config.servers.contains_key("missing"),
            "变量缺失的 server 应跳过"
        );
        assert!(
            outcome
                .warnings
                .iter()
                .any(|warning| warning.contains("missing")),
            "应记录跳过原因：{:?}",
            outcome.warnings
        );
    }

    #[test]
    fn malformed_global_is_empty_without_panic() {
        let outcome = loads("[[servers\nid = ", None);
        assert!(outcome.config.servers.is_empty());
        assert!(!outcome.warnings.is_empty());
    }

    #[test]
    fn malformed_project_falls_back_to_global() {
        let outcome = loads(
            r#"
allow_project_servers = true
[[servers]]
id = "global"
command = "global"
"#,
            Some("[[servers\n"),
        );
        assert!(outcome.config.servers.contains_key("global"));
        assert!(
            outcome
                .warnings
                .iter()
                .any(|warning| warning.contains("项目") || warning.contains("project")),
            "应记录项目配置解析失败：{:?}",
            outcome.warnings
        );
    }

    #[test]
    fn invalid_or_duplicate_servers_are_skipped_or_last_wins() {
        let outcome = loads(
            r#"
[[servers]]
id = ""
command = "no-id"
[[servers]]
id = "dup"
command = "first"
[[servers]]
id = "dup"
command = "second"
[[servers]]
id = "no-command"
"#,
            None,
        );
        assert!(!outcome.config.servers.contains_key(""));
        assert!(!outcome.config.servers.contains_key("no-command"));
        assert_eq!(
            outcome
                .config
                .servers
                .get("dup")
                .map(|s| s.command.as_str()),
            Some("second")
        );
    }

    #[test]
    fn doc_example_parses() {
        let doc_global = r#"
allow_project_servers = false

[[servers]]
id = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
env = { TOKEN = "${TOKEN}" }
cwd = "."
enabled = true
auto_start = true
"#;
        let outcome = loads(doc_global, None);
        let server = outcome
            .config
            .servers
            .get("filesystem")
            .expect("文档示例应可解析");
        assert_eq!(server.command, "npx");
        assert_eq!(server.env.get("TOKEN").map(String::as_str), Some("secret"));
        assert_eq!(server.cwd.as_deref(), Some("."));
        assert!(!outcome.config.allow_project_servers);
    }

    #[test]
    fn project_can_disable_global_server_without_command() {
        let outcome = loads(
            r#"
allow_project_servers = true
[[servers]]
id = "shared"
command = "global"
"#,
            Some(
                r#"
[[servers]]
id = "shared"
enabled = false
"#,
            ),
        );
        let shared = outcome.config.servers.get("shared").expect("shared");
        assert!(!shared.enabled);
        assert!(shared.command.is_empty());
    }

    #[test]
    fn disabled_server_is_retained_but_not_started() {
        let outcome = loads(
            r#"
[[servers]]
id = "off"
command = "server"
enabled = false
auto_start = false
"#,
            None,
        );
        let off = outcome.config.servers.get("off").expect("off");
        assert!(!off.enabled);
        assert!(!off.auto_start);
    }
}
