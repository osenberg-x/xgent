# Spec Delta

## Purpose

让 XGent 作为 MCP Client 连接本地 stdio Server，把 Server 暴露的工具自动注册给 Agent，并沿用现有确认、执行与错误处理链路。

## ADDED Requirements

### Requirement: 配置来源与合并

系统 SHALL 从全局 `~/.xgent/agent/mcp.toml` 与项目 `.xgent/mcp.toml` 读取 `[[servers]]`；全局 `allow_project_servers` 默认 `false`，关闭时忽略项目文件。开启后项目同名 server 覆盖全局项，其余按 id 追加；`enabled = false` 的 server 不连接；字符串中的 `${ENV_VAR}` 在连接前展开，变量缺失时跳过该 server 并记录警告。

#### Scenario: 默认忽略项目配置
- **WHEN** 全局 `allow_project_servers` 未设置或为 `false`，项目存在 `.xgent/mcp.toml`
- **THEN** 只连接全局 server，项目文件中的 server 与覆盖均不生效

#### Scenario: 全局与项目合并
- **WHEN** `allow_project_servers = true`，全局与项目都定义 server，其中同名项参数不同
- **THEN** 同名项以项目配置为准，其他全局项保留，最终每个 id 只连接一次

#### Scenario: 环境变量展开
- **WHEN** `env` 或参数中包含 `${TOKEN}` 且环境变量存在
- **THEN** 启动子进程前替换为实际值；变量缺失时该 server 被跳过并记录警告

#### Scenario: 停用与损坏配置
- **WHEN** server 设置 `enabled = false`，或 `mcp.toml` 无法解析
- **THEN** 对应 server 不启动；解析失败不阻止应用启动与其他 server 连接

### Requirement: stdio 启动与初始化

系统 SHALL 为启用的 auto_start server 启动子进程并通过 stdin/stdout 换行分隔 JSON-RPC 通信；启动后发送 `initialize`（含协议版本与 clientInfo），成功后发送 `notifications/initialized`；初始化超时或进程提前退出时关闭该连接并记录错误。

#### Scenario: 成功握手
- **WHEN** server 正常响应 initialize
- **THEN** 客户端发送 initialized 通知，并进入工具发现阶段

#### Scenario: 初始化失败隔离
- **WHEN** 某个 server 启动失败、initialize 超时或进程提前退出
- **THEN** 该连接标记失败，其他 server 与应用启动继续

### Requirement: 工具发现与分页

系统 SHALL 在初始化后调用 `tools/list` 并跟随 `nextCursor`，最多 32 页或 256 个工具（先到者为准）；每个工具必须有 `name` 与对象型 `inputSchema`；缺失或非法项跳过并记录警告；发现结果按稳定顺序返回。

#### Scenario: 分页汇总
- **WHEN** server 将工具分多页返回且总页数不超过 32 页
- **THEN** 客户端汇总所有页，不重复、不丢失

#### Scenario: 非法工具跳过
- **WHEN** 某工具缺少 name 或 inputSchema
- **THEN** 跳过该项，其他合法工具仍被注册

#### Scenario: 数量上限
- **WHEN** server 返回的工具数超过 256 个，或分页超过 32 页
- **THEN** 只保留上限内的稳定结果，并记录警告

### Requirement: 工具注册

系统 SHALL 将发现的工具注册为 `mcp__<server>__<tool>`，向 LLM 暴露工具名称、描述与输入 schema；工具名清洗为 `[A-Za-z0-9_-]`，超长时截断并附加稳定哈希；server id 非法、清洗后为空或完整 id 冲突时跳过并记录警告；注册失败 MUST NOT 影响内置工具。

#### Scenario: 注册后可见
- **WHEN** server 暴露合法工具
- **THEN** 执行器注册对应完整 id，后续请求的工具列表包含其 schema

#### Scenario: 冲突与非法名称
- **WHEN** 两个工具清洗后映射到相同完整 id，或清洗后名称为空
- **THEN** 冲突项被跳过并记录警告，内置工具保持可用

### Requirement: 工具调用

系统 SHALL 把 Agent 的工具调用转为 `tools/call`，参数为 JSON 对象；返回内容中的文本块拼接为工具结果；MCP `isError`、JSON-RPC 错误、超时与断连 MUST 转为可读错误而不 panic；调用被中断时向 server 发送取消通知并返回中断。

#### Scenario: 文本结果
- **WHEN** server 返回一个或多个文本内容块
- **THEN** 客户端按顺序拼接文本作为工具结果

#### Scenario: 逻辑错误
- **WHEN** server 返回 `isError: true` 或 JSON-RPC 错误
- **THEN** 工具结果标记为错误并包含可读原因，Agent 循环继续

#### Scenario: 超时与断连
- **WHEN** 调用超时或 server 已退出
- **THEN** 返回可读错误，不阻塞其他工具调用

#### Scenario: 中断
- **WHEN** 调用过程中收到取消信号
- **THEN** 客户端发送取消通知并返回中断错误

### Requirement: 安全默认

MCP 工具 MUST 使用 `Exec` 层并默认 `NeedsConfirmation`；系统 MUST NOT 因 Server 自报只读标注自动放行；用户 MAY 在 `[tool_policy]` 按完整工具 id 提升为 `Approved`。

#### Scenario: 默认确认
- **WHEN** 用户未配置该 MCP 工具的信任策略
- **THEN** 每次调用都进入确认流程

#### Scenario: 策略提升
- **WHEN** `[tool_policy]` 将某 MCP 工具设为 Approved
- **THEN** 该工具后续调用不再确认

#### Scenario: 只读标注不生效
- **WHEN** server 在工具定义中声明只读
- **THEN** 客户端仍按 Exec 层处理，不改变确认要求

### Requirement: 生命周期与失败隔离

系统 SHALL 在应用退出时终止 MCP 子进程；单个 server 失败只影响其自身工具；MCP Client MUST NOT 写项目文件；连接与工具发现 SHALL 在后台执行，不阻塞 UI 主线程。

#### Scenario: 退出清理
- **WHEN** 应用退出
- **THEN** 所有 MCP 子进程被终止

#### Scenario: 单点失败
- **WHEN** 一个 server 崩溃
- **THEN** 其他 server 的工具仍可调用，应用不退出

#### Scenario: 不写盘
- **WHEN** 客户端连接、发现并调用工具
- **THEN** 客户端自身不修改项目文件
