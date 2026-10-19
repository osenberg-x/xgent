# Runtime Parameters

## Purpose

This capability is the set of numeric limits that decide how much context the agent asks for, when a conversation is compacted, how long a command may run, how long a confirmation may wait, how wide a directory scan may go, and how long a provider may take to produce its first token. Until now every one of them was a constant compiled into a crate, so a user on an 8k-token model never triggered compaction while a user on a 1M-token model was compacted far too early, and no setting could change either outcome.

After this change the values live in configuration, the same code paths read them, and a configuration file that never mentions them produces exactly the numbers the constants produced.

## Configuration surface

Two files, matching where each kind of value already lives.

`ProviderConfig` (global `config.toml`, `[providers.<id>]`) — properties of an endpoint:

| Key | Type | Default when absent or `0` | Replaces |
| --- | --- | --- | --- |
| `context_window` | `u32` | `128_000`, with one startup warning | `context_window: 128_000` literal in `xgent_app/src/main.rs` |
| `model_context_windows` | map of model id → `u32` | entry absent, so the provider value or fallback applies | — |
| `max_output_tokens` | `u32` | `8192` | `"max_tokens": 8192` in `xgent_provider/src/anthropic.rs` |
| `first_event_timeout_secs` | `u64` | `30` | `const FIRST_EVENT_TIMEOUT_SECS` in `openai_compat.rs` and `anthropic.rs` |

`ProjectConfig` (`.xgent/config.toml`) — per-repository working limits, next to the existing `tool_policy` and `context_strategy`:

| Section and key | Type | Default when absent or `0` | Replaces |
| --- | --- | --- | --- |
| `[agent] retrieval_max_tokens` | `u32` | `8_000` | `max_tokens: 8_000` in `xgent_agent/src/bridge.rs` |
| `[agent] max_events_per_frame` | `u32` | `64` | `while processed < 64` in `xgent_agent/src/agent_loop.rs` |
| `[tools] confirm_timeout_secs` | `u64` | `300` | `Duration::from_secs(300)` in `xgent_tools/src/executor.rs` |
| `[tools] command_timeout_secs` | `u64` | `60` | `COMMAND_TIMEOUT_SECS` in `builtins/run_command.rs` |
| `[tools] command_max_output_bytes` | `usize` | `65_536` | `MAX_OUTPUT_BYTES` in `builtins/run_command.rs` |
| `[context] tree_max_depth` | `u32` | `4` | `TREE_MAX_DEPTH` in `xgent_context/src/on_demand.rs` |
| `[context] tree_max_entries` | `usize` | `200` | `TREE_MAX_ENTRIES` |
| `[context] rg_timeout_secs` | `u64` | `10` | `RG_TIMEOUT_SECS` |
| `[context] max_search_files` | `usize` | `20` | `MAX_SEARCH_FILES` |
| `[context] max_keywords` | `usize` | `16` | `MAX_KEYWORDS` |

`0` means "unset, use the built-in default" for every numeric key. A `0` typed deliberately does not mean "unlimited".

## Context window

The window is what compaction measures against, so its value decides when a conversation is summarized.

- Resolution order for the running model: `model_context_windows[<model>]` when it has an entry for the model the agent will use, then `context_window` when it is non-zero, then `128_000` accompanied by one warning on stderr that names the model and states the fallback is being used.
- The resolved value is the single input to compaction decisions; nothing downstream re-derives or overrides it.
- A configured window is honoured for both the conversation's own threshold and the reserve computation (`effective_reserve_tokens`), because both take the window as their argument.
- An entry in `model_context_windows` for a model that is not running has no effect; it does not become the fallback.

Acceptance: A6

Scenario: provider window drives compaction
验收：A6

Given `[providers.openai] context_window = 32768` and the agent running a model with no per-model entry
When the conversation's estimated tokens exceed the threshold derived from 32768
Then compaction is triggered against 32768 rather than against 128000

Scenario: the model-specific window wins
验收：A6

Given `context_window = 32768` and `model_context_windows = { "gpt-4o" = 128000 }` while running `gpt-4o`
When the agent resolves the window
Then it uses 128000

Scenario: the fallback is visible
验收：A6

Given neither key set for the running provider
When the agent starts
Then the window is 128000 and one startup warning states that the fallback was used

## Agent loop parameters

Two values that bound per-turn cost and per-frame work.

- `[agent] retrieval_max_tokens` is the `max_tokens` field of the `ContextQuery` the agent builds before each model call; it is read once per conversation turn from configuration carried by the bridge.
- `[agent] max_events_per_frame` bounds how many agent events one ECS frame drains from the channel. Draining stops at the bound and the remaining events stay in the channel for later frames — the bound is a stall guard, and no event is discarded by it.
- With the `[agent]` section absent, the query budget is 8000 and the frame bound is 64, and the drain loop's bound is a value read from the bridge rather than a literal.

Acceptance: A7

Scenario: a smaller retrieval budget reaches the provider
验收：A7

Given `[agent] retrieval_max_tokens = 2000`
When the agent builds its context query
Then the query's `max_tokens` is 2000 and the hub trims merged chunks to that budget

Scenario: the frame bound comes from configuration
验收：A7

Given `[agent] max_events_per_frame = 8` and a channel holding more than eight pending events
When one agent-loop frame drains events
Then at most eight are processed in that frame and the rest remain available to the next frame

## Tool parameters

Three values that bound how long a tool may occupy the agent and how much output it may produce.

- `[tools] command_timeout_secs` bounds `run_command`'s execution; a command that outlives it is terminated and reported as it is today.
- `[tools] command_max_output_bytes` caps the captured output of `run_command`; the cap is applied with the existing head/tail truncation, and stays independent of the shared `MAX_TOOL_OUTPUT_BYTES` ceiling that `truncate_output` enforces on all tool output.
- `[tools] confirm_timeout_secs` bounds how long the executor waits for a confirmation decision before returning "确认请求超时". Cancellation still wins over the timeout: an abort during the wait returns the aborted error immediately, as before.
- All three reach the tools through the parameters passed to `default_tools` and the executor, not through globals.
- With `[tools]` absent, the values are 60, 65536 and 300.

Acceptance: A8

Scenario: a configured command timeout kills a slow command
验收：A8

Given `[tools] command_timeout_secs = 1`
When `run_command` executes a command that sleeps longer than one second
Then the command is terminated and the result reports the timeout

Scenario: a configured output cap truncates captured output
验收：A8

Given `[tools] command_max_output_bytes = 1024`
When `run_command` produces output far larger than that
Then the reported output stays within the configured cap and keeps the truncation marker

Scenario: a configured confirmation timeout expires
验收：A8

Given `[tools] confirm_timeout_secs = 1` and a confirmation that is never answered
When the executor waits for the decision
Then it returns the timeout result after roughly one second rather than five minutes

## Provider parameters

Two values that bound what the adapters ask for and how long they wait.

- `[providers.<id>] max_output_tokens` is the explicit output ceiling sent in the Anthropic Messages request body. Adapters that send no output ceiling today (OpenAI-compatible, Response API) are not given one by this change; the key has no effect there.
- `[providers.<id>] first_event_timeout_secs` bounds the wait for the first SSE event in the OpenAI-compatible and Anthropic adapters; on expiry the stream reports the same `Network`-kind error it reports today.
- The idle timeout between events, and the connect timeout, are unchanged constants; only the first-event bound becomes configurable.
- The daemon constructs providers through the same `build_provider`, so a configured value applies to the pooled provider without any new IPC field.

Acceptance: A9

Scenario: the output ceiling reaches the request body
验收：A9

Given `[providers.anthropic] max_output_tokens = 4096`
When the Anthropic adapter builds a messages request
Then the body carries `max_tokens` equal to 4096

Scenario: an absent ceiling keeps 8192
验收：A9

Given `max_output_tokens` unset or `0`
When the Anthropic adapter builds a messages request
Then the body carries 8192, exactly as before

Scenario: the first-event wait is configurable
验收：A9

Given `[providers.x] first_event_timeout_secs = 1` and an endpoint that produces no event for several seconds
When a chat stream is opened against it
Then the wait gives up after about one second and reports the same error kind as the 30-second default would

## Retrieval parameters

Five values that bound how much of the repository the built-in provider looks at.

- `tree_max_depth` and `tree_max_entries` bound the directory summary; a shallower limit omits deeper paths and a smaller entry limit stops the summary earlier, in the same text format as today.
- `rg_timeout_secs` bounds the `ripgrep` invocation; exceeding it falls back to the built-in traversal exactly as the 10-second default does.
- `max_search_files` bounds how many matched files are read and chunked.
- `max_keywords` caps keyword extraction, keeping question order and de-duplication, so a value below the number a Chinese question expands into still yields the first N keywords rather than none.
- These limits are per-provider values supplied when the provider is constructed; they are not process-global statics, and a provider constructed from defaults behaves as the constants did.
- The keyword extraction algorithm, the `@file:` routing, the literal `rg` invocation and the `spawn_blocking` structure delivered by the previous round are unchanged; only their bounds are injectable.

Acceptance: A10

Scenario: a smaller tree limit shortens the summary
验收：A10

Given `[context] tree_max_depth = 1`
When the built-in provider builds the tree summary for a project with files below `src/sub/`
Then the summary omits paths deeper than the limit

Scenario: a keyword cap that bites
验收：A10

Given `[context] max_keywords = 2` and a question that would produce five keywords
When keywords are extracted
Then exactly the first two in question order are returned

Scenario: one searched file
验收：A10

Given `[context] max_search_files = 1` and three files containing the searched keyword
When the provider searches
Then at most one file contributes a chunk

## Defaults preserve behavior

The invariant that makes this change safe to land.

- A `config.toml` and a `.xgent/config.toml` that contain none of the new keys deserialize to values numerically equal to the constants they replace, asserted as data rather than inferred from reading the default functions.
- A round trip (serialize a default `ProjectConfig` and `ProviderConfig`, parse again) keeps every new value at its default, including a `0` written explicitly.
- No existing configuration file needs editing for existing behavior to continue.
- `xgent_settings_core` stays free of Bevy, `xgent_provider` stays free of Bevy, and no crate gains a dependency on another xgent crate beyond the existing graph.

Scenario: an untouched config file yields today's numbers
验收：A11

Given a TOML document containing none of the new keys
When it is parsed into `ProjectConfig` and `ProviderConfig`
Then the resulting values equal 128000, 8000, 64, 8192, 30, 300, 60, 65536, 4, 200, 10, 20 and 16 respectively

## Regression expectations

- The workspace compiles with the widened constructors: `xgent_app`, `xgent_daemon` and `xgent_plugin_host` build, and the existing tests of every touched crate pass.
- Existing compaction tests keep their meaning; a test that asserted behavior against an implicit 128k window now states the window it uses.
- `docs/dev-tutorial.md` gains the configuration keys, and `docs/notes/agent-gap-disposition.md` gains the round's result, both in Chinese.
