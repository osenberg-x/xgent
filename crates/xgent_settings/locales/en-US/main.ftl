app-title = XGent
welcome = Welcome
chat-placeholder = Type a message and press Ctrl+Enter to send
chat-empty = Start a conversation
confirm-write-file = Confirm write file { $path }?
confirm-run-command = Confirm run command: { $cmd }
confirm-title = Confirm Action
confirm-will-write = will write file:
confirm-diff-label = diff preview:
provider-not-configured = No provider configured
settings-saved = Settings saved
file-panel-placeholder = Files
settings-kind = Provider Type
chat-tab-label = Chat
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
file-panel-empty = Open a project to see the file tree
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
editor-back-to-chat = ← Back to Chat
topbar-new-session = New Session
topbar-settings = Settings
status-ready = Ready
status-thinking = Thinking…
status-streaming = Generating…
status-tool-running = Running tool…
status-confirming = Awaiting confirmation
status-aborting = Aborting…
status-error = Error
tool-pending = Pending
tool-running = Running
tool-done = Done
tool-failed = Failed
tool-denied = Denied
tool-result = Result
tool-expand = Click to expand
tool-fold-result = Result: { $lines } lines · Click to fold
tool-unfold-result = Result: { $lines } lines · Click to expand
palette-placeholder = Type a command...
hotkey-toggle-terminal = Toggle Terminal
terminal-title = Terminal
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
preview-loading = · Loading...
preview-read-error = Read failed: { $error }
preview-bytes = · { $bytes } bytes · Read-only preview
preview-error = · { $error }

# Error prefixes
error-not-configured = ⚠ [Not configured]
error-auth-failed = ⚠ [Auth failed]
error-network = ⚠ [Network]
error-stream-parse = ⚠ [Parse]
error-provider = ⚠
error-retry-hint = Type again to continue

# Retry status
retry-attempt = ⟳ Retrying (attempt { $n })…
retry-attempt-infinite = ⟳ Retrying (attempt { $n }, infinite)…
retry-last-error = Last error: { $error }

# Compaction notice
compaction-notice = ✦ Previous context compacted ({ $before } → { $after } tokens)

# Conversation info
conversation-info = Session #{ $id } · { $turns } turns{ $tokens }
conversation-tokens = · ↑ { $tokens } tokens

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
history-loading = Loading...
history-close = x
history-message-count = { $count } messages
history-restore = Restore
history-restore-failed = Restore failed: session file missing or corrupted
