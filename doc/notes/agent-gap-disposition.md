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
