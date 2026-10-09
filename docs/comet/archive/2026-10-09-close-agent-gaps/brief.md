# Outcome

Close the R1 and R2 tiers of `doc/notes/agent-gap-review.md` — the safety, runaway-cost, and MVP-acceptance-blocking defects — by implementing them in code with tests, and record the R3/R4 remainder as explicitly deferred with the reason for each.

After this change lands, the ranked gap list contains only R3 experience gaps and R4 roadmap items; every R1 and R2 entry is resolved.

# Scope

## Coverage boundary

The R1 and R2 tiers of `doc/notes/agent-gap-review.md` (14 entries, R1-1 … R2-4) are the implementation target. Every entry in that report is classified as implemented-here, deferred-to-R3/R4, or blocked, and the classification is recorded.

### Implemented in this change

| Gap | Change |
| --- | --- |
| R1-1 | Agent loop iteration ceiling plus cumulative token ceiling |
| R1-2 | `provider.cancel` RPC so abort stops the upstream request |
| R1-3 | `read_file` output truncation plus `offset`/`limit` |
| R1-4 | `edit_file` tool with original-file backup |
| R1-5 | `run_command` dangerous-command detection made reachable |
| R1-6 | Plugin `run_command` args filtering |
| R1-7 | Finite retry bound by default; 429 and 5xx retried with `Retry-After` honored |
| R1-8 | API Key read from the OS keychain when available, TOML otherwise |
| R1-9 | `ToolUpdateCallback` changed to `Arc` and wired end to end |
| R1-10 | Remove the blocking lock from the Bevy system |
| R2-1 | `ResponseApiProvider` and `CustomApiProvider` return real responses |
| R2-3 | `requirements.md` F-04 wording corrected to match shipped behavior |
| R2-4 | `context_strategy` selects the built-in provider; unimplemented strategies error instead of falling back |

### Deferred, with reason

| Tier | Reason |
| --- | --- |
| R2-2 NF-04 message stream recording/replay | Requires a session format decision (where a recording lives, how a replay relates to a live session). Not a safety or acceptance defect. Deferred to the R3/R4 pass. |
| R3-1 … R3-21 | Experience-tier gaps. Each is a self-contained feature (CJK tokenization, Markdown rendering, LCS diff, regex search, lazy loading, i18n bridge wiring, VirtualList adoption, settings for hardcoded values, and so on). Deferred so this change stays reviewable. |
| R4-1 … R4-10 | Roadmap tier. Blocked or large-feature items: F-15 pet is blocked on D-07 and OQ-05; 3D/TUI/Web are forbidden by `AGENTS.md` §6.5; D-P2/3/4/5/6/7/9 are undecided design records; cost statistics need OQ-10; terminal and editor residuals are multi-day features. |

# Non-goals

- No R3 or R4 implementation. The gap report remains their tracker.
- No placeholder or stub for any deferred or blocked item.
- No 3D, TUI, Web, pet, LSP, or vector-retrieval work.
- No restructuring of existing documents; corrections are limited to claims contradicted by code.
- No change to the gap report itself.

# Constraints and invariants

- Every implemented item keeps the existing ECS contract: cross-thread work goes through tokio channels, subsystems communicate through Events/Messages, no direct method calls between subsystems.
- Safety defaults are preserved: a tool is not auto-approved unless the configured policy or the `UiOnly` tier says so. R1-5 tightens this and never loosens it.
- `read_file` truncation and `edit_file` line addressing operate on UTF-8 character boundaries, never splitting a character.
- `edit_file` and modified `write_file` write atomically, matching the existing `write_file` pattern.
- The daemon cancel path frees the spawned stream task and its provider handle; no orphan tasks remain.
- New behavior is covered by tests in the crate that owns it.
- `cargo check --workspace` and `cargo test --workspace` pass before handoff.

# Decisions

- D-C1: The gap report is the input, not the output. It is not edited.
- D-C2: R1-8 defaults to the macOS Keychain via the `security` CLI and the Linux `secret-tool`; Windows keeps the TOML path because Credential Manager is not implemented. An existing TOML key is never deleted; it is simply shadowed when the keychain holds a value.
- D-C3: R2-1 `ResponseApiProvider` targets the OpenAI Responses API shape (`input` items plus `output` text and function-call items). `CustomApiProvider` covers user-defined base URL, extra headers, and model id over the OpenAI-compatible wire format.
- D-C4: R2-3 resolves the F-04 conflict in favor of the stricter behavior already shipped and already documented in `AGENTS.md` §5.4; `requirements.md` text is corrected, not the code.
- D-C5: R2-4 makes a `context_strategy` with no implementation return a visible configuration error rather than silently degrading, so a misconfigured project cannot look like a working one.
- D-C6: R1-7 default `max_retries` becomes a finite number (5) matching the existing `RetryConfig::delay_for` exponential backoff; `Retry-After` is honored when the provider supplies it.

# Acceptance examples

- A1: Every entry in the R1 and R2 tiers of `doc/notes/agent-gap-review.md` is either implemented in this change or recorded as deferred with a named reason, and the classification covers all 14 entries.
- A2: The agent loop stops after a bounded number of tool-call iterations or when a cumulative token ceiling is reached, and it reports which bound it hit instead of silently stopping.
- A3: The retry configuration defaults to a finite retry count, 429 and 5xx responses are retried, and a provider-supplied `Retry-After` overrides the computed backoff delay.
- A4: Aborting a response issues a cancel through the daemon to the provider, the spawned stream task terminates, and no further tokens are produced for the abandoned stream.
- A5: `read_file` accepts `offset` and `limit`, truncates output that exceeds its cap with an explicit marker, and never splits a UTF-8 character.
- A6: A new `edit_file` tool applies a line-range replacement to an existing file, writes atomically, leaves a backup of the original, and reports a clear error when the specified lines do not match the expected content.
- A7: A dangerous command in `run_command` yields a stricter approval outcome than an ordinary command, and plugin `run_command` rejects an argument outside the manifest's allowed set.
- A8: Plugin tools supply a real `preview_diff` and a real `summarize` through the WIT bridge, and the tool update callback delivers streaming tool output to the UI as `Arc` instead of a borrowed boxed closure.
- A9: An API key stored in the OS keychain is used in preference to the TOML value, a missing or unreadable keychain falls back to TOML, and the TOML value is preserved rather than deleted.
- A10: `ResponseApiProvider` and `CustomApiProvider` produce real streamed responses through the existing `LlmProvider` trait instead of returning a placeholder error, and neither contains a "尚未实现" path.
- A11: `context_strategy` selects the built-in provider at startup, and a strategy without an implementation surfaces a configuration error rather than falling back to another strategy.
- A12: `requirements.md` F-04 no longer claims read-only operations are auto-approved, and the correction is limited to claims contradicted by the shipped code.
- A13: Each of the seven deferral groups (R2-2, the R3 tier, the R4 tier) records what it defers and why, so no entry in the gap report has an unstated disposition.
- A14: `cargo check --workspace` and `cargo test --workspace` both pass.