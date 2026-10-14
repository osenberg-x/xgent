# 缺口处置台账

> 本文件记录 `doc/notes/agent-gap-review.md` 中每条缺口的处置结果。
> 报告本身不修改（它是输入）；本台账是输出的索引。
>
> 生成于 `close-agent-gaps` 变更。

## 一、已实现（13 条；R1 全部 10 条 + R2 的 3 条可做项，R2-2 见下节）

| 缺口 | 落点 | 说明 |
|:---|:---|:---|
| R1-1 agent loop 无迭代上限 | `xgent_agent/src/bridge.rs`（`LoopBounds` / `BoundedReason` / `Bounded` 事件） | 迭代轮次闸 + 累计 token 闸，命中时发 `Bounded` 事件说明命中的界，正常终止而非错误 |
| R1-2 abort 不取消上游 | `xgent_agent/src/bridge.rs` + `xgent_daemon/src/provider_pool.rs` + `methods::PROVIDER_CANCEL` | 新增 `provider.cancel` RPC；daemon 维护活跃流登记表并在取消时终止推送 task；`ProviderClient::cancel` 新增默认实现；cancel token 分支与 Abort 命令分支**都**取消上游（两分支竞争同一信号，漏一条即失效） |
| R1-3 `read_file` 无截断/无行区间 | `xgent_tools/src/builtins/read_file.rs` + `lib.rs::truncate_output` | 新增 `offset`/`limit`（按行）、按字符边界截断、超范围返回空或钳制而非报错 |
| R1-4 无 `edit_file` | `xgent_tools/src/builtins/edit_file.rs` + `atomic.rs` | 行范围替换；`old_content` 校验不匹配则报错不写入；原子写；写前备份原文件 |
| R1-5 危险命令检测失效 | `run_command.rs` + `tool.rs::ToolTier::Dangerous` | 新增 `Dangerous` tier（severity 高于 Exec），使 `resolve_policy` 的动态升级分支真正可达；模式表从 3 项扩到 16 项 |
| R1-6 插件 args 无过滤 | `xgent_plugin/src/host_state.rs::check_args` | 通用危险参数黑名单（`--upload-pack`/`--ext-diff`/`-c` 等）+ shell 元字符 + 清单可选 `command-args` 子命令白名单 |
| R1-7 重试无界/429 不重试 | `bridge.rs::RetryConfig` + `chat.rs::ErrorKind` + `provider.rs::parse_retry_after` | `max_retries=None` 钳到有限缺省；新增 `RateLimited`/`ServerError` 两种可重试 kind；`Retry-After` 优先于计算退避（仍受 `max_delay_ms` 约束） |
| R1-8 API Key 明文 | `xgent_settings_core/src/keychain.rs` + `build_provider` | macOS `security` / Linux `secret-tool` 读取优先，失败回退 TOML；**永不删除已有 TOML 值** |
| R1-9 `ToolUpdateCallback` 断链 | `tool.rs` + `executor.rs` + `bridge.rs` + `xgent_ui/src/tool_panel.rs` | 回调改为 `Arc`；executor 三条执行路径都传真实回调；新增 `ToolProgressMessage` 与 UI 增量刷新；插件 `push-update` 桥接接通 |
| R1-10 `blocking_lock` | `xgent_agent/src/agent_loop.rs` | 改 `try_lock()`，抢不到即跳过本帧（事件留在 channel 下帧再取），主线程不再被异步 task 阻塞 |
| R2-1 两个占位 provider | `xgent_provider/src/response_api.rs` + `custom.rs` | Response API 按 `input`/`response.*` 事件实现并翻译为统一 `ChatEvent`；Custom 复用兼容适配器（用户 base URL + 额外请求头 + 模型 id），缺 base URL 显式报错。两者均无「尚未实现」路径 |
| R2-3 F-04 文档矛盾 | `doc/design/requirements.md` F-04 | 措辞订正为与已交付行为一致（默认全部需确认），未改代码 |
| R2-4 `context_strategy` 无效 | `xgent_context/src/lib.rs` + `xgent_app/src/main.rs` | 按配置选 provider；无实现的策略返回 `Unsupported` 并在启动时报错退出，**不静默回退** |

顺带修复（本次变更中发现，与上述缺口直接相关）：

- `build_plugins.sh` 的 `ROOT` 解析到仓库父目录，打包必然失败（已修正为脚本所在目录）。
- `crates/xgent_settings_core/src/paths.rs` 两个测试未持 `ENV_LOCK`，并发时误读临时目录导致偶发失败（已加锁）。

## 二、延后

| 组 | 内容 | 理由 |
|:---|:---|:---|
| R2-2 | NF-04 消息流录制/回放 | 需要先定录制存储位置与"回放 vs 实时会话"的关系语义。这是会话格式决策，不是安全或验收缺陷，不应塞进一次安全整改里 |
| R3（21 条） | CJK 分词、Markdown 渲染、LCS diff、正则搜索、懒加载、i18n 桥接接线、VirtualList 接入、10 项硬编码转配置、检索方案 B–E、编辑器多语言 grammar 等 | 每条都是自包含特性。其中 R3-2 的向量/LSP/混合检索额外阻塞于嵌入模型与向量库选型、以及 D-06 的二次重定 |
| R4（10 条） | F-12 成本统计、F-13 MCP、F-15 宠物、F-10 回溯提交、插件 UI 面板、SQLite 索引、终端与编辑器剩余能力、3D/TUI/Web | 混合了三类：**可做但需多天**（终端/编辑器剩余、SQLite 索引、MCP stdio）、**阻塞于未决设计**（F-15 阻塞 D-07/OQ-05；D-P2/D-P3/D-P4/D-P6 未决；F-12 阻塞 OQ-10）、**被项目规则禁止**（3D/TUI/Web，`AGENTS.md` §6.5） |

`doc/notes/agent-gap-review.md` 继续作为 R3/R4 的追踪台账，本文件不重复其细节。

## 三、遗留说明

- **策略不再有「未实现」占位 provider**：`build_context_provider` 与两个 provider 适配器均已落地或显式报错，不存在"返回尚未实现错误"的路径。
- **Windows 无凭据存储集成**：`keychain.rs` 的 Windows 分支返回"不支持"，回退 TOML。这是已知缺口而非静默失败。
- **`Retry-After` 仅支持 delta-seconds**：HTTP-date 形态需要日期解析依赖，本次未引入，解析失败即退回计算退避。
## 四、本次变更发现的既有问题（非本次范围，仅记录）

| 问题 | 位置 | 说明 |
|:---|:---|:---|
| 清单权限字段 kebab-case 未映射 | `crates/xgent_plugin/src/manifest.rs` | TOML 写 `fs-read`，结构体字段是 `fs_read`，无 `rename_all`，故 `fs-read` 实际解析为空。本次只给新增的 `command_args` 加了 `rename = "command-args"`，**未改** `fs_read`——改它会改变既有 fs 权限的实际生效范围，属独立变更 |
| `build_plugins.sh` 的 ROOT 解析错误 | `build_plugins.sh` | 原为 `dirname/..`，脚本在仓库根时解析到父目录，打包必然失败。已修正（本次需要它来重建插件，故顺手修） |
| `paths.rs` 两个测试未持 ENV_LOCK | `crates/xgent_settings_core/src/paths.rs` | 并发时读到被其他测试改写的环境变量而偶发失败。已加锁 |

## 五、独立验证发现并已修复的问题

首次独立验证（14 项中 12 项通过，A3/A4 失败）暴露了两个真实链路断裂，均已修复：

| 问题 | 根因 | 修复 |
|:---|:---|:---|
| **A3 `Retry-After` 在 IPC 边界被丢弃** | `ProviderPool::chat` 的 `map_err` 只取 `kind` + `to_string()`，丢掉 `retry_after_secs`；`session::provider_chat` 也只把 kind 编进 `error.data`；`IpcProviderClient` 再从 `data` 还原时硬编码 `None`。结果 `ProviderError::retry_after_secs()` 在生产代码中无任何调用者，限流下永远按客户端退避重试 | 引入 `ProviderCallError{kind, message, retry_after_secs}`；`error.data` 编码为 `{kind, retry_after_secs}` 对象；UI 侧兼容旧版裸 kind 形态并还原两者。两侧各加单测 |
| **A4 drop 接收端不等于取消上游 HTTP 请求** | `openai_compat::chat` 用 `tokio::spawn` 跑 `run_stream`，该 task 独占 body 流；所有 `tx.send(..)` 都写作 `let _ = ..` 吞掉错误。daemon 取消后接收端被 drop，发送立即返回 Err 但被忽略，provider task 继续把 body 读完——上游照常生成并计费，正是 R1-2 要消除的失效模式 | 三个 provider 的流循环在每轮开头检查 `tx.is_closed()`，为真则 `return`，`s` 被 drop 使 reqwest 关闭 body 与连接。加了两个针对性单测（接收端预先丢弃须立即返回；中途丢弃须很快退出） |

第二轮验证确认上述两处修复到位，并指出首轮我为 A4 写的两个测试**没有判别力**
（预生成的内存上游在接收端 drop 后 `send` 立即返回 Err，不加守卫也会瞬间跑完）。
已重写为可观测拉取次数的上游，并**实测验证判别力**：临时移除 `is_closed()`
守卫后两个用例分别报「实际拉了 1000 帧」与「丢弃时 7 → 等待后 9735」并失败，
还原守卫后通过。第二轮同时指出 anthropic 适配器在取首事件前缺守卫，
已补齐使其与另两个 provider 语义一致。

同时修复的验证发现：

| 问题 | 修复 |
|:---|:---|
| `BoundedMessage` 无 UI 消费者 → 用户看不到命中的界（A2 的核心表述未被满足） | `xgent_ui::chat_panel::show_bounded_notice` 把 `detail` 固化为一条 assistant 气泡 |
| `edit_file` 未校验写入大小上限（`write_file` 有） | 补 `MAX_WRITE_BYTES` 检查 |
| 台账写「模式表扩到 18 项」，实际 16 项 | 数字订正 |
| `plugin_host/tool.rs` 残留一段已过期的「待 ToolUpdateCallback 改 Arc 后接通」注释，紧挨着已生效的新代码 | 旧注释删除 |
| A4 的测试对修复不敏感（见上） | 重写为可观测拉取计数并实测判别力 |

---

## 六、close-context-gaps（2026-10-14）：R3 检索组

承接 §二 中延后的 R3 条目，本轮关闭 `xgent_context` 里互相咬合的三条：R3-1（中文检索完全失效 + relevance 假文案）、R3-8（线程内阻塞 IO 违反 NF-03）、R3-9（Hub 合并后不复核预算）。正式 Spec 见 `docs/comet/specs/context-retrieval/spec.md`。

| 缺口 | 落点 | 说明 |
|:---|:---|:---|
| R3-1 中文分词 | `on_demand.rs`：`split_runs` / `cjk_bigrams` / `extract_keywords` | CJK 连续串切成**相邻二字组合**；单字 run 自然产出 0 个词片；整句再也不会成为一个关键词（旧行为下中文提问必然零命中）。长度判据由 `w.len() >= 3`（字节）改为 `chars().count() >= 3`；新增中英虚词表 `CN_FUNC_CHARS` / `EN_STOPWORDS` 作精度过滤；上限 `MAX_KEYWORDS` 8→16，因为 bigram 会成倍放大数量，8 个会截掉中文问句后半段 |
| R3-1 relevance 假文案 | `on_demand.rs::relevance_for` | chunk 的 `relevance` 改为「来源角色 + 内容里实际命中的关键词」，替代恒定文案「匹配用户问题或当前文件」；搜索命中但片段被裁到看不见关键词时，如实写「未包含任何关键词」 |
| R3-1 家族 `@file:` | `on_demand.rs::file_refs` | 编辑器 `@` 引用经 bridge 以 `@file:<path>` hint 传入，旧实现把**整串当检索词**（任何文件内容都不含 `@file:...`，等于引用无效）。现解析出路径直接读该文件，且不再进入关键词 |
| R3-1 家族 rg | `on_demand.rs::rg_args` | 由 `keywords.join("\|")` 拼正则改为 `--fixed-strings` + 每个关键词一个 `-e`。关键词是用户输入的文本：`.` 会变通配，`(`/`+` 之类让模式非法从而 rg 失败并触发全量遍历回退 |
| R3-8 | `on_demand.rs::collect_tree_lines` / `collect_matching_files` + `spawn_blocking` | 遍历与降级搜索的 `std::fs` 调用全部收进非 async 函数，async 侧只经 `tokio::task::spawn_blocking` 调用。返回内容不变（同样的树文本、相对路径、忽略表、深度与条目上限、大小写敏感语义） |
| R3-9 | `hub.rs::clamp_to_budget` | 合并后按 `query.max_tokens` 复核：按 provider 顺序贪心保留装得进的 chunk，装不下的**整块丢弃**（Hub 不截断别的 provider 交来的文件片段），`total_tokens` 由保留 chunk 重算；`tree_summary` 不占 chunk 预算 |

仍留在台账 §二 的 R3 行内、本轮未做：R3-2（B/C/D/E 空壳 provider，阻塞于嵌入模型与向量库选型 + D-06 二次重定）、R3-3（`search_files` 正则/glob，属 `xgent_tools`）、R3-17（硬编码参数转配置）。

### 判别力反证（11 个变异体）

沿用 §五 的方法：逐个把修复改回缺陷形态，确认对应用例**确实失败**；全部还原后基线转绿。

| 变异 | 失败用例 |
|:---|:---|
| M1 关闭 CJK bigram（整段当一个关键词） | `extract_keywords_splits_cjk_into_bigrams`、`_caps_at_sixteen_in_order`、`_drops_single_cjk_char`、`_measures_chars_not_bytes`、`retrieve_finds_file_from_chinese_question` |
| M2 长度判据改回按字节 | `extract_keywords_measures_chars_not_bytes` |
| M3 rg 改回拼接 `a\|b` 正则 | `rg_args_matches_each_keyword_literally` |
| M4 `@file:` 线索不再排除 | `extract_keywords_excludes_file_ref_hint` |
| M5 relevance 回到固定文案 | `relevance_lists_only_keywords_present_in_content` |
| M6 Hub 不再复核预算 | `over_budget_merge_keeps_prefix_and_recomputes_total`、`zero_budget_keeps_nothing`、`clamp_drops_chunks_larger_than_budget` |
| M7 只关闭外层标点断开（仅按 CJK 类别切分） | 无一失败——经实测判定为**行为等价变异**（见下），不是盲区 |
| M8 分隔符被跳过而非作为边界（粘回一整段） | `no_bigram_spans_a_separator`、`extract_keywords_filters_short`、`_caps_at_sixteen_in_order`、`_drops_function_words` |
| M9 `tree_summary` 不走 spawn_blocking | `traversal_helpers_are_awaited_via_spawn_blocking` |
| M10 `async fn` 体内直接 `std::fs::read_dir` | `std_fs_calls_never_appear_in_async_fns` |
| M11 降级搜索不走 spawn_blocking | `traversal_helpers_are_awaited_via_spawn_blocking` |

M7 是本轮真正学到的东西，但**结论与我先前的记述不同**——独立验证把它证伪了一半：

- 先用为「盲区」的说法是错的。我用一个独立小程序对两种切分做产出对比（`上下文 检索` / `上下文,检索` / `上下文。检索` / `hello world`）：M7 与现行实现**输出完全一致**。CJK 的边界由「CJK / 非 CJK 类别切换」保证，外层分隔符断开对中文不起作用，所以这条变异对中文路径本就是**行为等价变异**，没有任何输入行为测试能区分它，也不该指望区分。
- 两者唯一的真实差异在**含下划线的标识符**：现行实现把 `find_me_x` 切成 `find`（`_` 属 ASCII 标点，先断开），M7 保留整串 `find_me_x`。现行行为与变更前一致（旧实现同样按 ASCII 标点断开），故不是回归。若要锁死这条选择，加一条 `extract_keywords("find_me_x")` 的断言即可——属测试补强，不改行为，本轮未做。
- 真正抓回「分隔符粘连」这类回归的是 **M8**（分隔符被 `continue` 跳过而非当作边界），它使 `no_bigram_spans_a_separator`、`extract_keywords_filters_short`、`_caps_at_sixteen_in_order`、`_drops_function_words` 四个用例失败。

另两处独立验证纠正：

| 纠正 | 事实 |
|:---|:---|
| 「本机 PATH 无真实 ripgrep，rg 分支未端到端执行」**是错的** | `sh -c 'command -v rg'` 解析到 `~/.qoder-cn/plugins/cache/.../bin/rg`（ripgrep 15.0.0）。`which rg` 只显示 shell alias，容易误判为「不在 PATH」。验证者用真实 rg 跑了构造出的 argv：`--fixed-strings` + 逐词 `-e` 只命中 `lit.rs`，而旧的 `a+b\|ContextHub` 拼接正则还会误命中 `other.rs`，`(` 这类词旧路径下 rg 退出码 2 并触发全量回退。A1/A4 因此比自报的更强 |
| `doc/dev-tutorial.md` 引用的 Spec 路径在归档前是悬空链接 | 归档发布后才生效，属正常时序，不是缺陷 |

后续待办（本轮范围外，记此以免遗忘）：

- `@file:<path>` 解析未限制在项目根内（绝对路径与 `..` 可越界读取）。`read_file_chunk` 对 `current_file` 早已如此，本轮把同一逻辑延伸到 hints，未新增约束也未收紧。收紧会改变已确认行为，需另立变更。
- 降级遍历大小写敏感、rg 路径 `-i` 不敏感（既有分歧，统一它会改变返回文件集合）。
- `cargo test --workspace` 仍未跑（全量测试二进制链接在本机 20 分钟以上），覆盖面依赖 `cargo check --workspace --all-targets` + 本 crate 全量测试。

`NF-03 不进异步线程` 这条约束无法用行为测试判别（两种写法返回结果一致），故改为**源码级断言**：`std_fs_calls_never_appear_in_async_fns` 逐行核对每个 `std::fs::` 调用点所属函数头不是 `async fn`；`traversal_helpers_are_awaited_via_spawn_blocking` 断言两个遍历函数在 `spawn_blocking` 闭包内被调用。代价是对源码排版敏感（调用写成跨多行的闭包体即失效），M9/M11 已确认二者有判别力。

### 工具陷阱（复现过一次，值得记住）

`cargo fmt` 与反证脚本并发跑会互相踩：脚本用 `.bak` 回写源码时 `shutil.move` 保留了旧 mtime，cargo 判定产物仍新于源码，**直接复用了变异体的缓存二进制**，于是「还原后基线」报出一个并不存在的失败。反证脚本必须在回写后 `touch` 源文件；且格式化要在变异批次之外单独跑。

---

## 七、close-search-config-gaps（2026-10-14）：R3-3 与 R3-17

承接 §六 末尾点名的两条未阻塞 R3 遗留项。正式 Spec 见 `docs/comet/specs/code-search/spec.md`（R3-3）与 `docs/comet/specs/runtime-parameters/spec.md`（R3-17）。

### R3-3 `search_files` 只子串匹配

| 落点 | 说明 |
|:---|:---|
| `builtins/search_files.rs`：`Matcher` / `compile_matcher` | 新增 `regex`（**默认关**，`false` 时保持字面量子串，既有调用与既有测试逐字不变）、`case_insensitive`（字面与正则两种模式都适用）。正则由 `regex::RegexBuilder` 逐行匹配，不跨行；lookaround 与反向引用属该引擎不认识的构造，与括号不配对同判为非法模式 |
| `crates/xgent_tools/src/glob.rs`（新增，crate 内） | `glob` 自建翻译成锚定正则：`*` 不跨 `/`、`**` 跨、`?` 单字符、`{a,b}` 分支，其余字符转义。**不含 `/` 的模式只匹配文件名**（ripgrep `--glob` 约定，`"*.rs"` 在任意深度命中）。不引 `globset`：本工具用不到它的 gitignore 语义与字集能力，而 glob 规则必须能在工具描述里一次说清 |
| `FILE_TYPES` 表 + `type_extensions` | `file_type` 用 ripgrep `--type` 的名称词表（`rust`/`python`/`cpp`… 一个名称覆盖多个扩展名），扩展名比较大小写不敏感 |
| `ToolSchema` 描述 | 声明四个新参数、默认值与「非法参数是错误而不是未找到」这条判据——选参数的是模型，描述就是契约 |
| 关键契约 | **坏参数是错误，不是零结果**：非法 regex / 非法 glob / 未知 `file_type` 都在遍历开始之前返回 `is_error`，错因点名是哪个参数以及为什么非法。改前 `"(foo"` 会得到「未找到匹配」，模型据此断定代码不存在并给出错答 |
| 不变的部分 | 忽略目录、1MB 单文件跳过、`MAX_DEPTH`/`MAX_RESULTS`、取消边界、输出格式 `路径:行号: 内容`、`ToolTier::Read` + `Concurrency::Shared`。被筛掉的文件不读、也不占用结果上限 |

### R3-17 关键参数硬编码

台账点名的 13 个限额全部落成配置（共 14 个键，上下文窗口另有按模型的映射表）。核心不变量：**缺省值逐项等于它替换掉的常量**，不写配置的工程行为与改前等价——由 serde 测试当作数据断言，而不是靠人读 `default` 函数。

| 配置键 | 缺省（= 改前常量） | 接线 |
|:---|:---|:---|
| `[providers.<id>] context_window` / `model_context_windows` | `128_000` | `GlobalConfig::resolve_context_window` → `xgent_app` → `AgentBridgeConfig`；次序为按模型 > provider 级 > 内置回落，**走回落时启动打一条 warn** |
| `[providers.<id>] max_output_tokens` | `8192` | `AnthropicProvider::with_max_output_tokens` → 请求体 `max_tokens`。OpenAI 兼容 / Response API 改前就不发输出上限，本键对它们无效（已写进 Spec，不静默吞掉） |
| `[providers.<id>] first_event_timeout_secs` | `30` | 两个适配器的 `with_first_event_timeout_secs`，`build_provider` 与 `CustomApiProvider::with_config` 负责透传；idle 与 connect 超时未动 |
| `[agent] retrieval_max_tokens` / `max_events_per_frame` | `8_000` / `64` | `AgentBridgeConfig` → 检索查询预算；每帧上限作为 `AgentBridge` 字段被 ECS drain 读取（到界即停，剩余留在 channel，**不丢事件**） |
| `[tools] confirm_timeout_secs` / `command_timeout_secs` / `command_max_output_bytes` | `300` / `60` / `65_536` | `ToolParams::from_tool_config` → `ToolExecutor::with_params` 与 `RunCommand::with_params`；`default_tools(&ToolParams)` |
| `[context] tree_max_depth` / `tree_max_entries` / `rg_timeout_secs` / `max_search_files` / `max_keywords` | `4` / `200` / `10` / `20` / `16` | `RetrievalLimits::from_context_limits` → `OnDemandContextProvider::with_limits`，透传给同步遍历函数与取词函数 |

约定与取舍：数值键 `0` = 未设置（`Option` + TOML `null` 会让每个消费方都处理 `None`，且这些闸的存在意义就是止损，"完全不限"不可表达）。取值一律走 `effective_*()` 访问器，`0` 的解析只发生在配置层。工具与检索 crate **不 import 配置节类型**，转换在 `xgent_app` 完成，`Tool` / `ContextProvider` 契约与配置层解耦。

未转配置的常量（本就有意的边界）：`MAX_TOOL_OUTPUT_BYTES`（所有读取类工具共享的天花板，`run_command` 的配置值只能在其下收窄）、`IDLE_TIMEOUT_SECS`（60s，台账未点名）、降级遍历的 `VISITED_NODE_LIMIT`（2000，纯止损闸，不属于按工程调优的参数）。

### 实现期间测出并修好的真实断裂：确认超时形同虚设

`[tools] confirm_timeout_secs` 的行为测试（永不回应的确认回调）直接挂死，暴露出改前的等待结构：

```rust
r = tokio::time::timeout(Duration::from_secs(300), confirm.confirm(req))
   => match r { Ok(rx) => match rx.await { ... } }   // ← rx.await 不在超时之内
```

生产实现 `BridgeConfirm::confirm` **立即**返回 receiver（用户的点击发生在之后），所以 `timeout` 包住的只是「取回 receiver」这一步，永不超时；真正的决策等待 `rx.await` 无界——确认请求除了 Abort 没有任何出口，`确认请求超时` 这条错误分支在生产路径上不可达。这与 §五 那批「类型改到位但链路没接上」的缺陷同类：只有让测试真的等一次才暴露。

修法与配套：

- `executor.rs`：把超时改为覆盖「取回 receiver + 等到决策」两段，`biased` 让取消仍优先判定（`abort_beats_confirm_timeout` 用例锁住这一点，R1-2 的取消语义不能因为加超时而退化）。
- `xgent_ui/src/confirm_dialog.rs::close_on_settled`：超时后 executor 返回的是错误工具结果，弹窗不会自己消失——它是模态遮罩，留着会继续吃键盘焦点，且「允许」按钮接在一个已被丢弃的 receiver 上（点了没反应）。新增一条以 `ToolResultMessage` 到达为判据的关闭路径，并复用既有 `close_dialog` 还原焦点（内层 tool-call 循环串行，等确认期间不会有别的工具结果进来，故判据成立）。
- 行为变化：用户 5 分钟不响应确认，现在会得到 `确认请求超时` 而不是对话一直挂在确认态。这是 R3-17 台账与 `AGENTS.md` §5.4 一直在描述的行为，此前并未真实发生。

### 一处对 R3-21 的订正（不是本轮范围，但影响下一条变更的估价）

`doc/notes/agent-gap-review.md` R3-21 记「查找替换仅字面匹配，无正则」。实际读码结果更强：`xui::FindState`（`crates/xui/src/text_editor/find.rs`，含 `find_all` / `replace_current` / `replace_all` 与十余条单测）在 `crates/xgent_ui/src/` 里**零消费者**（全目录检索 `find`/`Find`/`replace` 只命中 `tool_panel`、`editor/tabs::find_by_path` 等无关符号）。也就是说编辑器查找替换不是「只支持字面量」，而是产品里根本不可达。下一条变更应按「接 Cmd+F/Cmd+H UI + 正则模式」估工作量。

### 判别力反证（13 个变异体）

方法同 §五/§六：逐个把修复改回缺陷形态 → 跑目标 crate 的 lib 测试 → 确认确有失败 → 还原并 `touch` 源文件（不 touch 会让 cargo 复用变异体的缓存二进制，见 §六 末尾的工具陷阱）。

| 变异 | 结果 | 失败用例 |
|:---|:---|:---|
| MU1 `regex` 开关失效（永远走字面量） | 抓到（3 failed） | `regex_mode_matches_by_pattern`、`lookaround_and_backreference_are_invalid_patterns` 等 |
| MU2 非法正则静默退回字面量 | 抓到（2 failed） | `invalid_regex_is_reported_before_traversal`、`lookaround_and_backreference_are_invalid_patterns` |
| MU3 字面模式丢弃大小写折叠 | **首轮未抓到 → 是我的测试失效** | 见下 |
| MU4 glob 的「无 `/` 只匹配文件名」规则关闭 | 抓到（3 failed） | `glob::tests::bare_extension_glob_matches_name_at_any_depth`、`glob_extension_filter_narrows_files` 等 |
| MU5 未知 `file_type` 静默不过滤 | 抓到（1 failed） | `unknown_file_type_lists_supported_names` |
| MU6 `run_command` 超时回到常量 60 | 抓到（1 failed） | `configured_command_timeout_kills_slow_command` |
| MU7 确认超时回到常量 300 | 抓到（120s 内不结束 = TIMEOUT） | `configured_confirm_timeout_expires`（旧形态下直接挂死，正是 §七 那条断裂的表现） |
| MU8 窗口解析忽略按模型映射 | 抓到（1 failed） | `context_window_precedence_is_model_then_provider_then_fallback` |
| MU9 缺省值漂移（`max_keywords` 16→8） | 抓到（2 failed） | `new_sections_default_to_previous_constants`、`toml_without_new_sections_resolves_to_builtin_defaults` |
| MU10 `build_provider` 不交 `max_output_tokens` | 抓到（1 failed） | `build_provider_passes_new_config_keys`（源码级断言） |
| MU11 检索预算回到字面量 8_000 | 抓到（1 failed） | `retrieval_budget_from_config_reaches_context_query` |
| MU12 每帧上限回到字面量 64 | 抓到（1 failed） | `drain_bound_is_read_from_bridge_not_a_literal`（源码级断言） |
| MU13 目录树深度回到字面量 4 | 抓到（1 failed） | `configured_tree_depth_limit_shortens_summary` |
| MU14 关键词上限回到字面量 16 | 抓到（2 failed） | `configured_keyword_cap_keeps_first_in_order`、`retrieval_limits_are_actually_read` |

### MU3 未抓到暴露的是我自己的缺陷（本轮最有用的一条）

首轮 MU3 报「108 passed，一个都没失败」。按 §六 的 M7 经验，我先怀疑是等价变异——但大小写折叠不可能等价。真实原因是我在插入相邻测试时把一次编辑的换行吃掉了：

```rust
/// A3：忽略大小写对字面与正则两种模式都生效，缺省仍区分大小写。    #[tokio::test]
async fn case_insensitive_applies_to_both_modes() {
```

`#[tokio::test]` 被并进文档注释行，于是这个用例**从未注册进测试二进制**：源码里 21 个 `#[test]/#[tokio::test]`，二进制只有 20 个（总数 108 而非 109）。编译器不报错，只给一条 `dead_code` 警告——而我几次 `cargo check` 都把输出过滤过，警告没进过眼睛。「全绿 + 108 passed」看上去完全自洽。

补回换行后，同一变异立刻失败（`left: 1, right: 3`）。两条可复用的判据：

- **`cargo test -- <filter>` 报 `0 passed; N filtered out` 必须当作失败**，而不是"通过"。反证脚本原先按过滤器跑单个用例，一旦名字没写对就是空跑，`exit=0` 看起来完全正常。改为整 crate 跑 lib 测试后这类哑火无处藏身。
- **变异批次里"没被抓到"的默认解释不是等价变异，而是先怀疑测试自己失效**：核对源码声明数与 `-- --list` 注册数是否一致，再判等价性。§六 的 M7 是真等价（独立程序对比产出一致），M3 是假阴性（用例根本没运行），两者的处置完全相反——前者记录说明，后者修测试。

### 本轮的验证结果

见 `docs/comet/changes/close-search-config-gaps/verification.md`（Runtime 正式报告）。开发期检查与未跑的检查按本机实测说明：`cargo check --workspace --all-targets`、五个受影响 crate 的 `cargo test --lib`、`cargo fmt -p <crate> -- --check` 为形式化检查；`cargo clippy --workspace` 与 `cargo test --workspace` 未纳入形式化计划——本机磁盘剩 18G、swap 近满，任何链接整个 bevy 图的命令都要 10 分钟以上（本轮 `cargo check --workspace --all-targets` 冷路径实测 9m43s，`cargo test` 单 crate 编译 13m48s），超出命令上限。

### 遗留与下一步

- 设置面板仍只暴露 provider 三项（R3-13）：本轮新增 14 个键后，"只能手改 TOML"的面子更大了，`[tools]`/`[agent]`/`[context]` 与 `context_window` 是最该进 UI 的几项。
- 编辑器查找替换（R3-21）按 §七 订正后的实际状态估：先接 UI，再谈正则。
- `@file:<path>` 越界读取（§六 遗留）未动，仍属独立变更。
- `IDLE_TIMEOUT_SECS`（60s）、降级遍历 `VISITED_NODE_LIMIT`（2000）、共享的 `MAX_TOOL_OUTPUT_BYTES`（64KB）保持常量：台账未点名，且前两者是纯止损闸而非按工程调优的参数。
