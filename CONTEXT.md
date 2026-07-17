# XGent

面向个人开发者日常编码的桌面端 AI Code Agent。本 glossary 钉死对话与 provider 配置链路中容易混淆的领域术语，仅记语言，不记实现。

## Language

**Provider**:
配置层指 `GlobalConfig.providers` map 中的一条 `ProviderConfig`（按 id 索引）；运行时层指 daemon `ProviderPool` 中按 id 缓存的 `LlmProvider` trait 实例。同一 id 在两层指同一概念，但"配置存在"与"实例就绪"是不同状态。
_Avoid_: 模型、API、服务

**Provider 就绪**:
某 ProviderConfig 满足发起对话所需的最小字段集。判据按 `ProviderKind` 分：`Ollama` 下 `api_base` 非空即就绪（本地部署通常无 key）；`OpenAiCompat`/`ResponseApi`/`Anthropic`/`Custom` 下 `api_base` 与 `api_key` 均非空才就绪。"配置存在"（map 有该 key）不等于"就绪"。
_Avoid_: 可用、配好了

**default provider**:
`GlobalConfig.default_provider` 指向的全局默认 Provider id。区别于"当前选中 provider"——MVP 无运行时切换 UI，当前选中 = default。命令行 `--provider` 与项目配置 `provider_override` 可覆盖，但覆盖值不写回 default。
_Avoid_: 默认模型、主 provider

**model**:
settings_panel `ModelInput` 填入的模型名，落点为 `ProviderConfig.model_overrides["default"]`（插入式单字段写：设该 provider 的 `model_overrides` map 中键 `"default"` 的值，不覆盖 map 其他条目）。语义是"该 provider 默认用这个模型"，绑定到具体 provider，非全局。区别于 `GlobalConfig.default_model`——后者是"未指定 provider 时的全局兜底模型"，不由 settings_panel 保存路径写入。`ChatRequest.model` 派生时从当前选中 provider 的 `model_overrides["default"]` 取，缺则回退到 `default_model`。
_Avoid_: 默认模型、model id、模型名

**ErrorKind**:
错误分类 enum，变体按"用户可采取的行动"划分，不按底层错误源或 HTTP 状态码划分。MVP 起始变体：`NotConfigured`（闸门拦截：provider 未就绪，引导开 settings_panel）、`AuthFailed`（provider 鉴权失败：API key 错/失效，引导检查 key）、`Network`（连接/超时，可重试）、`StreamParse`（SSE/JSON 解析失败，可重试）、`ProviderError`（provider 返回非鉴权类错误，含原始 message 供排查）。约束：UI 不感知 HTTP 状态码——`ProviderError::Api{status,body}` 的 status 不透给 UI，由 daemon 侧映射到 `AuthFailed`（401/403）或 `ProviderError`（其余）。
_Avoid_: 错误码、错误类型、exception

**最小可用字段集（Provider 配置）**:
settings_panel 收集的 5 个字段：`provider_id`、`kind`、`api_base`、`api_key`、`model`。区别于 `ProviderConfig` 全量 7 字段——`timeout_secs`/`max_retries` 用默认值，UI 不暴露。与「Provider 就绪」判据对齐：这 5 字段足够判定就绪 + 构造 provider 实例。
_Avoid_: provider 表单、配置表单

**Provider 配置（MVP）**:
F-07 在 MVP 阶段的"Provider 切换"边界：用户在 settings_panel 填最小可用字段集（含选 `kind` 下拉），保存后经 daemon 全局配置生效。`kind` 下拉 MVP 暴露 4 变体（`OpenAiCompat`/`ResponseApi`/`Anthropic`/`Ollama`），隐藏 `Custom`——因 `Custom` 需配套的请求模板/header 映射 UI，MVP 未实现，暴露只产半成品配置。MVP 的"自定义 API"由 `OpenAiCompat` + 用户填任意 `api_base` 覆盖（兼容大量第三方接口），真正的 `Custom` provider 留 F-14 或后续。运行时切换 UI（不重启换 provider）MVP 不做，当前选中 = `default_provider`。
_Avoid_: provider 管理、provider 切换面板

## Agent 核心类型（O1-O4 优化后）

**ChatEvent**:
provider 流式输出的事件枚举（跨进程协议类型，UI ↔ daemon）。细粒度变体：`Start`→（`TextStart`/`TextDelta`/`TextEnd` | `ThinkingStart`/`ThinkingDelta`/`ThinkingEnd` | `ToolCallStart`/`ToolCallDelta`/`ToolCallEnd`）*→`Done{reason,usage}`|`Error`。`#[serde(tag="type")]` 使 JSON-RPC notification 按 `type` 字段分发。MVP 不发射 Thinking 事件（OpenAiCompat 不解析 reasoning），变体定义预留给 Anthropic 适配器。
_Avoid_: 流式 chunk、stream event

**StopReason**:
`ChatEvent::Done` 的字段，标记流结束原因：`Stop`/`ToolUse`/`Length`/`Aborted`/`Error`。**agent loop 不依赖 reason 决定是否继续**——`tool_calls.is_empty()` 才决定（对齐 omp）。reason 供 UI 展示与错误恢复参考（如 Length 后是否重试）。
_Avoid_: finish_reason、停止原因

**AgentMessage**:
agent 层消息枚举：`User`/`Assistant`/`ToolResult`/`Notification`。`Notification` 是 UI-only（不发给 LLM）。Conversation 持有 `Vec<AgentMessage>`，调用 LLM 前经 `convert_to_llm` 过滤 UI-only 类型。对齐 omp 的 AgentMessage 设计。
_Avoid_: 消息、message

**ChatMessage**:
LLM 层消息类型（provider 接收的格式）：`{ role: Role, content: Vec<ContentBlock> }`。**结构化**——content 是块数组，非字符串。对齐 Anthropic 协议原生形态；OpenAiCompat 的 `message_to_json` 按 role 展开为 OpenAI 协议形态（assistant+ToolCall→content+tool_calls 字段；Tool→role:tool+content+tool_call_id）。区别于 AgentMessage：ChatMessage 是 LLM 可理解子集，无 UI-only 变体。
_Avoid_: LLM message、provider message

**ContentBlock**:
消息内容块枚举：`Text`/`ToolCall{id,name,args}`/`ToolResult{tool_call_id,content,is_error}`/`Image{data,mime_type}`。ChatMessage.content 与 AssistantMessage.content 共用。MVP 不实现 Image 的 UI 上传，类型定义保留。
_Avoid_: content part、消息块

**ToolTier**:
工具安全分级（静态）：`Read`（读操作无副作用）/`Write`（修改 workspace/session 状态）/`Exec`（执行代码/shell，高危）。`Tool::tier()` 返回静态值，`Tool::approval_for(&input)` 可按参数动态升级（如 RunCommand 检测 `rm -rf` 始终返回 Exec）。区别于 SecurityPolicy——后者是运行时决议结果，由 `resolve_policy` 从 ToolTier + 用户配置推导。
_Avoid_: 工具级别、approval level

**SecurityPolicy**:
工具运行时安全决议结果（非 trait 方法返回值）：`Approved`/`NeedsConfirmation`/`Denied`。由 `resolve_policy(tool_id, tier, input, tool, policy)` 按"配置 denied → 配置 approved → tool.approval_for 动态 tier → MVP 默认全 NeedsConfirmation"顺序推导。MVP 默认全 NeedsConfirmation；P1 引入 ApprovalMode（always-ask/write/yolo）后 Read 在 yolo 下自动批准。
_Avoid_: 工具策略、approval

**Concurrency**:
工具并发模式：`Shared`（可与其他 Shared 工具并行）/`Exclusive`（独占，等前序全部完成）。`Tool::concurrency()` 声明。内置工具：ReadFile/SearchFiles=Shared，WriteFile/RunCommand=Exclusive。
_Avoid_: 并行模式

**ToolError**:
工具执行错误类型：`Failed(String)`/`Aborted`/`Timeout(u64)`。`Tool::execute` 返回 `Result<ToolResult, ToolError>`。语义区分：`Aborted` 让 agent loop 走 abort 路径（停止后续工具）；`Failed`/`Timeout` 走错误回灌路径（错误文本回灌 LLM 让模型自纠）。非 ToolError 的 panic 由 agent loop catch 块兜底为 `is_error:true` 的 ToolResult。
_Avoid_: 工具异常、tool exception

## 编辑器（F-11，P1）

**CodeEditor**:
xui 提供的通用代码编辑器组件（裸件），基于 bevy_ui 自造。中等能力边界：多行编辑 + 行号 + undo/redo + 查找替换 + tree-sitter 语法高亮。不含 LSP、不含 split view。放 xui crate（可独立发布），依赖 tree-sitter + grammar。
_Avoid_: 编辑器、editor widget

**EditorBuffer**:
单个打开文件的可变内存表示：路径、文本内容、光标、选区、undo 栈、脏标记（未保存修改）。一个 EditorBuffer 对应一个编辑器标签页。多标签页 = 多 EditorBuffer。落盘前 buffer 是真相之源；落盘后与磁盘一致。
_Avoid_: 文档、file model

**脏 buffer**:
EditorBuffer 的 `dirty` 标记为真——用户有未保存的本地修改。脏 buffer 检测到外部文件变更时不静默重载，弹窗让用户三选（丢弃本地 / 保留本地 / 对比合并）。未脏 buffer 检测到外部变更时静默重载。这是"未保存修改 vs 外部修改"冲突的协调策略。
_Avoid_: 未保存、modified

**UI-only Tier**:
新增 ToolTier 变体（与 Read/Write/Exec 并列），标记"只修改 UI 元数据不修改 workspace 状态"的动作：打开文件、跳转行、滚动、选区、切换标签页。默认 Approved（不走 NeedsConfirmation）。agent 驱动编辑器动作走此 tier，避免每次跳转都弹确认框。WriteFile 工具仍走 Write tier / NeedsConfirmation。
_Avoid_: 编辑器工具、view action

**EditorCommand**:
agent 驱动编辑器动作的 ECS Event 载荷：`OpenFile{path,line}` / `GoTo{line,col}` / `SetSelection{range}` / `ScrollTo{line}` / `CloseTab{path}`。由 agent 经 UI-only Tier 工具调用发出生，编辑器系统订阅执行。反向（编辑器 → agent）经 EditorState Resource 暴露，由 @ 引用显式拉取，不走 Event。
_Avoid_: 编辑器指令、editor action

**EditorState**:
Bevy Resource，聚合所有打开 EditorBuffer 的只读视图：当前活跃标签、各 buffer 路径/光标/选区/脏标记。agent 不被动注入此状态，仅在用户显式 @ 引用时由 ContextProvider 查询并组装为 ContextChunk。类似 Cursor 的 @ 引用机制。
_Avoid_: 编辑器上下文、editor context

**@ 引用**:
用户在对话输入框用 `@` 前缀显式请求编辑器状态作为上下文的语法。MVP 三种：`@file:<path>` 拉取文件内容；`@cursor` 拉取当前光标位置所在符号/行；`@selection` 拉取当前选区文本。由输入解析器识别，转 ContextQuery 给 ContextProvider。不主动注入，用户控制上下文边界。
_Avoid_: 文件引用、mention

**用户保存**:
用户在编辑器按保存（Cmd+S）的落盘路径：直接 `fs::write`，**不经 WriteFile 工具、不经 NeedsConfirmation 确认**。落盘后通知 daemon 广播 peer.fileChanged 给同项目其他客户端。与 agent 调 WriteFile 工具（走 Write tier / NeedsConfirmation）是两条独立路径——用户主动行为默认信任，agent 行为默认需确认。
_Avoid_: 手动保存、save action

**grammar 分发（D-06）**:
tree-sitter grammar 的打包与加载策略。P1 编辑器 MVP 阶段**只内置 Rust 一种语言 grammar**（随二进制发布），验证编辑器可行性后再扩展。不做按需下载、不做 lazy load。理由：最小可行、离线可用、避免网络/缓存层复杂度。后续多语言需求出现时再评估扩展策略。
_Avoid_: grammar 加载、tree-sitter 打包
