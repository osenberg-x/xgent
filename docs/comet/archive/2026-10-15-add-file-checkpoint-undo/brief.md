# Outcome

Give the user a way to take back the file changes the agent made. Today there is none: `write_file` and `edit_file` overwrite project files, and nothing in the repository can put them back (`revert`/`rollback`/`checkpoint` have zero hits in `crates/**/*.rs`; the only `undo` is the editor's in-memory text stack, which has nothing to do with what the agent wrote to disk).

Worse, the write path already produces a safety net that nobody uses: every successful write drops `<name>.<ext>.xgent-bak` next to the target (`atomic::backup_existing`, called only by `write_file.rs:120` and `edit_file.rs:118`, read only by one test). It is a single slot that the next write overwrites, it is never consumed, and it sits inside the user's project where `git status` shows it.

After this change:

1. Before the agent's write tools touch a file, the pre-write bytes are recorded in a per-project checkpoint store under `<project>/.xgent/checkpoints/`, grouped by conversation turn.
2. The user can undo the most recent turn's file changes from the command palette, repeatedly, walking back turn by turn.
3. An undo never overwrites a change made by someone other than the agent: each file is restored only if it still holds exactly what the agent wrote; otherwise it is skipped and reported.
4. The orphan `.xgent-bak` files stop being created.

# Scope

## Sources and their role

`doc/notes/agent-gap-review.md` R3-11 (「无撤销 agent 文件改动的机制 · `planned-not-built`」) and §四 of `doc/notes/agent-gap-disposition.md` are the input; `AGENTS.md` §5.4 (security model) and the write path in `crates/xgent_tools` are the constraints. No requirement document currently names this feature — `requirements.md` F-06 is 「新建、切换、查看历史会话」and F-10's 「回溯到某次提交」is git-based rollback (R4-3, a different mechanism). This change therefore **adds** a capability rather than completing a written requirement, and it is scoped to the agent's own file writes.

| Source item | Read | Kept behaviour | Spec location | Acceptance |
| --- | --- | --- | --- | --- |
| R3-11 「无撤销 agent 文件改动的机制」 | complete | checkpoint store + per-turn undo | specs/file-checkpoint-undo/spec.md → Recording a change / Undoing a turn | A1, A2, A6 |
| R3-11 「唯一的 undo 是编辑器内文本快照，与 agent 写文件无关」 | complete | editor undo stack untouched; disk undo is a separate mechanism | → Relationship to the editor | A9 |
| `agent-gap-disposition.md` §一 R1-4 「写前备份原文件」 | complete | superseded by checkpoints; `.xgent-bak` creation removed | → What replaces the backup file | A1 |
| `AGENTS.md` §5.4 默认所有工具需确认 | complete | undo is user-initiated, so it is not a tool and needs no confirmation dialog; it refuses instead of asking | → Guardrails | A6, A9 |
| F-10 「回溯到某次提交」(R4-3) | complete | out of scope (git history rollback), different feature | — | — |

## In scope

| Piece | Where |
| --- | --- |
| `CheckpointStore`: per-turn manifests + snapshot blobs, atomic writes, retention, undo, conflict refusal | new `crates/xgent_tools/src/checkpoint.rs` |
| Snapshot-before-write for the built-in write tools, through one choke point that every execution path already goes through | `crates/xgent_tools/src/executor.rs` + `Tool::write_targets` default method in `tool.rs`, implemented by `write_file.rs` / `edit_file.rs` |
| Turn identity: the async loop marks the start of a turn with the conversation's numeric session id | `crates/xgent_agent/src/bridge.rs` (`AgentCommand::StartLoop` payload) and `agent_loop.rs` |
| User-facing undo: palette command, notice, refusal when the agent is busy | `crates/xgent_ui/src/command_palette.rs` (+ new `undo` module), i18n keys in both locales |
| Removing `backup_existing` from the write path and its test | `crates/xgent_tools/src/atomic.rs`, `write_file.rs`, `edit_file.rs`, `executor.rs` test |

## Out of scope but adjacent

- Undoing what `run_command` or a plugin's `host.write_file` changed on disk — see Non-goals; this is a real limit of the feature and is stated in the tool's own notice.
- The editor's own undo stack and the daemon-side file-state sync (`SideEffect` still has no consumer).

# Non-goals

- No agent-callable undo tool. The model must not be able to roll back files: an agent-initiated revert can destroy edits the user made after the agent's turn, and the security model gives the agent no path that needs one. Undo is a user action only.
- No redo, no undo history browsing UI, no per-tool-call granularity. Undo granularity is one conversation turn, repeatable to walk backwards.
- No whole-session bulk revert this round (per-turn, repeated, already covers the need; a bulk action over 30 files deserves its own confirmation design).
- No snapshotting of `run_command` side effects, plugin `host.write_file` writes, or files the agent edits through a shell (`sed -i`, `git apply`, `cargo fmt`).
- No git integration and no requirement that the project be a git repository; the store works identically either way. (F-10's 「回溯到某次提交」 stays R4-3.)
- No new reload path for editor buffers. The existing filesystem-watch pipeline already reloads a clean open buffer and raises the conflict dialog for a dirty one; the undo relies on it rather than duplicating it.
- No deletion of `*.xgent-bak` files already left in user projects (deleting a user's files is not this change's call); it stops creating new ones.
- No config keys for retention in this round (fixed 20 turns per session, a named constant), and no daemon/IPC surface for checkpoints.

# Constraints and invariants

- **Snapshot before write, never after.** A checkpoint entry exists on disk before the tool's `atomic_write` is attempted; otherwise an undo could not know the pre-state of a file whose write failed midway.
- **A failed snapshot degrades visibly, not silently.** If the checkpoint cannot be recorded (disk full, permissions), the write still proceeds — undo must not break the product's main function — but the missing coverage is recorded for that turn and reported in the undo notice instead of pretending the file is undoable.
- **Undo restores only what the agent wrote.** A file whose current bytes differ from what the agent wrote (hash mismatch) is left alone and reported. Undo never recreates a file the user deleted, and never rebuilds a file whose pre-state did not exist.
- **Undo is confined to the project root** — every path goes through `resolve_in_project`, so no checkpoint entry can point outside the project.
- **One entry per file per write**, replayed newest-first, so a file written three times in one turn returns to its pre-turn state through the intermediate states.
- The store lives under `<project>/.xgent/`, which retrieval already ignores (`on_demand.rs` ignore list), so checkpoints never pollute the agent's own context.
- Undo only runs while the conversation is idle; while a turn is in flight the command refuses with a message rather than racing the writer.
- All user-visible strings go through i18n with entries in **both** `zh-CN/main.ftl` and `en-US/main.ftl` (NF-05, `AGENTS.md` §5.6).
- `xgent_tools` stays Bevy-free apart from what it already pulls; no new dependency; no `unwrap()`/`expect()` outside tests; Chinese doc comments and commit messages (`AGENTS.md` §5.7, §5.8).
- `cargo fmt -p <crate>` only — `cargo fmt --all` rewrites the `../bevy` checkout.
- New behaviour is covered by tests and every new test is checked against a mutant (the counter-proof method in project memory; a mutant that no test catches is investigated as a possibly-dead test before it is ever called behaviour-equivalent).

# Decisions

- **D-U1: checkpoints replace the orphan `.xgent-bak`, they do not join it.** The backup file was recorded as a delivered feature in §一 (R1-4), but no code reads it, it holds only the immediately-previous state, and it writes clutter into the user's project. Keeping both would mean two half-mechanisms; the checkpoint store is strictly stronger (per-turn grouping, recorded identity, project-local storage) and preserves the crash-safety the backup was for, because the snapshot is written before the target.
- **D-U2: the executor takes the snapshot, not the tools.** `ToolExecutor::execute` is the single point that every approved/confirmed write passes through and it already has the resolved `ToolCtx`. A new `Tool::write_targets(input, ctx) -> Vec<PathBuf>` (default empty, mirroring the existing default-method style of `approval_for`/`preview_diff`) declares which paths a call will write; `write_file`/`edit_file` implement it with the same `resolve_in_project` they already use. Cost: a tool that forgets to declare its targets is silently non-undoable — which is pinned by a test that asserts both write tools declare theirs.
- **D-U3: turn granularity, keyed by `(session_id, turn_no)`.** One turn is one user request round; the numeric `SessionId` is passed into `AgentCommand::StartLoop` rather than derived from `Conversation::id.to_string()`, because the existing `restore()` parses `session:N` stems to `0` — a latent id mismatch that must not be copied into checkpoint keys.
- **D-U4: the manifest is the unit of undo.** `turns/<session>/<turn>.json` lists entries (relative path, kind = created|modified, blob, pre/post hashes); blobs are content-addressed by turn+index. Writing a manifest entry before the target write means a turn that dies part-way still has an undoable record. Undo = read newest manifest, replay newest-entry-first, then delete the manifest and its blobs.
- **D-U5: verification by recorded post-write hash, not mtime.** A hash comparison at undo time answers "is this still the content the agent wrote"; mtime/size would either false-reject (touch, git checkout) or false-accept (same length, different bytes).
- **D-U6: refusal is the default under conflict; nothing is force-overwritten.** Partial success with an explicit skipped list is more useful than an all-or-nothing transaction that refuses to undo the safe files because one file is contested.
- **D-U7: undo reports through the existing toast, with the detail kept where the user can read it.** The toast TTL is 2.2s, so a plain success is one line; when files were skipped or had no checkpoint, the same information is written as a conversation notice (the established `show_bounded_notice` / `show_compacted_notice` shape) so it outlives the toast.
- **D-U8: retention is 20 turns per session, evicting oldest manifests and their blobs.** Bounded by `MAX_WRITE_BYTES` (64KiB) per snapshot, so the worst case is ~1.3MiB per session; no configuration in this round.
- **D-U9: no confirmation dialog for undo.** It is initiated by the user, it is bounded to the last turn, and every destructive branch (external change, user-deleted file) is a refusal rather than an overwrite.

# Acceptance examples

- A1: after `write_file` (and after `edit_file`) succeeds, `<project>/.xgent/checkpoints/` contains a manifest entry for that turn holding the file's pre-write bytes (or a "did not exist" marker), and no `<name>.xgent-bak` file is created anywhere; the existing executor test that asserted the `.xgent-bak` file is updated to assert the checkpoint instead.
- A2: within one turn the same file is written three times with contents c1→c2→c3; undoing that turn leaves the file with c0 (the content before the turn), and the entries are replayed newest-first.
- A3: a file the agent created and never had before is deleted by undo; a file that existed before the turn but that the user has since deleted is NOT recreated, and is reported as skipped.
- A4: if the current bytes of a checkpointed file differ from what the agent wrote (the user or a build edited it), undo leaves it untouched, still restores the other files of that turn, and its report names the skipped path.
- A5: the command palette lists an undo command that is discoverable by typing, and invoking it performs the restore; the command id is registered through `CommandRegistry::try_register` and routed by `handle_palette_triggers`, not through a plugin `plugin.` prefix.
- A6: undo applies to the newest turn of the current session only; invoking it repeatedly walks back turn by turn and finally reports nothing-left-to-undo; while the conversation status is not Idle it refuses with a message and changes nothing.
- A7: a snapshot that could not be recorded (e.g. the checkpoint directory is unwritable) does not block the write — the file is still written, the tool result still succeeds, and the undo report states that this turn had uncovered files instead of claiming full coverage.
- A8: more than 20 recorded turns in one session keeps only the 20 newest: the oldest manifest and its blobs are gone, and undo still works over the retained ones.
- A9: undo writes through the same atomic path as the tools (temp file in the same directory + rename + `sync_all`), so an interrupted restore leaves the file at its previous content, and no `.xgent-tmp` leftovers remain; an open clean editor buffer for a reverted file is reloaded by the existing filesystem-watch pipeline and a dirty buffer reaches the existing conflict dialog rather than being silently replaced.
- A10: every new user-facing string (palette label, success/skipped/nothing-to-undo/busy notices) has keys in both `zh-CN/main.ftl` and `en-US/main.ftl`, and no new user-visible string is hardcoded in Rust (the existing hardcoded Chinese in `agent_loop.rs:61` is not made worse).
- A11: `cargo test -p xgent_tools --lib`, `cargo test -p xgent_agent --lib`, `cargo test -p xgent_ui --lib` and `cargo check --workspace --all-targets` pass; `Tool::write_targets` returns empty for `read_file`, `search_files` and `run_command` so those tools' behaviour is unchanged; `backup_existing` is gone from `atomic.rs` with no remaining callers.

# Verification expectations

- Formal checks stay per-crate and cheap: `cargo test -p xgent_tools --lib`, `-p xgent_agent`, `-p xgent_ui`, `cargo check --workspace --all-targets`, `cargo fmt -p <crate> -- --check` for touched crates. Check-plan `argv` must not repeat the program name and `cwdRef` is `.` (learned the hard way this round: Runtime execs `executable + argv` directly). Timeouts generous — on this machine a per-crate cargo suite has taken up to ~11 minutes under IO pressure.
- Filesystem behaviour is tested against `tempfile::tempdir()` roots, not the real project, and the checkpoint root is injected so store tests never touch `~/.xgent` or another test's directory.
- Counter-proof: for A1..A8 and A11 at least one mutant each. Additional explicit check for this change: source test-attribute count vs `cargo test --lib -- --list` registration count, because a swallowed newline last round silently de-registered a test and made a mutant look uncaught.
- The A9 "buffer reload" claim is not unit-testable headlessly; it is verified by reading the existing pipeline (`fs_event_bridge::bridge_file_changed` → `editor/conflict.rs::handle_file_changed`) and asserted at the code level (undo performs no bespoke reload and introduces no second path). Say so plainly in the report rather than claiming UI verification.
