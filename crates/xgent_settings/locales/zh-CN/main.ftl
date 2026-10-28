confirm-title = 确认执行
confirm-will-write = 将写入文件：
confirm-diff-label = diff 预览：
confirm-allow = 允许执行
file-panel-title = 资源管理器

# 面板标题（v8 M5：槽位注册表 —— 描述符的 title_key 指向这些 key）
panel-chat = 对话
panel-file-drawer = 文件
panel-session-history = 历史会话
panel-terminal = 终端
panel-editor = 预览
panel-diff = 差异
hint-send = Ctrl+Enter 发送
hint-abort = Esc 中断
hint-palette = Ctrl+Shift+P 命令
hint-toggle-sideview = Ctrl+\ 分屏
hint-terminal = Ctrl+` 终端
role-user = 用户
role-assistant = XGent
confirm-deny = 拒绝
cmd-session-new = 新建会话
cmd-session-history = 历史会话
cmd-lang-en = 切换语言为英文
cmd-lang-zh = 切换语言为中文
cmd-settings-open = 打开设置
hotkey-palette = 命令面板
hotkey-files = 快速打开文件
hotkey-abort = 中断对话
hotkey-settings = 打开设置
hotkey-toggle-filepanel = 切换文件面板
hotkey-focus-input = 聚焦输入框
hotkey-editor-view = 切换到编辑器视图
hotkey-chat-view = 切换回对话视图
hotkey-editor-close-tab = 关闭当前标签
hotkey-editor-cycle-tab = 循环切换编辑器标签
hotkey-toggle-sideview = 切换右侧分屏
topbar-new-session = 新建会话
settings-title = Provider 配置
settings-provider-id = Provider ID（如 openai、deepseek）
settings-api-base = API Base URL
settings-api-key = API Key
settings-model = 模型名（如 gpt-4o-mini）
settings-kind = Provider 类型
settings-kind-openai-compat = OpenAI 兼容
settings-kind-response-api = Response API
settings-kind-anthropic = Anthropic
settings-kind-ollama = Ollama
settings-save = 保存
settings-close = 关闭
status-ready = 就绪
status-thinking = 思考中…
status-streaming = 生成中…
status-tool-running = 执行工具…
status-confirming = 等待确认
status-aborting = 中断中…
status-error = 出错
tool-running = 执行中
tool-done = 完成
tool-failed = 失败
tool-denied = 已拒绝
tool-fold-result = 结果：{ $lines } 行 · 点击折叠
tool-unfold-result = 结果：{ $lines } 行 · 点击展开
hotkey-toggle-terminal = 切换终端
terminal-no-tabs = 无终端
terminal-status-created = ◐ 启动中 · { $shell } · { $cwd }
terminal-status-running = ● 运行中 · { $shell } · { $cwd }
terminal-status-exited = ○ 已退出 (code={ $code }) · { $shell }
terminal-shell-powershell = powershell
terminal-shell-shell = shell
terminal-new-tab = ＋
terminal-clear = 清屏
terminal-close = ✕
terminal-prompt = ❯


# 错误前缀
error-not-configured = ⚠ [未配置]
error-auth-failed = ⚠ [鉴权失败]
error-network = ⚠ [网络]
error-stream-parse = ⚠ [解析]
error-rate-limited = ⚠ [限流]
error-server = ⚠ [服务端错误]
error-provider = ⚠
error-retry-hint = 重新输入可继续对话

# 重试状态
retry-attempt = ⟳ 重试中（第 { $n } 次）…
retry-attempt-infinite = ⟳ 重试中（第 { $n } 次，无限重试）…
retry-last-error = 上次失败：{ $error }

# 压缩提示
compaction-notice = ✦ 前序对话已摘要压缩（{ $before } → { $after } tokens）

# 会话信息

# 状态栏
status-encoding = UTF-8 · LF · Rust
status-provider-not-configured = 未配置 provider

# 编辑器弹窗
dirty-close-title = 关闭未保存的标签？
dirty-close-body = { $path } 有未保存的修改，关闭将丢失。
dirty-close-discard = 丢弃修改
dirty-close-cancel = 取消
conflict-title = 文件已被外部修改
conflict-body = { $path } 在编辑器外被修改。
conflict-body-dirty = 你有未保存的本地修改。
conflict-discard = 丢弃本地
conflict-keep-local = 保留本地
conflict-diff = 对比合并

# 会话历史
history-title = 历史会话
history-empty = 暂无历史会话
history-close = ✕
history-message-count = { $count } 条消息
history-restore = 恢复

# v7 状态栏：会话段 / 陪伴开关
status-session = #{ $id } · { $rounds } 轮
status-companion-on = ★ 陪伴已开启
status-companion-off = ☆ 陪伴已关闭

## v7 qa 快捷 chips（M4-T8）
qa-explain = 解释代码
qa-explain-prompt = 请解释以下代码的逻辑和设计：
qa-refactor = 重构
qa-refactor-prompt = 请重构以下代码，改善结构和可读性：
qa-test = 生成测试
qa-test-prompt = 请为当前上下文中的代码生成单元测试：
qa-fix = 修复问题
qa-fix-prompt = 请分析并修复以下问题：
qa-review = 代码审查
qa-review-prompt = 请对当前上下文中的代码进行审查：

## v7 上下文条 / 欢迎空态（M4-T5/T6）
context-scope-label = 上下文
context-add = 添加上下文
welcome-title = 开始新会话
welcome-sub = 输入问题，或从快捷操作开始
welcome-recent = 最近会话

toast-copied = 已复制到剪贴板

diff-empty-nobuffer = 无打开文件
diff-empty-clean = 无未保存更改

# --- agent 文件改动撤销（R3-11）---
cmd-undo-changes = 撤销上一轮文件改动
undo-done = 已撤销第 { $turn } 轮的文件改动（{ $count } 个文件）
undo-nothing = 没有可撤销的 agent 文件改动
undo-busy = agent 正在执行，先停止后再撤销
undo-unavailable = 撤销不可用：本会话未启用文件检查点
undo-detail = 撤销详情：{ $summary }
undo-part-restored = 恢复 { $count } 个
undo-part-removed = 删除 { $count } 个
undo-part-skipped = 跳过 { $count } 个
undo-part-uncovered = 无检查点 { $count } 个
undo-part-failed = 写盘失败 { $count } 个
undo-skip-external = 已被外部修改
undo-skip-deleted = 已被删除
undo-skip-blob-lost = 改前内容丢失
undo-skip-unverified = 无法确认归属
undo-noop = 本轮没有需要恢复的改动
undo-coverage-note = 注：仅 write_file / edit_file 的写入有检查点，run_command 与插件写文件不在撤销范围内
undo-part-with-list = { $part }：{ $list }
undo-skip-item = { $path }（{ $reason }）
undo-list-sep = 、

# --- 设置面板：配置可见性与热应用（R3-13）---
settings-loading = 正在读取当前配置……
settings-section-provider = 模型服务
settings-section-tool-policy = 工具信任级别
settings-tool-policy-hint = 默认每个工具都要确认；危险命令（如 rm -rf）即使设为自动执行也仍会询问
settings-policy-auto = 自动执行
settings-policy-ask = 询问
settings-policy-deny = 拒绝
settings-section-agent = 对话循环
settings-section-tools = 工具执行
settings-section-context = 上下文检索
settings-section-language = 语言
settings-section-density = 密度
settings-density-comfortable = 舒适
settings-density-compact = 紧凑
settings-lang-zh-CN = 中文
settings-lang-en-US = English
settings-retrieval-max-tokens = 每轮检索 token 预算
settings-max-parallel-tools = 同轮并发工具调用上限
settings-max-events-per-frame = 每帧事件处理上限
settings-confirm-timeout = 确认等待秒数
settings-command-timeout = 命令执行秒数上限
settings-command-max-output = 命令输出上限（字节）
settings-tree-max-depth = 目录树深度上限
settings-tree-max-entries = 目录树条目上限
settings-rg-timeout = 搜索超时秒数
settings-max-search-files = 搜索返回文件数上限
settings-max-keywords = 关键词数量上限
settings-context-window = 上下文窗口（token）
settings-max-output-tokens = 单次输出上限（token）
settings-first-event-timeout = 首事件等待秒数
settings-request-timeout = 请求超时秒数
settings-retry-initial-delay = 重试首次等待（毫秒）
settings-retry-max-delay = 重试最大等待（毫秒）
settings-max-retries = 重试次数上限
settings-field-unset = 未设置；留空或填 0 即恢复此缺省
settings-error-not-a-number = 只能填非负整数
settings-error-out-of-range = 不能超过 { $max }
settings-needs-provider-id = 请先填写 Provider ID
settings-nothing-to-save = 没有需要保存的改动
settings-saved-count = 已保存 { $count } 项
# 部分失败时要点名已落盘的键（只报个数用户看不出哪个进去了）
settings-written-keys = 已写入：{ $keys }
settings-failed-prefix = 以下项写入失败
settings-failed-item = { $key }：{ $reason }
settings-list-sep = 、
settings-fetch-models-needs-fields = 需要填写 Provider ID 和 API Base
settings-model-list-empty = （无模型）
settings-report-sep = ；
settings-field-anthropic-only = 仅 Anthropic provider 发送该字段；其他类型下改动不影响请求

# --- v8 M7-T3：硬编码 tooltip 与错误文案迁移 ---
tooltip-expand-panel = 展开面板
tooltip-command-palette = 命令面板
tooltip-settings = 设置
editor-read-cancelled = 读取任务取消
