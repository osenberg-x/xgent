# Outcome

`Tool::concurrency()` currently declares `Shared`/`Exclusive` for every tool and has no consumer anywhere in the workspace: the agent's inner tool-call loop (`crates/xgent_agent/src/bridge.rs`, the `for (call_id, name, args) in &outcome.tool_calls` block) awaits each `executor.execute(...)` one after another. When a model asks for five file reads or three code searches in one turn, the user waits for the sum of their latencies even though nothing in those tools depends on each other.

After this change, consecutive read-only tool calls that need no user confirmation run **concurrently**, bounded by a configured limit, while everything the model and the user observe stays exactly as it is today: results are delivered in the order the model asked for them, every `tool_call` still has exactly one paired `tool_result`, exclusive tools still never overlap, confirmations are still asked one at a time, and with the default security configuration nothing runs in parallel at all.

# Scope

## Sources and their role

- `doc/notes/agent-gap-review.md` §R3-15 — the gap statement (`unwired-design`): declared API, zero consumers, serial `for` loop. Evidence line quoted there: `bridge.rs:680`.
- `crates/xgent_agent/src/bridge.rs` — the loop being scheduled; `SharedConfirm` (lines 170-183) and `BridgeConfirm::confirm` (1519-1533) define why confirmations cannot overlap; the abort path in the same block defines the placeholder rules that must be preserved.
- `crates/xgent_tools/src/{tool.rs,executor.rs,security.rs}` — `Tool::concurrency()` (tool.rs:184, default `Shared`), `Tool::tier()`, `resolve_policy`, and `ToolExecutor::execute` as the single place where policy, confirmation and checkpoints are applied.
- `crates/xgent_plugin/src/wasm_host.rs:227-236` — the plugin task consumes calls with `while let Some(call) = rx.recv().await { call(...).await }`, i.e. one plugin instance already executes its calls strictly sequentially. Overlapping two plugin calls is therefore safe but buys nothing; this is a fact the scheduler must not mistake for a hazard.
- `crates/xgent_tools/src/checkpoint.rs` (previous change) — the per-turn manifest is read-modify-written; two concurrent writes would lose entries, which is why `Exclusive` remains a barrier.
- `AGENTS.md` §5.4 (default `NeedsConfirmation` for all tools), §5.7 (style), §5.8 (Chinese comments/commits).
- Project memory: "comet native 检查计划的 argv 不含程序名", "变异反证要先判等价性", "验收证据引用测试名不等于覆盖范围" — they shape the verification plan and the evidence rules below, not the product behaviour.

## In scope

- `xgent_tools::ToolExecutor`: an advisory scheduling query that answers "may this call overlap with its neighbours" from three independent signals — resolved policy, declared `concurrency()`, and security `tier()`.
- `xgent_agent::bridge::run_agent_loop`: group consecutive eligible calls, run each group concurrently, emit results in the model's original order, keep the existing abort/placeholder and `req.messages` pairing logic.
- `xgent_settings_core::project`: `[agent] max_parallel_tools` (default `4`), `effective_max_parallel_tools()`, wiring `xgent_app` → `AgentBridgeConfig` → `run_agent_loop`.
- Behaviour tests for overlap, exclusivity, confirmation serialisation, ordering, the cap, the default-config regression, and abort; a serde default test for the new key.
- `doc/dev-tutorial.md` and `doc/notes/agent-gap-disposition.md` §九.

## Out of scope but adjacent

- Parallelism across assistant turns (tool calls from different rounds are still round-by-round).
- Streaming the next LLM call while tools run.
- Making one plugin instance run two of its own calls at once (the plugin task is sequential by design; changing it is a plugin-system decision, not a scheduling one).
- MCP tools: `crates/xgent_tools/src/mcp.rs` defines only an unimplemented transport trait with no `impl Tool for`, so there is nothing to schedule yet.
- Any UI change: no progress bars, no per-group visual grouping, no new user-visible strings.

# Non-goals

- Not a throughput benchmark: no wall-time assertions (they flake on CI and on this machine); overlap is proven with deterministic in-flight counters.
- Not a relaxation of the security model: with the shipped default (every tool `NeedsConfirmation`) the scheduler must observe zero parallelism.
- Not making write tools concurrent, not adding a redo/undo interplay, not exposing the new limit in the settings panel (that is R3-13).
- Not fixing the trait default `Concurrency::Shared` itself (changing a default that every existing tool inherits would silently alter behaviour beyond this scope); this change instead refuses to trust it alone.

# Constraints and invariants

- **Protocol integrity**: every `tool_call` produces exactly one `AgentEvent::ToolResult` and exactly one `ContentBlock::ToolResult` in `req.messages`, carrying the same `call_id` — including calls that never started because a cancel arrived first. An OpenAI-compatible request with an orphan `tool_call` breaks the next `StartLoop`.
- **Order is the model's, not the scheduler's**: `ToolResult` events and `req.messages` tool results are emitted in the original `outcome.tool_calls` order regardless of completion order. Group completion is the granularity at which results become visible.
- **Three-signal eligibility**: a call may overlap only if (a) its resolved policy needs no user confirmation (`Approved`, or `NeedsConfirmation` whose tool is in the session `allowed_all` set), (b) `tool.concurrency() == Concurrency::Shared`, and (c) `tool.tier()` is **exactly** `Read`. Missing any one of the three keeps it serial. (`UiOnly` was admitted in the first draft and removed during Build: `resolve_policy` approves `UiOnly` unconditionally, so admitting it would parallelise the shipped `editor.*` tools with no opt-in at all, and would rest their ordering on the incidental fact that `EditorTool::execute` does not await.) Rationale, established from source: `SharedConfirm` holds a **single** `Mutex<Option<oneshot::Sender<ConfirmDecision>>>`, so a second concurrent confirmation would overwrite the first sender and lose a user decision (the first await would see the channel closed and report "确认被取消"); and `Tool::concurrency()` **defaults to `Shared`**, so a tool that forgets to declare `Exclusive` would otherwise be scheduled in parallel — `tier()` is the independent second signal that it really is read-only.
- **`Exclusive` is a barrier in both directions**: it starts only when nothing else is in flight, and no eligible call starts while it runs. This also protects the checkpoint store's per-turn manifest read-modify-write and its "one entry per file per write, replayed newest-first" rule from the previous change.
- **Bounded**: at most `[agent] max_parallel_tools` calls in flight (default 4). `0` means unset and resolves to the default — the limit exists to bound subprocess/socket/fd counts, so "unlimited" is not expressible (same convention as the other R3-17 keys).
- **Default configuration is a no-op**: most tools default to `NeedsConfirmation` and a call needing confirmation is never eligible; the `UiOnly` tier is the exception the policy auto-approves, which is precisely why the `tier()` rule admits only `Read`. An unconfigured project therefore executes exactly as today.
- **Cancellation semantics unchanged**: `cancel_token` still wins over confirmation timeouts (`biased` select inside `execute` untouched); in-flight calls of an aborted group return `ToolError::Aborted` and get a result, and calls in groups that had not started yet get the existing placeholder text.
- **One policy gate**: scheduling is decided *outside* `execute` but execution still goes **through** `execute`, which remains the only place that resolves policy, asks for confirmation and records checkpoints. No bypass path, no second implementation.
- `xgent_tools` stays Bevy-free; no new dependency; no `unwrap()`/`expect()` outside tests; Chinese doc comments; `cargo fmt -p <crate>` only.

# Decisions

- **D-P1: the loop schedules, the executor answers questions.** `ToolExecutor` gains `parallel_safe(tool_id, input, ctx) -> bool` (async: it reads the policy config and the `allowed_all` set). `run_agent_loop` builds groups from it. Putting the scheduler inside the executor would force event emission and message-pairing concerns — which belong to the agent loop — into the tool layer, and would duplicate the abort/placeholder logic that already lives at the call site.
- **D-P2: `join_all` over borrowed futures, not `tokio::spawn` per call.** Spawning needs `'static` clones of everything and leaves tasks running after the loop returns (a cancelled batch would keep writing files after `Done`). Polling several futures inside the agent task keeps borrows, keeps cancellation cooperative through the shared token, and needs no join-handle bookkeeping. Consequence to state plainly in the Spec: one tool that busy-loops without awaiting would stall its whole group — every tool in the workspace is IO/await-based, and `run_command` (the only CPU-adjacent one) is `Exclusive`.
- **D-P3: only *consecutive* eligible calls group together; an ineligible call is a barrier.** Reordering to hunt for parallelism would change the order the model sees and could starve an early write behind later reads.
- **D-P4: eligibility is advisory and re-resolved inside `execute`.** The two reads of the policy/allow-all state are not atomic. That is harmless in one direction only: `allowed_all` can ever *remove* a confirmation, never add one, so a call planned as parallel and later found to need confirmation cannot happen for a tool whose group was built from the same snapshot... except when the user clicks "AllowAll" mid-batch. Then the effect is that a later call still asked for confirmation it no longer needed — the safe direction. The Spec records the direction of the degradation.
- **D-P5: `tier()` must be exactly `Read`, in addition to `Shared`.** Chosen over "trust `concurrency()`" because `concurrency()` has a permissive default (see constraints), and over "also admit `UiOnly`" because the policy approves that tier without asking, which would silently parallelise ordered editor commands in a default configuration. Cost: `editor.*` calls gain no overlap (they are instantaneous anyway); a read-only tool annotated with a wrong tier gains nothing but loses nothing either.
- **D-P6: `max_parallel_tools` becomes a project-config key with default 4.** This project just finished moving 13 hardcoded limits into configuration (R3-17); shipping a new literal would reopen exactly that gap. No settings-panel surface this round.
- **D-P7: all `ToolCall` events are still emitted before execution starts** (as today), so the UI shows every card immediately; only results become group-wise. No new "batch" concept is introduced in events.

# Acceptance examples

- **A1**: two `Approved` read-only tools in one `tool_calls` list, each blocking until it observes two calls in flight (bounded wait, then it reports what it saw) → both observe concurrency 2 and succeed. With the serial loop they would each time out and report 1. The assertion is the observed count, not elapsed time.
- **A2**: a list of `[shared, exclusive, shared]` → the exclusive call observes exactly one in-flight tool (itself) at every moment of its run, the two shared calls do not overlap with it, and results still arrive in order 0,1,2.
- **A3**: two calls whose policy is `NeedsConfirmation` (and not in `allowed_all`) → the confirmation slot never holds two pending requests: the fake confirm callback asserts that it is called, returns a decision, and is only called again after the first has resolved. The decisions apply to their own calls (`Deny` on the first, `Allow` on the second → the first is denied, the second runs).
- **A4**: three eligible calls with durations `30ms, 10ms, 20ms` → `ToolResult` events come out in call order (ids `c0, c1, c2`) and `req.messages` carries the three tool results in the same order, each paired with its own `call_id`.
- **A5**: with `max_parallel_tools = 1`, a batch of four overlapping-asking tools never observes 2 in flight; with `max_parallel_tools = 4` and eight such tools, the recorded maximum is exactly 4 (never 5+), and all eight produce results.
- **A6**: default configuration (no `[tools]` approvals, no AllowAll) → the observed maximum in-flight count is 1 for a list of read calls, **and also for `UiOnly` calls that the policy auto-approves without any confirmation request being raised** (the second case is the discriminating one, since only the `tier()` rule excludes it); the event stream and `req.messages` are unchanged in shape.
- **A7**: cancel arrives while a group of three is in flight → every `call_id` gets exactly one `ToolResult`, the in-flight ones report the interruption, calls in later groups get the existing "未执行" placeholder, and the loop leaves through the abort path it uses today.
- **A8**: `[agent] max_parallel_tools` — a TOML with no `[agent]` section resolves to `4`; `max_parallel_tools = 0` also resolves to `4`; `max_parallel_tools = 2` resolves to `2` and reaches `run_agent_loop` (proved behaviourally by A5's `= 1` / `= 4` cases, so the cap is demonstrably read from configuration and not a literal).
- **A9**: `cargo test -p xgent_agent --lib`, `cargo test -p xgent_tools --lib`, `cargo test -p xgent_settings_core --lib`, `cargo check --workspace --all-targets` and `cargo fmt -p <crate> -- --check` pass; `Tool::concurrency` has a real consumer (it is read on the scheduling path, verified by A1/A2 behaviour rather than by grep alone); `ToolExecutor::execute` keeps its existing signature and all existing executor tests still pass untouched.

# Verification expectations

- Formal checks stay per-crate and cheap: `cargo test -p xgent_agent --lib`, `-p xgent_tools`, `-p xgent_settings_core`, `cargo check --workspace --all-targets`, `cargo fmt -p <crate> -- --check` for touched crates, and clippy over the touched crates. Check-plan `argv` must NOT repeat the program name (Runtime execs `executable + argv`), `cwdRef` is `"."`, and timeouts are generous (≥ 1800000 ms) because a per-crate suite here has taken up to ~14 minutes under IO pressure.
- Overlap must be tested with deterministic primitives: shared `AtomicUsize` in-flight counters plus a bounded wait (`tokio::time::timeout`) inside the fake tool, recording the maximum observed. No assertion on total elapsed time, no `sleep`-longer-than-X race windows.
- Every new test must be checked against a mutant (project-standard counter-proof). Planned mutants at least: scheduler degraded to serial (A1 must fail), `Exclusive` allowed into a group (A2), confirmation-eligible calls allowed into a group (A3), results emitted in completion order (A4), cap hardcoded to a literal / ignored (A5), tier condition dropped (a `Write`-tier tool with default `Shared` must not overlap), abort placeholder dropped (A7), default config allowed to parallelise (A6).
- A mutant that no test catches is investigated first as "is my test even running" (compare `#[test]`/`#[tokio::test]` attribute counts with `cargo test --lib -- --list` registration counts) before it is ever called behaviour-equivalent.
- Anything not observable headlessly is stated as such rather than claimed: the real UI feel of group-wise results and the actual latency win on a live provider are not verified by these tests.
