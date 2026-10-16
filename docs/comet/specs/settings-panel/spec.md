# Settings Panel

## Purpose

This capability is the user-facing configuration surface: which settings a person can see and change without opening a TOML file, how a change reaches disk, and when it takes effect. It covers the settings panel in `crates/xgent_ui/src/settings_panel.rs`, the daemon-side read and write path it depends on (`config.read` / `config.write` / `config.changed`), and the runtime handles each consumer reads so that a saved value is actually used.

Before this capability existed, four provider fields were the whole surface, the panel could not show what was already configured, and the two configuration files' remaining keys — tool policy, agent caps, tool limits, retrieval limits, provider context limits — were reachable only by hand-editing TOML, with project-scope keys unreachable over IPC at all because the daemon rejected that scope.

## What the panel exposes

| Group | Fields | Where the value lives |
|:---|:---|:---|
| Provider | provider id, kind, api base, api key, model, request timeout, retry settings, `context_window`, `max_output_tokens`, `first_event_timeout_secs` | global `~/.xgent/agent/config.toml`, `providers.<id>.*` |
| Tool policy | one three-state row per registered tool | project `.xgent/config.toml`, `[tool_policy] approved` / `denied` |
| Agent | `retrieval_max_tokens`, `max_parallel_tools`, `max_events_per_frame`, plus the conversation window resolved from the provider | project `[agent]`, global `providers.<id>.context_window` |
| Tools | `confirm_timeout_secs`, `command_timeout_secs`, `command_max_output_bytes` | project `[tools]` |
| Retrieval | `tree_max_depth`, `tree_max_entries`, `rg_timeout_secs`, `max_search_files`, `max_keywords` | project `[context]` |
| Language | current UI language | global `preferences.language` |

Anything not in this table is not editable from the UI: theme, plugin settings, `context_strategy`, `provider_override`, `model_override`, and the free-form maps `extra_headers` / `model_context_windows`. The panel does not render a control for a key whose value would be ignored, and it does not claim coverage it does not have — the tool-policy section lists the tools registered in the `ToolExecutor` at the moment the panel is built, which includes plugin proxy tools, and nothing else.

## Display rules

The panel is a read model, not a memory of the last keystroke. Opening it reads the current configuration and binds every field to what was read; closing and reopening shows the same values, including the four provider fields.

Numeric fields distinguish *set* from *unset* because the configuration semantics do (`0`/absent means "use the built-in default" — the R3-17 rule, and for `max_parallel_tools` specifically `0` means `4`, never "unlimited"). An unset field is rendered with the effective value as its visible number and a hint that says it is not set; the user can leave it empty or type `0` to return to that state, and both write `0`. A field the user has filled shows the stored value, not the default it happens to equal.

A field that holds an unparseable or out-of-range value is reported at that field, in i18n'd text, before any key is written. Validation never rewrites a value to make it valid, and never lets a rejected field silently fall back to the default — the user sees which field was rejected and why, and the previously stored value stays on disk.

The tool-policy rows display the **resolved** answer, computed by the same `resolve_policy` precedence that decides execution (denied → approved with dynamic escalation → `approval_for(input)` → tier default). Consequences that are part of the contract, not incidental:

- A `Read`/`Write`/`Exec` tool with no configuration displays 询问 (`NeedsConfirmation`), which is the shipped default from `AGENTS.md` §5.4.
- A `UiOnly` tool (`editor.*`) displays 自动执行 with no configuration, because `resolve_policy` approves it unconditionally. Hiding that would misrepresent which calls actually run unsupervised.
- The three states are mutually exclusive by construction: choosing 自动执行 removes the id from `denied`, choosing 拒绝 removes it from `approved`, choosing 询问 removes it from both. The panel cannot produce a configuration where an id appears in both lists, which is why `denied`'s precedence never has to be explained to the user.
- Setting `run_command` to 自动执行 does not make a dangerous command quiet: `approval_for` still escalates per-input, so `{"command":"rm -rf /"}` keeps asking. The UI changes the configured default, never the dynamic escalation.

## Saving

One save action writes exactly the fields whose value differs from what was read, and nothing else. Each dirty key is sent as its own `config.write`, in the panel's own order; a `scope=project` request carries the project root the panel belongs to.

- All keys land: a single line of confirmation, and the displayed values are the written values.
- Some keys land, one fails: the report names the keys already written and the key that failed with the daemon's reason, and the panel stays open with the failed field's value still visible. A partial save is not presented as a failure of everything, and the fields that did save are not rolled back (each key is independent by design; the runtime applies exactly what landed).
- Nothing is dirty: no write is issued.
- Validation failed: no write is issued for any key.

The panel closing is a consequence of a completed save, never of an attempted one: pressing 保存 while the required provider id is empty, or while a field is invalid, or when every write failed, leaves the panel open and says why.

## Reaching disk: the daemon is the only writer

Both files are written exclusively by the daemon. `xgent_ui` never touches the filesystem for configuration; it sends ECS messages that `xgent_app` turns into `config.read`/`config.write` calls, which is the shape the provider fields already used.

**Global scope** keeps its existing key set and gains the provider advanced fields `context_window`, `max_output_tokens`, `first_event_timeout_secs` (and `model_context_windows` for reads). An unknown key still answers `未知配置 key`, a wrong type still answers with its field name, and a write under `providers.*` still invalidates the provider pool so that the next request is served by a provider built with the new credentials and limits.

**Project scope** is implemented rather than rejected. `ConfigReadRequest` and `ConfigWriteRequest` accept `scope=project` plus a project root; the daemon holds a per-root authoritative `ProjectConfig`, loading `<root>/.xgent/config.toml` on first use and saving it atomically through `ProjectConfigStore`. The accepted key grammar is the dotted form of the sections named in the table above (`tool_policy.approved`, `agent.max_parallel_tools`, `tools.confirm_timeout_secs`, `context.tree_max_depth`, …), the same shape as global keys. A project write for a root the client has no relationship to, or an absent root, is an explicit error — never a silent fallthrough to global scope, because the two scopes do not share field semantics and a silent downgrade would tell the user their project setting was saved when the global one changed.

`ConfigChanged` gains the project root alongside scope and key, and is delivered to the clients that subscribed to that project. Global changes keep their existing broadcast-to-all behaviour. Old clients that send a request without the new optional fields continue to get exactly today's behaviour.

## Taking effect

A saved value is read through a shared handle at the point where it is used, so no consumer caches a stale copy across a change. The mechanism is the one the codebase already uses for `retry_config` and `loop_bounds`: the consumer holds `Arc<RwLock<T>>`, the settings layer writes it, and the code path reads it at the moment of decision.

| What changed | Effective from | Read at |
|:---|:---|:---|
| Tool policy (`approved` / `denied`) | the next tool call | `ToolExecutor::execute` → `resolve_policy`, reading the policy out of `ToolCtx` |
| `[agent] retrieval_max_tokens`, `max_parallel_tools`, and the conversation `context_window` | the next user message | the agent loop's run-settings handle, read when a loop iteration starts |
| `[agent] max_events_per_frame` | the next frame drain | the bridge's shared cap, read by the ECS drain |
| `[tools] confirm_timeout_secs` | the next confirmation request | the executor's shared `ToolParams` |
| `[tools] command_timeout_secs`, `command_max_output_bytes` | the next `run_command` call | `RunCommand`, reading the shared `ToolParams` |
| `[context]` limits | the next retrieval | `OnDemandProvider`'s shared `RetrievalLimits` |
| `providers.<id>.api_base` / `api_key` / `kind` / `timeout_secs` / `max_output_tokens` / `first_event_timeout_secs` / retry fields | the next provider request | the daemon's provider pool, invalidated on `providers.*` writes as before |
| `preferences.language` | immediately, in every window | the existing `Localizer::switch` path |

One apply routine handles both cases — the window that wrote the value and a window that learned about it from `config.changed`. A setting change in one window therefore produces the same displayed state *and* the same runtime behaviour in every other window on that project, without either window restarting.

What stays startup-only is stated rather than implied: the four groups above are the whole live-apply set, and keys outside them (`context_strategy`, plugin settings, theme) keep their current startup reading. The panel offers no control for them, so there is no user-visible claim to correct.

## Failure and degradation

- **Daemon unreachable or write error**: the value is not shown as saved, the failure reason is displayed, and no runtime handle is touched. A failed write must never leave the running configuration differing from what the user sees on screen — the handles are updated only from the values the daemon accepted.
- **A key the daemon rejects** (unknown name, wrong type): that key's field keeps its previous displayed value and the reason is surfaced; other keys in the same save are unaffected and reported as written.
- **Corrupt project TOML**: the daemon's read for that root returns an error naming the file, and the panel reports it instead of showing an empty configuration. A project whose `.xgent/config.toml` cannot be parsed is a visible problem, never "all defaults" presented as fact.
- **Concurrent edits from two windows**: last write wins per key, as before this capability. Each window's display and runtime converge on the value that was actually stored, so no window keeps believing its own write survived; a real merge policy is out of scope.

## Relationship to other capabilities

- **Runtime parameters** (`docs/comet/specs/runtime-parameters/spec.md`): that capability moved the numbers out of constants into configuration and defined `0 = unset`. This capability is the surface and the propagation — it changes no default, no fallback, and no unit.
- **Tool concurrency** (`tool-concurrency`): `max_parallel_tools` becomes changeable at runtime, so a group's cap is read per loop iteration instead of frozen at bridge construction. Scheduling rules are untouched.
- **Agent safety gaps** (`agent-safety-gaps`): confirmation timeouts and the dynamic escalation branch are now user-reachable. Making a timeout shorter is allowed; nothing in the UI may make a call that resolves to `NeedsConfirmation` behave as `Approved`.
- **File checkpoint undo** (`file-checkpoint-undo`): the retention count stays a constant, so the panel offers no control for it and does not imply one exists.

## Non-goals of this capability

Theme switching, plugin enable/disable and plugin custom settings, retrieval-strategy selection, provider/model override selection, keychain migration of `api_key`, table editors for free-form maps, search/filter inside the settings panel, import/export of configuration, and a merge policy for concurrent edits.
