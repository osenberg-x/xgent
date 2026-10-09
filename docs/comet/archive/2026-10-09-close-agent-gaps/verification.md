---
generated_from_state_version: 15
---

# Verification

## Current result

- Result: **Archived**
- Verification status: **Checks completed; result confirmed**
- Goal cycle: 1
- Iteration: 3
- Verifier attempt: 1
- Completed: 2026-10-09T00:16:57.796Z
- Summary: 第三轮验证 14/14 通过。关键：A4 测试判别力经独立反证实验确认——把工作区复制到 /tmp、仅在副本中移除两个 tx.is_closed() 守卫，两个用例分别以『实际拉了 1000 帧』与『丢弃时 7 → 等待后 9735』FAILED，还原后 46 passed；diff -q 确认项目文件字节未变。cargo check/test/clippy --all-targets 退出码 0，537 tests passed / 0 failed / 0 clippy warning。已在工作区外完成实验并保持严格只读。

## Acceptance

| ID | Result | Source | Criterion | Reason |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | A1: Every entry in the R1 and R2 tiers of `doc/notes/agent-gap-review.md` is either implemented in this change or recorded as deferred with a named reason, and the classification covers all 14 entries. | 第三轮独立验证通过。报告本身未被修改（git status 显示 doc/notes/agent-gap-review.md 仍是未跟踪的原始审查产物，本次未编辑其内容）。 |
| A2 | passed | brief.md | A2: The agent loop stops after a bounded number of tool-call iterations or when a cumulative token ceiling is reached, and it reports which bound it hit instead of silently stopping. | 第三轮独立验证通过。两处测试分别把闸压到 3 轮 / 150 token 并断言命中的界与终止语义（Idle 而非 Error）。 |
| A3 | passed | brief.md | A3: The retry configuration defaults to a finite retry count, 429 and 5xx responses are retried, and a provider-supplied `Retry-After` overrides the computed backoff delay. | 第三轮独立验证通过。第一轮判 failed（Retry-After 在 IPC 边界被丢弃）。第二轮复核确认五段链路已打通；其提出的『流内构造点仍为 None』经复核不构成缺陷——Retry-After 是响应头，2xx 响应头发出后即固定，流内不存在可读的头，429 按定义落在建流路径，而建流路径正是已打通的那条；流内传输链本身是通的（ChatEvent::Error.retry_after_secs 有 serde 默认值与全链路反序列化，真有值可直达重试层）。 |
| A4 | passed | brief.md | A4: Aborting a response issues a cancel through the daemon to the provider, the spawned stream task terminates, and no further tokens are produced for the abandoned stream. | 第三轮独立验证通过。前两轮均判 failed 的核心项：第一轮 drop 接收端不终止上游 body；第二轮确认修复但指出我的测试无判别力。本轮补齐 anthropic 首事件前守卫并重写为有判别力的测试（已用移除守卫的反证实验验证）。 |
| A5 | passed | brief.md | A5: `read_file` accepts `offset` and `limit`, truncates output that exceeds its cap with an explicit marker, and never splits a UTF-8 character. | 第三轮独立验证通过。截断与切片都不切碎 UTF-8 字符；offset 越界返回空、limit 越界钳制，均不报错（模型常按估算行号请求）。 |
| A6 | passed | brief.md | A6: A new `edit_file` tool applies a line-range replacement to an existing file, writes atomically, leaves a backup of the original, and reports a clear error when the specified lines do not match the expected content. | 第三轮独立验证通过。写入路径与 write_file 共用 atomic::atomic_write（同目录 rename，原子）；写前备份为 <name>.xgent-bak。 |
| A7 | passed | brief.md | A7: A dangerous command in `run_command` yields a stricter approval outcome than an ordinary command, and plugin `run_command` rejects an argument outside the manifest's allowed set. | 第三轮独立验证通过。危险命令检测此前命中与未命中都返回 Exec，severity 相等使升级分支不可达；Dangerous tier 使其真正生效（有针对性单测证明普通命令仍放行）。 |
| A8 | passed | brief.md | A8: Plugin tools supply a real `preview_diff` and a real `summarize` through the WIT bridge, and the tool update callback delivers streaming tool output to the UI as `Arc` instead of a borrowed boxed closure. | 第三轮独立验证通过。summarize 由同步改 async 是必要的：WIT 方法为 async，同步签名无法 await WASM 调用。这是 trait 破坏性变更，已全量改完 9 处实现并通过编译与测试。 |
| A9 | passed | brief.md | A9: An API key stored in the OS keychain is used in preference to the TOML value, a missing or unreadable keychain falls back to TOML, and the TOML value is preserved rather than deleted. | 第三轮独立验证通过。TOML 值永不被删除是硬约束（凭据存储不可用时必须仍有回退），已写入文档注释。Windows 分支显式返回不支持并回退 TOML，非静默失败。 |
| A10 | passed | brief.md | A10: `ResponseApiProvider` and `CustomApiProvider` produce real streamed responses through the existing `LlmProvider` trait instead of returning a placeholder error, and neither contains a "尚未实现" path. | 第三轮独立验证通过。Custom 复用已验证的兼容适配器而非复制 HTTP 逻辑，避免超时/错误/协议两份实现漂移；代价是它只能覆盖 OpenAI 兼容形态的自定义接口（这是 F-07 原文对第三方的定义）。 |
| A11 | passed | brief.md | A11: `context_strategy` selects the built-in provider at startup, and a strategy without an implementation surfaces a configuration error rather than falling back to another strategy. | 第三轮独立验证通过。静默回退会让误配置的项目看起来完全正常，故选择启动失败并给出可操作提示。 |
| A12 | passed | brief.md | A12: `requirements.md` F-04 no longer claims read-only operations are auto-approved, and the correction is limited to claims contradicted by the shipped code. | 第三轮独立验证通过。按已交付的更严格行为收敛文档，代码未改。 |
| A13 | passed | brief.md | A13: Each of the seven deferral groups (R2-2, the R3 tier, the R4 tier) records what it defers and why, so no entry in the gap report has an unstated disposition. | 第三轮独立验证通过。每条延后都指向具体的阻塞决策编号，而非笼统的「后续再做」。 |
| A14 | passed | brief.md | A14: `cargo check --workspace` and `cargo test --workspace` both pass. | 第三轮独立验证通过。本变更触及 44 个文件、约 2907 行新增。 |

## Checks

_No Runtime checks were recorded._

### Builder-reported evidence

These are Builder reports, not Runtime check receipts or independent verification results.

- cargo check --workspace: passed — 0 error
- cargo test --workspace: passed — 0 failed；连续 4 次运行稳定
- cargo clippy --workspace: passed — 0 warning（含 needless_borrow / unused_cast / dead_code 逐项清理）
- cargo fmt --all: passed — 已格式化
- ./build_plugins.sh: passed — 修正 ROOT 后成功重建 wasm 插件并打包（WIT 新增导出要求插件重编译）
- Known limitation: R3/R4 共 31 条未实现，理由与逐条分类记在 doc/notes/agent-gap-disposition.md；本次只交付 Shape 确认的 R1+R2 范围
- Known limitation: OS keychain 的真实读写路径无自动化测试覆盖（需真实 macOS Keyring / Linux libsecret 环境）。单测只覆盖函数存在性、空值处理、未知条目幂等、resolve 优先级；分支逻辑经代码审查，未经端到端验证
- Known limitation: Retry-After 仅支持 delta-seconds；HTTP-date 需日期解析依赖，本次未引入，解析失败退回计算退避
- Known limitation: manifest 的 fs-read/fs-write/network 字段 kebab-case 未映射（既有问题，本次未改——改动会变更既有 fs 权限的实际生效范围）。command-args 之外插件 fs 权限的实际生效范围与文档不符
- Known limitation: 插件 args 白名单只约束首个非 flag 参数（子命令），flag 依赖通用危险黑名单——语法上无法区分 flag 与其值
- Known limitation: CustomApiProvider 只能覆盖 OpenAI 兼容形态的自定义接口（复用兼容适配器），不支持自定义 body 模板；requirements.md 原文只要求「支持自定义 API」，未要求 body 模板
- Known limitation: token 闸只统计 provider 回报的 usage；provider 不回报时 tokens_used 恒为 0，累计 token 上限不触发，仅剩轮次闸兜底
- Known limitation: read_file 先读全文件再切片：offset/limit 只减少回灌 LLM 的内容量，不减少磁盘/内存读取量
- Known limitation: 本次为当前目录（main）直接开发，非隔离分支；工作区仍有与本变更无关的未提交内容（.gitignore 的 Comet 块、若干工具配置目录、docs/openspec/）
- Known limitation: 对外破坏性变更（已在 brief 与本清单声明，仓内调用点全部更新且编译测试通过）：ErrorKind 新增 RateLimited/ServerError、ProviderError::Api 新增 retry_after_secs、ToolExecutor::execute 新增 on_update 形参、Tool::execute 的 on_update 由 Option<&> 改 Option<Arc<>>、Tool::summarize 改 async、xgent_context::build_context_provider 返回类型改为 BuiltContextProvider、PluginConfig/WIT 新增 command-args 与 tool.preview-diff

## Blockers

_None._

## Risks and skipped work

- response_api 在 Start 前缺 is_closed 守卫（已在本轮补齐，与另两个 provider 对齐）
- 台账标题『14/14』与表内 13 行不符（已订正为 13 条 + R2-2 延后）
- A4 中途丢弃测试依赖墙钟等待，重载机器上最脆弱（但对照实验显示修复前后相差约三个数量级：0 vs 9735 帧）
- Windows keychain 未实现，回退 TOML（D-C2 已记录）
- manifest fs-read kebab-case 未映射的既有问题仍在（§四已记录，本次有意不动）

## Previous iterations

| Goal cycle | Iteration | Attempt | Outcome | Unresolved | Summary | Completed |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 1 | fail | A3, A4 | 首轮只读验证：cargo check/test 全绿；14 项中 12 项通过，A3（Retry-After 跨 IPC 断链）与 A4（drop 接收端不终止上游 body）失败——两者均为真实链路断裂。 | 2026-10-08T17:53:57.457Z |
| 1 | 2 | 1 | pass | — | 第二轮验证 14/14 通过；cargo check/test/clippy 退出码 0，537 tests passed / 0 failed。同时指出 A4 测试无判别力与 anthropic 守卫缺失，已在本轮修复。 | 2026-10-08T23:50:41.418Z |
| 1 | 2 | 1 | recovery | — | 第二轮验证指出 A4 测试无判别力与 anthropic 首事件前缺守卫：已重写 A4 测试为可观测拉取计数并用移除守卫的反证实验确认会失败（1000 帧 / 7→9735），已补齐 anthropic 守卫，清理过期注释与全部 clippy 警告。回到 Build 重新提交并再跑一轮验证。 | 2026-10-08T23:50:47.742Z |
| 1 | 3 | 1 | pass | — | 第三轮验证 14/14 通过。关键：A4 测试判别力经独立反证实验确认——把工作区复制到 /tmp、仅在副本中移除两个 tx.is_closed() 守卫，两个用例分别以『实际拉了 1000 帧』与『丢弃时 7 → 等待后 9735』FAILED，还原后 46 passed；diff -q 确认项目文件字节未变。cargo check/test/clippy --all-targets 退出码 0，537 tests passed / 0 failed / 0 clippy warning。已在工作区外完成实验并保持严格只读。 | 2026-10-09T00:16:57.796Z |



## Conclusion

第三轮验证 14/14 通过。关键：A4 测试判别力经独立反证实验确认——把工作区复制到 /tmp、仅在副本中移除两个 tx.is_closed() 守卫，两个用例分别以『实际拉了 1000 帧』与『丢弃时 7 → 等待后 9735』FAILED，还原后 46 passed；diff -q 确认项目文件字节未变。cargo check/test/clippy --all-targets 退出码 0，537 tests passed / 0 failed / 0 clippy warning。已在工作区外完成实验并保持严格只读。
