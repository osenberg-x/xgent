confirm-title = Confirm Action
confirm-will-write = will write file:
confirm-diff-label = diff preview:
settings-kind = Provider Type
file-panel-title = Explorer
hint-send = Ctrl+Enter Send
hint-abort = Esc Abort
hint-palette = Ctrl+Shift+P Palette
hint-toggle-sideview = Ctrl+\ Split
hint-terminal = Ctrl+` Terminal
role-user = User
role-assistant = XGent
settings-kind-openai-compat = OpenAI Compatible
settings-kind-response-api = Response API
settings-kind-anthropic = Anthropic
settings-kind-ollama = Ollama
settings-title = Provider Configuration
settings-provider-id = Provider ID (e.g. openai, deepseek)
settings-api-base = API Base URL
settings-api-key = API Key
settings-model = Model name (e.g. gpt-4o-mini)
settings-save = Save
settings-close = Close
confirm-allow = Allow
confirm-deny = Deny
cmd-session-new = New Session
cmd-session-history = Session History
cmd-lang-en = Switch language to English
cmd-lang-zh = Switch language to Chinese
cmd-settings-open = Open Settings
hotkey-palette = Command Palette
hotkey-files = Quick Open File
hotkey-abort = Abort Chat
hotkey-settings = Open Settings
hotkey-toggle-filepanel = Toggle File Panel
hotkey-focus-input = Focus Input
hotkey-editor-view = Switch to Editor View
hotkey-chat-view = Switch to Chat View
hotkey-editor-close-tab = Close Current Tab
hotkey-editor-cycle-tab = Cycle Editor Tabs
hotkey-toggle-sideview = Toggle Side View
topbar-new-session = New Session
status-ready = Ready
status-thinking = Thinking…
status-streaming = Generating…
status-tool-running = Running tool…
status-confirming = Awaiting confirmation
status-aborting = Aborting…
status-error = Error
tool-running = Running
tool-done = Done
tool-failed = Failed
tool-denied = Denied
tool-fold-result = Result: { $lines } lines · Click to fold
tool-unfold-result = Result: { $lines } lines · Click to expand
hotkey-toggle-terminal = Toggle Terminal
terminal-no-tabs = No terminal
terminal-status-created = ◐ Starting · { $shell } · { $cwd }
terminal-status-running = ● Running · { $shell } · { $cwd }
terminal-status-exited = ○ Exited (code={ $code }) · { $shell }
terminal-shell-powershell = powershell
terminal-shell-shell = shell
terminal-new-tab = +
terminal-clear = Clear
terminal-close = x
terminal-prompt = >

# Error prefixes
error-not-configured = ⚠ [Not configured]
error-auth-failed = ⚠ [Auth failed]
error-network = ⚠ [Network]
error-stream-parse = ⚠ [Parse]
error-rate-limited = ⚠ [Rate limited]
error-server = ⚠ [Server error]
error-provider = ⚠
error-retry-hint = Type again to continue

# Retry status
retry-attempt = ⟳ Retrying (attempt { $n })…
retry-attempt-infinite = ⟳ Retrying (attempt { $n }, infinite)…
retry-last-error = Last error: { $error }

# Compaction notice
compaction-notice = ✦ Previous context compacted ({ $before } → { $after } tokens)

# Conversation info

# Status bar
status-encoding = UTF-8 · LF · Rust
status-provider-not-configured = No provider configured

# Editor dialogs
dirty-close-title = Close unsaved tab?
dirty-close-body = { $path } has unsaved changes that will be lost.
dirty-close-discard = Discard
dirty-close-cancel = Cancel
conflict-title = File changed externally
conflict-body = { $path } was modified outside the editor.
conflict-body-dirty = You have unsaved local changes.
conflict-discard = Discard Local
conflict-keep-local = Keep Local
conflict-diff = Compare

# Session history
history-title = Session History
history-empty = No saved sessions
history-close = x
history-message-count = { $count } messages
history-restore = Restore

# v7 状态栏：会话段 / 陪伴开关
status-session = #{ $id } · { $rounds } rounds
status-companion-on = ★ companion on
status-companion-off = ☆ companion off

## v7 qa quick chips (M4-T8)
qa-explain = Explain
qa-explain-prompt = Please explain the logic and design of the following code:
qa-refactor = Refactor
qa-refactor-prompt = Please refactor the following code for better structure and readability:
qa-test = Generate tests
qa-test-prompt = Please generate unit tests for the code in context:
qa-fix = Fix issues
qa-fix-prompt = Please analyze and fix the following issues:
qa-review = Code review
qa-review-prompt = Please review the code in context:

## v7 上下文条 / 欢迎空态（M4-T5/T6）
context-scope-label = Context
context-add = Add context
welcome-title = Start a new session
welcome-sub = Type a question, or start from a quick action
welcome-recent = Recent sessions

toast-copied = Copied to clipboard

diff-empty-nobuffer = No open file
diff-empty-clean = No unsaved changes

# --- undo agent file changes (R3-11) ---
cmd-undo-changes = Undo last turn's file changes
undo-done = Undid file changes of turn { $turn } ({ $count } files)
undo-nothing = No agent file changes to undo
undo-busy = Agent is running; stop it before undoing
undo-unavailable = Undo unavailable: file checkpoints are disabled for this session
undo-detail = Undo details: { $summary }
undo-part-restored = restored { $count }
undo-part-removed = deleted { $count }
undo-part-skipped = skipped { $count }
undo-part-uncovered = without checkpoint { $count }
undo-part-failed = write failed { $count }
undo-skip-external = changed externally
undo-skip-deleted = deleted by user
undo-skip-blob-lost = pre-write content missing
undo-skip-unverified = ownership unverifiable
undo-noop = Nothing to restore in this turn
undo-coverage-note = Note: only write_file and edit_file writes are checkpointed; run_command and plugin writes cannot be undone
undo-part-with-list = { $part }: { $list }
undo-skip-item = { $path } ({ $reason })
undo-list-sep = ,{" "}

# --- settings panel: config visibility and live apply (R3-13) ---
settings-loading = Loading current configuration…
settings-section-provider = Provider
settings-section-tool-policy = Tool trust levels
settings-tool-policy-hint = Every tool asks by default; a dangerous command (such as rm -rf) still asks even when set to auto
settings-policy-auto = Auto
settings-policy-ask = Ask
settings-policy-deny = Deny
settings-section-agent = Agent loop
settings-section-tools = Tool execution
settings-section-context = Context retrieval
settings-section-language = Language
settings-lang-zh-CN = 中文
settings-lang-en-US = English
settings-retrieval-max-tokens = Retrieval token budget per turn
settings-max-parallel-tools = Max parallel tool calls per turn
settings-max-events-per-frame = Max events per frame
settings-confirm-timeout = Confirmation timeout (seconds)
settings-command-timeout = Command timeout (seconds)
settings-command-max-output = Command output cap (bytes)
settings-tree-max-depth = Directory tree depth cap
settings-tree-max-entries = Directory tree entry cap
settings-rg-timeout = Search timeout (seconds)
settings-max-search-files = Max files per search
settings-max-keywords = Max keywords
settings-context-window = Context window (tokens)
settings-max-output-tokens = Max output tokens
settings-first-event-timeout = First-event timeout (seconds)
settings-request-timeout = Request timeout (seconds)
settings-retry-initial-delay = Initial retry delay (ms)
settings-retry-max-delay = Max retry delay (ms)
settings-max-retries = Max retries
settings-field-unset = Not set; leave empty or 0 to restore this default
settings-error-not-a-number = Must be a non-negative integer
settings-error-out-of-range = Cannot exceed { $max }
settings-needs-provider-id = Fill in the Provider ID first
settings-nothing-to-save = Nothing to save
settings-saved-count = Saved { $count } item(s)
# A partial save must name the keys that landed, not just count them
settings-written-keys = written: { $keys }
settings-failed-prefix = These items failed to save
settings-failed-item = { $key }: { $reason }
settings-list-sep = ,{" "}
settings-fetch-models-needs-fields = Provider ID and API Base are required
settings-model-list-empty = (no models)
settings-report-sep ={"; "}
settings-field-anthropic-only = Only Anthropic providers send this field; changing it does not affect requests for other kinds
