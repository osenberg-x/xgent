# Tool Concurrency

## Purpose

Every tool declares whether it may share the execution window (`Concurrency::Shared`) or must own it (`Concurrency::Exclusive`). This capability is the consumer of that declaration: when one assistant turn asks for several tool calls, the ones that provably do not interfere run at the same time, and the ones that do run alone and in order. The model, the conversation transcript and the user must not be able to tell that anything ran concurrently — only that it finished sooner.

## Who decides

`ToolExecutor::parallel_safe(tool_id, input, ctx)` answers the scheduling question. It is advisory: `ToolExecutor::execute` remains the only place that resolves the security policy, asks for confirmation and records file checkpoints, and every scheduled call still passes through it.

A call is eligible for overlap only when all three of these hold:

| Signal | Required value | Why this signal is needed |
|:---|:---|:---|
| Resolved policy | needs no user confirmation — `Approved`, or `NeedsConfirmation` whose tool id is already in the session `allowed_all` set | the confirmation channel holds **one** pending request at a time; two concurrent requests would overwrite each other's decision sender |
| `Tool::concurrency()` | `Shared` | the tool's own declaration that it writes nothing it depends on |
| `Tool::tier()` | exactly `Read` | two independent reasons. `concurrency()` **defaults to `Shared`** in the trait, so a tool that forgets to declare `Exclusive` would otherwise be trusted — the tier is that second, independent signal. And `UiOnly` is deliberately excluded: `resolve_policy` approves `UiOnly` **unconditionally** (no confirmation), so admitting it would make the shipped `editor.*` tools run concurrently in a default configuration the user never opted into, and `open_file` → `goto` carries ordering semantics that must be guaranteed by the rule rather than by the fact that `EditorTool::execute` happens not to await |

Anything else — a write tool, an exec tool, a dangerous tool, a `UiOnly` tool, a tool awaiting confirmation, an unknown tool id — is serial. Serial means: run it alone, in the position the model gave it.

## Grouping rule

The tool-call list is walked in the model's order. Consecutive eligible calls form one group; the group runs concurrently; the walk resumes after it. An ineligible call is a barrier: it runs alone, and the next group starts only after it finishes.

The scheduler never reorders calls to find more parallelism. A write that the model asked for before five reads stays before them — the reads after it wait, even though they could have overlapped with each other earlier.

Groups are bounded: at most `max_parallel_tools` calls of one group are in flight simultaneously (project configuration `[agent] max_parallel_tools`, default `4`; `0` means unset and resolves to the default). Larger groups execute in waves of that size, still in order.

## Execution model

A group is executed by polling its futures concurrently inside the agent task (`join_all`), not by spawning one task per call. Consequences that are part of the contract:

- Cancellation stays cooperative: all calls share the loop's `CancellationToken`, and when the agent task is dropped no tool call keeps running in the background.
- A tool that never yields (a CPU loop without an await point) would stall every other member of its group. Every tool in this workspace is await-based IO, and the one plausibly CPU-heavy tool (`run_command`) is `Exclusive`, so it never shares a group.
- Calls into the same WASM plugin instance are already serialised by that plugin's own task loop; overlapping them is harmless but gains nothing. This capability does not change the plugin host.

## Ordering and pairing

Order is the model's, not the scheduler's.

- All `ToolCall` events are emitted before execution begins (as before this capability): every card appears in the UI immediately.
- `ToolResult` events are emitted in the original `tool_calls` order. A call that finished first is held until every earlier call in its group has been reported. Group completion is therefore the granularity at which results become visible.
- `req.messages` gets one assistant message containing all `ToolCall` blocks followed by the `Tool` role results in the same original order — the OpenAI-compatible pairing that the existing loop already produces must not change in shape.
- Every `call_id` receives exactly one result, including calls that never started.

## Abort and failure

Cancellation is observed at group boundaries: calls already in flight return through their own `ToolError::Aborted` path and get a result; calls in groups that had not started get the placeholder result ("未执行") so that no `tool_call` is left unpaired. The per-call placeholder text and the loop's existing abort exit path are unchanged.

A tool that fails logically (an error result, a denial) does not affect its group: the other calls continue, and the failure is reported in its own slot.

## Degradation direction

Two reads of the policy state are not atomic: `parallel_safe` decides scheduling, `execute` decides what actually happens. The only way they can disagree is that a user clicks "Allow all" while a group is running, so a later call was scheduled serially although it no longer needed confirmation. The effect is one unnecessary confirmation prompt at most — never an unconfirmed execution, never a lost decision. When in doubt the scheduler is serial.

With the shipped default configuration nothing runs in parallel, and it is worth being precise about *why*, because two different mechanisms would each be insufficient alone. Most tools resolve to `NeedsConfirmation`, so the policy signal already excludes them. `UiOnly` tools are the exception: `resolve_policy` approves them without asking, which is exactly why the `tier()` rule admits only `Read` — otherwise the shipped `editor.*` tools would overlap in a configuration the user never opted into. The capability is therefore inert out of the box and becomes active only when a user approves read-only tools.

## Relationship to other capabilities

- **File checkpoints**: write tools are `Exclusive`, so at most one write is in flight at a time and the per-turn checkpoint manifest keeps its "one entry per file per write, replayed newest-first" property. The scheduler must not be the place where that is discovered to be violated: eligibility requires `tier()` too, precisely so a write tool that forgot to declare `Exclusive` cannot break the checkpoint store.
- **Confirmation**: unchanged, and stays one-at-a-time by design.
- **Editor commands** (`UiOnly`, e.g. `editor.open_file` / `editor.goto`): stay serial. Their ordering is semantic (`open this file`, then `jump to that line`), and because the policy auto-approves this tier, the `tier()` rule is the only thing keeping them out of a concurrent group. Nothing in this capability parallelises editor calls.
- **Loop bounds and steering**: unchanged; token and round budgets count the same rounds, and steering still interrupts at the same points (between rounds, not inside a group).

## Configuration

| Key | Default | Meaning |
|:---|:---|:---|
| `[agent] max_parallel_tools` | `4` | Maximum tool calls of one group in flight at the same time. `0` = unset → default. Bounds subprocess/socket/file-descriptor counts, so "unlimited" is not expressible. |

It is read by `xgent_app`, carried in `AgentBridgeConfig`, and consumed by the agent loop; the tool crates do not import the configuration types (same decoupling as the other runtime parameters).

## Regression expectations

- `ToolExecutor::execute` keeps its signature and behaviour; existing executor, security and checkpoint tests pass without modification.
- With no `[tools]` approvals configured, the event stream, `req.messages` and observed in-flight counts are identical to the pre-change loop for the same tool-call list.
- No new user-visible string is introduced, so neither locale catalogue changes.
- `cargo check --workspace --all-targets`, per-crate `cargo test --lib` for `xgent_agent` / `xgent_tools` / `xgent_settings_core`, and `cargo fmt -p <crate> -- --check` for the touched crates pass.
