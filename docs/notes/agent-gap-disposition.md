# 缺口处置台账

> 本文件记录 `docs/notes/agent-gap-review.md` 中每条缺口的处置结果。
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
| R2-3 F-04 文档矛盾 | `docs/design/requirements.md` F-04 | 措辞订正为与已交付行为一致（默认全部需确认），未改代码 |
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

`docs/notes/agent-gap-review.md` 继续作为 R3/R4 的追踪台账，本文件不重复其细节。

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

### 判别力反证（13 个变异体）

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
| `docs/dev-tutorial.md` 引用的 Spec 路径在归档前是悬空链接 | 归档发布后才生效，属正常时序，不是缺陷 |

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

`docs/notes/agent-gap-review.md` R3-21 记「查找替换仅字面匹配，无正则」。实际读码结果更强：`xui::FindState`（`crates/xui/src/text_editor/find.rs`，含 `find_all` / `replace_current` / `replace_all` 与十余条单测）在 `crates/xgent_ui/src/` 里**零消费者**（全目录检索 `find`/`Find`/`replace` 只命中 `tool_panel`、`editor/tabs::find_by_path` 等无关符号）。也就是说编辑器查找替换不是「只支持字面量」，而是产品里根本不可达。下一条变更应按「接 Cmd+F/Cmd+H UI + 正则模式」估工作量。

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

## 八、add-file-checkpoint-undo（2026-10-15）：R3-11 agent 文件改动的撤销

承接 §七「遗留与下一步」之外台账里最重的一条：R3-11「没有任何机制能撤销 agent 改过的文件」。
正式 Spec 见 `docs/comet/specs/file-checkpoints/spec.md`（归档时发布）。

### 机制

| 落点 | 说明 |
|:---|:---|
| `crates/xgent_tools/src/checkpoint.rs`（新增） | `CheckpointStore`：`begin_turn` / `snapshot_before_write` / `finalize_after_write` / `undo_last_turn`。磁盘布局 `turns/<session>/<turn>.json` 清单 + `blobs/<session>/<turn>-<index>-<hash>.bin` 改前内容。保留 20 轮（`DEFAULT_MAX_TURNS`） |
| `tool.rs`：`Tool::write_targets`（默认空） | 一次调用会写哪些**已解析的绝对路径**。`write_file`/`edit_file` 用它们本就有的 `resolve_in_project` 实现，读取类工具保持空 → 项目外路径不可能成为撤销目标 |
| `executor.rs`：`run_tool` | 三条执行路径（自动批准 / 本次允许 / 会话允许）唯一的收口点：写前快照 → 执行 → **仅在结果非 `is_error` 时**回填校验值 |
| `atomic.rs` | 新增 `atomic_write_bytes`（同目录 tmp + `sync_all` + rename，撤销恢复复用它）；`backup_existing` 与其两个调用点删除 |
| `xgent_agent/bridge.rs` | `AgentBridgeConfig.checkpoints` → `AgentBridge.checkpoints`；`AgentCommand::StartLoop` 携带 `session: u64`（数值 `SessionId`，不走 `session:N` 反解——既有 `restore()` 会把它解析成 0）；`run_agent_loop` 每轮外层循环开始处 `begin_turn` |
| `xgent_ui/src/undo.rs`（新增）+ 命令面板 | `agent.undo-changes` 经 `try_register` 注册、`handle_palette_triggers` 在自己的分支里路由（不走 `plugin.` 兜底）。Idle 才放行；结果一行 toast，有跳过/无检查点/写盘失败时再落一条常驻提示 |
| i18n | 两套语料各 16 个键（`cmd-undo-changes`、`undo-*`、`undo-skip-*`） |

### 三条设计取舍（都写进 Spec，不留实现细节给人猜）

- **校验靠内容哈希，不靠 mtime**（FNV-1a 64，只判「还是不是我写的那份」，非安全摘要）。`touch`、`git checkout` 会让 mtime 判据假阴性，等长改写会让它假阳性。
- **记不上快照时降级方向是「丢撤销覆盖」而不是「丢写入」**：检查点目录不可写时文件照写、工具结果照成功，该轮标为 uncovered 并在撤销汇报里点名。撤销机制不能反过来弄坏主功能。
- **撤销只交给用户，不进 agent 工具表**：模型不能自己回滚自己的工作；且撤销只改磁盘，缓冲区一致性沿用既有 fs-watch 通路（干净 buffer 静默重载、脏 buffer 进冲突对话框），不开第二条重载路径。

### 实现期间自己测出的三个真实断裂

1. **blob 名在同一轮内互相覆盖**。名字原本只有「轮号 + 路径哈希」，同一轮第三次写入的快照盖掉了第一、第二次的清单条目仍指向同一个 blob → 撤销只回到 `c2`。补上条目序号（`<turn>-<index>-<hash>.bin`）后 `three_writes_in_a_single_turn_undo_to_pre_turn_content` 才通过。
2. **保留淘汰差一**。淘汰写在 `begin_turn` 里，第 21 轮开始时看到的还是 20 份清单 → 判定「不超」而全部留下，实际留了 21 轮。改挂到「本轮首次创建清单」时（`record_entry`）。
3. **FTL 注释符写错**。用 `;` 起注释让 `en-US/main.ftl` 整份语料解析失败，12 个 `xgent_settings` 测试同时炸。fluent 只认 `#`。

三者都不是「看代码看出来的」，是行为测试跑出来的——第 1 条尤其：单元级快照/恢复测试各自都过，只有「同一文件写三次」这条序列暴露了名字冲突。

### 判别力反证（12 个变异体）

| 变异 | 结果 | 失败用例 |
|:---|:---|:---|
| M1 `snapshot_before_write` 直接返回 | 抓到（14 failed） | 快照与撤销全链 |
| M2 blob 名去掉条目序号（轮内互相覆盖） | 抓到（1 failed） | `three_writes_in_a_single_turn_undo_to_pre_turn_content` |
| M3 `Created` 条目不删文件 | 抓到（2 failed） | `created_file_is_removed_on_undo` 等 |
| M4 用户删掉的文件被重建 | 抓到（1 failed） | `user_deleted_file_is_not_resurrected` |
| M5 撤销不比对写后哈希 | 抓到 | `externally_modified_file_is_skipped_and_others_still_restore` |
| M6 uncovered 不落账 | 抓到 | `unwritable_checkpoint_dir_degrades_without_blocking_the_write` |
| M7 保留窗口差一 | 抓到 | `retention_keeps_only_the_newest_turns` |
| M8 轮号不从磁盘探测 | **首轮未抓到 → 真盲区** | 见下 |
| M9 `write_file` 不声明写入目标 | **首轮未抓到 → 真盲区** | 见下 |
| M10 Idle 闸消失（忙时也能撤销） | 抓到 | `gate_refuses_while_busy_and_without_store` |
| M11 `en-US` 少一个键 | **首轮批次接错线**（跑的是 `xgent_settings --lib`，而该断言住在 `xgent_ui --lib`）→ 改跑对 crate 后抓到 | `both_locales_define_every_new_key` |
| M12 执行器不调快照 | 抓到 | `executor_driven_write_file_is_undone`、`write_tools_record_checkpoints_instead_of_backup_files` |

M8/M9 的处置与 §七 MU3 不同：这次不是测试失效（注册数与声明数一致、用例确实跑了），而是**确实没有一条断言覆盖那两处**。补的三条用例分别把「轮号从磁盘续接」「空轮次不占轮号」「write_file 的目标声明与端到端撤销」钉住，重跑后 M8 由 `empty_turn_does_not_reuse_turn_number` 失败、M9 由 `executor_driven_write_file_is_undone` + `write_file_declares_its_resolved_target` 失败，12/12 全部被抓。

判据（可复用）：**「没被抓到」有两种成因——测试自己失效，或覆盖真的有洞。先用「源码声明数 vs `--list` 注册数」排除前者，再用「这条断言到底在读哪个字段」排查后者**。上一轮是前者，这一轮是后者，两种都必须补东西，不能一句「等价变异」了事。


批次本身也出了两种错，都被这轮的多写序列暴露：M11 按「键属于哪个 crate」而不是「断言在哪个 crate」选了测试目标（`xgent_settings --lib` 14 条全过看起来完全正常）；`NO-TEST-RAN` 哨兵用子串 `"0 passed"` 判「用例没跑」，于是 **`60 passed` 也被它命中**（`6` 后面正好接 `0 passed`），把真正失败的 M10 标成了可疑。两处都已修：变异体的测试目标按断言位置选，哨兵改用正则取那份结果的**完整数字**并判 `> 0`。

教训不是「脚本有个 bug」，而是**止损判据本身也要有判别力**：一条靠子串碰出来的哑火哨兵，既会误报也会漏报——它这轮误报了真失败的 M10；反过来 `600 passed` 这类数字它同样命中。数字判定必须锚定边界，不能用「看起来像」。

### 独立验证者回查后订正的三条自评证据（本轮最该记住的一段）

只读验证者（不是我）逐字读测试体与文案后，判下 11/11，但**否决了我三条自评证据的措辞**——三条都在验收之后被补成事实，而不是被辩解掉：

1. **「`read_only_tools_declare_no_write_targets` 钉住 read_file / search_files / run_command」是假的**：该用例只断言了前两个。现已补 `RunCommand::default()` 的断言。反证 M15：把 `Tool::write_targets` 的 trait 默认从空改成返回项目根，这条用例立刻失败。
2. **「undo 插件被集成测试 harness 构建的同一个 app 装配」是假的**：`crates/xgent_ui/tests/` 里没有任何 harness 实例化 `UndoPlugin`，`request_undo` / `collect_undo_results` 只有纯函数部分（`gate`、文案构造）被单测覆盖，两条系统本体只在真实 GUI 运行里才跑。报告已按「代码级 + 系统本体未 headless 验证」降级。
3. **「提示文案与 Spec 都说明了覆盖边界」的文案那半是假的**：撤销详情里从未出现「命令与插件写入不在范围内」，只有 Spec 写了。现补 `undo-coverage-note` 键（两套语料），`detail()` 只要产出常驻提示就带上这句。反证 M13：删掉这句，`skipped_files_are_listed_with_reasons` 失败。

顺带把**标点交回语料**：新增 `undo-part-with-list` / `undo-skip-item` / `undo-list-sep` 三个键接管冒号、括号与列表分隔符。改前 Rust 侧硬编码 `（）`/`：`/`、`，英文语料渲染出来是 `c.rs（changed externally）`——NF-05 说的「用户可见字符串走 i18n」是含标点的。反证 M14：把 en-US 的分隔符换成顿号，`both_locales_define_every_new_key` 与 `english_detail_uses_its_own_punctuation` 双双失败。

同一批清理：删掉无人消费的 `pub const BACKUP_SUFFIX`（备份机制已移除，「留着识别历史残留」是给它自己听的借口，没有任何代码读它）、`undo_turn()` 的冗余二次赋值、`try_register(...).ok()` 改成 `tracing::warn!`（id 冲突意味着命令根本没进面板，静默吞掉只会让人以为功能不存在）。

判据：**验收证据里写测试名，不等于那条测试覆盖了你描述的范围。名字不是身体**——写证据时逐个读回测试体，把它真正断言的字段抄下来。

### 顺带发现的既有缺陷（不在本轮范围）

`EditorIoRuntime.handle` 在 `xgent_app` 里**从未被注入**（全目录只有构造点的 `None`），编辑器文件 IO 因此一直走同步降级路径。撤销刻意依赖 `AgentBridge` 自带的 runtime，不碰这个资源；编辑器侧的注入应作为独立变更处理。

### 遗留与下一步

- 只覆盖 `write_file`/`edit_file`。`run_command` 的副作用（`sed -i`、`git apply`、`cargo fmt`）与插件 `host.write_file` 不可撤销。Spec 从一开始就写了这条边界，**用户可见文案是验收之后才补上的**（见上节第 3 条）；现在撤销详情会一并说明，但仍不暗示「agent 做过的事都能退回」。
- 无 redo、无撤销历史 UI、无整会话回滚（本轮明确的 non-goal）。
- 保留 20 轮是常量，检查点不可关（应用总是挂上存储），也没有对应设置面板项（与 R3-13 同一个缺口）。
- 历史遗留的 `*.xgent-bak` 不清理；应用仍不确保用户项目把 `.xgent/` 加进 gitignore（既有缺口，记账待办）。
- A9 的「撤销后编辑器缓冲区一致」是读既有通路验证的，不是 GUI 跑出来的——本应用无法 headless 驱动，报告里如实这么写。

## 九、add-parallel-tool-execution（2026-10-15）：R3-15 工具并行执行

`docs/notes/agent-gap-review.md` R3-15 记的是 `unwired-design`：`Tool::concurrency()` 从 ADR-0007 起就有 `Shared`/`Exclusive` 两值，**全仓零读取方**，agent 内层 tool-call 循环把 `outcome.tool_calls` 逐个 `await` 完。本轮让这个声明变成真判据。能力规格见 `docs/comet/specs/tool-concurrency/spec.md`。

### 机制

| 落点 | 说明 |
|:---|:---|
| `xgent_tools/src/executor.rs`：`parallel_safe(tool_id, input, ctx)` | 并行资格需**三条互相独立**的信号全过：① 解析出的策略不需要用户确认（`Approved`，或 `NeedsConfirmation` 但工具已进会话 AllowAll 集合）；② 工具自报 `Concurrency::Shared`；③ `tier()` **恰好**是 `Read`（`UiOnly` 被排除，见下）。判定只是建议——策略、弹窗、检查点仍只在 `execute` 里发生 |
| `xgent_agent/src/bridge.rs`：`execute_tool_call` + 分组调度 | 从当前下标取**连续**可并行区间（长度钳到 `cap`）；区间为空即屏障，单独跑完再继续。同组用 `futures::join_all` 在**当前任务里**轮询，不 `spawn`——loop 被取消或提前返回时不会留下后台任务继续写文件 |
| 顺序与配对 | `ToolResult` 事件与回灌 `req.messages` 一律按模型给出的**原始下标**发（先跑完的不抢）；`ToolCall` 事件仍在执行前全部发完；每个 `call_id` 恰好一个 result——同组已启动成员各有真实结果，因中断**根本没启动**的分组补占位 |
| `xgent_settings_core` + `xgent_app` | `[agent] max_parallel_tools`，缺省 `4`，`0` = 未设置 → 落回 4（**不是不限**：这闸是挡子进程/socket/fd 数量的）。经 `AgentBridgeConfig` 进 `run_agent_loop`，工具 crate 不 import 配置类型 |

### 四条读码事实决定了判据的形状

1. `SharedConfirm` 是**单槽**（`bridge.rs:170`，持 `Mutex<Option<oneshot::Sender>>`）。两个待确认调用并发时，第二个会覆盖第一个的 sender，被覆盖那次只能看到 channel closed 并报「确认被取消」——用户的一次决策就此丢失。所以①不是优化，是必须。
2. `Tool::concurrency()` 的 **trait 默认值就是 `Shared`**（`tool.rs:184`）。只看②等于让"忘了声明 `Exclusive` 的写工具"自动进并发组，两个写同一路径会打断检查点清单的 read-modify-write（"一文件一条目、倒序回放"直接破）。所以③不是冗余，是第二道独立声明。
3. `resolve_policy` 对 `UiOnly` **无条件放行**（不经确认），而 `EditorTool` 是 `UiOnly + Shared` 且已发货。所以「默认配置什么都不并行」这条不变量**不能**只靠策略信号成立——必须靠 `tier()==Read` 把 `UiOnly` 挡在外面，否则 `open_file → goto` 这类有顺序语义的编辑器命令会在用户没配任何东西时就并发。这条是独立验证者抓出来的（见下文「验证者抓到的一条真实缺陷」），第一版规格写的是 `Read` 或 `UiOnly`。
4. 插件调用在插件自己的 task 里本就**串行**（`wasm_host.rs:231` 逐条 `recv().await` 再 `call().await`），所以两个插件调用重叠无害但也无收益；`store.data_mut().cancel_token/push_update` 因此不会被并发写坏。本轮不改插件 host。

**默认配置下这条能力是惰性的**：多数工具默认 `NeedsConfirmation`（`AGENTS.md` §5.4）已被策略信号挡下；`UiOnly` 由策略自动放行，这一半靠 `tier()==Read` 才挡住。于是不改配置就与串行时代逐字相同，用户把只读工具提进 `approved` 或点了 AllowAll，并发才出现。

### 测试判据：用「在跑数 + 有界等待」，不要用耗时

`bridge_tests.rs` 的 `ProbeTool` 进入即在跑数 +1 并记历史最大，然后**有界等待**"至少看到 N 个同时在跑"（400ms 超时），再停留 `holds_ms`（期间 race cancel token）。串行调度下等待必然超时、观测值停在 1——所以"调度器退回串行"会被直接抓到。总耗时判据既 flake，又能被"串行但很快"骗过，明确不用。

### 判别力反证（11 个变异体）

| 变异 | 结果 | 失败用例 |
|:---|:---|:---|
| AP1 调度器退回串行（`cap = 1`） | 抓到（4 failed） | overlap、order、cap=2 等 |
| AP2 忽略上限（`end - idx < cap` → `true`） | 抓到（3 failed） | `cap_two_bounds_the_group`、`cap_one_serialises…` |
| AP3 结果配对顺序反转（`.iter().rev()`） | 抓到（1 failed） | `results_keep_model_order_even_when_completion_differs` |
| AP4 不启动分组的占位补齐被删 | 抓到（1 failed） | `abort_before_later_groups_places_placeholders_for_them` |
| AP5 删掉 `concurrency` 判据 | **首轮批次接错线** → 改跑对 crate 后抓到 | `exclusive_call_is_a_barrier_for_its_neighbours` |
| AP6 删掉 tier 判据 | 抓到（1 failed） | `parallel_safe_requires_all_three_signals`（`Write`+`Shared` 那例） |
| AP7 `NeedsConfirmation` 当成可并行 | 抓到（2 failed） | `confirmations_are_asked_one_at_a_time_and_never_overlap` 等 |
| AP8 AllowAll 不算数 | 抓到（1 failed） | `parallel_safe_flips_after_allow_all` |
| AP9 `parallel_safe` 恒 false | 抓到（2 failed） | overlap 与 order 两条 |
| AP10 `0` 解释成不限 | 抓到（3 failed） | `max_parallel_tools_zero_means_default_not_unlimited` |
| AP11 缺省值漂移 4→8 | 抓到（3 failed） | `new_sections_default_to_previous_constants` 等 serde 数据断言 |
| AP12/13 把资格判据退回 `Read \| UiOnly` | 双双抓到 | AP12 → `bridge_tests::default_config_parallelises_nothing_even_for_auto_approved_ui_only`；AP13 → `executor::parallel_safe_requires_all_three_signals` 的 UiOnly 那例 |

**AP5 是上一轮 M11 的同类错误重犯**：变异体改的是 `xgent_tools` 的 `parallel_safe`，批次就跑了 `xgent_tools --lib`（129 全绿，看起来"没抓到"）；但隔离 `concurrency` 判据的用例住在 `xgent_agent` 的 `exclusive_call_is_a_barrier_for_its_neighbours`。上一轮已经写过"按断言所在 crate 选测试目标"的判据，这轮仍然先按文件所属 crate 选了一遍——说明判据要落到脚本接口里才算数：本轮给批次加了 `TEST_CRATE` 覆盖表，而不是靠我记得。

**A2 的探针 tier 是刻意选的**：中间那个独占探针用 `Read` + `Exclusive`（"读一个有状态会话"那种必须独占的只读工具）。若给它 `Exec` 层，tier 判据会替它挡下并发，于是删掉 `concurrency` 判据也照样全绿——两条判据各有自己的判别用例，才谈得上"三条独立信号"。这条是写 AP5 时才想清楚的。

### 独立验证者抓到的一条真实缺陷（资格判据因此收紧）

候选 2 的验证判 **9/9 通过**，但它的 risks 里带出一条足以推翻已声明不变量的事实错误：规格、dev-tutorial 与本台账当时都写着「默认配置下没有任何调用会并行，因为所有工具都是 `NeedsConfirmation`」。**前提就是错的**——`resolve_policy` 对 `UiOnly` 层无条件放行，而已发货的 `EditorTool` 正是 `UiOnly + Shared`。也就是说 `editor.*` 调用在用户什么都没配的默认配置下**已经在成组并发**，而我为了「默认惰性」这条约束写下的判据 `tier ∈ {Read, UiOnly}` 恰好把它放进来了；更糟的是这条并发路径的正确性押在「`EditorTool::execute` 恰好在 emit 之前不 await」这种实现偶性上，而 `open_file → goto` 是有顺序语义的。

处置不是把它记成 known limit 然后归档，而是 `--revise-implementation` 回 Build 修：

- 资格判据第③条由 `Read | UiOnly` 收紧为**恰好** `Read`（`executor.rs`），顺序性由判据保证而不是由某个实现碰巧不阻塞保证；
- 补两条判别：`executor` 侧新增 `UiOnlySharedTool`（形状等同已发货的 editor 工具）断言不并行，即便策略自动放行；`bridge_tests::default_config_parallelises_nothing_even_for_auto_approved_ui_only` 断言默认配置下 `UiOnly` 探针并发度 1 且 `confirm_requests == 0`（后者把它与 A3 那条待确认用例区分开——A6 从此有了自己的用例，不再借 A3 代打）；
- 规格/文档里那句错前提逐处改成准确表述：多数工具靠策略信号挡下，`UiOnly` 靠 tier 信号挡下，两条各管一半。

判据：**验证通过不等于文档为真。** 九条验收项全过，是因为代码满足了我写下的判据；而那份判据的**理由**里有一句与源码不符——只有独立读者会去核那句话。风险清单不是免责清单：写「默认配置惰性」时我应该自己去读 `resolve_policy` 对 `UiOnly` 的分支，而不是引用需求文档的概括。

### 顺带发现的既有偏差（不在本轮范围）

`crates/xgent_tools/Cargo.toml:18` 依赖 `bevy`，而 `AGENTS.md` §4 的 crate 表把 `xgent_tools` 标为「不依赖 Bevy」。读码结论：整个 crate 只有 `executor.rs:307` 一处用到 bevy——`#[derive(bevy::prelude::Resource, Clone)] pub struct ToolExecutorResource`。也就是说这条依赖只为一个 Resource 派生宏存在，代价是工具 crate 把整个 bevy 图拖进任何想复用它的地方（也拖慢本台账反复提到的 per-crate 编译）。要么把包装类型上移到 `xgent_agent`，要么订正 `AGENTS.md` §4 的表述——两者都是独立变更，本轮只记录不改动（本轮 brief 里"no new dependency"这条自查未违反，但"Bevy-free"那句概括与事实不符，一并订正在此）。

### 形式化检查替我抓到的一处本地漏项

第一候选被 Runtime 的 **c8（`cargo fmt -p <crate> -- --check`）退回 Build**：最后一轮测试编辑后我只格式化了 `xgent_agent`/`xgent_tools`，漏了 `xgent_settings_core`——新加的 `max_parallel_tools_zero_means_default_not_unlimited` 里那条 assert 超长未换行。其余 7 项（agent 68 / tools 129 / settings_core 44 / 邻近基线 / `check --workspace --all-targets` / clippy）当时已经全绿。

这条的价值不在"格式错了改回来"，而在**Handoff 里我先把 fmt 干净写成了证据，之后才真去扫全 8 个 crate**——上一节刚立的判据（证据要指到实际跑过的命令）我自己第一遍也没守住。固定做法：`cargo fmt -p <crate>` 必须按**本轮所有被改 crate 的清单**逐个跑，改完任何一行测试后重跑，不能凭"我记得格式化过了"。

### 边界（诚实记录）

- 只合并**连续**可并行的调用，不重排凑并行：模型先写的写后面跟着四个读，那四个读要等写完才成组。
- 结果按组可见（组完成才报告），因此同组里先跑完的要等前面的；`ToolProgress` 仍可能乱序交错，那是进度不是结果。
- 同组在同一个任务里轮询：将来若有工具写出不带 await 点的 CPU 死循环，会卡住整组。现有工具都是 await IO，唯一偏 CPU 的 `run_command` 是 `Exclusive`。测试抓不到这种未来违规。
- 无跨轮、跨对话的全局限流；`max_parallel_tools` 只管一轮里一个组，也没有设置面板项（仍属 R3-13）。
- 没有对真实 provider 做耗时基准：本轮证明的是"调度性质"（并发确实在发生），不是"快了多少"。
- A6 没有独立用例：默认策略下"什么都不批"与 A3 是同一次运行，写成两条只会虚增覆盖数，故如实记为共用。


## 十、close-settings-gaps（2026-10-16）：R3-13 设置面板覆盖配置

§七/§八/§九 的「遗留与下一步」连着三次点名同一条：设置面板只暴露 provider 三项，而这几轮新增的键（`max_parallel_tools`、检查点保留、搜索与检索限额、14 个运行参数）全都只能手改 TOML。正式能力规格见 `docs/comet/specs/settings-panel/spec.md`（归档时发布）。

`docs/notes/agent-gap-review.md` R3-13 的原文比实际轻：它写「`ToolPolicyConfig.approved/denied`、重试参数、语言、主题、插件设置均无 UI」。读码发现三件事更糟：

- **面板是只写不读**：`spawn_panel` 从不预填，重开就是四个空框；`handle_save_button` 在 `provider_id` 为空时**照样把 `open` 置 false**（`settings_panel.rs:509-518`）——用户点了保存、面板关了、什么都没写。
- **项目作用域在 IPC 层根本写不进去**：`config_read`/`config_write` 对 `scope=project` 直接回「未实现」（`session.rs:206-215,237-246`），而 `[tool_policy]`/`[agent]`/`[tools]`/`[context]` 全在项目配置里。
- **全局白名单不含那几个 provider 限额**：写 `providers.<id>.context_window` 会得到 `未知配置 key`（`config_store.rs:131-145`）。

所以「可达」要同时补四层：协议 → daemon → 面板 → 运行时。

### 机制

| 落点 | 说明 |
|:---|:---|
| `xgent_core/src/config.rs` | `ConfigReadRequest`/`ConfigWriteRequest`/`ConfigChanged` 增 `project_root: Option<String>`，一律 `#[serde(default)]`——新增协议字段不带 default 会让旧客户端整条请求反序列化失败（`legacy_wire_without_project_root_still_parses` 钉这条） |
| `xgent_daemon/src/config_store.rs` | `ConfigCoordinator` 加 `projects: HashMap<PathBuf, ProjectConfig>`（首次访问读盘、`project_root` 为空时补上请求带来的根）；`read_project`/`write_project` 支持 `project`/四段只读 + 13 个可写键；写走「改副本 → 落盘 → 换入内存」，落盘失败内存不动 |
| `xgent_daemon/src/session.rs` | `scope=project` 真实现：缺根或根不是已存在的目录都是 `INVALID_PARAMS`（不静默降级 global）；通知按 `broadcast_to_project` 路由；全局白名单补 `context_window`/`max_output_tokens`/`first_event_timeout_secs`，`model_context_windows` 明确只读 |
| `xgent_ui/src/settings_panel.rs` | 六组：provider（含限额）/工具三态策略/agent/tools/context/语言；字段表 `project_num_fields`/`provider_num_fields` 的缺省值一律取 `xgent_settings_core::defaults`；保存只发脏键并带逐键报告 |
| `xgent_tools` / `xgent_context` / `xgent_agent` | 四组共享句柄：`ToolCtx.tool_policy`、`ToolExecutor.params`（`RunCommand` 同 `Arc`）、`OnDemandContextProvider.limits`、`AgentBridge.run_settings`（`RunSettings{retrieval_max_tokens, max_events_per_frame, max_parallel_tools, context_window}`，存**生效值**） |
| `xgent_app/src/settings_io.rs` | `LiveSettings` 持四组句柄；`apply_snapshot(project, global, provider_id, model)` 是**唯一**一条把权威配置落进运行时的通路，本窗口写入与对端广播共用；`GlobalConfigRes`/`ProjectConfigRes` 终于有了消费者（后者此前从未被 `insert_resource`） |
| i18n | 本轮新增的 43 个 `settings-*` 键两套语料都有；`settings-panel-i18n` 测试**从源码扫出用到的键**再逐个查两套语料，而不是维护一张手写清单 |

### 五条设计取舍（都写进 Spec）

- **项目配置也经 daemon 写**（不是 UI 直写磁盘）。daemon 是唯一写入方，也是唯一能把 `config.changed` 广播给对端的地方；两个窗口直写同一份 `.xgent/config.toml` 只会互相盖掉。协议里 `ConfigScope::Project` 本来就留着，这是填已声明的洞而不是新开通道。
- **项目根走请求参数，不从 `fs.watch` 订阅推断**。一个客户端可以订多个根，猜「第一个」会写到用户没在看的项目。
- **工具策略用三态行，不用两张字符串列表**。`resolve_policy` 里 `denied` 优先于 `approved`；两张列表会让用户写出一个「第二条盖掉第一条」的组合而显示与判定相反。行的状态由 `ToolExecutor::resolved_policy` 给出——显示与执行同一条判据，显示错了比不显示更糟。`UiOnly` 因此**如实显示「自动执行」**（§九 独立验证者抓出的正是这条前提）。
- **不写覆盖时本来就自动执行的工具，不进 `approved` 列表**（`policy_lists`）。手改 TOML 的人看到的应当就是被真正改变的东西。
- **生效边界按消费点划**，做不到热应用的键干脆不给控件：主题只有一套预设（R3-14）、`PluginHost::update_plugin_settings` 无调用方、`context_strategy` 的 B/C/D/E 还是空壳、`model_override` 全仓零读取方。给不可达的键做开关就是在演一个假的设置项。

### 实现期间自己测出的六个真实断裂

1. **显示值被当作用户输入写回**（本轮最有用的一条）。未设置的数值字段要显示内置缺省（`4`、`8000`…），于是「打开面板 → 点保存」会把这些缺省一路写进用户的配置文件——`untouched_fields_produce_no_writes` 抓到它才修的。判据：脏检查的基准必须是**存储值**，且 `stored == 0 && parsed == effective` 视为未改动。
2. **provider id 为空时挡掉整批保存**。我按「provider 字段要有 id 才写得下去」写了 early-return，结果只改工具策略也存不了——`project_only_save_works_without_provider_id` 暴露的是我自己新造的缺陷。改成：只有**真的存在全局项**且 id 为空才拒，并给出原因。
3. **daemon 测试会覆盖开发机的真实配置——而且是我这轮补隔离时又漏了一处**。`ConfigCoordinator::write` 真落盘到 `~/.xgent/agent/config.toml`。我先给 `config_store.rs` 那批测试加了隔离并实测 md5 不变，随后在 `session.rs` 新增的两条 provider 池测试却直接发全局 `config.write` 且没隔离 → 包括 Runtime 形式化检查 c4 在内的 `cargo test -p xgent_daemon` 把你的配置写成了夹具（`providers.myai / old.example.test / sk-old`），无备份、无 keychain 条目、磁盘只有系统更新快照，**不可恢复**。独立验证者用 `XGENT_AGENT_DIR=/tmp/…` 复现了这次写入，并据此否决了我「md5 一致 / 隔离已修好」的说法。现在的处置：隔离收进 `src/test_util.rs::isolate_agent_dir()`（进程级临时目录 + 串行锁，幂等、不 Drop 复原），所有全局写入测试统一调用，另加 `global_write_lands_inside_the_isolated_dir` 钉住隔离本身生效。判据：**「跑一次自检」不等于修复——自检必须针对最终候选重跑**；以及测试越界写用户目录属于损害，不是风格问题。你的配置文件我已重置为全新安装的默认空值（被污染的那份留在 `/tmp/xgent-config.test-contaminated.toml`）。
4. **读锁守卫跨 `await`**。`match resolve_policy(.., &ctx.tool_policy.read()) { … => self.allowed_all.lock().await … }` 的临时守卫活到整个 match 结束，future 因此不 `Send`，编译当场炸。写法是 `let resolved = …;` 让守卫在语句末尾释放（`parallel_safe` 与 `execute` 各一处）。
5. **headless 装配里 Bevy UI 不产出子节点坐标**。没有 `UiRoot` 时面板根 `ComputedNode` 是 `0×0`，实测所有子节点 `border_box().min` 全都相同（一起重叠在原点），picking 命中谁取决于 spawn 顺序而不是位置。旧的 `settings_input_focus.rs`（开面板→点击→敲字）正是靠这个巧合通过的：面板一变形，同一次点击就落到别的输入框上。本轮把显示类断言改成读组件（`EditableText` / `SettingsNumInput`），键盘派发测试直接设 `InputFocus`，并补 `every_settings_input_carries_tab_index` 锁住「输入框带 `TabIndex`」这条契约（变异 M29 删掉 `TabIndex(0)` 即失败）。验证者指出：直接把旧用例的点击断言删掉等于把回归网剪了——所以现在的口径是「机制 + 契约」两段自动覆盖，真机端到端点击只能人肉验，且必须这样写出来，不能含混成已覆盖。
6. **fluent 把参数包进双向隔离标记**。断 `settings-failed-item` 渲染成 `"a：b"` 失败，左边实际是 `"⁨a⁩：⁨b⁩"`（U+2068/U+2069）。i18n 测试因此按**可见文本**比对（剥掉 U+2066..U+2069），标点仍归语料。另外 `ScrollArea` 自身是含 `Node` 的 Bundle，再给它补一个 `Node` 会在 spawn 时 panic（bundle 组件重复）——面板卡片改用 `xui::scroll_area::ScrollArea::vertical()` 并只改 `area.node` 的字段。

### 判据怎么写才算真的"热应用"

四条生效通路各有一条**行为**判据，全部按"写入句柄 → 下一次消费点读到"来断言，而不是读回句柄自身（读回只证明 `RwLock` 能写）：

| 生效边界 | 判据（用例名） |
|:---|:---|
| 策略 → 下一次工具调用 | `approved_tool_stops_asking_after_policy_handle_changes`（数确认次数 1→仍为 1）、`denied_tool_is_refused_after_policy_handle_changes`（`denied` 标记 + 一次都没问） |
| 危险命令仍要问 | `approved_run_command_still_confirms_dangerous_input`（同一份 `approved` 下普通命令 0 次、`rm -rf` 1 次） |
| 显示与执行同判据 | `resolved_policy_agrees_with_execution_rules`（`UiOnly` 无配置即 `Approved`；同一 id 同时进两张表时 `Denied` 胜出） |
| 确认超时 → 下一次确认 | `confirm_timeout_written_to_handle_expires_next_request`（构造时 300s，写入 1s 后到期） |
| 命令上限 → 下一次调用 | `command_timeout_written_to_shared_handle_applies_next_call`、`output_cap_written_to_shared_handle_applies_next_call`（先跑一次不截断，写入 1KB 后下一次截断） |
| 检索限额 → 下一次检索 | `retrieval_limits_written_to_shared_handle_apply_next_retrieve`（走公开 `retrieve`，深度 4→1 后 `deep.rs` 消失）、`keyword_cap_written_to_shared_handle_applies_next_retrieve` |
| `[agent]` → 下一条消息 | `max_parallel_tools_written_to_handle_bounds_next_turn`（装配 4、写入 1 → 观测并发 1）、`retrieval_budget_written_to_handle_reaches_next_query`、`tool_policy_written_to_handle_stops_asking_next_turn` |
| 每帧上限 → 下一帧 | `frame_event_cap_written_to_handle_is_read_next_frame`（配 2 仍拼出全文本） |
| 落盘与通知 | `project_write_lands_on_disk_and_reads_back`（真读 `<root>/.xgent/config.toml`，并断 `ConfigChanged.project_root`）、`project_config_write_notifies_only_that_projects_subscribers`、`failed_project_save_leaves_memory_unchanged`（`#[cfg(unix)]` 把 `.xgent` 改只读） |
| 面板显示与脏检查 | `panel_opens_with_the_stored_values`、`reopening_the_panel_redraws_the_same_values`、`unset_field_shows_effective_default_and_says_so`、`untouched_fields_produce_no_writes`、`empty_api_key_field_does_not_wipe_the_stored_key` |

daemon 既有测试全部走 `with_config` 且不落盘，模块注释却写着「持久化」——本轮的项目作用域测试**必须真读文件**，否则"写进内存副本"就能冒充"配置已保存"。

### 边界（诚实记录）

- **GUI 指针链路只在真实窗口里成立**。headless 装配没有 `UiRoot`，面板根量不出尺寸、子节点坐标全部重叠（实测），所以显示与保存的断言一律读组件而不是"点这个位置"。`PointerPress → AcquireFocus` 那半段的回归网因此只剩 `settings_input_focus.rs` 的"焦点已设 → 敲字落进框"，真实点击三态行/滚动这条路没被自动跑过。
- **面板能改的键就是可达的全部**：`preferences.theme`（只有一套暗色预设，`preferences.theme` 至今无消费者，属 R3-14）、`plugin.enabled`/`auto_update`/`auto_install`/`plugin_settings`（`PluginHost::update_plugin_settings` 无调用方）、`context_strategy`（B/C/D/E 仍是空壳，选了就 `exit(2)`）、`provider_override`、`model_override`（全仓零读取方的死键）、`extra_headers` 与 `model_context_windows` 的表编辑，都不在本面板里。**不给会被忽略的键做开关**。
- `model_context_windows` 在 daemon 侧**只读**：按单字段写一个数字没有确定语义（写哪个模型？）。整份 provider 写入仍带着它，面板显示的是解析后的生效窗口。
- **多窗口同键仍是 last-write-wins**（本轮没做合并策略），广播只保证"显示与运行时跟上磁盘上真的存下的那份"。
- **落盘失败的键不回滚已成功的键**：每个键独立可达，面板按 daemon 的原话点名哪个键失败；不做两阶段提交。
- 项目根的校验是「非空 + 是已存在的目录」，**不要求客户端先 `fs.watch`**：启动订阅是异步的，压在订阅成功上会让"先保存后订阅完成"的窗口收到一条解释不了的失败。
- `xgent_tools` 仍依赖 bevy（只为 `ToolExecutorResource` 一个派生宏），`AGENTS.md` §4 那句「不依赖 Bevy」仍与事实不符——§九 已记账，本轮未动。
- 没做真实 provider 的往返测量：本轮证明的是"配置确实被读到"，不是"改完快了多少"。

### 遗留与下一步

- 设置面板没有搜索/分组自定义，也没有"恢复默认"按钮；键多了以后这两条会开始疼。
- `GlobalConfigRes` 是 daemon 视图 + 本地文件的混合体（`plugin_settings`/`plugin` 仍来自启动时读盘的那份，IPC 白名单不含它们）。要让面板覆盖插件设置，得先给 daemon 补这两类键的读写与 host 热重载。
- 编辑器查找替换（R3-21）仍是下一条最痛的：`xui::FindState` 在 `xgent_ui` 里零消费者，产品里根本不可达。
- `@file:<path>` 越界读取（§六 遗留）仍未动。
### 判别力反证（28 个变异体）

方法同前几轮：把每处修复改回缺陷形态 → 跑**断言所在 crate** 的测试 → 确认确有失败 → 还原 + `touch` 源文件（不 touch 会让 cargo 复用变异体的二进制）。测试目标按断言位置选（§八 M11 / §九 AP5 两次踩过的坑），整 crate 跑不按名字过滤。

| 变异 | 结果 | 失败用例（实测数字） |
|:---|:---|:---|
| M1 `parallel_safe` 不看共享策略（读空配置） | 抓到（1 failed） | `approved_tool_stops_asking_after_policy_handle_changes` |
| M2 `execute` 不看共享策略 | 抓到（10 failed） | 含 `denied_tool_is_refused_after_policy_handle_changes`、既有确认/策略用例一起炸 |
| M3 确认超时读构造时缺省 | 抓到（2 failed，**耗时 301s**） | `confirm_timeout_written_to_handle_expires_next_request` + 既有 `configured_confirm_timeout_expires`——301s 正是"退回 300s 常量"的表现 |
| M4 `run_command` 参数改成构造快照 | 抓到（4 failed） | `command_timeout_…` / `output_cap_…` 各两条 |
| M5 检索限额改回 `RetrievalLimits::default()` | 抓到（2 failed） | `retrieval_limits_written…` / `keyword_cap_written…` |
| M6 loop 不读句柄而用内置缺省 | 抓到（9 failed） | 含 `max_parallel_tools_written_to_handle_bounds_next_turn`、`retrieval_budget_written…` |
| M7 每帧上限回到字面量 64 | 抓到（1 failed） | `frame_event_cap_written_to_handle_is_read_next_frame` |
| M8 项目写只改内存不落盘 | 抓到（1 failed） | `project_write_lands_on_disk_and_reads_back` |
| M9 先换入内存再落盘 | 抓到（1 failed） | `failed_project_save_leaves_memory_unchanged` |
| M10 项目变更改成全局广播 | 抓到（1 failed） | `project_config_write_notifies_only_that_projects_subscribers` |
| M11 缺 `project_root` 时给个假根继续 | 抓到（2 failed） | `project_config_without_root_is_rejected_not_downgraded`、`resolve_project_root_rejects_…` |
| M12 白名单去掉 `context_window` | 抓到（3 failed） | `write_provider_context_limit_fields_reach_config` 等 |
| M13 数值解析失败时静默当 1 | 抓到（1 failed） | `project_write_wrong_type_names_the_field_and_keeps_previous_value` |
| M14 去掉"未设置显示值不算改动"判据 | 抓到（2 failed） | `untouched_fields_produce_no_writes` 等 |
| M15 Plain 语义留空也写 0 | 抓到（1 failed） | `empty_input_means_unset_or_unchanged_by_mode` |
| M16 密钥框留空也发写请求 | 抓到（1 failed） | `empty_api_key_field_does_not_wipe_the_stored_key` |
| M17 校验失败后不 early-return | 抓到（1 failed） | `invalid_number_blocks_the_whole_save`（首版变异体写成 `drop(q_status)` 编译不过，改成合法形态才判得准——"未抓到"要先排除变异体本身无效） |
| M18 UiOnly 的默认自动执行也写进 `approved` | 抓到（1 failed） | `policy_rows_fold_into_two_lists_without_redundant_entries` |
| M19 输入框不预填（回到只写不读） | 抓到（5 failed） | 预填相关的一批（含 `panel_opens_with_the_stored_values`、`reopening_…`） |
| M20 provider 键不拼 `providers.<id>.` 前缀 | 抓到（1 failed） | `changed_fields_are_written_with_their_daemon_keys` |
| M21 provider id 为空挡掉整批 | 抓到（1 failed） | `project_only_save_works_without_provider_id` |
| M22 `apply_snapshot` 里 `if window != 0` 的守卫去掉 | **无差别 → 删掉该分支** | 见下 |
| M23 句柄存原始值而非生效值 | 抓到（1 failed） | `new_handles_carry_effective_values`（6 passed / 1 failed） |
| M24 去掉 `project_root` 的 `#[serde(default)]` | **等价变异** | 36 passed 全绿，见下 |
| M25 `providers.*` 不再失效 provider 池 | 抓到（1 failed） | `provider_write_invalidates_the_pool_cache` |
| M26 从 en-US 删一个键 | 抓到（1 failed） | `every_panel_key_exists_in_both_locales` |
| M27 把 en-US 的报告分隔符换成中文分号 | 抓到（1 failed） | `punctuation_belongs_to_the_corpus_not_to_rust` |
| M28 bridge 把策略拷成新的空句柄 | 抓到（1 failed） | `tool_policy_written_to_handle_stops_asking_next_turn` |

28 个里 25 个当场抓到，两条不是"没抓到就完事"：

- **M22 是死代码，不是缺测试**。去掉 `if window != 0 { rs.context_window = window; }` 的守卫后 `cargo test -p xgent_app` 全绿——因为 `GlobalConfig::resolve_context_window` 的三级次序最差也回落到内置 `128_000`，**永远不会返回 0**，那个 `if` 从没真正生效过。处置是把这个分支删掉（连注释一起说明它在防一个不存在的输入），而不是给它补一条测试把它钉成"必要设计"。上一轮 §八 M8/M9 的"没抓到"是覆盖有洞，这一轮的"没抓到"是代码多余——两种成因不同，处置也就不同。
- **M24 证明了我在注释里写错的因果**。我原本写「新增协议字段必须 `#[serde(default)]`，否则旧客户端整条 `config.read` 反序列化失败」，据此给 `Option<String>` 字段都加了 `default`。把 `default` 摘掉后 `legacy_wire_without_project_root_still_parses` 照旧通过：serde 对 `Option` 字段的缺省本来就是 `None`，`default` 只是把这件事写明，不是它生效的前提。真正需要 `default` 的是**非 `Option`** 字段。代码注释与 `docs/dev-tutorial.md` 注意点 29 都已改成准确说法，测试保留（它锁的是"旧线格式仍能解析"这个真实契约，不锁那个属性）。

批次本身也再次验证了一条判据：**"未抓到"的默认解释不是等价变异**。M17 第一版报"编译期失败"（我把变异写成 `drop(q_status)`，后面还要用它），如果直接按"没抓到"记录，就等于把一条有效的判别用例误判成无效。先确认变异体本身合法，再谈等价性。

### 独立验证者回查后的第一轮否决（12/14，两条 fail + 一条高危）

只读验证者（不是我）逐条复核后判 **A9、A14 fail**，并提出一条我没写进 known limits 的高危风险。三条都成立，且都不是措辞问题：

1. **A9 的"失败值仍可见"是假的**。`drain_save` 每次保存后都会推进 `SettingsSnapshotReady.tick`，而 `toggle_panel` 见到 tick 变化就按 daemon 的值重建输入框——于是被拒的那个值在下一帧就被磁盘上的旧值换掉了。我的覆盖用例只投了一条 `SettingsSaveReportMessage`，从没触发那次重建，所以它绿着，而行为是错的。修法：面板侧加 `hold_rerender`（有失败的键就挡住这一轮重渲染，关闭面板解除），用例改成"改框 → 失败报告 → 推进 tick → 断言框里仍是我填的 7"，并追加"重开面板后回到磁盘值"。变异 M30（把 `!snapshot.hold_rerender` 换成 `true`）确认这条判据有牙。**——这段修法随后被第二轮否决：hold 在真实帧顺序里赶不上那次重建，且只有关闭按钮会解除；最终处置见下一节。**
2. **A14 的回归网被我剪掉了**。我因为几何塌陷把 `settings_input_focus.rs` 里"点击 → 焦点落在该输入框"的断言连同 `click_left` 一起删了，只留下手动设 `InputFocus`——验证者指出：此时删掉 `text_input_node` 的 `TabIndex(0)` 全套测试仍然全绿，也就是说那条历史缺陷（点输入框打不了字）从此无人看守。补 `every_settings_input_carries_tab_index` 锁组件契约（M29 实测：删 `TabIndex` → 1 failed），机制段仍由 `tests/interaction_model.rs` 覆盖，边界文字改成"真机端到端点击只能人肉验"。
3. **高危：我新加的 `session.rs` provider 池两条测试在写你的真实配置**。验证者不仅指出，还用 `XGENT_AGENT_DIR=/tmp/…` 复现了写入位置，并据此**否决了我写在 dev-tutorial 与本轮台账第 3 条里的"md5 一致 / 隔离已修好"**——那句对最终候选而言是假的。你的 `~/.xgent/agent/config.toml` 已被写成测试夹具且无法恢复（无 keychain 条目、无 sessions 目录、无用户数据快照）；我把文件重置成全新安装的默认空值，被污染的那份留在 `/tmp/xgent-config.test-contaminated.toml`。修法与判据见上面第 3 条。

另外两条被点名、也确实该改的：

- `max_output_tokens` 只有 Anthropic 适配器会发送，面板却把它做成一个对所有 kind 都生效的框。现在该字段带一条"仅 Anthropic"的说明（`settings-field-anthropic-only`，`NumField::note_key`），M32 证明这条说明被测试看着。
- 重渲染时我把 `PolicyDraft::default()` 传进了 `spawn_panel`，而保存读的是 World 里的真草稿——高亮回到磁盘值、保存仍带未保存的选择，显示与写入分叉。改为沿用真实 `Res<PolicyDraft>`，M31 证明。

修完后重跑的第 2 轮反证：M29/M30/M31/M32 各失败 1 条（另 M17、M25 复跑仍抓到；M24 仍是等价变异——serde 对 `Option` 缺字段本就填 `None`，已在代码注释与 §4.5.2/注意点 29 写成准确因果）。**"自检跑过一次"不等于"最终候选干净"**，这是本轮最贵的一课。

### 第二轮否决与迭代 3 的修复

只读验证者对迭代 2 候选判 **13 passed / A9 fail → 整体 fail**，两条都是实的：

1. **A9 的修复在真实装配里不生效**。`drain_save` 是"在同一帧里推进 `tick` + 写报告"，而 bevy 的 `Messages` 要到**下一帧**才可读，面板链里 `toggle_panel` 又排在 `apply_save_report` 之前——于是 `hold_rerender` 是在重建**之后**才被置上的，挡不住任何东西。验证者另起一个 `bevy_app` 复现装配，两种顺序都跑出 `rebuilds=1`；我的用例之所以绿，是因为它手动"先 hold 再推 tick"，那是真实运行永远不会产生的顺序。判据：**覆盖用例的触发顺序必须由被测程序自己产生**，手工摆出来的顺序只能证明"这个顺序下行为如此"。（上一轮记在下面的 M30 只证了"hold 字段被读"，没证"hold 在真实顺序里赶得上"——变异反证也受同一条限制。）
2. **`global_write_lands_inside_the_isolated_dir` 根本不存在**。全仓 grep 零命中，却被我写进 handoff、`docs/dev-tutorial.md` 与上面第 3 条。这是同一失效模式的第三次：`settings_io` 的测试模块那次也是"追加没落盘"，我只信了 diff 没回读源码。处置：**凡声称"新增了某条测试"，一律 `grep -n "fn <测试名>"` 回读 + 按名字单跑一次**（`cargo test <name>` 打出 `1 passed` 才算）。
3. 另外点出的两条泄漏也确实成立：`hold_rerender` 只有关闭按钮会解除，热键关闭 / 全部成功 / 对端变更三条路径都会把它留在资源里，之后的合法刷新被永久挡掉。

**迭代 3 的做法：把显示策略收回推进 `tick` 的那一方，面板不留状态。** `settings_io` 里从 `drain_save` 抽出一个纯函数 `apply_save_result`（抽出是为了让这条判据能被单测直接跑到），只在 `report.failed` 为空时 `tick += 1`；面板侧 `hold_rerender` 整个删掉（连带 `Default` 手写实现退成 derive）。资源与句柄照旧按 daemon 接受的值更新，所以运行时永远读磁盘真值，只有显示停在编辑态，且随后任何一次合法的快照推进都会把显示拉回来。

新增/改写的证据与反证：

| 变异 | 结果 | 抓住它的用例 |
|:---|:---|:---|
| M33 失败也推进 `tick`（回到 A9 原缺陷） | 抓到（1 failed） | `a_rejected_key_updates_state_without_redrawing_the_panel`（xgent_app 单测，直接跑到判定点）。**订正**：迭代 5 把判定点从 `apply_save_result` 移回 `drain_save` 并做了端到端装配，这条用例随之被 `drain_save_advances_the_render_counter_only_when_every_key_landed` 与 `a_failed_save_leaves_the_rejected_value_in_the_real_panel_frame` 取代；上一节"另加 `global_write_lands_inside_the_isolated_dir` 钉住隔离本身生效"是真的，本句引用的用例名则已不在树里——按"声称加过测试就 grep 回读"的自家判据把它订正掉。 |
| M34 去掉 `GlobalConfigStore::save`（全局写不落盘） | 抓到（1 failed） | `global_write_lands_inside_the_isolated_dir`（这次真的在文件里；另以 `HOME=/tmp/…` 跑整套 daemon，假家目录没出现 `config.toml`，真实文件 md5 前后一致） |
| M35 面板永不按快照重建 | 抓到（1 failed） | `failed_save_keeps_the_rejected_value_on_screen`（改成"失败报告不动显示 → 随后一次合法 `tick` 推进必须重建"，把 hold 泄漏那条也一起判掉） |
| M37 loop 无视运行期窗口（硬编码宽窗口） | 抓到（1 failed） | `runtime_narrowed_window_drives_the_next_compaction_decision`（对照：全程宽窗口不压缩；bridge 构造**之后**经句柄改窄 → 下一条对话压缩） |
| M38 面板不显示快照读取失败的原因 | 抓到（1 failed） | `snapshot_read_failure_is_reported_instead_of_a_default_config` |

验证者点的风险里，两条按建议补上了（A11 的行为半边、"TOML 坏了要在面板上说出来"），第三条按边界如实记录：**daemon 的配置错误串会原样进英文面板**，所以把 `期望…，收到:` 与 `model_context_windows 为只读（…）` 改成半角标点、不带全角括号，并由 `config_write_errors_avoid_fullwidth_punctuation` 钉住"这些串里没有全角标点"。原因文本本身仍是中文——daemon 没有 `Localizer`，把诊断文案国际化要改协议（错误码 + 参数），不属于本次能力，写进 known limits 而不是假装已经解决。

### 第三轮否决（A9 三连败）与迭代 5 的修复

迭代 4 候选 `76f0f255` 的 Runtime 检查 13/13 全绿，只读验证者仍判 **A9 fail**（13 passed / 1 failed），而且理由是对的、也是新的：

> `apply_save_result` 里那句 `if report.failed.is_empty() { ready.tick += 1 }` 按代码是对的，但它守的东西不成立——`toggle_panel` 的重渲染闸门是 `is_changed() && tick > 0`，而 `ResMut::deref_mut` 会**无条件**标脏（bevy 的 `change_detection_mut_impl!`）。`drain_save` 每次都带着 `ResMut<SettingsSnapshotReady>` 并且一定会写它，`tick > 0` 从第一次 `drain_snapshot` 之后就恒成立，所以每一帧都会重建，被拒的值照旧被磁盘值换掉。验证者用临时探针用例在**真实**的 `SettingsPanelPlugin` 上量到两种帧序都是 `field="4"`，并指出我的两条用例为什么都绿：单测只断言计数器，UI 用例投报告时**根本没碰**那份资源——那是 app 不会产生的一帧。

三轮的共同形状终于清楚了：**A9 是一条跨系统行为（app 写状态、面板据此决定重画），而我只在两边各测各的**。所以迭代 5 做了三件事：

1. **让信号可比较，而不是靠变更标记**。`toggle_panel` 改成一个 `Local<u32>` 记住"上次渲染时的 tick"，只在 `snapshot.tick > *last_rendered` 时重建；`is_changed()` 不再参与判定。
2. **失败那一帧连资源都不碰**。`drain_save` 里推进 `tick` 的那句被 `report.failed.is_empty()` 包住，且这是唯一一处 `deref_mut`——没有失败就没有标脏，也不留"将来谁再看 `is_changed()` 就会被骗"的伏笔。`apply_save_result` 因此退成"只把权威值落到两份资源与四组句柄"（不再管计数，也不再返回 provider 标记）。
3. **把两半装进同一帧序跑**。`drain_save` 原先还兼着"provider 写过 → 经 IPC 重取 `ProviderInfo`"，这让它离不开 `IpcClientResource`/`AgentBridge`，测试装不进去。拆出 `refresh_provider_after_save`（读 `SettingsSaveReportMessage`，识别判据是纯函数 `touches_provider`）之后，`xgent_app` 里的 `a_failed_save_leaves_the_rejected_value_in_the_real_panel_frame` 能把真实的 `drain_snapshot` + `drain_save` 与真实的 `SettingsPanelPlugin` 装进同一个 `App`：投一份带失败键的 `SaveTaskResult` 进通道，让 Bevy 自己的调度跑完，再去读输入框的文本。断言是三段的——被拒的值留在框里、`tick` 一格不动、随后一次合法的快照推进把显示拉回磁盘值（防"永久停更"）。

反证按新结构重跑，两边互补，缺一边就漏一边：

| 变异 | 抓它的用例 |
|:---|:---|
| M40 面板闸门退回 `is_changed() && tick > 0` | UI 侧 `failed_save_keeps_the_rejected_value_on_screen`（集成用例抓不到：现在的 app 在失败帧根本不标脏，两种闸门此时等价——这正是"面板不看脏标记"这条规则要单独钉的理由） |
| M41 `drain_save` 不看 `failed` 就推进 `tick` | 集成用例 + `drain_save_advances_the_render_counter_only_when_every_key_landed` |
| M42 面板永不按增量重建 | 集成用例（第三段断言）与 UI 用例 |
| M33/M34/M35/M37/M38（上一轮那批） | 复跑仍各抓到 1 条 |

另外两条如实记录的收获与边界：

- 上一轮我把 `drain_save` 整体留在 IPC/bridge 上，理由是"少一层消息"；正是这个耦合让端到端装配做不出来，于是两轮都只能"各测一半"。**能拆出一个不依赖外部 I/O 的落状态段，本身就是为了可测性而做的正确切分**，不是多余的一层。
- 验证者同时点名 A14 的"pre-existing tests untouched in behaviour"字面未满足：`settings_input_focus.rs` 的真实 picking 那一半在 headless 下量不到子节点坐标（`UiRoot` 缺失 → 面板根 `0×0`），我把它换成了组件契约断言 `every_settings_input_carries_tab_index`。这不叫"改了既有测试的行为"，但它确实不再自动跑真机点击链路，因此写进 known limits 而不是含糊过去。

### 第四轮否决（A9 换了第三条腿）与迭代 6

迭代 5 候选 `222c471d`：Runtime 13/13 全绿，独立验证者判 **13 passed / A9 fail**，但这一轮它明确确认**前三轮那个缺陷已经关了**——它自己从 bevy 源码把语义读了一遍（`system_param.rs:730-777` 取 `ResMut` 不标脏、`change_detection/traits.rs:493-501` 只在 `deref_mut` 标脏），核实"有键被拒时 app 根本不碰那份资源"成立，`drain_snapshot` 是唯一的另一个写者且它写的时候一定推进 `tick`；还重跑了 M40 探针，复现了我声称的互补性（把面板闸门退回 `is_changed()` 只有 `xgent_ui` 那条用例炸，集成用例保持绿），并判定反向的不对称是结构性的（`xgent_ui` 没有对 `xgent_app` 的 dev-dependency），**不算覆盖洞**。

它抓的是**同一句验收项里另一个从句**：A9 与 spec「Saving」第 42 行、brief 的 Outcome/Scope 都写的是"报告**点名**已写入的键"（*names the keys already written*），而 `apply_save_report` 在失败分支只渲染了 `已保存 { $count } 项`——`ev.written` 里那些键名在 `xgent_ui` 全程没有被读过，所以名字从来没上过屏幕。我引的那条用例断言的是 `contains("2")`，把错的形状钉成了契约。

处置（迭代 6）：

1. 双语料新增 `settings-written-keys`（zh `已写入：{ $keys }` / en `written: { $keys }`），键名之间用已有的 `settings-list-sep`（zh `、` / en `, `）连接——分隔符仍归语料，Rust 侧不写死标点。
2. `partial_save_reports_written_and_failed_keys` 改成断言**两个已写入的键名都出现在状态行**，并反向断言"不该再出现裸个数"；变异 M43（退回只报个数）→ 1 failed。
3. 验证者顺带点的小洞："留空即写 0"此前只在 `parse_num_field` 的解析层证过，消息层没人管。`changed_fields_are_written_with_their_daemon_keys` 补了一条断言：留空的 `tools.confirm_timeout_secs` 必须以 `Value::Number(0)` 发出。变异 M44（Sentinel 留空当成"不改"）→ 2 failed（解析层与消息层各一条）。
4. 两处**过期声称**按"引用用例名要 grep 回读"的自家判据订正，而不是删掉：`toggle_panel` 的文档注释原文写"`drain_save` 只要碰到这份资源（它每次都碰）"——迭代 5 之后失败帧根本不碰，已改成"两边各留一道"；本文件上一轮的变异表引了 `a_rejected_key_updates_state_without_redrawing_the_panel`，该用例在迭代 5 的拆分中被删，已就地标注由哪两条现役用例接手。

判据的增量：**同一句验收项要被拆成"每个名词都指得到一个渲染点"来核对**。前三轮的成因分别是"信号来得太晚""信号被无关的脏标记淹没""报出来的内容形状不对"——前两类靠帧序与可比较计数解决，第三类只能逐词对照文本：`names the keys` 里的 *names* 是动词，不是"提到有个数"。固定动作：把 brief/spec 那句话里的每个动词与名词在显示函数里找一个对应的字符串，找不到就是没实现；而覆盖用例的断言要照那句话写，不要照当时的实现写（我这轮正是 `contains("2")` 把缺陷钉成了契约）。

### 第五轮：结论"全过"却记不进 Runtime，以及读码读出来的两条缝

迭代 6 候选 `f3c0a0b1`：Runtime 检查 13/13，独立验证者判 **14/14 pass**。它对 A9 的命名从句查得比我细——不但不信任我的断言，还**离线复算了 fluent 渲染**（自建 `/tmp/v6-fluent-probe`，fluent-bundle 0.16.0，同一 `new_concurrent` + `format_pattern`）：zh 与 en 两个 locale 里两个键名都连续出现、隔离标记（U+2066..U+2069）只包整段 placeable 而不插在中间、参数名匹配、英文侧没有中文标点；并自己复现了 M43（退回计数 → `18 passed; 1 failed`）。

**但这份结论没有落进 Runtime**：它提交 `final-result` 时把每条 `result` 写成 `"pass"`，而 schema 要的是 `passed|failed|blocked`，Runtime 判 `invalid-data` 拒收并**关闭了该次 Verifier 尝试**；改好的 `/tmp/v6-final.json` 再也挂不上（"requires an active Verifier attempt"）。恢复只能按新的 continuation 走：同一份实现重新提交候选（iteration 7）→ 重新派发 Verifier。判据：**往 Runtime 回结构化结论时，枚举值逐字照 template 抄，别按语义"顺手写"**——一次格式失误的代价是整轮重派发，而语义结论本身并没有被推翻。

验证者读码另外点出两条我这边没人管的真缝，都按"补成事实"处理而不是辩解：

1. `apply_save_result` 把写完后重读的 `SnapshotResult.error` 丢了——daemon 连不上或项目 TOML 坏掉时，保存通路只报逐键结果，顶部提示行永远是上一次的值。处置：在**全成功**那一支把原因落到 `ready.error` 并推进计数（顶部行要靠重建才显示）；**有键被拒**那一支照旧不碰这份资源——重建会抹掉被拒的值，那种场合由逐键报告说清楚。两条边界一起写进 `a_clean_save_surfaces_a_re_read_failure_and_a_rejected_one_does_not`，变异 M45（摘掉那句赋值）失败 1 条。
2. handoff 里 A1 的证据还写着 `settings_panel_values -> 18 passed`（该套件已是 19 条）。数字随用例增删必须重跑再抄，不能沿用上一轮的记录。

另记一条核对结论：A2 里"注册的 UiOnly 编辑器工具显示自动执行"指的不是测试夹具，生产代码确有该工具（`crates/xgent_tools/src/editor_tool.rs:210`），验证者把它列成的 risk（brief 措辞里的 `editor.open_file` 与实际注册 id）属文案粒度问题，不影响判据。
