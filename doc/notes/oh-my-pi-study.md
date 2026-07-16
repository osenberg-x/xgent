# oh-my-pi (omp) 架构学习报告

> 本文档记录对开源 AI coding agent 项目 [oh-my-pi](https://github.com/can1357/oh-my-pi) 的架构学习，供 XGent 设计参考。
>
> 状态：学习报告 · 2026-07-16

---

## 0. 项目概览

**oh-my-pi (omp)** 是一个基于 TypeScript（Bun 运行时）的终端 AI coding agent，fork 自 Mario Zechner 的 [Pi](https://github.com/badlogic/pi-mono)。定位为"最强大的 agent 表面，开箱即用"。

核心数据：**40+** providers · **32** 个内置工具 · **14** 个 LSP 操作 · **28** 个 DAP 操作 · **~55k** 行 Rust 核心（native 绑定）。

### 0.1 Monorepo 包结构

| 包 | 职责 |
|:---|:---|
| `packages/ai` | 多 provider LLM 客户端，统一流式接口 |
| `packages/catalog` | 模型目录：内嵌 models.json、provider 描述、模型身份/分类 |
| `packages/agent` | agent runtime：工具调用与状态管理 |
| `packages/coding-agent` | 主 CLI 应用（omp 的核心） |
| `packages/tui` | 终端 UI 库：差分渲染 |
| `packages/natives` | 原生绑定（文本/图片/grep 操作） |
| `packages/hashline` | 基于 content-hash 的行锚点 patch 语言 |
| `packages/snapcompact` | 上下文压缩：将历史归档为 bitmap 图片 |
| `packages/utils` | 共享工具（logger、stream、temp file） |
| `crates/pi-natives` | Rust crate：性能关键操作（grep/glob/diff） |

### 0.2 技术栈

- **语言**：TypeScript（edition 2024）+ Rust（native 绑定）
- **运行时**：Bun（非 Node.js）
- **模板引擎**：Handlebars
- **Schema**：Zod v4（工具参数）+ arktype
- **持久化**：JSONL（会话）+ SQLite（元数据/历史）
- **LLM 传输**：SSE（HTTP 流式）+ WebSocket（Codex）

---

## 1. 分层架构

```
┌─────────────────────────────────────────────────────────┐
│                    CLI 入口 (cli.ts)                      │
│         worker-host dispatch + 命令路由                   │
├─────────────────────────────────────────────────────────┤
│              应用层 (coding-agent)                        │
│  AgentSession · Modes(TUI/Print/RPC) · SystemPrompt      │
│  MCP · Capability · SlashCommands · Extensibility        │
├─────────────────────────────────────────────────────────┤
│              Agent Runtime (agent)                        │
│  Agent · agentLoop · ToolExec · Compaction               │
│  EventStream · Steering/Abort · AppendOnlyContext        │
├─────────────────────────────────────────────────────────┤
│              LLM 抽象层 (ai)                              │
│  streamSimple · 40+ Provider 适配器 · 统一事件流           │
│  Tool Schema(Zod) · Thinking/Reasoning · ServiceTier     │
├─────────────────────────────────────────────────────────┤
│              基础设施 (utils/natives/hashline)            │
│  Logger · Stream · FSCache · Native(grep/glob/diff)      │
└─────────────────────────────────────────────────────────┘
```

关键分层原则：
- `ai` 包是纯 LLM 抽象，不含 agent 逻辑。
- `agent` 包是 agent runtime，不含 CLI/UI 逻辑。
- `coding-agent` 是应用层，组装 agent + UI + 工具 + MCP。

---

## 2. Agent Loop 核心架构

`packages/agent/src/agent-loop.ts`（2377 行）是 omp 的核心引擎。

### 2.1 双层循环结构

```
runLoopBody()
├── 外层 while(true) — follow-up/aside 消息驱动
│   ├── 内层 while(hasMoreToolCalls || pendingMessages.length > 0)
│   │   ├── yieldIfDue() — 防止事件循环 busy-wait
│   │   ├── agentPauseGate.waitUntilResumed() — 全局暂停门控
│   │   ├── 注入 pendingMessages（steering + aside）
│   │   ├── syncContextBeforeModelCall() — 刷新 systemPrompt/tools
│   │   ├── streamAssistantResponse() — 流式调用 LLM
│   │   ├── executeToolCalls() — 并发工具执行
│   │   ├── emitTurnEnd() + onTurnEnd hook
│   │   └── 轮询 steering 消息 → pendingMessages
│   ├── onBeforeYield() hook
│   └── 轮询 lateSteering + aside + followUp → 若有则 continue
└── endAgentStream() — 推送 agent_end
```

**关键设计**：`stopReason` 不决定循环是否继续，`toolCalls` 是否存在才决定。`runnableStop = stopReason === "toolUse" || stopReason === "stop"`，只有 `length`（max_tokens 截断）才阻止工具执行。

### 2.2 流式响应处理

**消息转换边界**：`AgentMessage[]` → `Message[]` 只在 LLM 调用前通过 `convertToLlm` 回调完成。这允许自定义消息类型（UI 通知、artifact 等）在内部自由使用。

**上下文构建流水线**：
1. `transformContext`（AgentMessage 层面：剪枝、注入外部上下文）
2. `convertToLlm`（AgentMessage → LLM Message）
3. `normalizeMessagesForProvider`（如 Cerebras 剥离 thinking blocks）
4. `appendOnlyContext.build()` 或手动构建（稳定 prefix cache）
5. `transformProviderContext`（最终 provider context 变换）

**流式事件处理**：
- 使用 `Promise.race([responseIterator.next(), abortRacePromise])` 实现可中断的流式消费
- abort listener 注册一次，复用于整个流
- `partialMessage` 实时更新到 `context.messages` 末尾，UI 可实时看到增量
- `snapshotAssistantMessage()` 深拷贝确保订阅者拿到不可变视图

### 2.3 中断信号分层

```
external abort（用户/系统）
    ↓
steering abort（用户插话中断工具）
    ↓
IRC abort（子 agent 间通信中断）
```

- 只有声明 `interruptible: true` 的工具（如 job poll）才响应 IRC 中断
- 前台工具（bash、write）不受 IRC-only 中断影响——避免中断正在产生副作用的工具

### 2.4 Steering 与 Follow-up

三类注入消息：

| 类型 | 来源 | 中断性 | 注入时机 |
|:---|:---|:---|:---|
| **Steering** | 用户实时输入 (`steer()`) | 中断工具执行 | 内层循环每次迭代 + 工具完成后 |
| **Aside** | 后台任务完成、LSP 诊断 | 不中断工具 | 工具 batch 完成后、yield 前 |
| **Follow-up** | 延续对话的消息 | 不中断 | agent 准备停止时 |

**消费 vs 非消费队列分离**：
- `getSteeringMessages()` — 消费性，只在注入边界调用，dequeue 消息
- `hasSteeringMessages()` — 非消费性，工具执行中 peek，不消费队列
- 设计原因：abort 期间 dequeue 会把消息困在即将死亡的 run 中；peek 不消费，消息在 abort 后的 continue 中仍能投递

### 2.5 Abort / Continue / Pause

- **Abort 信号传播**：`Agent.abort()` → `AbortController.abort()` → signal 传入 loop/stream/tool。支持 tool-scoped abort（只中断特定工具）。
- **Continue**：`agentLoopContinue()` 从当前 context 继续，不添加新消息。最后一条消息必须是 user 或 toolResult。
- **Deadline**：`config.deadline` 是绝对 Unix epoch 毫秒时间戳，通过 `AbortSignal.any` 合并到主 signal。
- **全局暂停门控**：`agentPauseGate` 是进程级单例。`pause()` 创建 Promise 门控，所有 agent loop 在安全点（model call 前、tool call 前）park。暂停不 abort——in-flight 工作执行到完成，然后 park。

### 2.6 Append-Only Context（Prefix Cache 优化）

`AppendOnlyContextManager` + `StablePrefix`：
- system prompt + tool specs 计算一次并冻结（fingerprint 比较）
- 消息只追加不重新序列化
- 配合稳定 prefix，每 turn 只有用户新消息 delta 是 cache miss
- `invalidate()` 在 MCP 重连等场景强制重建

### 2.7 Owned Dialect（In-Band Tool Calling）

某些模型（GLM、Kimi、DeepSeek 等）不支持原生 function calling。omp 通过 dialect 系统：
- 将工具目录渲染为文本 prompt
- 将模型文本输出解析回 `toolCall` blocks
- `encodeInbandToolHistory` 将历史 tool calls/results 重编码为文本
- `wrapInbandToolStream` 将流式文本重新物化为 native toolCall content blocks

---

## 3. 工具系统

### 3.1 AgentTool 接口

```typescript
interface AgentTool<TParameters, TDetails, TTheme> extends Tool<TParameters> {
    name: string;
    label: string;                    // UI 显示名
    description: string;
    parameters: TSchema;              // Zod schema
    execute: AgentToolExecFn;         // 执行函数
    approval?: ToolApproval;          // 安全分级
    concurrency?: "shared" | "exclusive" | ((args) => ...);
    loadMode?: "essential" | "discoverable";
    intent?: "omit" | "optional" | "require" | ((args) => string);
    interruptible?: boolean;          // 是否响应 IRC 中断
    lenientArgValidation?: boolean;   // 宽松参数校验
    renderCall?(args, options, theme);   // TUI 渲染
    renderResult?(result, options, theme, args?);
    formatApprovalDetails?(args);     // 审批提示详情
}
```

### 3.2 工具执行生命周期

```
toolCall 入站
  → 参数校验 (validateToolArguments, 支持 lenientArgValidation)
  → beforeToolCall 钩子 (可 block / mutate args)
  → tool.execute(toolCallId, params, signal, onUpdate, context)
  → 流式 onUpdate 回调推送 partialResult
  → coerceToolResult 结构归一化 (防第三方工具返回畸形数据)
  → afterToolCall 钩子 (字段级覆盖)
  → emitToolResult 发出 tool_execution_end 事件 + 构造 ToolResultMessage
```

### 3.3 安全策略（Approval 系统）

**三层 tier 分级** × **三种 approval mode** 矩阵 + per-tool 用户覆盖：

| Tier | 含义 |
|:---|:---|
| `read` | 读数据或更新 UI-only 元数据 |
| `write` | 修改 workspace/session 状态但不执行任意代码 |
| `exec` | 执行代码、shell、浏览器、spawn agent |

| Mode | 自动批准 | 需确认 |
|:---|:---|:---|
| `always-ask` | `read` | `write`, `exec` |
| `write` | `read`, `write` | `exec` |
| `yolo`（默认） | `read`, `write`, `exec` | 无 |

- `approval` 可声明为静态 tier、`{tier, reason, override}` 对象、或 `args→decision` 动态函数
- 解析顺序：工具自身决策 → 用户 per-tool 覆盖 → mode tier 比较
- `override: true` 可强制 prompt（如 bash 检测到 `rm -rf /` 等危险模式）

### 3.4 并发执行

通过 `concurrency` 字段控制：
- `shared`（默认）：与其他 shared 工具并行执行
- `exclusive`：串行，等前序全部完成后独占执行（如 WriteTool、TodoTool）
- 函数式：按 args 动态决议

调度器维护 `lastExclusive` + `sharedTasks` 队列。

### 3.5 工具发现机制

两层发现：
1. **loadMode**：`essential`（默认加载：read/bash/write/edit/glob/eval）vs `discoverable`（按需激活：todo/github/debug/browser）
2. **discoveryMode** 配置：`off` / `auto` / `all` / `mcp-only`

`search_tool_bm25` 工具基于 BM25 算法（k1=1.2, b=0.75, 字段加权 name×6/label×4/summary×2）对 DiscoverableTool 索引做相关性搜索，命中后动态注入工具集。

### 3.6 错误处理

- **抛 ToolError 而非返回错误文本**：ToolError 可重写 `render()` 自定义 LLM 可见信息
- **ToolAbortError**：由 `throwIfAborted()` 统一包装 signal abort
- **ToolResultBuilder**：链式构造 result，支持 `isError`（非抛出式失败）、`useless`（压缩时可选移除）、`meta`（截断/源信息）
- **coerceToolResult**：边界处强制规范化第三方工具的 malformed 返回值，防止持久化损坏
- agent-loop 的 catch 块兜底所有未捕获异常为 `isError` result

### 3.7 超时处理

`TOOL_TIMEOUTS` 配置表为 bash/eval/browser/ssh/fetch/lsp/debug 各工具定义 default/min/max，`clampTimeout()` 钳制到合法区间。下载类工具用 `combineSignals(signal, timeoutMs)` 将外部 abort 与超时合并。

### 3.8 内置工具清单

| 类别 | 工具 |
|:---|:---|
| 文件与搜索 | `read` `write` `edit` `ast_edit` `ast_grep` `search` `find` |
| 运行时 | `bash` `eval`（Python/JS） `ssh` |
| 代码智能 | `lsp` `debug`（DAP） |
| 协调 | `task`（子 agent） `irc` `todo` `job` `ask` |
| 外部 | `browser` `web_search` `github` `generate_image` `inspect_image` `tts` |
| 记忆与状态 | `checkpoint` `rewind` `retain` `recall` `reflect` |
| 其他 | `resolve`（预览确认） `search_tool_bm25`（工具发现） |

### 3.9 编辑系统（hashline）

`packages/hashline` 是基于 content-hash 的行锚点 patch 语言：
- 每个 file section 以 `[PATH#TAG]` 开头，TAG 是文件内容的 4-hex hash
- 操作：`SWAP` `SWAP.BLK` `DEL` `DEL.BLK` `INS.PRE/POST/HEAD/TAIL` `INS.BLK.POST` `REM` `MV`
- **Stale anchor 拒绝**：patch 前验证文件内容 hash，不匹配则拒绝（防止编辑过期文件损坏代码）
- 支持快照存储 + 3-way merge 恢复
- 抽象 `Filesystem` 接口：InMemory / Node / 任何自定义后端

---

## 4. Provider 抽象层

### 4.1 统一流式接口

所有 provider 发射统一的 `AssistantMessageEvent` 序列：

```
start
→ text_start → text_delta* → text_end       （文本块）
→ thinking_start → thinking_delta* → thinking_end  （推理块）
→ toolcall_start → toolcall_delta* → toolcall_end   （工具调用块）
→ done(reason: "stop"|"length"|"toolUse") | error(reason: "aborted"|"error")
```

`AssistantMessageEventStream` 保证：final result 由终止事件 resolve；事件立即按 push 顺序投递（无 batch/merge）。

### 4.2 Provider 适配器

| API 类型 | Provider |
|:---|:---|
| `anthropic-messages` | Anthropic、Bedrock Claude、Vertex Claude |
| `openai-completions` | OpenAI Chat Completions |
| `openai-responses` | OpenAI Responses API |
| `openai-codex-responses` | OpenAI Codex（WebSocket） |
| `google-generative-ai` | Google Gemini |
| `google-vertex` | Vertex AI |
| `ollama-chat` | Ollama |
| `cursor-agent` | Cursor |
| 其他 | Mistral、Groq、Cerebras、Together、xAI、OpenRouter 等 |

### 4.3 关键能力

- **Thinking/Reasoning 支持**：统一 `Effort` 级别（off/minimal/low/medium/high/xhigh/max），provider 各自映射
- **Tool Choice**：`auto` / `none` / `any` / `required` / `{type:"function",name}` / soft requirement
- **Service Tier**：`flex` / `scale` / `priority`，按 provider family 独立控制
- **Prompt Cache**：`promptCacheKey` / `sessionId` / `cacheRetention`
- **Stream 超时保护**：`streamFirstEventTimeoutMs`（首事件超时）+ `streamIdleTimeoutMs`（事件间空闲超时）
- **Auth Retry**：a/b/c 三 key 轮换策略，`ApiKeyResolver` 动态解析
- **Partial JSON 流式解析**：`parseStreamingJsonThrottled()`，≥256 字节增长才重新解析，O(n²)→O(n)

### 4.4 Model Catalog

`packages/catalog` 维护内嵌的 `models.json`（从 models.dev、provider catalog discovery、OpenCode docs 生成）。包含：
- 模型身份分类（family/version 解析）
- Thinking metadata / generated policies
- Provider descriptors / resolution rules
- 定价信息（premium multipliers、codex pricing fallback）

---

## 5. 会话管理系统

### 5.1 JSONL + 树形分支

**物理层**：每个 session 是一个 `.jsonl` 文件，每行一个 JSON 对象。

**逻辑层**：entries 构成树形结构，通过 `id` + `parentId` 实现 forking/branching。

**Entry 类型**（14 种）：
- `SessionHeader` — 文件首行，含 id/version(v3)/cwd/parentSession
- `message` — 封装 AgentMessage（user/assistant/toolResult/developer）
- `compaction` — 上下文压缩记录
- `branch_summary` — 分支摘要
- `model_change` / `thinking_level_change` / `service_tier_change` — 配置变更审计
- `mode_change` — 模式切换
- `custom` / `custom_message` — 扩展数据（前者不参与 LLM context，后者参与）
- `ttsr_injection` — 时间旅行规则注入记录
- `mcp_tool_selection` — MCP 工具发现选择状态
- `label` — 标签
- `session_init` — 子 agent 初始化上下文快照

### 5.2 持久化策略

**同步 append + 异步 drain**：
- Hot path：文件最新 → 同步 append 一行 JSONL（方法返回时即持久化）
- Cold path：文件不同步 → 同步全量重写
- 原子重写：`commitGuard` 机制防止并发覆盖

**大内容处理**：
- 超过 500,000 字符截断
- 签名块（`thinkingSignature` 等）绝不截断（截断后 provider replay 会 400）
- 大图片外置化到 content-addressed BlobStore（`blob:sha256:<hash>`）

**Title Slot**：文件第一行是固定 256 字节的 title slot，允许原地更新标题而不重写整个文件。

### 5.3 存储后端抽象

统一 `SessionStorage` 接口，四种实现：
1. **FileSessionStorage** — 直接文件操作（默认），temp+rename 原子写
2. **SqlSessionStorage** — PostgreSQL/MySQL/SQLite 三方言
3. **RedisSessionStorage** — Redis STRING + HASH
4. **MemorySessionStorage** — 内存（测试用）

关键设计：内存索引 + 异步写排队。同步操作从内存索引返回，写操作排队异步执行。

### 5.4 会话恢复

加载流程：
1. 流式 JSONL 解析（≥8MB 用字节级流式解析）
2. Title slot 剥离
3. 版本迁移（v1→v2→v3）
4. Blob 引用解析（还原 inline 图片）
5. 过期压缩摘要 elide
6. `buildSessionContext()` 沿 leaf→root 路径遍历，处理 compaction/branch summary

### 5.5 YieldQueue — 后台事件延迟注入

agent 流式输出时，后台完成的事件（async job 结果、LSP 诊断）不能直接注入 LLM context（会破坏 tool_use/tool_result 顺序），所以排队等待。

- `streaming` 模式：逐条注入
- `idle` 模式：批量构建消息注入
- `drainLazy()`：返回 thunk 数组，在注入时刻才执行 staleness 检查

### 5.6 ToolChoiceQueue — 强制工具选择

Generator-based directive 系统，管理 `tool_choice` 参数的强制注入。支持序列、requeue、onInvoked 回调（绕过工具自身 `execute()`）。

---

## 6. 上下文压缩系统

### 6.1 触发条件

| 触发方式 | 说明 |
|:---|:---|
| 手动 | `/compact [instructions]` |
| 溢出恢复 | 上下文溢出错误后自动压缩 |
| 不完整输出恢复 | `stopReason === "length"` 后 |
| 阈值维护 | 成功 turn 后上下文超阈值 |
| Mid-turn 维护 | 工具循环中下一轮 provider 请求前 |
| 空闲维护 | `runIdleCompaction()` 在非流式时 |

### 6.2 压缩策略

**Pre-compaction pruning**（工具输出修剪）：
- 保护最新 40,000 token 的工具输出
- 要求至少 20,000 token 总节省
- 最小修剪 50 token（占位符本身约 8 token，更小的修剪反而增长）
- 保护 skill 工具结果、skill:// 读取、活跃 plan 引用文件

**Useless-result elision**：工具可标记结果为 `useless`（零匹配搜索、超时 job poll），在压缩时用 `[Uneventful result elided]` 替代。

**Cut-point 逻辑**：
- 硬规则：从不在 `toolResult` 处切割
- 有效切点：user/assistant/branchSummary/compactionSummary/custom_message
- Split-turn 处理：切点不在 user-turn 开始时，生成两个摘要（历史摘要 + turn-prefix 摘要）

### 6.3 Snapcompact 策略

将丢弃的历史序列化、whitespace 压缩、打印到 model-aware PNG 帧：
- Claude 读 X.org 8x13 glyph（11px advance，black ink）
- Gemini 读 8x13 glyph（22px pitch）
- GPT/Codex 同 Gemini shape
- 无需 model/API key/网络，安全用于溢出恢复
- 帧持久化在 `CompactionEntry.preserveData.snapcompact`

### 6.4 分支摘要

在 tree 导航时触发（非 token 溢出）：从旧 leaf 到公共祖先的条目被摘要。

### 6.5 Display Transcript

压缩不再视觉上重启对话。TUI 渲染 display transcript：每条路径条目按时间顺序，压缩点显示为分隔线。只有 LLM context 在压缩边界重置，滚动回看保持完整。

---

## 7. 系统提示词构建

### 7.1 架构

**静态 Markdown 模板 + Handlebars 动态渲染 + 并行数据收集**。

```
prompts/
├── system/              # 系统提示词模板（Handlebars .md）
│   ├── system-prompt.md       # 主模板
│   ├── project-prompt.md       # 项目上下文尾部块
│   ├── custom-system-prompt.md # 自定义模式
│   └── personalities/          # 人格预设
├── tools/               # ~50+ 工具描述文件
├── agents/              # 子代理提示词
├── steering/            # 转向/干预提示词
└── ...
```

**关键设计**：提示词以 `.md` 文件而非硬编码字符串存储，通过 Bun 的 `import with { type: "text" }` 编译期内联，无运行时 IO。

### 7.2 构建流程

`buildSystemPrompt(options)`：
1. **并行数据收集**（`Promise.all` + `withDeadline` 超时保护）：
   - 自定义提示词解析
   - 上下文文件发现（CLAUDE.md/AGENTS.md）
   - 技能加载
   - 工作目录树构建
   - 硬件信息收集（CPU/GPU，GPU 探测结果磁盘缓存）
2. **超时降级**：每个步骤有独立超时和 fallback
3. **去重**：字节级 + 段落级语义去重
4. **模板渲染**：Handlebars + post-render 格式化（RFC 2119 归一化、去粗体）

### 7.3 上下文文件发现

通过 Capability 系统多源发现，支持 8 种格式：
- `.omp/AGENTS.md`（native，最高优先级 100）
- `.claude/CLAUDE.md`（priority 80）
- `.codex/AGENTS.md`、`.agent/AGENTS.md`（priority 70）
- `.gemini/GEMINI.md`（priority 60）
- `.config/opencode/AGENTS.md`（priority 55）
- `.github/copilot-instructions.md`（priority 30）
- 独立 `AGENTS.md`（agents-md，priority 10）

**@import 展开**：上下文文件中 `@path/to/file` 内联引入其他文件（递归深度 5，循环检测，代码块中不展开）。

**Sticky rules**：`RULES.md` 被加载为 always-apply rule（附加在当前 turn 附近），而非普通上下文文件。

---

## 8. MCP 集成

### 8.1 传输层

统一 `MCPTransport` 接口，三种实现：

| 传输 | 协议 | 特点 |
|:---|:---|:---|
| **stdio** | JSON-RPC over stdin/stdout | 子进程管理，Windows .cmd/.bat/npm shim 特殊处理 |
| **http** | Streamable HTTP（POST + SSE 响应） | Session ID 管理，SSE 后台监听，401/403 自动 token 刷新 |
| **sse** | 旧版 HTTP+SSE（2024-11-05 协议） | 持久 SSE 连接 + POST endpoint |

### 8.2 MCPManager

- **并行连接**：所有服务器并行连接，250ms 等待启动
- **延迟工具回退**：连接超时但有缓存 → `DeferredMCPTool`（首次调用时才连接）
- **重连断路器**：30 秒窗口内最多 5 次重连
- **工具排序**：按名称稳定排序，保证 prompt caching 字节不变

### 8.3 工具桥接

MCP 工具被桥接为 agent 的 `CustomTool`：
- 命名：`mcp__<serverName>_<toolName>`
- 出站参数净化：移除框架内部字段（`intent`），移除空可选参数，`local://` URL 转文件路径
- 重试：网络级错误触发重连 + 单次重试

### 8.4 工具缓存

SQLite 存储（`agent.db`），key 为 `mcp_tools:<serverName>`：
- 缓存键：config 的 stable JSON + SHA-256 hash
- TTL：30 天
- 用途：启动时快速展示工具列表，不等慢速服务器

### 8.5 OAuth 集成

- 从 401/403 错误自动发现 OAuth 端点
- PKCE 流程 + 动态客户端注册（DCR）
- 本地回调服务器
- Token 刷新持久化

---

## 9. Capability 发现系统

控制反转架构：调用方只需 `loadCapability("mcps")`，provider 负责从各种格式源发现并归一化。

```
Capability<T>           ← 定义"要找什么"
  ├── id, key(item), validate(item)
  └── providers[]        ← 按优先级排序

loadCapability(id)
  1. 构建 LoadContext {cwd, home, repoRoot}
  2. 过滤 providers（disabled/enable/include/exclude）
  3. Promise.all 并行调用所有 provider.load(ctx)
  4. 按 key 去重（first wins = 最高优先级）
  5. validate 过滤
  6. 返回 CapabilityResult {items, all, warnings}
```

已注册能力：`mcps`、`context-files`、`system-prompt`、`skills`、`rules`、`tools`、`instructions`、`extension-modules`、`extensions`。

---

## 10. 子 Agent 与并行任务

### 10.1 task 工具

`task` 工具 fan out 子 agent 到隔离的 worktree，每个 worker 运行自己的工具表面，最终 yield 是 schema-validated 对象。

- 子 agent 运行 headless（`tools.approvalMode: yolo`），不等待 UI 确认
- 父 `task` 的 approval 是授权边界
- 子 agent 通过 `yield` 工具提交结果（增量 section + 终态 result）
- 支持 schema 校验重试（MAX_SCHEMA_RETRIES=3）

### 10.2 IRC

子 agent 间短文本通信（IRC DM），用于并行 worker 间的协调。

### 10.3 Agent Lifecycle

`registry/agent-registry.ts` + `agent-lifecycle.ts` 管理 agent 注册与生命周期。

---

## 11. 其他关键系统

### 11.1 LSP 集成

`lsp/` 目录实现完整的 LSP 客户端：
- diagnostics、navigation、symbols、renames、code actions、raw requests
- 写操作走 `workspace/willRenameFiles`，re-exports/barrel files/aliased imports 自动更新
- 延迟诊断（DeferredDiagnostics）+ diagnostics ledger

### 11.2 DAP 调试器

`dap/` 目录实现 DAP session 管理：
- 断点、单步、线程、栈、变量
- 支持 lldb-dap、delve、debugpy

### 11.3 TTSR（Time-Traveling Stream Rules）

规则在模型偏离脚本时激活：regex 匹配中断流，注入规则为 system reminder，从同一点重试。注入在压缩中存活。

### 11.4 Advisor

第二个模型监视每个 turn，注入内联笔记（concern、blocker）。运行在自己的 context 和 model 上。

### 11.5 Hindsight 记忆

agent 在 session 间记住代码库：`retain` 写入事实，`recall` 检索，`reflect` 综合。项目隔离。

### 11.6 Native 绑定

`crates/pi-natives`（Rust）：ripgrep、glob、find、brush（bash）链接进进程，避免 fork-exec。Windows 上无需 WSL。

---

## 12. 关键设计模式总结

| 模式 | 说明 |
|:---|:---|
| **双层循环** | 外层 follow-up + 内层 tool-call，分离"继续工作"和"处理工具" |
| **Push-based EventStream** | 生产者 push，消费者 async iterate，result() 返回 final promise |
| **中断信号分层** | external > steering > IRC，interruptible 工具才响应 IRC |
| **消费/非消费队列分离** | peek 不消费、dequeue 才消费，保证 abort 后消息不丢失 |
| **coerceToolResult 边界规范化** | untyped 结果进入系统处强制类型安全 |
| **Soft tool requirement** | reminder-then-escalate，避免立即强制 toolChoice 导致 cache invalidation |
| **Pause gate** | 安全点 park 而非 abort，in-flight 工作执行到完成 |
| **Snapshot 不可变性** | 深拷贝流经事件流的所有消息 |
| **Append-only JSONL + 树形分支** | 比 SQLite 更简单、更可恢复、天然支持 branching |
| **同步 append + 异步 drain** | 方法返回时即持久化，批量 flush 优化性能 |
| **StablePrefix + AppendOnlyLog** | 冻结 prefix 以最大化 provider prompt cache 命中率 |
| **Capability 控制反转** | 调用方不依赖具体配置路径，provider 负责多源发现 |
| **模板 + 数据分离** | 提示词 .md 文件 + Handlebars，编译期内联 |
| **断路器模式** | MCP 重连有滑动窗口断路器，防止 fork-bomb |
| **延迟初始化** | DeferredMCPTool 允许在连接完成前注册工具 |
| **渐进降级** | 系统提示词构建每步有超时和 fallback |
