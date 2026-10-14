# Outcome

Close the two entries that `doc/notes/agent-gap-disposition.md` §六 leaves as the remaining unblocked R3 work: `search_files` can only do substring matching (R3-3), and ten load-bearing runtime parameters are compile-time constants with no configuration entry point (R3-17).

After this change lands:

1. `search_files` supports regular-expression matching, case-insensitive matching, glob path filtering and named file-type filtering — and reports a malformed pattern as an error instead of quietly returning "no matches".
2. Every one of the ten parameters in the R3-17 table can be set in `config.toml` (global provider section) or `.xgent/config.toml` (project sections), and a project that sets nothing behaves exactly as it does today, because each new key's default is the constant it replaces.

# Scope

## Sources and their role

`doc/notes/agent-gap-review.md` is triage input and `doc/notes/agent-gap-disposition.md` §六 is the handoff that names this round's two entries. Neither is a requirements source in the full-coverage sense: the target is exactly the two ledger rows R3-3 and R3-17, and every other row of the report is already classified in the disposition ledger (R3-2 blocked, R3-4 … R3-21 and R4 deferred). Neither report is edited by this change; the disposition ledger gains a section recording the outcome, as in the previous rounds.

R3-17 names ten parameters, so each is mapped explicitly:

| Parameter (current constant) | Location today | Spec section | Acceptance |
| --- | --- | --- | --- |
| context window `128_000` | `crates/xgent_app/src/main.rs` | runtime-parameters → Context window | A6 |
| retrieval token budget `8_000` | `crates/xgent_agent/src/bridge.rs` | runtime-parameters → Agent sections | A7 |
| agent events drained per frame `64` | `crates/xgent_agent/src/agent_loop.rs` | runtime-parameters → Agent sections | A7 |
| Anthropic `max_tokens` `8192` | `crates/xgent_provider/src/anthropic.rs` | runtime-parameters → Provider sections | A9 |
| first-event timeout `30s` | `openai_compat.rs`, `anthropic.rs` | runtime-parameters → Provider sections | A9 |
| tool confirmation timeout `300s` | `crates/xgent_tools/src/executor.rs` | runtime-parameters → Tool sections | A8 |
| command timeout `60s` | `crates/xgent_tools/src/builtins/run_command.rs` | runtime-parameters → Tool sections | A8 |
| command output cap `64KB` | same file | runtime-parameters → Tool sections | A8 |
| traversal `TREE_MAX_DEPTH 4`, `TREE_MAX_ENTRIES 200`, `RG_TIMEOUT_SECS 10`, `MAX_SEARCH_FILES 20`, `MAX_KEYWORDS 16` | `crates/xgent_context/src/on_demand.rs` | runtime-parameters → Retrieval sections | A10 |

R3-3 names one capability with four missing features (regex, glob filter, case switch, file-type filter); each maps to the `code-search` spec and to A2 … A5.

## In scope

| Gap | Change |
| --- | --- |
| R3-3 | `search_files` gains `regex`, `case_insensitive`, `glob`, `file_type` arguments, with a matcher that treats a bad pattern as a tool error |
| R3-3 | The tool's `ToolSchema` description teaches a model how to use the new arguments |
| R3-17 | `ProviderConfig` gains `context_window`, `model_context_windows`, `max_output_tokens`, `first_event_timeout_secs`; adapters and `xgent_app` read them |
| R3-17 | `ProjectConfig` gains `[agent]`, `[tools]`, `[context]` sections carrying the remaining six parameters, injected through `AgentBridgeConfig`, `default_tools`, `ToolExecutor`, `OnDemandContextProvider` |
| R3-17 | Every default equals the constant it replaces, asserted by serde tests |
| both | Tests in each touched crate, each checked to fail without the change (counter-proof method already used in the two previous rounds) |

## Out of scope but adjacent

- The settings panel UI for any of these keys — R3-13 stays open, these are TOML-only.
- The editor's find/replace and `@selection` reference — R3-21, R3-20. During Shape it was found that R3-21 understates itself: `xui::FindState` has no consumer in `xgent_ui` at all, so the editor's find/replace is not merely literal, it is unreachable. Recorded here so the next round can price the real work.
- Search result presentation (context lines around a hit, per-file grouping, multiline regex over whole files).

# Non-goals

- No new retrieval providers, and no change to `repo_map`, `vector`, `lsp`, `hybrid` (R3-2, blocked on embedding/vector-store selection and D-06).
- No automatic context-window discovery. `LlmProvider::list_models` already returns `ModelInfo.context_window`, but probing it at startup adds a network round trip and a failure mode to launch; the window stays explicit configuration with a documented fallback.
- No new dependency beyond `regex`, which is already in `Cargo.lock` (1.13.0) as a transitive dependency. No `globset`, no `walkdir`, no `ignore`, no tokenization library.
- No change to the security model: `search_files` keeps `ToolTier::Read` and `Concurrency::Shared`; the default "every tool needs confirmation" policy is untouched.
- No change to `MAX_TOOL_OUTPUT_BYTES` (the shared 64KB truncation ceiling for all read-type tool output in `xgent_tools::truncate_output`). Only `run_command`'s own capture cap becomes configurable.
- No JSON-RPC protocol change, no new daemon method. The daemon builds providers through the same `build_provider`, so provider-level keys apply there without new wiring.
- No config migration tooling and no validation dialog: absent keys mean defaults.
- No rewrite of the traversal/search logic in `xgent_context` beyond making its five limits injectable; the CJK segmentation, `@file:` handling, literal `rg` invocation and `spawn_blocking` structure delivered by the previous round stay as they are.
- No edit of `doc/notes/agent-gap-review.md`.

# Constraints and invariants

- **Defaults are behavior-preserving.** For all fourteen new configuration values the default equals the constant being replaced (`128_000`, `8_000`, `64`, `8192`, `30`, `300`, `60`, `65536`, `4`, `200`, `10`, `20`, `16`). A repository with an existing `config.toml` that never mentions the new keys must behave bit-for-bit as before, including which chunks fit a budget and when a long-running command is killed.
- **Absent opt-in flags keep the old semantics.** `search_files` with only `pattern` (and with `path`) matches literally and case-sensitively, in the same output format `path:line: text`, with the same ignored directories, the same 1MB per-file skip, the same `MAX_RESULTS`/`MAX_DEPTH` bounds, the same cancellation boundaries and the same truncation notice.
- A malformed pattern is a failure, not an empty result: `is_error = true`, the message names the offending argument, and no directory traversal happens.
- Regex is matched per line, not across lines, and the reported match is the line that matched — the tool's output shape is unchanged.
- Every new numeric configuration key treats `0` as "unset, use the built-in default"; "no limit at all" is not expressible, which is deliberate for cost- and stall-related bounds.
- `xgent_settings_core` stays Bevy-free; `xgent_provider` stays Bevy-free; no crate gains a dependency on another xgent crate beyond the existing graph.
- Chinese doc comments and commit messages, `thiserror` for new error types, no `unwrap()`/`expect()` outside tests (`AGENTS.md` §5.7, §5.8).
- `cargo fmt -p <crate>` only — `cargo fmt --all` rewrites the `../bevy` upstream checkout.
- New behavior is covered by tests, and every new test is checked to fail without the fix; a mutant that changes no observable behavior is reported as behavior-equivalent rather than papered over with a new assertion.

# Decisions

- **D-S1 — use the `regex` crate rather than an in-crate engine.** `regex 1.13.0` is already resolved in `Cargo.lock` through bevy, so a direct workspace dependency costs no new compilation, and a hand-written subset of regex would be exactly the kind of silently-wrong matcher this tool exists to avoid. Declared once in `[workspace.dependencies]`.
- **D-S2 — regex is opt-in per call (`regex: false` by default).** All existing callers and the model's habitual `"pattern": "foo"` calls keep literal semantics; turning `(` into an error by default would break working conversations for no user benefit. This also keeps the previous round's fix (`rg --fixed-strings` for keywords in `xgent_context`) unentangled.
- **D-S3 — glob is translated in-crate to a path matcher** over `/`-separated, project-relative paths, supporting `*` (no separator), `**` (any), `?` (one character) and `{a,b}` alternation; a pattern containing no `/` is matched against the file name only, which is ripgrep's `--glob` convention and what `"*.rs"` means to a user. This avoids `globset` for four constructs and keeps the rules the tool documents itself.
- **D-S4 — a bad `regex`, a bad `glob` or an unknown `file_type` returns `is_error` and stops before traversal.** The current defect is precisely that an unusable search looks like a successful empty one; a model that gets "未找到匹配" from a typo'd pattern will conclude the code is absent and answer wrong.
- **D-S5 — `file_type` takes a named type (`rust`, `python`, `go`, …) mapped to an extension set**, not a bare extension. It mirrors ripgrep's `--type`, the vocabulary a model already knows, and one name can cover `h`/`hpp` for `cpp`.
- **D-S6 — provider-scoped parameters go on `ProviderConfig`, process-scoped ones in new `ProjectConfig` sections.** `context_window`, `max_output_tokens` and `first_event_timeout_secs` are properties of an endpoint and already travel to both the daemon and the UI through `ProviderConfig`; retrieval budget, tool timeouts and traversal limits are per-repository working conditions and belong where `tool_policy` and `context_strategy` already live.
- **D-S7 — `0` means "use the built-in default".** `Option<u32>` would force every consumer to handle `None`, and TOML `null` is already an awkward way to express "unlimited" for `max_retries`. Keeping one numeric type with one sentinel is the smaller contract; the trade-off is that "unlimited" is not expressible, which is acceptable because each of these bounds exists to stop runaway cost or a stalled frame.
- **D-S8 — context window resolution precedence is per-model map > provider value > `128_000` with one warning on stderr at startup.** The warning is the point: the fallback silently mis-sizes compaction today, and the whole reason this parameter is in R3-17 is that a wrong window either never triggers compaction or triggers it far too early.
- **D-S9 — the per-frame event cap is a field on `AgentBridge`** read by the ECS drain system, because the bridge is already the carrier of loop configuration; no new Bevy `Resource` for one integer.
- **D-S10 — new `with_config`-style constructors on the provider adapters, and a parameters struct for tools and the retrieval provider, leaving existing constructor signatures intact.** `with_timeout` and `new` are used by daemon code and tests; adding a struct argument to those would churn call sites that have nothing to do with this change.
- **D-S11 — `xgent_tools`' tool list becomes `default_tools(&ToolParams)`.** `run_command` needs two per-tool values, so the unit-struct `RunCommand` gains fields; the single construction function in `xgent_tools::lib` plus its call sites in `xgent_app` and tests are updated, rather than giving tools a global.

# Acceptance examples

- A1: `search_files` called with only `pattern` returns the same matches, the same `path:line: text` format, the same ignored-directory behavior, the same "未找到匹配" text and the same truncation notice as before this change; the pre-existing `search_files` tests pass unmodified.
- A2: with `"regex": true`, `"pattern": "^fn\\s+\\w+"` matches lines by regular expression. With `"regex": true` and an invalid pattern such as `"(foo"`, the result is `is_error`, its output names the `pattern` argument and the reason, and no file in the project is opened or traversed — in particular it does not return "未找到匹配".
- A3: `"case_insensitive": true` matches `FOO`, `foo` and `Foo` in both literal mode and regex mode, and with the flag absent (default) matching stays case-sensitive.
- A4: `"glob": "*.rs"` searches only `.rs` files at any depth, `"glob": "src/**/*.rs"` searches only `.rs` files under `src/`, a pattern with unbalanced braces is reported as `is_error` naming the `glob` argument with no traversal, and a file whose path does not match the glob contributes no line even when its content matches.
- A5: `"file_type": "rust"` restricts the search to the extension set behind `rust`, `"file_type": "unknown_kind"` returns `is_error` listing the supported type names, and `file_type` combined with `glob` requires both to match.
- A6: with `[providers.openai] context_window = 32768` and no per-model entry, the agent's compaction threshold is computed from 32768; adding `[providers.openai.model_context_windows] "gpt-4o" = 128000` while running model `gpt-4o` makes the agent use 128000 instead; with neither set, the agent uses 128000 and prints one startup warning naming the fallback.
- A7: setting `[agent] retrieval_max_tokens = 2000` makes the context query the agent issues carry `max_tokens = 2000`, and setting `[agent] max_events_per_frame = 8` makes the agent loop drain at most 8 events in one frame; with the section absent both use 8000 and 64, and no literal `64` remains in the drain loop's bound.
- A8: setting `[tools] command_timeout_secs = 5` kills a command that outlives 5 seconds, `[tools] command_max_output_bytes = 1024` truncates captured output to at most that many bytes, `[tools] confirm_timeout_secs = 2` times a pending confirmation out after 2 seconds; with `[tools]` absent the values are 60, 65536 and 300.
- A9: setting `[providers.anthropic] max_output_tokens = 4096` puts `max_tokens: 4096` in the Anthropic request body, and `[providers.x] first_event_timeout_secs = 1` makes the first-event wait use one second in both the OpenAI-compatible and the Anthropic adapter; absent, the body carries 8192 and the wait is 30 seconds.
- A10: setting `[context] tree_max_depth = 2`, `tree_max_entries = 5`, `rg_timeout_secs = 1`, `max_search_files = 1`, `max_keywords = 2` changes the built-in retrieval provider's behavior accordingly (shallower and smaller tree, fewer searched files, at most two keywords); absent, the limits are 4, 200, 10, 20 and 16 — the constants they replace.
- A11: `cargo test -p xgent_settings_core -p xgent_provider -p xgent_tools -p xgent_context -p xgent_agent`, `cargo test -p xgent_ui` and `cargo check --workspace --all-targets` pass, so the new configuration types and the widened tool and bridge constructors did not break `xgent_app`, `xgent_daemon` or `xgent_plugin_host`.

# Verification expectations

- Formal checks stay cheap and deterministic: per-crate `cargo test` for the touched crates, `cargo check --workspace --all-targets` (after it has been warmed), and `cargo fmt -p <crate> -- --check` for each touched crate. `cargo clippy --workspace` and `cargo test --workspace` are not in the formal plan — each links the whole bevy graph and exceeds the 10-minute command ceiling on this machine; they are run once by the Builder as development checks and reported with their real result.
- The behavior-preserving invariant (A7 … A10 "absent → current value") is asserted by serde tests that deserialize a config document containing none of the new keys, not only by reading the `default` functions.
- Counter-proof: for A2, A4, A5, A6, A7, A8, A9 and A10 at least one mutant each is attempted (revert the fix to the previous defect form, confirm the intended test fails, restore). A mutant that no test distinguishes is reported as behavior-equivalent with the reason, and a test is added only when the mutation genuinely changes behavior.
- A2's "no traversal happens" claim must be shown by an observable (for example a temp project whose file mtimes/counts prove nothing was read, or a check that the error returns before the walk is entered), not asserted from the message text alone.
