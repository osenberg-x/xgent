# Agent Safety and MVP Acceptance Gaps

## Purpose

This capability closes the R1 and R2 tiers of `doc/notes/agent-gap-review.md`: the defects that can cause unbounded execution, unbounded cost, or that block acceptance of an already-confirmed MVP requirement.

The gap report is the input and is not modified by this change. After this capability is delivered, every R1 and R2 entry is resolved and the report's remaining entries are R3 experience gaps and R4 roadmap items.

## Agent loop bounds

The inner tool-call loop terminates on a bound, never on LLM cooperation alone.

- `max_iterations` caps how many tool-call rounds run for one user turn. The default is a finite number.
- A cumulative token ceiling caps the total tokens the loop may consume for one user turn.
- `max_tokens` on the outgoing request makes the per-request bound meaningful; the OpenAI-compatible request body carries it.
- When a bound is hit, the loop stops and reports which bound it reached, so the user sees why the turn ended.
- Reaching a bound is a normal termination, not an error: the conversation state stays consistent and the next user turn proceeds normally.

### Retry bounds

- `max_retries` defaults to a finite value instead of `None`. A missing value means the default, not unbounded retries.
- Retryable errors include provider rate limiting (HTTP 429) and server errors (5xx), in addition to network and stream-parse failures.
- When a provider response carries a `Retry-After` header, that delay replaces the computed backoff.
- The existing fixed and exponential backoff modes keep working, and `max_delay_ms` still caps the computed delay.

## Stream cancellation

Aborting a response stops the work that produces it.

- A cancel method exists in the JSON-RPC method set alongside the other provider methods.
- The UI issues the cancel when the user aborts.
- The daemon stops the stream task registered for that stream id and drops its provider handle, so the upstream HTTP request is cancelled rather than drained to completion.
- No tokens are billed for an abandoned stream after the cancel is delivered.
- Cancelling a stream that already finished is a no-op and reports success.

## File reading limits

`read_file` cannot overflow the context window in one call.

- The input schema accepts `offset` and `limit`, both expressed in lines and both optional.
- Output that exceeds the tool's output cap is truncated with an explicit marker naming what was omitted.
- Truncation and line slicing operate on UTF-8 character boundaries and never split a character.
- Reading a byte range or a line range past end of file yields a clear message rather than an error.
- The output cap matches the cap used by other tools so a single tool cannot exceed the shared limit.

## File editing with backup

An agent edit does not require resubmitting a whole file, and is reversible.

- A new `edit_file` tool replaces a line range in an existing file.
- The tool verifies the expected original content for the target range before writing, and reports a mismatch instead of overwriting silently.
- The write is atomic, using the same temporary-file-and-rename pattern the existing file-writing tool uses.
- The original content is kept as a backup reachable by the revert path.
- When the specified line range does not exist, the tool reports a clear error.

## Command approval

Dangerous commands are recognized, and plugin commands are constrained.

- A dangerous-command pattern yields a stricter approval outcome than an ordinary command of the same tier, so configuring the ordinary command as approved does not auto-approve the dangerous variant.
- The dangerous-pattern list covers destructive filesystem operations, privilege escalation, disk and device writes, remote content fetch, permission and ownership changes, raw disk copy, and fork bombs.
- Plugin `run_command` rejects an argument outside the set its manifest permits, so a permitted program name does not permit arbitrary arguments.
- A rejection is reported as a permission error rather than executing the command.

## Plugin tool integration

Plugin tools present complete information and stream their progress.

- Plugin tools supply a real `preview_diff` through the WIT bridge, so the confirmation dialog shows the actual change rather than plain text.
- Plugin tools supply a real `summarize` through the WIT bridge, so the confirmation dialog shows a plugin-authored summary.
- The tool update callback is a shareable closure that can outlive the call that created it, so streaming tool output reaches the UI.
- The tool executor passes a real update callback instead of none.
- The plugin host forwards intermediate tool output rather than discarding it.

## Credentials

API keys prefer the operating system credential store.

- An API key held in the OS keychain is used in preference to the value in the TOML configuration.
- macOS reads the keychain through the `security` command; Linux reads it through `secret-tool`.
- A missing keychain, an unavailable helper command, or a read failure falls back to the TOML value.
- An existing TOML value is never deleted, so a fallback is always available.
- The TOML value continues to work unchanged on platforms with no credential store integration.

## Provider completeness

The two placeholder providers produce real responses.

- `ResponseApiProvider` sends the request in the OpenAI Responses API shape and maps the streamed response back into the shared chat event stream, including function calls.
- `CustomApiProvider` serves a user-defined base URL, extra request headers, and model identifier over the OpenAI-compatible wire format.
- Neither provider contains a placeholder path that returns an unimplemented error.
- Both keep the existing stream timeouts, tool-call aggregation by index, and error mapping.

## Context strategy selection

The configured retrieval strategy is the one that runs.

- Startup builds the built-in context provider from the configured strategy rather than hardcoding one strategy.
- A strategy that has an implementation runs that implementation.
- A strategy without an implementation surfaces a configuration error rather than silently falling back to a different strategy, so a misconfigured project cannot appear to work.

## Requirements wording

The requirements text matches the shipped confirmation behavior.

- The F-04 requirement no longer states that read-only operations are auto-approved, because the shipped policy requires confirmation for read, write, and execute tiers alike.
- The correction touches only claims contradicted by the code.

## Deferral record

Every gap report entry has a stated disposition.

- The R2-2 message stream recording and replay item is deferred with its reason.
- The R3 experience tier is deferred with its reason.
- The R4 roadmap tier is deferred with its reason, including the entries blocked on undecided design records and the entries forbidden by project rules.
- No deferred or blocked entry ships as a stub.

## Scenarios

### Loop stops at a bound and names it

Scenario: bounded execution
验收：A2

Given an agent turn where the model keeps requesting tools
When the iteration ceiling or the cumulative token ceiling is reached
Then the tool loop stops, the conversation remains consistent, and the user is told which bound was hit

### Retry is finite and respects server hints

Scenario: bounded retry
验收：A3

Given a provider that answers 429 or 503, or a configuration with no explicit retry count
When retries are computed
Then the retry count is finite by default, rate limiting and server errors are retried, and a `Retry-After` value from the provider replaces the computed delay

### Abort stops the upstream request

Scenario: cancellation reaches the provider
验收：A4

Given a stream that is producing tokens
When the user aborts the response
Then a cancel is issued through the daemon, the stream task stops, and no further tokens are produced for that stream

### Reading a file cannot overflow the context

Scenario: bounded and sliced reads
验收：A5

Given a file larger than the tool output cap
When the agent reads it, optionally with a line offset and limit
Then the returned content is truncated with an explicit marker, the slice is honored, and no character is split

### Editing a file is verifiable and reversible

Scenario: line-range edit with backup
验收：A6

Given an existing file
When `edit_file` replaces a line range whose content matches what was expected
Then the file is updated atomically and the original is kept as a backup, and a content mismatch reports an error instead of overwriting

### Dangerous commands and plugin arguments are constrained

Scenario: approval tightens on danger
验收：A7

Given a command the dangerous-pattern list matches, and a plugin whose manifest permits a program but not all of its arguments
When each runs
Then the dangerous command needs confirmation even when the ordinary command is approved, and the plugin argument is rejected as a permission error

### Plugin tools show real diffs and stream progress

Scenario: complete plugin tool integration
验收：A8

Given a plugin tool that produces a diff and streams intermediate output
When the agent runs it
Then the confirmation dialog shows the plugin's diff and summary, and the intermediate output reaches the UI

### Keychain credentials win with TOML fallback

Scenario: credential precedence
验收：A9

Given an API key present in both the OS keychain and the TOML configuration
When the provider is built
Then the keychain value is used, and an unreadable keychain falls back to the preserved TOML value

### Placeholder providers produce real responses

Scenario: provider completeness
验收：A10

Given a configuration selecting either placeholder provider
When a chat request is streamed
Then both providers produce real streamed events including function calls, and neither returns an unimplemented error

### A configured strategy runs, a missing one errors

Scenario: strategy selection
验收：A11

Given a project configuration with a retrieval strategy
When startup runs
Then the matching provider is used, and a strategy with no implementation surfaces a configuration error instead of falling back

### Requirements wording matches shipped behavior

Scenario: documentation correction
验收：A12

Given the F-04 requirement text and the shipped confirmation policy
When the two are compared
Then the requirement no longer claims read-only operations are auto-approved, and no unrelated wording changed

### Every gap has a disposition

Scenario: deferral recorded
验收：A13

Given the gap report
When each entry is classified
Then all R1 and R2 entries are implemented or deferred with a reason, and the R3 and R4 tiers are recorded as deferred with their reasons

### Workspace builds and tests pass

Scenario: green build
验收：A14

Given the implemented changes
When `cargo check --workspace` and `cargo test --workspace` run
Then both complete successfully