# XGent Agent 实现缺口审查报告

> 审查对象：XGent agent 实现（`crates/` 下 18 个 crate，约 41k 行 Rust）+ `doc/` 设计文档体系。
> 审查目的：梳理"还缺哪些功能"，并按风险与阻断程度排出优先级，供后续迭代排序。
> 本报告为只读审查产物，未修改任何既有文档。

## 一、排序规则

本清单严格按以下顺序排名，读者可据此判断"为什么这条在上面"：

| 层级 | 含义 | 说明 |
|:---:|:---|:---|
| **R1** | 安全 / 失控成本 / 无界执行 | 可导致无限执行、token 无止境消耗、上下文被单次调用撑爆、误执行破坏性操作 |
| **R2** | 阻断已确认 MVP 验收 | 需求文档已把该项列为 MVP 且标记为已实现，代码实际未实现 |
| **R3** | 日常使用降级 | 不阻断 MVP 验收，但影响主要使用路径的可用性 |
| **R4** | P1 / P2 路线能力 | 设计已规划、尚未开始或仅留占位 |

同一层级内按影响面（blast radius）排序，不按实现成本排序。

## 二、缺口类别标签

每条缺口带一个类别标签：

| 标签 | 含义 |
|:---|:---|
| `built-but-defective` | 有代码但不完整、无界、或与自身声明的契约矛盾 |
| `planned-not-built` | 设计文档规划了，代码中无实现 |
| `unwired-design` | 架构/插件设计声明的 trait、扩展点、上移职责无任何调用点 |
| `doc-inconsistency` | 既有文档的声称与当前代码矛盾，读者据此会得到错误认知 |

---

## 三、优先级缺口清单

### R1 — 安全 / 失控成本 / 无界执行

#### R1-1 agent loop 无迭代上限，工具可无限执行 · `built-but-defective`

内层循环仅在"LLM 不再返回 tool_calls"时退出，无计数器、无累计 token 上限、无墙钟超时。

- 证据：`crates/xgent_agent/src/bridge.rs:496` — `while has_tool_calls {`
- 全仓检索 `max_iterations` / `max_iter` / `MAX_ITER` / `iteration_limit` → 零命中
- 放大路径：LLM 反复返回同一 tool_call（如 `run_command` 失败后重试同一条命令）即进入无限工具执行，直接对应无限 token 计费

#### R1-2 用户中断不取消上游 LLM 请求，token 继续消耗 · `built-but-defective`

UI 侧 abort 只是 drop 接收端，daemon 侧已 spawn 的任务仍把 LLM 流消费到自然结束，HTTP 请求不取消。

- 证据：`crates/xgent_core/src/methods.rs:6-18` — 6 个 RPC method 中无 `provider.cancel`
- 证据：`crates/xgent_daemon/src/provider_pool.rs:103` — `tokio::spawn(async move { ... }` 后无生命周期登记，任务无取消路径

#### R1-3 `read_file` 无输出截断、无行区间参数，单次调用即可撑爆上下文 · `built-but-defective`

schema 只接受 `{path}`，全文回灌 LLM。对照 `run_command` 有 64KB 截断（`run_command.rs:29`），读取无任何上限。

- 证据：`crates/xgent_tools/src/builtins/read_file.rs:26-35`（schema 无 `offset`/`limit`）、`:78` — `tokio::fs::read_to_string(&full)` 后原样 `output: content`
- 与 R1-1 组合形成放大回路

#### R1-4 无 `edit_file` / `apply_patch` 类工具，改一行需提交整个文件 · `planned-not-built`

内置工具仅 4 个（`read_file`/`write_file`/`search_files`/`run_command`）加 `EditorTool`（UI-only）。全仓检索 `edit_file` / `apply_patch` / `str_replace` / `multi_edit` → 零命中。

- 证据：`crates/xgent_tools/src/lib.rs:32` — `default_tools()` 只返回 4 个工具
- 后果：模型每次修改都需回传全文，既浪费 token 又放大 R1-3 的风险；且无原文件备份/回滚

#### R1-5 `run_command` 无沙箱，危险命令检测是逻辑死代码 · `built-but-defective`

以用户全权限 `sh -c` 在项目根执行，无命令白名单、无环境变量限制、无网络限制。`approval_for` 的危险检测命中与未命中返回完全相同的 tier。

- 证据：`crates/xgent_tools/src/builtins/run_command.rs:1` — "（无沙箱，靠用户确认）"
- 证据：`crates/xgent_tools/src/builtins/run_command.rs:33` — `DANGER_PATTERNS` 仅 3 项（`rm -rf`/`sudo`/`mkfs`），缺 `curl`/`chmod`/`dd`/fork bomb
- 证据：`crates/xgent_tools/src/builtins/run_command.rs:86-93` — 命中 `DANGER_PATTERNS` 返回 `ToolTier::Exec`，未命中走 `self.tier()` 也是 `Exec`，`run_command.rs:85` 注释自述"逻辑等价但保留方法以支持 P1"
- 补强路径：`crates/xgent_tools/src/security.rs:38-45` 中 approved 配置 + 动态 tier 升级才生效，而 `severity()` 恒等（`tool.rs:99-106`），升级分支实际不可达

#### R1-6 插件 `run_command` 只校验 program 名，args 无任何过滤 · `unwired-design`

宿主侧权限校验仅比对 `cmd.program` 是否在清单白名单内，`cmd.args` 原样透传给 `tokio::process::Command`。

- 证据：`crates/xgent_plugin/src/host_state.rs:118-125` — `if !self.manifest.permissions.command.iter().any(|c| c == &cmd.program) { return PermissionDenied }`，随后 `command.args(&cmd.args)`
- 与设计文档 §9.2 提示的 argv 注入面一致，但代码未做 args 白名单/黑名单

#### R1-7 重试边界两头失守：可重试范围过窄，无上限时又无限重试 · `built-but-defective`

归入 R1 的理由是失控成本一侧：`max_retries` 缺省为 `None` 时**无限重试**，网络层反复失败会持续占用连接与配额，且用户没有任何中止手段（无退避上限之外的次数封顶）。

- 证据：`crates/xgent_agent/src/bridge.rs:112` — `max_retries == None` → `can_retry` 恒返回 true（无限重试）
- 另一面（可用性缺陷，同一处配置）：`crates/xgent_agent/src/bridge.rs:82-85` — `matches!(kind, ErrorKind::Network | ErrorKind::StreamParse)`，429/5xx 的 `ProviderError` 不重试、立即失败
- 全仓检索 `RateLimit` / `rate_limit` / `429` → 源码零命中（无退避响应头解析，未处理 `Retry-After`）

#### R1-8 API Key 明文存 TOML · `planned-not-built`

- 证据：`crates/xgent_settings_core/src/global.rs:61` — "API Key（MVP 明文存 TOML，未来考虑 keychain，见 D-02）"
- 阻塞项：D-02（API Key 是否用 OS keychain）仍未决

#### R1-9 `ToolUpdateCallback` 全链路恒传 `None`，长时工具无流式进度 · `unwired-design`

设计 §6.2 定义 `on_update` 供长时工具推送中间结果，执行器实际总传 `None`。

- 证据：`crates/xgent_tools/src/executor.rs:130` — `SecurityPolicy::Approved => tool.execute(input, ctx, signal, None).await`
- 证据：`crates/xgent_tools/src/tool.rs:127` — trait 声明；`crates/xgent_plugin_host/src/tool.rs:129` 自述"ToolExecutor MVP 总传 None...待 ToolUpdateCallback 改 Arc 后接通"
- 后果：`run_command` 长时输出只能等结束才呈现（与 D-P8 同源）

#### R1-10 异步桥接层在 Bevy 系统线程内 `blocking_lock` · `built-but-defective`

`agent_poll_system` 是 Bevy 系统，在主系统线程内对 tokio Mutex 取阻塞锁；一旦持锁 task 未完成即整帧卡死。另有 `EditorCommandSink::emit` 用 `std::sync::Mutex` 并直接 `unwrap()`（非 tokio 锁，属惯用写法，风险低于前者，仅记录）。

- 证据：`crates/xgent_agent/src/agent_loop.rs:172` — `editor_rx.rx.blocking_lock()`（Bevy 系统线程内阻塞）
- 证据：`crates/xgent_agent/src/bridge.rs:1237` — 锁为 `Arc<std::sync::Mutex<..>>`，`:1246` — `.lock().unwrap()`
- 与 AGENTS.md §5.2「所有子系统只通过 ECS Events/Messages 通信」的契约存在偏差

---

### R2 — 阻断已确认 MVP 验收

#### R2-1 F-07 的两个 provider 是空壳 · `planned-not-built`

`requirements.md` F-07 把"新的响应式接口（如 Response API）"与"支持自定义 API"列为 **MVP**，两个适配器全部方法返回"尚未实现"错误。

- 证据：`crates/xgent_provider/src/response_api.rs:31` — `Err(ProviderError::Config("ResponseApiProvider 尚未实现".into()))`
- 证据：`crates/xgent_provider/src/custom.rs:30` — `Err(ProviderError::Config("CustomApiProvider 尚未实现".into()))`
- 工厂已注册这两种 kind：`crates/xgent_provider/src/lib.rs:25`
- 后果：用户选到该 kind 时拿到的是误导性的"配置错误"而非"暂不支持"

#### R2-2 NF-04 的消息流录制/回放未实现 · `planned-not-built`

`requirements.md` NF-04 要求"支持无渲染的 headless 运行与消息流录制/回放"。

- 全仓检索 `replay` / `录制` / `回放` / `record_stream` → 零命中

#### R2-3 F-04 与安全策略文档矛盾，且只读操作也要弹窗 · `doc-inconsistency`

`requirements.md` F-04 写"只读类操作自动放行"，代码把 `Read`/`Write`/`Exec` 全映射为 `NeedsConfirmation`。AGENTS.md §5.4 已更新为"含只读类"，但需求原文未同步。

- 证据：`crates/xgent_tools/src/security.rs:58` — `ToolTier::Read | ToolTier::Write | ToolTier::Exec => SecurityPolicy::NeedsConfirmation`
- 文档位置：`doc/design/requirements.md` F-04 行 vs `AGENTS.md` §5.4

#### R2-4 `context_strategy` 配置项运行时无效 · `built-but-defective`

配置可设 `RepoMap`/`Vector`/`Hybrid`，但 `main.rs` 硬编码只注册 `OnDemandContextProvider`；`build_context_provider` 工厂是死代码（仅测试调用）。

- 证据：`crates/xgent_app/src/main.rs:106-107` — `context_hub.set_builtin(vec![Arc::new(OnDemandContextProvider::new(...)`
- 证据：`crates/xgent_context/src/lib.rs:32` — `match strategy` 工厂，全仓仅 `lib.rs:58`/`lib.rs:79` 测试调用
- 证据：`crates/xgent_settings_core/src/project.rs:31` — "MVP 仅用 `OnDemand`（方案 A），其余为后续阶段占位"

---

### R3 — 日常使用降级

#### R3-1 中文检索完全失效 · `built-but-defective`

产品定位"前期以中文为主"，但分词按 ASCII 标点与空白切分，无空格的中文句子整体成为一个 keyword，随后拼成正则必然零匹配。

- 证据：`crates/xgent_context/src/on_demand.rs:207` — `split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())`
- 证据：`crates/xgent_context/src/on_demand.rs:209` — `w.len() >= 3` 是**字节**长度判断，2 个汉字（6 字节）过闸但仍是整句
- 相关：`crates/xgent_context/src/on_demand.rs:193` — `relevance` 是硬编码文案 `"匹配用户问题或当前文件"`，非真实打分

#### R3-2 检索方案 B/C/D/E 四个 provider 全是空壳 · `planned-not-built`

- 证据：`crates/xgent_context/src/repo_map.rs:1`、`vector.rs:1`、`lsp.rs:1`、`hybrid.rs:1` — 均为占位，`retrieve` 直接返回 `ContextResult::default()`
- 触发条件已到：`requirements.md` §7.2 写"F-11 编辑器上线后触发 OQ-08 检索升级 C → D → E"，编辑器已上线

#### R3-3 `search_files` 仅子串匹配 · `built-but-defective`

无正则、无 glob 过滤、无大小写开关、无按文件类型过滤。

- 证据：`crates/xgent_tools/src/builtins/search_files.rs:212` — `if line.contains(pattern)`

#### R3-4 所有非 Rust 文件用 Rust grammar 解析，高亮错误而非降级 · `built-but-defective`

`Language` 枚举只有 `Rust` 一个变体且为 `#[default]`，而 `xgent_ui` 构造编辑器时用 `..default()`，从不设置语言。

- 证据：`crates/xui/src/text_editor.rs:44-47` — `pub enum Language { #[default] Rust }`
- 证据：`crates/xgent_ui/src/editor/tabs.rs:244-247` — `xui::TextEditor { line_height, ..default() }`
- 全仓 `Language::` 赋值点全在 `xui` 内部测试与 `highlight.rs:67`，`xgent_ui` 零赋值
- 注：本条记录**当前缺陷表现**（缺 grammar 分发时的错误降级）；"要加哪些语言"属路线项，见 R4-9 的多语言 grammar，两条不重复计数

#### R3-5 助手消息无 Markdown 渲染 · `planned-not-built`

对话面板把助手输出整段塞进单个 `Text` 节点，代码块/行内代码/列表/加粗均未解析。

- 证据：`crates/xgent_ui/src/chat_panel.rs:486` — `commands.entity(current).insert(Text::new(String::new()))`（助手消息即单个 `Text`）
- 证据：`crates/xgent_ui/src/chat_panel.rs:495` — `Query<&mut Text, With<CurrentAssistantText>>`，delta 直接累加进同一节点
- 全仓 `crates/xgent_ui/src/` 与 `crates/xui/src/` 检索 `markdown` → 零命中
- 计划位置：`doc/design/ui-gap-plan.md` 阶段 D

#### R3-6 `VirtualList` 组件零消费者 · `built-but-defective`

组件已实现并注册插件，但全仓无业务侧使用；`text_editor` 用的是另一套独立虚拟渲染。

- 证据：`crates/xui/src/virtual_list.rs:18` — `pub struct VirtualList`
- 引用仅存在于 `crates/xui/src/lib.rs:42`（re-export）、`:52`（插件注册）、`crates/xui/src/text_editor.rs:14`（注释提及）

#### R3-7 `xui` 的 i18n 桥接形同虚设 · `unwired-design`

`Strings`/`tr` 在 `xui` 内零消费者（仅测试注入），`xgent_ui` 走自己的 `i18n.rs` 直取 `Localizer`，绕过 trait 反转枢纽。

- 证据：`crates/xui/src/i18n_bridge.rs:12` — `pub struct Strings(...)`，`crates/xui/src/` 内除 `lib.rs` re-export 外无使用
- 证据：`crates/xgent_ui/src/i18n.rs:1` — "经 `xgent_settings::Localizer`（实现 `StringSource`）取本地化字符串"
- 补强：AGENTS.md §4 声称"i18n 用 trait 反转依赖：`xui` 经 trait 调用"，实际未接线
- 遗留：`crates/xgent_agent/src/agent_loop.rs:61` 有硬编码中文错误文案，未走 i18n

#### R3-8 线程内阻塞 IO 违反 NF-03 · `built-but-defective`

`async fn` 内用 `std::fs::read_dir` / `read_to_string` 遍历目录，大仓库阻塞 tokio 工作线程。

- 证据：`crates/xgent_context/src/on_demand.rs:230` — `let Ok(rd) = std::fs::read_dir(dir) else {`

#### R3-9 `ContextHub` 合并多个 provider 后不复核 token 预算 · `built-but-defective`

各 provider 各自控预算，合并时仅累加，无二次裁剪；且 provider 串行 await。

- 证据：`crates/xgent_context/src/hub.rs:59-77` — `result.total_tokens += r.total_tokens` 后直接返回

#### R3-10 文件树与预览无懒加载 · `built-but-defective`

全量载入 + 仅 5 帧 debounce，超大文件/大目录有卡顿风险。

- 证据：`crates/xgent_ui/src/file_panel.rs:466` — `const DEBOUNCE_FRAMES: u32 = 5`

#### R3-11 无撤销 agent 文件改动的机制 · `planned-not-built`

全仓检索 `revert` / `Revert` → 零命中。F-06 会话管理只有 list/restore，唯一的 undo 是编辑器内文本快照，与 agent 写文件无关。

#### R3-12 diff 仅公共前后缀，改中间一行退化为整段删增 · `built-but-defective`

无 LCS/Myers，影响确认弹窗与冲突视图的可读性。

- 证据：`crates/xgent_ui/src/diff.rs:26` — `let mut prefix = 0;`
- 自述：`crates/xgent_ui/src/diff.rs:4` — "复杂 diff（跨行移动）留待 P1"

#### R3-13 设置面板只暴露 provider 三项 · `built-but-defective`

`ToolPolicyConfig.approved/denied`（AGENTS.md §5.4 的核心安全旋钮）、重试参数、语言、主题、插件设置均无 UI，只能手改 TOML。

- 证据：`crates/xgent_ui/src/settings_panel.rs:1` — "设置面板：provider 配置（api_base / api_key / model）"

#### R3-14 主题只有 Default 一套，`preferences.theme` 无读取方 · `built-but-defective`

- 证据：`crates/xgent_ui/src/theme.rs:202` — `impl Default for Theme`
- `preferences.theme` 的读取点全在 `xgent_daemon/src/config_store.rs`（配置读写），无 UI 消费方

#### R3-15 工具并行执行未实现 · `unwired-design`

`Tool::concurrency()` 声明了 `Shared`/`Exclusive` 语义但全仓无调用点（仅 `tool.rs:7`/`:150` 注释提及）。多 tool_call 在同一 `for` 循环串行 await。

- 证据：`crates/xgent_agent/src/bridge.rs:680` — `for (call_id, name, args) in &outcome.tool_calls {`

#### R3-16 插件工具确认弹窗退化，无法展示真实 diff · `unwired-design`

WIT 已定义 `tool.preview-diff` 与动态 `approval_for`，宿主侧回退 trait 默认值。

- 证据：`crates/xgent_plugin_host/src/tool.rs:118` — "approval_for / preview_diff：MVP 裁决暂不接 WIT"
- 同处 `:114` — `summarize` 也未接 WIT（因 `Tool::summarize` 是同步签名，而 WIT 方法为 async）
- 设计位置：`doc/design/plugin-system-design.md` §5.3

#### R3-17 大量关键参数硬编码，无配置入口 · `built-but-defective`

| 参数 | 硬编码值 | 位置 |
|:---|:---|:---|
| 上下文窗口 | `128_000` | `crates/xgent_app/src/main.rs:157` |
| 检索 token 预算 | `8_000` | `crates/xgent_agent/src/bridge.rs:396` |
| Anthropic max_tokens | `8192` | `crates/xgent_provider/src/anthropic.rs:158` |
| 首事件超时 | 30s | `crates/xgent_provider/src/openai_compat.rs:24` |
| 确认超时 | 300s | `crates/xgent_tools/src/executor.rs:152` |
| 命令超时 | 60s | `crates/xgent_tools/src/builtins/run_command.rs:23` |
| 输出上限 | 64KB | `crates/xgent_tools/src/builtins/run_command.rs:29` |
| 检索参数 | 树深 4 / 条目 200 / rg 超时 10s / 搜索文件 20 | `crates/xgent_context/src/on_demand.rs:17-23` |
| 关键词数 | `truncate(8)` | `crates/xgent_context/src/on_demand.rs:213` |
| 每帧 agent 事件 | 64 | `crates/xgent_agent/src/agent_loop.rs:182` |

后果最直接的两条：`context_window` 不随实际模型变化（8k 小模型永不触发压缩，1M 模型被过早压缩）；每帧 64 上限在高吞吐时静默丢弃剩余事件，造成流式滞后。

#### R3-18 压缩失败仅落 stderr，无用户可见提示 · `built-but-defective`

- 证据：`crates/xgent_agent/src/bridge.rs:985` — `eprintln!("[compaction] 压缩失败，对话继续未压缩: {e}")`
- 同处 `:981` — `let model = "";` 作为 compactor 参数传入（形同虚设）
- 全仓无 tracing span、无重试

#### R3-19 `write_file` 的 diff 预览与超时保护缺失 · `built-but-defective`

- 证据：`crates/xgent_tools/src/builtins/write_file.rs:56` — `read_to_string(&full).await.unwrap_or_default()`，新建文件时 diff 退化为"全增"，与真实空文件不可区分
- 无 content 大小上限，无原文件备份

#### R3-20 `@selection` 引用未支持 · `built-but-defective`

- 证据：`crates/xgent_ui/src/editor/at_syntax.rs:107` — "@selection 暂不支持，原样保留，不收集 query"

#### R3-21 查找替换仅字面匹配，无正则 · `built-but-defective`

- 证据：`crates/xui/src/text_editor/find.rs:58` — `pub fn find_all(&mut self, text: &str)`

---

### R4 — P1 / P2 路线能力

#### R4-1 成本统计 F-12 未实现 · `planned-not-built`（阻塞于 OQ-10）

`TokenUsage` 类型已随流式响应收到，但无聚合、无费用换算、无 UI。

- 证据：`crates/xgent_ui/src/status_bar.rs:137` — "成本段（占位，OQ-10 成本统计细化后接入）"
- 数据来源已有：`crates/xgent_core/src/chat.rs:241` — `pub struct TokenUsage`

#### R4-2 MCP 支持 F-13 仅 trait 声明 · `planned-not-built`

- 证据：`crates/xgent_tools/src/mcp.rs:21` — "该 trait 当前为 P2 预留，尚无实现"，全文件仅抽象无传输层
- 优先级冲突：`requirements.md` 列为 P1，代码注释标 P2

#### R4-3 Git 集成的"回溯到某次提交"未实现 · `planned-not-built`

插件提供 `git_diff`/`git_log`/`git_status`/`git_commit`，无 checkout/reset。

- 证据：`crates/xgent_plugin_git/src/lib.rs:20-38`（四个工具定义）、`:78-117`（分发，无 checkout 分支）
- 文档冲突：`doc/design/requirements.md` F-10 含"回溯到某次提交"，`doc/dev-tutorial.md:44` 标 F-10 "✅ 已实现"

#### R4-4 虚拟宠物 F-15 未实现 · `planned-not-built`（阻塞于 D-07 / OQ-05）

`xgent_pet` crate 不存在，全仓无 `xgent_pet` 引用；状态栏仅有点亮态占位。

- 证据：`crates/xgent_ui/src/status_bar.rs:41` — "陪伴开关状态（宠物本体为 P1；本期为状态栏点亮态占位）"

#### R4-5 Git / 搜索 / 插件入口未接入主导航 · `unwired-design`

`xgent_plugin_git` 提供了 4 个工具和命令，但活动栏无对应入口，用户只能靠对话触发。

- 证据：`crates/xgent_ui/src/activity_bar.rs:6` — "不放搜索/Git/插件入口（F-05/F-10 未实现，非本期目标）"

#### R4-6 插件扩展点的已知限制 · `unwired-design`

| 扩展点 | 状态 | 证据 |
|:---|:---|:---|
| 插件 UI 面板（D-P3） | 未设计未实现 | `plugin-system-design.md` §14 标 P2 |
| Provider 插件接入 daemon（D-P4） | 未实现 | 同上，daemon 侧无 `PluginHost` |
| 插件 i18n（D-P6） | 未实现 | `host.get-locale` 未定义，插件字符串硬编码 |
| 插件 API 版本管理（D-P2） | 部分 | 清单有 `SchemaVersion`，WIT 无 `since_v0_x_x` |
| WASM 死循环 task 回收（D-P7） | MVP 接受 | Store/task 可能永久泄漏 |
| PluginOp channel 无背压（D-P9） | MVP 接受 | 注册风暴时单帧积压 |

#### R4-7 会话历史无 SQLite 元数据索引与全文检索 · `planned-not-built`（阻塞于 D-04）

- 证据：`crates/xgent_core/src/session.rs:1` — "会话持久化类型（JSONL entry），见 ADR-0008"，`sessions.db` 路径已定义但文件未使用
- 文档冲突：`doc/design/architecture.md` §7.1 写"f 会话历史（本地 SQLite）"

#### R4-8 终端剩余能力 · `planned-not-built`

按 `doc/design/terminal-design.md` §6 明示不支持：

- 全屏 TUI（alternate screen + 光标定位）—— `crates/xgent_terminal/src/render.rs:8` 自述不实现
- 上下历史（↑↓）、Tab 补全、多行输入、编码探测（GBK）
- `terminal.shell` / `terminal.cwd` / `terminal.args` 配置项（全仓 `xgent_settings_core` 无 terminal 字段）
- tab 持久化 / 跨会话恢复；命令拦截 / 输出脱敏；`cd` 越界告警
- 多窗口共享终端、PTY 持久化重连
- 遗留：`crates/xgent_ui/src/terminal/tabs.rs:6` — "pty_id 占位 0"

#### R4-9 编辑器剩余能力 · `planned-not-built`

- 多语言 grammar（D-06 已裁决 P1 只内置 Rust，扩展需二次重定分发策略）——路线的"加哪些语言"，与 R3-4"缺 grammar 时的错误降级"是同一缺口的两侧，不重复计数
- LSP 接入（跳转/重命名/hover/诊断）+ split view
- 3-way merge：`crates/xgent_ui/src/editor/conflict.rs:12` — "对比合并：打开 diff 视图（MVP 降级为并排只读）"
- `@` 引用补全 UI

#### R4-10 3D / TUI / Web 端 F-16/17/18 · `planned-not-built`

架构已预留扩展点（`architecture.md` §10），无代码实现。

---

## 四、文档与代码不一致（`doc-inconsistency`）

这些条目不单列优先级，因为它们本身不是功能缺口，而是会误导后续迭代的认知偏差。

**去重约定**：本表是索引，不是第二份缺口清单。带「同 R-x」标记的行已在 §三 对应条目中完整记录，此处只补充文档侧证据，**统计缺口数量时不得重复计数**；不带标记的行（D6、D8、D9、D12）是仅存在于文档层的偏差。

| # | 文档声称 | 代码实际 | 对应条目 | 证据 |
|:---:|:---|:---|:---|
| D1 | F-10 Git 集成已实现 | 缺"回溯到某次提交" | 同 R4-3 | `doc/dev-tutorial.md:44` vs `requirements.md` F-10；`crates/xgent_plugin_git/src/lib.rs:78-117` |
| D2 | F-04 "只读类操作自动放行" | `Read` 也需确认 | 同 R2-3 | `requirements.md` F-04 vs `crates/xgent_tools/src/security.rs:58`；AGENTS.md §5.4 已更新，需求原文未同步 |
| D3 | F-07 含 Response API / 自定义 API（MVP） | 两个 provider 空壳 | 同 R2-1 | `requirements.md` F-07 vs `response_api.rs:31`、`custom.rs:30` |
| D4 | NF-04 含消息流录制/回放 | 零实现 | 同 R2-2 | `requirements.md` NF-04；全仓检索零命中 |
| D5 | i18n trait 反转依赖（xui 经 trait 调用） | xui 侧零消费者 | 同 R3-7 | `AGENTS.md` §4 vs `crates/xui/src/i18n_bridge.rs:12`、`crates/xgent_ui/src/i18n.rs:11` |
| D6 | ADR 清单为 0001~0010 | 实际已有 0011~0014 | 文档独有 | `doc/README.md:63` vs `doc/decisions/` 目录（0011~0014 存在） |
| D7 | 代码现状 12 crate / 19k 行（2026-07-19） | 18 crate / 约 41k 行 | 文档独有 | `doc/README.md:5`、`doc/dev-tutorial.md:6`（13 crate / 22k）vs `crates/` 实际 |
| D8 | UI v7 M1~M7 全部落地 | `ui-v7-tasks.md` 仍有 71 项未勾选 | 文档独有 | `doc/plans/ui-v7-tasks.md` 头部声明已完成 vs 文件内未勾选项 |
| D9 | v3 UI 迁移任务 | `v3-ui-tasks.md` 82 项全部未勾选 | 文档独有 | `doc/plans/v3-ui-tasks.md` |
| D10 | F-13 MCP 为 P1 | 代码注释标 P2 | 同 R4-2 | `requirements.md` F-13 vs `crates/xgent_tools/src/mcp.rs:21` |
| D11 | 架构 §7.1 会话历史用本地 SQLite | 实现为 JSONL | 同 R4-7 | `doc/design/architecture.md` §7.1 vs `crates/xgent_core/src/session.rs:1` |
| D12 | `chat.rs:246` 遗留 TODO 注释 | "工具 schema 占位（step7 完善）"已过期 | 文档独有 | `crates/xgent_core/src/chat.rs:246` |

---

## 五、待决策点阻塞表

这些决策未落地会持续阻塞对应能力，建议与 R4 一起排期。

| 编号 | 主题 | 阻塞了什么 |
|:---|:---|:---|
| D-02 | API Key 是否用 OS keychain | R1-8 凭据安全 |
| D-04 | 会话历史 SQLite schema + 是否需全文检索 | R4-7 元数据索引、成本统计数据落点 |
| D-05 | 命令面板命令注册机制 | 外部/插件命令的稳定注册契约 |
| D-07 | 宠物等级度量 | R4-4 F-15 |
| OQ-05 | 宠物"对话陪伴"呈现形式 | R4-4 F-15 UI 形态与主对话流边界 |
| OQ-10 | 成本统计数据来源与展示形态 | R4-1 F-12 |
| D-P2 | 插件 API 版本管理 | 插件生态兼容性演进 |
| D-P3 | 插件 UI 面板扩展点 | 插件提供自定义面板 |
| D-P4 | Provider 插件接入 daemon | provider 扩展点实际可用 |
| D-P6 | 插件 i18n | NF-05 对插件字符串的覆盖 |
| D-P7 | WASM 死循环 task 回收 | 恶意/异常插件的 Store 泄漏 |
| D-P9 | PluginOp channel 背压 | 注册风暴时的稳定性 |

另有两项文档状态滞后（笔记正文已裁决但头部仍标"待决策"）：`doc/notes/context-retrieval-research.md`（OQ-08 已由 ADR-0010 分段裁决）、`doc/notes/i18n-research.md`（D-01 实际选了 fluent，与笔记倾向的 rust-i18n 不一致）。

---

## 六、建议的修复起手式

按"风险消除最快、依赖最少"排序，若要立刻开工，前四步互相独立可并行：

1. **R1-1 + R1-2**：给 agent loop 加迭代上限与累计 token 上限；补 `provider.cancel` RPC。这两条一起做完，无界执行与失控计费两个风险同时消失，是投入产出比最高的一组。
2. **R1-3 + R1-4**：`read_file` 加 `offset`/`limit` 与输出截断；新增 `edit_file` 工具（带原文件备份）。消除"改一行传全文"的成本放大。
3. **R1-5 + R1-6**：让 `approval_for` 的动态 tier 真正可达（引入更高 tier 或独立策略位），并给插件 `run_command` 加 args 过滤。
4. **R3-1**：中文分词。这一项与产品"前期以中文为主"的定位直接冲突，且改动局限在 `on_demand.rs` 的关键词提取函数内。

R2 的两条（F-07 空壳 provider、NF-04 录制回放）属于"要么实现要么从需求里划掉"的范围界定问题，建议先做决策再排期。